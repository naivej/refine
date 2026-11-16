//! Capacitor event rules from Pyfa v2.70.0 (GPLv3), e5483f8666ed2eda99c98985c59a29b07e3bb844.
//! See `eos/capSim.py` and `Fit.simulateCap` / `Fit.getCapSimData` in `eos/saveddata/fit.py`.

use std::{cmp::Reverse, collections::BinaryHeap};

use num_integer::Integer;

use super::sim::StatCapSim;
use crate::{PValue, UnitInterval, Value, svc::vast::stats::shared::regenerate, util::RMap};

#[cfg(test)]
mod tests;

const SUMMARY_LIMIT_MS: u64 = 21_600_000;
const TRACE_LIMIT_S: f64 = 3600.0;
const MAX_EVENT_WORK: usize = 1_000_000;
const MAX_TRACE_POINTS: usize = 100_000;

/// Raw capacitor trace options for the Pyfa compatibility trial.
#[derive(Copy, Clone, Debug)]
pub struct StatCapSimTraceRequest {
    /// Starting charge in GJ, from zero through the ship's capacitor capacity.
    pub start_cap_gj: f64,
    /// Inclusive event cutoff in seconds, from zero through 3600.
    pub end_time_s: f64,
}

pub struct StatCapSimPyfa {
    /// Stability estimate or time to depletion, evaluated from full capacitor.
    pub summary: StatCapSim,
    /// Requested post-activation events as (seconds, GJ), if requested.
    pub trace: Option<Vec<(f64, f64)>>,
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum StatCapSimPyfaError {
    #[error("trace starting charge must be finite and between zero and ship capacity")]
    StartCharge,
    #[error("trace end time must be finite and between zero and 3600 seconds")]
    TraceDuration,
    #[error("capacitor capacity and recharge duration must be positive for recharge between events")]
    Recharge,
    #[error("unsupported capacitor activation schedule")]
    Schedule,
    #[error("pinned Pyfa cannot select an injector when all waiting injectors are below the required amount")]
    InjectorShortage,
    #[error("capacitor simulation exceeded its work limit")]
    WorkLimit,
    #[error("capacitor trace exceeded 100000 event times")]
    TraceLimit,
}

impl StatCapSimTraceRequest {
    pub(super) fn validate(self, capacity: PValue) -> Result<(), StatCapSimPyfaError> {
        if !(0.0..=capacity.into_f64()).contains(&self.start_cap_gj) || !self.start_cap_gj.is_finite() {
            return Err(StatCapSimPyfaError::StartCharge);
        }
        if !(0.0..=TRACE_LIMIT_S).contains(&self.end_time_s) {
            return Err(StatCapSimPyfaError::TraceDuration);
        }
        Ok(())
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub(super) struct PyfaCycle {
    pub(super) duration_ms: u64,
    pub(super) cap_need: Value,
    pub(super) clip_size: u32,
    pub(super) reload_ms: PValue,
    pub(super) injector: bool,
    pub(super) stagger: bool,
}

// Derived field order matches the pinned simulator's priority for simultaneous activations.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
struct Activation {
    duration_ms: u64,
    cap_need: Value,
    shot: u32,
    clip_size: u32,
    reload_ms: PValue,
    injector: bool,
}
impl From<PyfaCycle> for Activation {
    fn from(cycle: PyfaCycle) -> Self {
        Self {
            duration_ms: cycle.duration_ms,
            cap_need: cycle.cap_need,
            shot: 0,
            clip_size: cycle.clip_size,
            reload_ms: cycle.reload_ms,
            injector: cycle.injector,
        }
    }
}

type EventQueue = BinaryHeap<Reverse<(PValue, Activation)>>;

pub(super) struct PyfaEvents {
    queue: EventQueue,
    period_ms: Option<u64>,
}

impl PyfaEvents {
    fn new(cycles: Vec<PyfaCycle>) -> Result<Self, StatCapSimPyfaError> {
        let mut groups = RMap::<PyfaCycle, u32>::new();
        for cycle in cycles {
            *groups.entry(cycle).or_default() += 1;
        }
        let mut queue = EventQueue::new();
        let mut period_ms = Some(1_u64);
        for (cycle, count) in groups {
            let mut activation = Activation::from(cycle);
            if cycle.clip_size != 0 {
                period_ms = None;
            }
            if cycle.injector {
                queue.extend((0..count).map(|_| Reverse((PValue::ZERO, activation))));
                continue;
            }
            if cycle.stagger && cycle.clip_size == 0 {
                activation.duration_ms /= u64::from(count);
            } else if cycle.stagger {
                let clip = f64::from(cycle.clip_size);
                let offset = (cycle.duration_ms as f64 * clip + cycle.reload_ms.into_f64()) / (f64::from(count) * clip);
                for index in 1..count {
                    queue.push(Reverse((
                        PValue::from_f64_unchecked(f64::from(index) * offset),
                        activation,
                    )));
                }
            } else {
                activation.cap_need *= Value::from_f64_unchecked(f64::from(count));
            }
            if activation.duration_ms == 0 || !activation.cap_need.is_finite() {
                return Err(StatCapSimPyfaError::Schedule);
            }
            period_ms = period_ms.and_then(|period| {
                let duration = activation.duration_ms;
                (period / period.gcd(&duration))
                    .checked_mul(duration)
                    .filter(|lcm| *lcm < SUMMARY_LIMIT_MS)
            });
            queue.push(Reverse((PValue::ZERO, activation)));
        }
        Ok(Self { queue, period_ms })
    }

    pub(super) fn simulate(
        cycles: Vec<PyfaCycle>,
        capacity: PValue,
        recharge_ms: f64,
        trace: Option<StatCapSimTraceRequest>,
    ) -> Result<StatCapSimPyfa, StatCapSimPyfaError> {
        let Self { queue, period_ms } = Self::new(cycles)?;
        if queue.is_empty() {
            return Ok(StatCapSimPyfa {
                summary: StatCapSim::Stable(UnitInterval::ONE),
                trace: trace.map(|_| Vec::new()),
            });
        }
        let mut remaining_work = MAX_EVENT_WORK;
        let full_start = trace.filter(|request| request.start_cap_gj == capacity.into_f64());
        let trace_queue = (trace.is_some() && full_start.is_none()).then(|| queue.clone());
        let summary_run = Simulation::new(
            queue,
            capacity,
            capacity.into_f64(),
            recharge_ms,
            full_start.map(|request| request.end_time_s),
        );
        let summary_run = summary_run.run(SUMMARY_LIMIT_MS as f64, period_ms, false, &mut remaining_work)?;
        let summary = summary_run.result(capacity);
        let trace = match trace {
            Some(request) if let Some(queue) = trace_queue => {
                let run = Simulation::new(
                    queue,
                    capacity,
                    request.start_cap_gj,
                    recharge_ms,
                    Some(request.end_time_s),
                );
                Some(run.run(TRACE_LIMIT_S * 1000.0, None, true, &mut remaining_work)?.trace)
            }
            Some(_) => Some(summary_run.finish_full_start_trace(&mut remaining_work)?),
            None => None,
        };
        Ok(StatCapSimPyfa { summary, trace })
    }
}

struct Simulation {
    queue: EventQueue,
    waiting: Vec<Activation>,
    capacity: PValue,
    tau_ms: f64,
    cap: f64,
    lowest_before: f64,
    lowest_after: f64,
    last_ms: f64,
    record_through_s: Option<f64>,
    trace: Vec<(f64, f64)>,
    repeat_boundary: Option<(PValue, Activation)>,
    last_trace_ms: Option<f64>,
}
impl Simulation {
    fn new(
        queue: EventQueue,
        capacity: PValue,
        starting_cap: f64,
        recharge_ms: f64,
        record_through_s: Option<f64>,
    ) -> Self {
        Self {
            queue,
            waiting: Vec::new(),
            capacity,
            tau_ms: recharge_ms / 5.0,
            cap: starting_cap,
            lowest_before: starting_cap,
            lowest_after: starting_cap,
            last_ms: 0.0,
            record_through_s,
            trace: Vec::new(),
            repeat_boundary: None,
            last_trace_ms: None,
        }
    }

    // The event semantics and watermarks follow Pyfa v2.70.0 (e5483f8), eos/capSim.py.
    fn run(
        mut self,
        time_limit_ms: f64,
        period_ms: Option<u64>,
        stop_at_trace_end: bool,
        work: &mut usize,
    ) -> Result<Self, StatCapSimPyfaError> {
        let period_ms = period_ms.map(|period| period as f64);
        let mut repeat_at = period_ms.unwrap_or(f64::INFINITY);
        let mut previous_repeat_cap = self.cap;
        let mut previous_waiting = Vec::new();
        let mut waiting_at_repeat = Vec::new();
        while let Some(Reverse((event_time, activation))) = self.queue.pop() {
            let now = event_time.into_f64();
            if now >= time_limit_ms
                || self
                    .record_through_s
                    .is_some_and(|end| stop_at_trace_end && now / 1000.0 > end)
            {
                break;
            }
            spend(work, 1)?;
            if now > self.last_ms {
                if self.capacity == PValue::ZERO || !self.tau_ms.is_finite() || self.tau_ms <= 0.0 {
                    return Err(StatCapSimPyfaError::Recharge);
                }
                self.cap = regenerate(
                    PValue::from_f64_unchecked(self.cap),
                    self.capacity,
                    PValue::from_f64_unchecked(((self.last_ms - now) / self.tau_ms).exp()),
                )
                .into_f64();
                self.lowest_before = self.lowest_before.min(self.cap);
                if now == repeat_at {
                    spend(work, self.waiting.len() * (self.waiting.len() + 1).ilog2() as usize)?;
                    waiting_at_repeat.extend_from_slice(&self.waiting);
                    waiting_at_repeat.sort_unstable();
                    if self.cap >= previous_repeat_cap && waiting_at_repeat == previous_waiting {
                        self.repeat_boundary = Some((event_time, activation));
                        break;
                    }
                    previous_repeat_cap = round_tenth(self.cap);
                    std::mem::swap(&mut previous_waiting, &mut waiting_at_repeat);
                    waiting_at_repeat.clear();
                    repeat_at += period_ms.unwrap();
                }
            }
            self.last_ms = now;
            let need = activation.cap_need.into_f64();
            if activation.injector && self.cap - need > self.capacity.into_f64() {
                self.waiting.push(activation);
                continue;
            }
            self.activate_waiting(Some(need), work)?;
            self.apply(activation);
            if self.cap < 0.0 {
                self.record()?;
                break;
            }
            self.lowest_after = self.lowest_after.min(self.cap);
            self.activate_waiting(None, work)?;
            self.record()?;
            self.reschedule(activation);
        }
        Ok(self)
    }

    fn apply(&mut self, activation: Activation) {
        self.cap = (self.cap - activation.cap_need.into_f64()).min(self.capacity.into_f64());
    }

    fn record(&mut self) -> Result<(), StatCapSimPyfaError> {
        let Some(end) = self.record_through_s else {
            return Ok(());
        };
        let seconds = self.last_ms / 1000.0;
        if seconds <= end {
            let remaining = self.cap.max(0.0);
            if self.last_trace_ms == Some(self.last_ms) {
                self.trace.last_mut().unwrap().1 = remaining;
            } else {
                if self.trace.len() >= MAX_TRACE_POINTS {
                    return Err(StatCapSimPyfaError::TraceLimit);
                }
                self.trace.push((seconds, remaining));
                self.last_trace_ms = Some(self.last_ms);
            }
        }
        Ok(())
    }

    fn activate_waiting(&mut self, emergency_cost: Option<f64>, work: &mut usize) -> Result<(), StatCapSimPyfaError> {
        while !self.waiting.is_empty() && self.cap < self.capacity.into_f64() {
            let room = self.capacity.into_f64() - self.cap;
            let (threshold, emergency) = match emergency_cost {
                Some(cost) if cost > self.cap => ((cost - self.cap).min(room), true),
                Some(_) => break,
                None => (room, false),
            };
            spend(work, self.waiting.len())?;
            let mut selected = None;
            for (index, candidate) in self.waiting.iter().enumerate() {
                let amount = -candidate.cap_need.into_f64();
                let eligible = if emergency {
                    amount >= threshold
                } else {
                    amount <= threshold
                };
                if eligible && selected.is_none_or(|(_, best)| if emergency { amount < best } else { amount > best }) {
                    selected = Some((index, amount));
                }
            }
            let Some((index, _)) = selected else {
                if emergency {
                    return Err(StatCapSimPyfaError::InjectorShortage);
                }
                break;
            };
            let activation = self.waiting.remove(index);
            self.apply(activation);
            self.reschedule(activation);
        }
        Ok(())
    }

    fn reschedule(&mut self, mut activation: Activation) {
        let mut next = self.last_ms + activation.duration_ms as f64;
        activation.shot += 1;
        if activation.shot == activation.clip_size {
            next += activation.reload_ms.into_f64();
            activation.shot = 0;
        }
        self.queue.push(Reverse((PValue::from_f64_unchecked(next), activation)));
    }

    fn result(&self, capacity: PValue) -> StatCapSim {
        if self.cap > 0.0 {
            let fraction = (self.lowest_before + self.lowest_after) / (2.0 * capacity.into_f64());
            if fraction > 0.0 {
                return StatCapSim::Stable(UnitInterval::from_f64_clamped(fraction));
            }
        }
        StatCapSim::Time(PValue::from_f64_unchecked(self.last_ms / 1000.0))
    }

    fn finish_full_start_trace(mut self, work: &mut usize) -> Result<Vec<(f64, f64)>, StatCapSimPyfaError> {
        let Some((time, activation)) = self.repeat_boundary else {
            // Pyfa keeps the summary's history when the six-hour run finishes without early repeat
            // detection.
            return Ok(self.trace);
        };
        // Pyfa runs a one-hour graph simulation after repeat detection. The prefix is the same up
        // to the repeat boundary; process the pending activation with repeat detection disabled.
        let trace_limit_ms = TRACE_LIMIT_S * 1000.0;
        if self.last_trace_ms.is_some_and(|recorded| recorded >= trace_limit_ms) {
            self.trace.pop();
        }
        if time.into_f64() < trace_limit_ms {
            self.last_ms = time.into_f64();
            self.queue.push(Reverse((time, activation)));
            return Ok(self.run(trace_limit_ms, None, true, work)?.trace);
        }
        Ok(self.trace)
    }
}

fn spend(work: &mut usize, amount: usize) -> Result<(), StatCapSimPyfaError> {
    *work = work.checked_sub(amount).ok_or(StatCapSimPyfaError::WorkLimit)?;
    Ok(())
}

fn round_tenth(value: f64) -> f64 {
    let scaled = value * 10.0;
    if !scaled.is_finite() {
        return value;
    }
    // Correct for binary multiplication rounding to a tie when the exact product differs.
    let rounded = if scaled.fract() == 0.5 {
        let residual = value.mul_add(10.0, -scaled);
        if residual < 0.0 {
            scaled.floor()
        } else if residual > 0.0 {
            scaled.ceil()
        } else {
            scaled.round_ties_even()
        }
    } else {
        scaled.round_ties_even()
    };
    rounded / 10.0
}
