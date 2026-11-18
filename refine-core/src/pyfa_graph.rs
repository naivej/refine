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
const MAX_SIMULATED_CAPACITOR_PLOT_POINTS: usize = 8500;

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

/// Pyfa's simulated capacitor curve, built from post-activation event points.
pub struct SimulatedCapacitorGraph<'a> {
    pub capacity: f64,
    pub recharge_time_s: f64,
    pub starting_amount: f64,
    pub events: &'a [(f64, f64)],
}

/// Failure while producing a bounded simulated capacitor plot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SimulatedCapacitorPlotError {
    InvalidInput,
    ResolutionUnderflow,
    PointLimit,
    Cancelled,
}

impl SimulatedCapacitorGraph<'_> {
    fn recharge(&self, start: f64, elapsed: f64) -> Option<f64> {
        PassiveChargeSample::at(elapsed, self.capacity, self.recharge_time_s, start).map(|s| s.amount)
    }

    /// Markers at and after the final event are absent in Pyfa.
    pub fn marker(&self, time: f64) -> Option<f64> {
        if !nonnegative(time) {
            return None;
        }
        let index = self.events.partition_point(|event| event.0 <= time);
        if !self.events.is_empty() && index == self.events.len() {
            return None;
        }
        if index == 0 {
            self.recharge(self.starting_amount, time)
        } else {
            let (t, cap) = self.events[index - 1];
            if t == time {
                Some(cap)
            } else {
                self.recharge(cap, time - t)
            }
        }
    }

    /// Pyfa's 200-resolution plot, including both sides of every event jump.
    /// Work is bounded before each interval and cancellation is checked per point.
    pub fn plot(
        &self,
        lower: f64,
        upper: f64,
        max_points: usize,
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<(f64, f64)>, SimulatedCapacitorPlotError> {
        use SimulatedCapacitorPlotError as Error;
        if !nonnegative(lower) || !nonnegative(upper) || lower > upper {
            return Err(Error::InvalidInput);
        }
        let max_points = max_points.min(MAX_SIMULATED_CAPACITOR_PLOT_POINTS);
        let mut points = Vec::new();
        let mut push = |time, cap| {
            if is_cancelled() {
                return Err(Error::Cancelled);
            }
            if points.len() >= max_points {
                return Err(Error::PointLimit);
            }
            points.push((time, cap));
            Ok(())
        };
        if self.events.is_empty() {
            let step = (upper - lower) / 200.0;
            // Pyfa's smooth getter emits one point when its step underflows.
            let last_index = if step == 0.0 { 0 } else { 200 };
            if last_index + 1 > max_points {
                return Err(Error::PointLimit);
            }
            for i in 0..=last_index {
                let time = lower + step * i as f64;
                let cap = self.recharge(self.starting_amount, time).ok_or(Error::InvalidInput)?;
                push(time, cap)?;
            }
            return Ok(points);
        }
        let last = self.events.last().ok_or(Error::InvalidInput)?.0;
        if lower > last {
            return Ok(points);
        }
        let end = upper.min(last);
        let spacing = (end - lower) / 200.0;
        // A final event exactly at the lower bound has no interpolation interval.
        if end > lower && spacing == 0.0 {
            return Err(Error::ResolutionUnderflow);
        }
        let before = self.events.partition_point(|e| e.0 < lower);
        let (mut prev_time, mut prev_cap) = if before == 0 {
            (
                lower,
                self.recharge(self.starting_amount, lower).ok_or(Error::InvalidInput)?,
            )
        } else {
            let (time, cap) = self.events[before - 1];
            (lower, self.recharge(cap, lower - time).ok_or(Error::InvalidInput)?)
        };
        push(prev_time, prev_cap)?;
        // Cap the number of loop iterations before converting float counts or sampling.
        let mut generated = 1usize;
        for &(time, cap) in self.events[before..].iter().take_while(|e| e.0 <= end) {
            if time > prev_time {
                let count = ((time - prev_time) / spacing).ceil();
                if !count.is_finite() || count > max_points.saturating_sub(generated) as f64 {
                    return Err(Error::PointLimit);
                }
                let count = count as usize;
                let length = (time - prev_time) / count as f64;
                for i in 1..=count {
                    let t = prev_time + length * i as f64;
                    let value = self.recharge(prev_cap, t - prev_time).ok_or(Error::InvalidInput)?;
                    push(t, value)?;
                }
                generated += count;
            }
            push(time, cap)?;
            generated += 1;
            prev_time = time;
            prev_cap = cap;
        }
        if end > prev_time {
            let count = ((end - prev_time) / spacing).ceil();
            if !count.is_finite() || count > max_points.saturating_sub(generated) as f64 {
                return Err(Error::PointLimit);
            }
            let count = count as usize;
            let length = (end - prev_time) / count as f64;
            for i in 1..=count {
                let t = prev_time + length * i as f64;
                let value = self.recharge(prev_cap, t - prev_time).ok_or(Error::InvalidInput)?;
                push(t, value)?;
            }
        }
        Ok(points)
    }
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
mod simulated_capacitor_tests {
    use super::SimulatedCapacitorGraph;

    #[test]
    fn final_event_is_a_plot_jump_but_not_a_marker() {
        let graph = SimulatedCapacitorGraph {
            capacity: 100.0,
            recharge_time_s: 100.0,
            starting_amount: 100.0,
            events: &[(0.0, 90.0), (10.0, 80.0)],
        };
        assert_eq!(graph.marker(0.0), Some(90.0));
        assert_eq!(graph.marker(10.0), None);
        assert_eq!(graph.marker(11.0), None);
        let plot = graph.plot(0.0, 20.0, 8500, &|| false).unwrap();
        assert_eq!(plot[0], (0.0, 100.0));
        assert_eq!(plot[1], (0.0, 90.0));
        assert_eq!(*plot.last().unwrap(), (10.0, 80.0));
        assert!(graph.plot(11.0, 20.0, 8500, &|| false).unwrap().is_empty());
    }

    #[test]
    fn empty_events_fall_back_to_passive_plot_and_marker() {
        let graph = SimulatedCapacitorGraph {
            capacity: 100.0,
            recharge_time_s: 100.0,
            starting_amount: 0.0,
            events: &[],
        };
        let plot = graph.plot(0.0, 100.0, 8500, &|| false).unwrap();
        assert_eq!(plot.len(), 201);
        assert_eq!(graph.marker(100.0), Some(plot[200].1));
    }
    #[test]
    fn subnormal_active_plot_rejects_resolution_underflow_before_sampling() {
        use super::SimulatedCapacitorPlotError;
        let graph = SimulatedCapacitorGraph {
            capacity: 100.0,
            recharge_time_s: 100.0,
            starting_amount: 100.0,
            events: &[(0.0, 90.0), (1.0, 80.0)],
        };
        assert_eq!(
            graph.plot(0.0, f64::from_bits(1), 8500, &|| false),
            Err(SimulatedCapacitorPlotError::ResolutionUnderflow)
        );
        let endpoint = SimulatedCapacitorGraph {
            events: &[(0.0, 90.0)],
            ..graph
        };
        assert_eq!(
            endpoint.plot(0.0, 1.0, 8500, &|| false).unwrap(),
            [(0.0, 100.0), (0.0, 90.0)]
        );
    }

    #[test]
    fn plot_checks_cancellation_at_each_point_and_limits_work() {
        use std::cell::Cell;

        use super::SimulatedCapacitorPlotError;
        let graph = SimulatedCapacitorGraph {
            capacity: 100.0,
            recharge_time_s: 100.0,
            starting_amount: 100.0,
            events: &[(0.0, 90.0), (10.0, 80.0)],
        };
        let checks = Cell::new(0);
        let result = graph.plot(0.0, 20.0, 8500, &|| {
            checks.set(checks.get() + 1);
            checks.get() == 4
        });
        assert_eq!(result, Err(SimulatedCapacitorPlotError::Cancelled));
        assert_eq!(checks.get(), 4);
        assert_eq!(
            graph.plot(0.0, 20.0, 3, &|| false),
            Err(SimulatedCapacitorPlotError::PointLimit)
        );
    }
    #[test]
    fn passive_zero_step_returns_one_lower_point() {
        let graph = SimulatedCapacitorGraph {
            capacity: 100.0,
            recharge_time_s: 100.0,
            starting_amount: 0.0,
            events: &[],
        };
        assert_eq!(
            graph.plot(0.0, f64::from_bits(1), 8500, &|| false).unwrap(),
            [(0.0, 0.0)]
        );
    }
    #[test]
    fn singleton_active_plot_retains_event_jump_and_is_empty_after_tail() {
        let graph = SimulatedCapacitorGraph {
            capacity: 100.0,
            recharge_time_s: 100.0,
            starting_amount: 100.0,
            events: &[(0.0, 90.0), (10.0, 80.0)],
        };
        let event = graph.plot(10.0, 10.0, 8500, &|| false).unwrap();
        assert_eq!(event.len(), 2);
        assert_eq!(event[0].0, 10.0);
        assert_eq!(event[1], (10.0, 80.0));
        assert_eq!(graph.marker(10.0), None);
        assert_eq!(graph.plot(5.0, 5.0, 8500, &|| false).unwrap().len(), 1);
        assert!(graph.plot(11.0, 11.0, 8500, &|| false).unwrap().is_empty());
    }
    #[test]
    fn plot_enforces_absolute_point_cap_even_if_caller_requests_more() {
        use super::SimulatedCapacitorPlotError;
        let events = vec![(0.0, 90.0); 8501];
        let graph = SimulatedCapacitorGraph {
            capacity: 100.0,
            recharge_time_s: 100.0,
            starting_amount: 100.0,
            events: &events,
        };
        assert_eq!(
            graph.plot(0.0, 1.0, usize::MAX, &|| false),
            Err(SimulatedCapacitorPlotError::PointLimit)
        );
    }
}
