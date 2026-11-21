//! Numerical graph samples compatible with Pyfa v2.70.0.
//!
//! The caller resolves the source fit and per-graph settings. In particular, a warp
//! graph uses the subwarp speed after disabling equipment that cannot run in warp.

use crate::{
    PValue, UnitInterval,
    def::AU,
    svc::{calc_regen, regenerate},
};

const KG_PER_MILLION: f64 = 1_000_000.0;
const ECM_BURST_PERIOD_S: f64 = 30.0;
const DRONE_RELOCK_DELAY_S: f64 = 3.0;
const MAX_ECM_CYCLES: u32 = 4096;

/// A resolved active source for the Pyfa Electronic Warfare Stats graph.
/// `strength` is a signed speed-factor percentage for a web, or an amount in GJ
/// per cycle for a neutralizer. Resolve fitting effects and states before sampling.
#[derive(Clone, Copy, Debug)]
pub struct PyfaEwarSource {
    pub optimal_m: f64,
    pub falloff_m: f64,
    pub strength: f64,
    /// Average neutralizer cycle duration in seconds; ignored for webs.
    pub cycle_s: f64,
}

fn pyfa_ewar_range_factor(source: &PyfaEwarSource, distance_m: f64) -> f64 {
    if source.falloff_m > 0.0 {
        if distance_m > source.optimal_m + 3.0 * source.falloff_m {
            return 0.0;
        }
        let ratio = (distance_m - source.optimal_m).max(0.0) / source.falloff_m;
        let exponent = ratio.powf(std::hint::black_box(2.0));
        std::hint::black_box(0.5_f64).powf(exponent)
    } else if distance_m <= source.optimal_m {
        1.0
    } else {
        0.0
    }
}

/// Sample at most two active webs in Pyfa's graph; applies resistance before stacking.
pub fn pyfa_ewar_web_percentage(
    sources: &[PyfaEwarSource],
    distance_m: f64,
    lock_range_m: f64,
    ignore_lock_range: bool,
    resistance: f64,
) -> Option<f64> {
    if sources.len() > 2
        || !nonnegative(distance_m)
        || !nonnegative(lock_range_m)
        || !(0.0..=1.0).contains(&resistance)
        || sources.iter().any(|s| {
            !nonnegative(s.optimal_m) || !nonnegative(s.falloff_m) || !s.strength.is_finite() || s.strength > 0.0
        })
    {
        return None;
    }
    // Pyfa's graph accepts km, then normalizes back to m before checking range.
    let distance_m = distance_m / 1000.0 * 1000.0;
    if !distance_m.is_finite() {
        return None;
    }
    if !ignore_lock_range && distance_m > lock_range_m {
        return Some(0.0);
    }
    let mut strengths = [0.0; 2];
    for (i, source) in sources.iter().enumerate() {
        strengths[i] = source.strength * (1.0 - resistance) * pyfa_ewar_range_factor(source, distance_m);
    }
    let combined = pyfa_ewar_multiplier(&strengths[..sources.len()])?;
    let value = (1.0 - combined) * 100.0;
    value.is_finite().then_some(value)
}

/// Stack up to two signed percent modifiers using Pyfa's multiplier order.
pub fn pyfa_ewar_multiplier(strengths: &[f64]) -> Option<f64> {
    if strengths.len() > 2 || strengths.iter().any(|s| !s.is_finite() || *s > 0.0) {
        return None;
    }
    let mut multipliers = [1.0_f64; 2];
    let mut count = 0;
    for strength in strengths {
        let multiplier = 1.0 + strength / 100.0;
        if multiplier < 1.0 {
            multipliers[count] = multiplier;
            count += 1;
        }
    }
    if count == 2 && multipliers[0] > multipliers[1] {
        multipliers.swap(0, 1);
    }
    let mut combined = 1.0;
    for (i, multiplier) in multipliers[..count].iter().copied().enumerate() {
        combined *= 1.0 + (multiplier - 1.0) * (-(i as f64).powi(2) / 7.1289).exp();
    }
    combined.is_finite().then_some(combined)
}

/// Sample at most two active neutralizers without victim capacity or signature limits.
pub fn pyfa_ewar_neut_gj_per_s(
    sources: &[PyfaEwarSource],
    distance_m: f64,
    lock_range_m: f64,
    ignore_lock_range: bool,
    resistance: f64,
) -> Option<f64> {
    if sources.len() > 2
        || !nonnegative(distance_m)
        || !nonnegative(lock_range_m)
        || !(0.0..=1.0).contains(&resistance)
        || sources.iter().any(|s| {
            !nonnegative(s.optimal_m) || !nonnegative(s.falloff_m) || !nonnegative(s.strength) || !positive(s.cycle_s)
        })
    {
        return None;
    }
    let distance_m = distance_m / 1000.0 * 1000.0;
    if !distance_m.is_finite() {
        return None;
    }
    if !ignore_lock_range && distance_m > lock_range_m {
        return Some(0.0);
    }
    let mut combined = 0.0;
    for source in sources {
        let strength = source.strength / source.cycle_s * (1.0 - resistance);
        combined += strength * pyfa_ewar_range_factor(source, distance_m);
    }
    combined.is_finite().then_some(combined)
}

/// Passive shield or capacitor charge, in the same units as the supplied capacity.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PassiveChargeSample {
    pub amount: f64,
    pub recharge_per_s: f64,
}

impl PassiveChargeSample {
    /// Sample the passive curve. Capacity and start can use HP, EHP or GJ.
    /// Simulated capacitor events are a separate graph and are not included.
    pub fn at(seconds: f64, capacity: f64, recharge_time_s: f64, starting_amount: f64) -> Option<Self> {
        if !nonnegative(seconds)
            || !positive(capacity)
            || !positive(recharge_time_s)
            || !nonnegative(starting_amount)
            || starting_amount > capacity
        {
            return None;
        }
        // Pyfa fitShieldRegen and fitCapacitor time graph getters.
        let max = PValue::from_f64_unchecked(capacity);
        let decay = PValue::from_f64_unchecked((-5.0 * seconds / recharge_time_s).exp());
        let amount = regenerate(PValue::from_f64_unchecked(starting_amount), max, decay).into_f64();
        let recharge_per_s = calc_regen(
            max,
            PValue::from_f64_unchecked(recharge_time_s),
            UnitInterval::from_f64_clamped(amount / capacity),
        )
        .into_f64();
        if !amount.is_finite() || !recharge_per_s.is_finite() {
            return None;
        }
        Some(Self { amount, recharge_per_s })
    }
}

/// A single immediate shield repair cycle.
#[derive(Clone, Copy, Debug)]
pub struct RemoteRepairCycle {
    pub duration_s: f64,
    pub amount_hp: f64,
}

/// A reload after a fixed number of equal repair cycles.
#[derive(Clone, Copy, Debug)]
pub struct RemoteRepairReload {
    pub magazine_cycles: u32,
    pub duration_s: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct RemoteRepairSchedule {
    pub cycle: RemoteRepairCycle,
    pub reload: Option<RemoteRepairReload>,
}

#[derive(Clone, Copy, Debug)]
pub struct MissileMagazine {
    pub cycle_s: f64,
    pub cycles: u32,
    pub reload_s: f64,
}

/// Distance reached at a flight tick under Pyfa's missile acceleration approximation.
pub(crate) fn pyfa_tick_range(speed: f64, mass: f64, agility: f64, radius: f64, tick: f64) -> f64 {
    let t = tick.min(mass * agility / 1e6);
    (speed / 2.0 * t + speed * (tick - t) - radius).max(0.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionSample {
    pub distance_m: f64,
    pub speed_m_per_s: f64,
    pub momentum_kg_m_per_s: f64,
}

impl MotionSample {
    /// Sample acceleration from rest with the speed, mass and inertia from the fit.
    pub fn at(seconds: f64, max_speed_m_per_s: f64, mass_kg: f64, inertia: f64) -> Option<Self> {
        if !nonnegative(seconds) || !nonnegative(max_speed_m_per_s) || !positive(mass_kg) || !positive(inertia) {
            return None;
        }
        // Pyfa graphs/data/fitMobility/getter.py integrates this acceleration curve.
        let time_constant = inertia * mass_kg / KG_PER_MILLION;
        if !positive(time_constant) {
            return None;
        }
        // Preserve the operation order of Pyfa's registered Mobility getters.
        // In particular, 1 - exp(x) rounds to zero at sufficiently small times.
        let exponent = (-seconds * KG_PER_MILLION) / (inertia * mass_kg);
        let decay = exponent.exp();
        let speed_m_per_s = max_speed_m_per_s * (1.0 - decay);
        let distance_t = max_speed_m_per_s * seconds + (max_speed_m_per_s * inertia * mass_kg * decay / KG_PER_MILLION);
        let distance_0 = max_speed_m_per_s * inertia * mass_kg / KG_PER_MILLION;
        let distance_m = distance_t - distance_0;
        let momentum_kg_m_per_s = speed_m_per_s * mass_kg;
        if ![distance_m, speed_m_per_s, momentum_kg_m_per_s]
            .into_iter()
            .all(f64::is_finite)
        {
            return None;
        }
        Some(Self {
            distance_m,
            speed_m_per_s,
            momentum_kg_m_per_s,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BumpSample {
    pub target_speed_m_per_s: f64,
    pub target_distance_m: f64,
}

impl BumpSample {
    /// Sample the target's motion after an elastic collision with a stationary target.
    pub fn at(
        source_speed_m_per_s: f64,
        source_mass_kg: f64,
        target_mass_kg: f64,
        target_inertia: f64,
    ) -> Option<Self> {
        if !nonnegative(source_speed_m_per_s)
            || !positive(source_mass_kg)
            || !nonnegative(target_mass_kg)
            || !nonnegative(target_inertia)
        {
            return None;
        }
        // Pyfa fitMobility uses S. Santorine's collision calculation.
        let source_mass = source_mass_kg / KG_PER_MILLION;
        let target_mass = target_mass_kg / KG_PER_MILLION;
        let target_speed_m_per_s = 2.0 * source_speed_m_per_s * source_mass / (source_mass + target_mass);
        let target_distance_m = target_speed_m_per_s * target_mass * target_inertia;
        if !target_speed_m_per_s.is_finite() || !target_distance_m.is_finite() {
            return None;
        }
        Some(Self {
            target_speed_m_per_s,
            target_distance_m,
        })
    }
}

/// The Fit Lock Time graph uses a fixed duration when the fit has no scan resolution.
pub fn lock_time_s(scan_resolution_mm: f64, fixed_lock_time_s: f64, signature_m: f64) -> Option<f64> {
    if !nonnegative(scan_resolution_mm) || !nonnegative(signature_m) {
        return None;
    }
    if scan_resolution_mm == 0.0 {
        return nonnegative(fixed_lock_time_s).then_some(fixed_lock_time_s);
    }
    if signature_m == 0.0 {
        return None;
    }
    // Pyfa eos/calc.py calculateLockTime caps a quick calculation at 30 minutes.
    Some((40_000.0 / scan_resolution_mm / signature_m.asinh().powi(2)).min(1800.0))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EcmBurstSample {
    pub opponent_lock_time_s: f64,
    pub opponent_lock_uptime_s: f64,
    pub inflicted_damage_hp: f64,
}

/// Resolved inputs to Pyfa's ECM Burst + Scanres Damps graph.
#[derive(Clone, Copy, Debug)]
pub struct EcmBurstScenario {
    pub source_signature_m: f64,
    pub source_ehp: f64,
    pub source_weapon_dps: f64,
    pub source_drone_dps: f64,
    pub opponent_scan_resolution_mm: f64,
    pub opponent_scan_resolution_multiplier: f64,
    pub opponent_dps: f64,
    pub uptime_adjustment_s: f64,
    pub max_cycles: u32,
}

impl EcmBurstScenario {
    /// The graph assumes a successful burst every 30 seconds.
    /// At most 4096 cycles can be sampled.
    pub fn sample(self) -> Option<EcmBurstSample> {
        if !positive(self.source_signature_m)
            || !positive(self.opponent_scan_resolution_mm)
            || !positive(self.opponent_scan_resolution_multiplier)
            || !positive(self.opponent_dps)
            || ![self.source_ehp, self.source_weapon_dps, self.source_drone_dps]
                .into_iter()
                .all(nonnegative)
            || !self.uptime_adjustment_s.is_finite()
            || self.max_cycles > MAX_ECM_CYCLES
        {
            return None;
        }
        let effective_scan = self.opponent_scan_resolution_mm * self.opponent_scan_resolution_multiplier;
        if !positive(effective_scan) {
            return None;
        }
        let opponent_lock_time_s = lock_time_s(effective_scan, 0.0, self.source_signature_m)?;
        let opponent_lock_uptime_s = (ECM_BURST_PERIOD_S - opponent_lock_time_s).max(0.0);
        // Pyfa adjusts uptime only for the damage series.
        let damage_uptime = (ECM_BURST_PERIOD_S - opponent_lock_time_s - self.uptime_adjustment_s).max(0.0);
        let damage_downtime = ECM_BURST_PERIOD_S - damage_uptime;
        let damage_per_cycle = damage_uptime * self.opponent_dps;
        if !damage_per_cycle.is_finite() {
            return None;
        }
        let mut inflicted_damage_hp = 0.0;
        let mut remaining_ehp = self.source_ehp;
        for _ in 0..self.max_cycles {
            let time_under_fire = damage_uptime.min(remaining_ehp / self.opponent_dps);
            let time_alive = damage_downtime + time_under_fire;
            remaining_ehp -= damage_per_cycle;
            inflicted_damage_hp += time_alive * self.source_weapon_dps;
            inflicted_damage_hp += (time_alive - DRONE_RELOCK_DELAY_S).max(0.0) * self.source_drone_dps;
            if remaining_ehp <= 0.0 {
                break;
            }
        }
        inflicted_damage_hp.is_finite().then_some(EcmBurstSample {
            opponent_lock_time_s,
            opponent_lock_uptime_s,
            inflicted_damage_hp,
        })
    }
}

/// Time in warp, excluding alignment and server tick rounding.
pub fn warp_time_s(distance_m: f64, warp_speed_au_per_s: f64, subwarp_speed_m_per_s: f64) -> Option<f64> {
    if !nonnegative(distance_m) || !positive(warp_speed_au_per_s) || !positive(subwarp_speed_m_per_s) {
        return None;
    }
    if distance_m == 0.0 {
        return Some(0.0);
    }
    // EVE University Warp Time Calculation, with the Pyfa fitWarpTime speed caps.
    let accel = warp_speed_au_per_s;
    let decel = (warp_speed_au_per_s / 3.0).min(2.0);
    let dropout_speed = (subwarp_speed_m_per_s / 2.0).min(100.0);
    let top_speed = warp_speed_au_per_s * AU;
    let full_speed_distance = AU + top_speed / decel;
    let (peak_speed, cruise_s) = if distance_m < full_speed_distance {
        (distance_m * accel * decel / (accel + decel), 0.0)
    } else {
        (top_speed, (distance_m - full_speed_distance) / top_speed)
    };
    let seconds = cruise_s + (peak_speed / accel).ln() / accel + (peak_speed / dropout_speed).ln() / decel;
    seconds.is_finite().then_some(seconds)
}

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

fn nonnegative(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

#[cfg(test)]
mod ecm_damp_stacking_tests {
    use super::pyfa_ewar_multiplier;
    #[test]
    fn scan_damps_match_pinned_two_module_multiplier() {
        let multiplier = pyfa_ewar_multiplier(&[-38.25, -38.25]).unwrap();
        assert!((multiplier - 0.4122192925348261).abs() < 1e-14);
        assert_eq!(pyfa_ewar_multiplier(&[]), Some(1.0));
        assert!(pyfa_ewar_multiplier(&[1.0]).is_none());
    }
}
