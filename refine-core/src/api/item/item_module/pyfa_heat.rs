use crate::{AttrId, EveAttrId, Fit, ItemCommon, Lender, ModRack, Module, ModuleState};

const HP: i32 = 9;
const LOW_SLOTS: i32 = 12;
const MID_SLOTS: i32 = 13;
const HIGH_SLOTS: i32 = 14;
const SPEED: i32 = 51;
const DURATION: i32 = 73;
const RIG_SLOTS: i32 = 1137;
const HEAT_ABSORPTION_RATE: i32 = 1180;
const HEAT_DAMAGE: i32 = 1211;
const HEAT_GENERATION_MULTIPLIER: i32 = 1224;
const HIGH_HEAT_ATTENUATION: i32 = 1259;
const MID_HEAT_ATTENUATION: i32 = 1261;
const LOW_HEAT_ATTENUATION: i32 = 1262;
const SIMULATION_LIMIT_S: f64 = 600.0;
// Monotone probabilities have only 100,001 distinct five-decimal rounded values.
const MAX_HEAT_SAMPLES: usize = 100_002;
const MAX_SURVIVOR_UPDATES: usize = 1_000_000;

/// A snapshot of the fitted inputs to Pyfa's Heat column for one overloaded module.
#[derive(Debug)]
pub struct PyfaModuleHeat {
    heat_rate: f64,
    slot_factor: f64,
    neighbor_attenuations: Vec<f64>,
    speed_ms: f64,
    duration_ms: f64,
    hp: f64,
    heat_damage: f64,
}

/// Pyfa's floored estimate of cycles until burnout and the corresponding time.
#[derive(Debug)]
pub struct PyfaHeatBurnout {
    pub cycles: u64,
    pub time_s: f64,
}

impl PyfaModuleHeat {
    /// Probability of heat damage at the supplied number of seconds since a cold start.
    /// Returns `None` for non-finite or negative time.
    pub fn damage_probability(&self, time_s: f64) -> Option<f64> {
        (time_s.is_finite() && time_s >= 0.0).then(|| self.probability(time_s))
    }

    /// Estimate burnout using Pyfa's per-cycle samples and constant-probability tail.
    /// Returns `None` if timing, damage or hitpoints prevent a finite estimate, or the
    /// fitted inputs would require more than one million survivor updates.
    pub fn burnout(&self) -> Option<PyfaHeatBurnout> {
        // Pyfa samples at speed if present, but its Heat column displays duration if present.
        let interval_s = preferred_cycle_s(self.speed_ms, self.duration_ms)?;
        let display_cycle_s = preferred_cycle_s(self.duration_ms, self.speed_ms)?;
        let damage_events = (self.hp / self.heat_damage).ceil();
        if self.hp <= 0.0 || self.heat_damage <= 0.0 || !(1.0..u64::MAX as f64).contains(&damage_events) {
            return None;
        }
        let damage_events = damage_events as u64;
        let mut probabilities = Vec::new();
        let mut time_s = interval_s;
        let mut last_display = String::from("0.00000");
        while time_s < SIMULATION_LIMIT_S && probabilities.len() < MAX_HEAT_SAMPLES {
            let probability = self.probability(time_s);
            probabilities.push(probability);
            let displayed = format!("{probability:.5}");
            if displayed == last_display {
                break;
            }
            last_display = displayed;
            time_s += interval_s;
        }
        let last_probability = *probabilities.last()?;
        if last_probability <= 0.0 {
            return None;
        }
        let sampled_cycles = probabilities.len();
        let expected = if damage_events > sampled_cycles as u64 {
            // Burnout cannot occur during sampling. The expected hit count is the sum
            // of the per-cycle probabilities, even without computing their distribution.
            let remaining = damage_events as f64 - probabilities.into_iter().sum::<f64>();
            sampled_cycles as f64 + remaining * (1.0 / last_probability)
        } else {
            expectation(&probabilities, damage_events as usize, last_probability)?
        };
        let cycles = expected.floor();
        let time_s = cycles * display_cycle_s;
        (cycles < u64::MAX as f64 && time_s.is_finite()).then_some(PyfaHeatBurnout {
            cycles: cycles as u64,
            time_s,
        })
    }

    fn probability(&self, time_s: f64) -> f64 {
        let local = self.slot_factor * (1.0 - (-time_s * self.heat_rate).exp());
        let no_neighbor_damage = self
            .neighbor_attenuations
            .iter()
            .map(|attenuation| 1.0 - attenuation * local)
            .product::<f64>();
        if no_neighbor_damage == 1.0 {
            local
        } else {
            1.0 - no_neighbor_damage * (1.0 - local)
        }
    }
}

impl Module<'_> {
    /// Prepare the Pyfa Heat-column estimator from the current fitted rack and pilot state.
    ///
    /// Returns `None` for modules outside overload, unloaded items, or invalid rack heat inputs.
    /// Call again after changing the fit to obtain an updated estimate.
    pub fn get_pyfa_heat(&mut self) -> Option<PyfaModuleHeat> {
        if self.get_state() != ModuleState::Overload {
            return None;
        }
        let rack = self.get_rack();
        let pos = self.get_pos().into_usize();
        let hp = fitted_attr(self, HP)?;
        let heat_damage = fitted_attr(self, HEAT_DAMAGE)?;
        let speed_ms = fitted_attr(self, SPEED).unwrap_or_default();
        let duration_ms = fitted_attr(self, DURATION).unwrap_or_default();

        let fit_uid = self.sol.u_data.items.get(self.uid).dc_module().unwrap().get_fit_uid();
        let mut fit = Fit::new(self.sol, fit_uid);
        let mut ship = fit.get_ship()?;
        let slot_count = [LOW_SLOTS, MID_SLOTS, HIGH_SLOTS, RIG_SLOTS]
            .into_iter()
            .map(|attr| fitted_attr(&mut ship, attr).unwrap_or_default())
            .sum::<f64>();
        let generation = fitted_attr(&mut ship, HEAT_GENERATION_MULTIPLIER).unwrap_or_default();
        let attenuation = fitted_attr(
            &mut ship,
            match rack {
                ModRack::High => HIGH_HEAT_ATTENUATION,
                ModRack::Mid => MID_HEAT_ATTENUATION,
                ModRack::Low => LOW_HEAT_ATTENUATION,
            },
        )
        .unwrap_or(0.25);

        let mut online_slots = 0;
        let mut absorption = 0.0;
        let mut neighbor_attenuations = Vec::new();
        for fitted_rack in [ModRack::High, ModRack::Mid, ModRack::Low] {
            let mut modules = fit.iter_modules(fitted_rack);
            while let Some(mut module) = modules.next() {
                let state = module.get_state();
                online_slots += usize::from(state != ModuleState::Offline);
                if fitted_rack == rack && state == ModuleState::Overload {
                    let rate = fitted_attr(&mut module, HEAT_ABSORPTION_RATE)?;
                    if rate < 0.0 {
                        return None;
                    }
                    absorption += rate;
                    let distance = pos.abs_diff(module.get_pos().into_usize());
                    if distance > 0 {
                        neighbor_attenuations.push(attenuation.powi(distance as i32));
                    }
                }
            }
        }
        let slot_factor = online_slots as f64 / slot_count;
        let heat_rate = generation * absorption;
        if !(0.0..=1.0).contains(&slot_factor)
            || !(0.0..=1.0).contains(&attenuation)
            || !heat_rate.is_finite()
            || heat_rate < 0.0
        {
            return None;
        }
        Some(PyfaModuleHeat {
            heat_rate,
            slot_factor,
            neighbor_attenuations,
            speed_ms,
            duration_ms,
            hp,
            heat_damage,
        })
    }
}

fn fitted_attr(item: &mut impl ItemCommon, id: i32) -> Option<f64> {
    let value = item
        .get_attr(&AttrId::Eve(EveAttrId::from_i32(id)))
        .ok()?
        .modified
        .into_f64();
    value.is_finite().then_some(value)
}

fn preferred_cycle_s(preferred_ms: f64, fallback_ms: f64) -> Option<f64> {
    let time_ms = if preferred_ms != 0.0 { preferred_ms } else { fallback_ms };
    let time_s = time_ms / 1000.0;
    (time_s > 0.0).then_some(time_s)
}

fn expectation(probabilities: &[f64], damage_events: usize, last_probability: f64) -> Option<f64> {
    if damage_events > MAX_SURVIVOR_UPDATES / probabilities.len() {
        return None;
    }
    // Track surviving probability mass by number of heat damage events so far.
    let mut survivors = vec![0.0; damage_events];
    survivors[0] = 1.0;
    let mut expected = 0.0;
    for (cycle, &probability) in probabilities.iter().enumerate() {
        expected += (cycle + 1) as f64 * probability * survivors[damage_events - 1];
        for hits in (1..damage_events).rev() {
            survivors[hits] = (1.0 - probability) * survivors[hits] + probability * survivors[hits - 1];
        }
        survivors[0] *= 1.0 - probability;
    }
    for (hits, survivor) in survivors.into_iter().enumerate() {
        let remaining = (damage_events - hits) as f64;
        expected += (probabilities.len() as f64 + remaining * (1.0 / last_probability)) * survivor;
    }
    Some(expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heat() -> PyfaModuleHeat {
        PyfaModuleHeat {
            heat_rate: 0.02,
            slot_factor: 0.5,
            neighbor_attenuations: vec![],
            speed_ms: 10_000.0,
            duration_ms: 20_000.0,
            hp: 40.0,
            heat_damage: 4.0,
        }
    }

    #[test]
    fn burnout_uses_speed_for_sampling_and_duration_for_display() {
        let mut estimator = heat();
        let both = estimator.burnout().unwrap();
        estimator.duration_ms = 0.0;
        let speed_only = estimator.burnout().unwrap();
        assert_eq!(both.cycles, speed_only.cycles);
        assert_eq!(both.time_s, 2.0 * speed_only.time_s);
        estimator.speed_ms = 0.0;
        estimator.duration_ms = 10_000.0;
        let duration_only = estimator.burnout().unwrap();
        assert_eq!(duration_only.cycles, speed_only.cycles);
        assert_eq!(duration_only.time_s, speed_only.time_s);
    }

    #[test]
    fn zero_probability_and_invalid_time_leave_burnout_unavailable() {
        let mut estimator = heat();
        assert_eq!(estimator.damage_probability(0.0), Some(0.0));
        assert!(estimator.damage_probability(-1.0).is_none());
        assert!(estimator.damage_probability(f64::INFINITY).is_none());
        assert!(estimator.damage_probability(f64::NAN).is_none());
        estimator.speed_ms = 1e-300;
        assert!(estimator.burnout().is_none());
        estimator.speed_ms = 10_000.0;
        estimator.heat_rate = 0.0;
        assert_eq!(estimator.damage_probability(60.0), Some(0.0));
        assert!(estimator.burnout().is_none());
    }

    #[test]
    fn burnout_bounds_work_for_high_damage_event_counts() {
        let mut estimator = heat();
        estimator.hp = 1e9;
        let burnout = estimator.burnout().unwrap();
        assert!(burnout.cycles > 500_000_000);

        estimator.heat_damage = f64::MIN_POSITIVE;
        assert!(estimator.burnout().is_none());
        assert!(expectation(&[0.1; 2_000], 1_000, 0.1).is_none());
    }
}
