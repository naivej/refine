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

/// The two registered EWAR series qualified by the Kestrel graph pilot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PyfaEwarKind {
    Web,
    Neutralizer,
    LockRangeDamp,
}

/// Native fit attributes resolved once for up to two active modules.
pub struct PyfaEwarPrepared {
    sources: [PyfaEwarSource; 2],
    count: usize,
    pub lock_range_m: f64,
    kind: PyfaEwarKind,
}

impl PyfaEwarPrepared {
    /// Resolve fitted states and modified item attributes once before sampling.
    pub fn from_fit(fit: &mut crate::Fit<'_>, kind: PyfaEwarKind) -> Option<Self> {
        use lender::Lender;

        use crate::{AttrId, EveAttrId, ItemCommon, ItemTypeId, ModRack, ModuleState};
        let lock_range_m = fit.stats().get_stat_lock_range().ok()?.into_f64();
        let (rack, type_id) = match kind {
            PyfaEwarKind::Web => (ModRack::Mid, 527),
            PyfaEwarKind::LockRangeDamp => (ModRack::Mid, 1969),
            PyfaEwarKind::Neutralizer => (ModRack::High, 12267),
        };
        let blank = PyfaEwarSource {
            optimal_m: 0.0,
            falloff_m: 0.0,
            strength: 0.0,
            cycle_s: 1.0,
        };
        let mut sources = [blank; 2];
        let mut count = 0;
        let mut modules = fit.iter_modules(rack);
        while let Some(mut module) = modules.next() {
            if module.get_type_id() != ItemTypeId::from_i32(type_id) {
                return None;
            }
            if module.get_state() != ModuleState::Active {
                continue;
            }
            if count == sources.len() {
                return None;
            }
            let mut attr = |id| {
                module
                    .get_attr(&AttrId::Eve(EveAttrId::from_i32(id)))
                    .ok()
                    .map(|v| v.modified.into_f64())
            };
            sources[count] = PyfaEwarSource {
                optimal_m: attr(54)?,
                falloff_m: attr(2044)?,
                strength: attr(match kind {
                    PyfaEwarKind::Web => 20,
                    PyfaEwarKind::Neutralizer => 97,
                    PyfaEwarKind::LockRangeDamp => 309,
                })?,
                cycle_s: attr(73)? / 1000.0,
            };
            count += 1;
        }
        Some(Self {
            sources,
            count,
            lock_range_m,
            kind,
        })
    }

    pub fn sample(&self, distance_m: f64, ignore_lock_range: bool, resistance: f64) -> Option<f64> {
        let sources = &self.sources[..self.count];
        match self.kind {
            PyfaEwarKind::Web | PyfaEwarKind::LockRangeDamp => {
                pyfa_ewar_web_percentage(sources, distance_m, self.lock_range_m, ignore_lock_range, resistance)
            }
            PyfaEwarKind::Neutralizer => {
                pyfa_ewar_neut_gj_per_s(sources, distance_m, self.lock_range_m, ignore_lock_range, resistance)
            }
        }
    }
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
    let mut multipliers = [1.0_f64; 2];
    let mut count = 0;
    for s in sources {
        let strength = s.strength * (1.0 - resistance) * pyfa_ewar_range_factor(s, distance_m);
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
    let value = (1.0 - combined) * 100.0;
    value.is_finite().then_some(value)
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

/// The qualified ordinary shield repair graph reports either instantaneous HP/s or cumulative HP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteRepairTimeSeries {
    Rps,
    Total,
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

#[derive(Clone, Debug)]
pub struct RemoteRepairTimeGraph {
    pub markers: Vec<(f64, f64)>,
    pub plot: Option<Vec<(f64, f64)>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteRepairTimeError {
    InvalidInput,
    PointLimit,
    Cancelled,
}

/// Plot and sample Pyfa's time-cache steps. `cycle = None` means no active repairer.
/// `application` is the graph-rounded range/lock multiplier resolved from native projection.
pub fn remote_repair_time_graph(
    cycle: Option<RemoteRepairCycle>,
    application: f64,
    series: RemoteRepairTimeSeries,
    times: &[f64],
    plot_range: Option<[f64; 2]>,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<RemoteRepairTimeGraph, RemoteRepairTimeError> {
    remote_repair_schedule_graph(
        cycle.map(|cycle| RemoteRepairSchedule { cycle, reload: None }),
        application,
        series,
        times,
        plot_range,
        is_cancelled,
    )
}

/// Plot an immediate shield repair with optional periodic magazine reloads.
pub fn remote_repair_schedule_graph(
    schedule: Option<RemoteRepairSchedule>,
    application: f64,
    series: RemoteRepairTimeSeries,
    times: &[f64],
    plot_range: Option<[f64; 2]>,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<RemoteRepairTimeGraph, RemoteRepairTimeError> {
    use RemoteRepairTimeError as Error;
    const MAX_TIME: f64 = 2500.0;
    const MAX_EVENTS: usize = 1024;
    if is_cancelled() {
        return Err(Error::Cancelled);
    }
    if !application.is_finite()
        || !(0.0..=1.0).contains(&application)
        || times.len() > 201
        || times.iter().any(|t| !t.is_finite() || !(0.0..=MAX_TIME).contains(t))
        || times.windows(2).any(|pair| pair[0] >= pair[1])
        || plot_range.is_some_and(|[a, b]| !a.is_finite() || !b.is_finite() || a < 0.0 || a > b || b > MAX_TIME)
    {
        return Err(Error::InvalidInput);
    }
    let horizon = times
        .last()
        .copied()
        .unwrap_or(0.0)
        .max(plot_range.map(|range| range[1]).unwrap_or(0.0));
    let mut events = Vec::new();
    let mut plot_event_limit = None;
    if let Some(RemoteRepairSchedule { cycle, reload }) = schedule {
        if !cycle.duration_s.is_finite()
            || cycle.duration_s <= 0.0
            || !cycle.amount_hp.is_finite()
            || cycle.amount_hp <= 0.0
        {
            return Err(Error::InvalidInput);
        }
        if reload.is_some_and(|reload| {
            reload.magazine_cycles == 0 || !reload.duration_s.is_finite() || reload.duration_s <= 0.0
        }) {
            return Err(Error::InvalidInput);
        }
        if series == RemoteRepairTimeSeries::Rps {
            let rate = (cycle.amount_hp / cycle.duration_s) * application;
            if !rate.is_finite() {
                return Err(Error::InvalidInput);
            }
            events.push((0.0, rate));
            if let Some(reload) = reload {
                plot_event_limit = plot_range.map(|_| 1);
                let mut start = 0.0;
                let last_cycle_offset = f64::from(reload.magazine_cycles - 1) * cycle.duration_s;
                let magazine_duration = f64::from(reload.magazine_cycles) * cycle.duration_s;
                // The time cache includes one activation after the horizon. A gap
                // becomes visible when that or an earlier activation follows a reload.
                while start + last_cycle_offset <= horizon {
                    if is_cancelled() {
                        return Err(Error::Cancelled);
                    }
                    if events.len() + 2 > MAX_EVENTS {
                        return Err(Error::PointLimit);
                    }
                    let reload_start = start + magazine_duration;
                    let next_start = reload_start + reload.duration_s;
                    if !next_start.is_finite() || reload_start <= start || next_start <= reload_start {
                        return Err(Error::InvalidInput);
                    }
                    events.extend([(reload_start, 0.0), (next_start, rate)]);
                    if plot_range.is_some_and(|[_, end]| start + last_cycle_offset <= end) {
                        plot_event_limit = Some(events.len());
                    }
                    start = next_start;
                }
            }
        } else {
            let mut time = 0.0;
            let mut cumulative = 0.0;
            let mut cycles_since_reload = 0;
            loop {
                if is_cancelled() {
                    return Err(Error::Cancelled);
                }
                if events.len() >= MAX_EVENTS {
                    return Err(Error::PointLimit);
                }
                cumulative += cycle.amount_hp;
                if !cumulative.is_finite() {
                    return Err(Error::InvalidInput);
                }
                events.push((time, cumulative * application));
                if time > horizon {
                    break;
                }
                let mut next = time + cycle.duration_s;
                if let Some(reload) = reload {
                    cycles_since_reload += 1;
                    if cycles_since_reload == reload.magazine_cycles {
                        next += reload.duration_s;
                        cycles_since_reload = 0;
                    }
                }
                if !next.is_finite() || next <= time {
                    return Err(Error::InvalidInput);
                }
                time = next;
            }
        }
    }
    // eos.utils.float.floatUnerr compares seven significant decimal digits.
    let unerr = |value: f64| -> f64 {
        if value == 0.0 || (value.abs() < 1_000_000.0 && value.fract() == 0.0) {
            value
        } else {
            format!("{value:.6e}").parse().expect("finite time")
        }
    };
    let rounded_times: Vec<_> = events.iter().map(|(time, _)| unerr(*time)).collect();
    let mut markers = Vec::with_capacity(times.len());
    for &time in times {
        if is_cancelled() {
            return Err(Error::Cancelled);
        }
        let rounded_time = unerr(time);
        let index = rounded_times.partition_point(|event| *event <= rounded_time);
        markers.push((time, if index == 0 { 0.0 } else { events[index - 1].1 }));
    }
    let plot = if let Some([min_time, max_time]) = plot_range {
        let mut points = Vec::new();
        let mut current_value = None;
        let mut current_time = None;
        for &(time, value) in &events[..plot_event_limit.unwrap_or(events.len())] {
            if is_cancelled() {
                return Err(Error::Cancelled);
            }
            let previous_value = current_value;
            current_value = Some(value);
            current_time = Some(time);
            if time < min_time {
                continue;
            }
            if points.is_empty() {
                let initial = previous_value.unwrap_or(0.0);
                points.push((min_time, initial));
                if time > min_time {
                    points.push((time, initial));
                }
                if previous_value != Some(value) {
                    points.push((time, value));
                }
                // Total repair has one activation beyond the plot end. A rate
                // gap can have two visible boundaries from that activation.
                if time > max_time && plot_event_limit.is_none() {
                    break;
                }
                continue;
            }
            if time >= max_time {
                points.push((max_time, previous_value.unwrap_or(0.0)));
                break;
            }
            if previous_value != Some(value) {
                if let Some(previous) = previous_value {
                    points.push((time, previous));
                }
                points.push((time, value));
            }
        }
        if current_time.is_none() {
            points.push((min_time, 0.0));
        }
        if max_time > current_time.unwrap_or(0.0) {
            points.push((max_time, current_value.unwrap_or(0.0)));
        }
        Some(points)
    } else {
        None
    };
    Ok(RemoteRepairTimeGraph { markers, plot })
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

#[cfg(test)]
mod remote_repair_time_tests {
    use std::cell::Cell;

    use super::{
        RemoteRepairCycle, RemoteRepairReload, RemoteRepairSchedule, RemoteRepairTimeError, RemoteRepairTimeSeries,
        remote_repair_schedule_graph, remote_repair_time_graph,
    };

    fn cycle() -> Option<RemoteRepairCycle> {
        Some(RemoteRepairCycle {
            duration_s: 8.0,
            amount_hp: 680.0,
        })
    }

    #[test]
    fn cumulative_marker_rounding_and_singleton_plot_follow_pyfa() {
        let graph = remote_repair_time_graph(
            cycle(),
            1.0,
            RemoteRepairTimeSeries::Total,
            &[7.9999994, 7.9999995],
            Some([5.0, 5.0]),
            &|| false,
        )
        .unwrap();
        assert_eq!(graph.markers, [(7.9999994, 680.0), (7.9999995, 1360.0)]);
        assert_eq!(graph.plot.unwrap(), [(5.0, 680.0), (8.0, 680.0), (8.0, 1360.0)]);
    }

    #[test]
    fn rps_and_offline_plots_preserve_pyfa_empty_and_post_horizon_shapes() {
        let rps = remote_repair_time_graph(
            cycle(),
            1.0,
            RemoteRepairTimeSeries::Rps,
            &[5.0],
            Some([5.0, 10.0]),
            &|| false,
        )
        .unwrap();
        assert_eq!(rps.markers, [(5.0, 85.0)]);
        assert_eq!(rps.plot.unwrap(), [(10.0, 85.0)]);
        let offline = remote_repair_time_graph(
            None,
            0.0,
            RemoteRepairTimeSeries::Total,
            &[0.0],
            Some([0.0, 0.0]),
            &|| false,
        )
        .unwrap();
        assert_eq!(offline.markers, [(0.0, 0.0)]);
        assert_eq!(offline.plot.unwrap(), [(0.0, 0.0)]);
    }

    #[test]
    fn tiny_cycles_are_bounded_and_cancellation_stops_generation() {
        let tiny = Some(RemoteRepairCycle {
            duration_s: 1e-10,
            amount_hp: 1.0,
        });
        assert!(matches!(
            remote_repair_time_graph(tiny, 1.0, RemoteRepairTimeSeries::Total, &[2500.0], None, &|| false),
            Err(RemoteRepairTimeError::PointLimit)
        ));
        let calls = Cell::new(0);
        let result = remote_repair_time_graph(cycle(), 1.0, RemoteRepairTimeSeries::Total, &[], None, &|| {
            calls.set(calls.get() + 1);
            calls.get() == 3
        });
        assert!(matches!(result, Err(RemoteRepairTimeError::Cancelled)));
        assert_eq!(calls.get(), 3);
    }

    #[test]
    fn distant_marker_does_not_extend_the_ancillary_rate_plot_past_its_horizon() {
        let schedule = RemoteRepairSchedule {
            cycle: RemoteRepairCycle {
                duration_s: 8.0,
                amount_hp: 950.0,
            },
            reload: Some(RemoteRepairReload {
                magazine_cycles: 7,
                duration_s: 60.0,
            }),
        };
        let graph = remote_repair_schedule_graph(
            Some(schedule),
            1.0,
            RemoteRepairTimeSeries::Rps,
            &[48.0, 56.0, 64.0, 116.0, 180.0],
            Some([7.999999, 8.000001]),
            &|| false,
        )
        .unwrap();
        assert_eq!(
            graph.markers,
            [(48.0, 118.75), (56.0, 0.0), (64.0, 0.0), (116.0, 118.75), (180.0, 0.0)]
        );
        assert_eq!(graph.plot.unwrap(), [(8.000001, 118.75)]);
        let before_gap = remote_repair_schedule_graph(
            Some(schedule),
            1.0,
            RemoteRepairTimeSeries::Rps,
            &[2500.0],
            Some([48.1, 50.0]),
            &|| false,
        )
        .unwrap();
        assert_eq!(
            before_gap.plot.unwrap(),
            [(48.1, 118.75), (56.0, 118.75), (56.0, 0.0), (50.0, 0.0)]
        );
    }
}
