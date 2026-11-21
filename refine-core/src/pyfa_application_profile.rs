//! Missile damage and range calculations for the qualified Kestrel application profiles.
use crate::pyfa_graph::pyfa_tick_range;

#[derive(Clone, Debug)]
pub struct Candidate {
    pub name: String,
    pub attrs: [f64; 11],
}
#[derive(Clone, Debug)]
pub struct LoadedModifiers {
    pub damage: [f64; 4],
    pub flight: [f64; 2],
    pub application: [f64; 3],
    pub launcher_damage: f64,
    pub cycle_ms: f64,
    pub ship_radius: f64,
}
#[derive(Clone, Debug)]
pub struct Profile {
    pub ammo: String,
    pub lower: f64,
    pub upper: f64,
    pub chance: f64,
    pub marker_dps: Vec<f64>,
    pub segment: Vec<(f64, f64)>,
}
struct Charge {
    name: String,
    priority: u8,
    lower: f64,
    upper: f64,
    chance: f64,
    raw: f64,
    er: f64,
    ev: f64,
    drf: f64,
}
fn volley(c: &Charge, d: f64, speed: f64, sig: f64) -> f64 {
    let range = if d <= c.lower {
        1.0
    } else if d <= c.upper {
        c.chance
    } else {
        0.0
    };
    let mut app = 1.0_f64;
    if c.er > 0.0 {
        app = app.min(sig / c.er);
        if speed > 0.0 {
            app = app.min(((c.ev * sig) / (c.er * speed)).powf(c.drf));
        }
    }
    c.raw * range * app
}
fn build_charges(
    candidates: &[Candidate],
    loaded: &LoadedModifiers,
    resist: [f64; 4],
) -> Result<Vec<Charge>, &'static str> {
    let mut charges = Vec::with_capacity(candidates.len());
    for c in candidates {
        let a = c.attrs;
        if a.iter().any(|x| !x.is_finite() || *x < 0.0) || a[0] <= 0.0 || a[1] <= 0.0 || a[4] <= 0.0 || a[6] <= 0.0 {
            return Err("invalid candidate attributes");
        }
        let priority = if c.name.contains("Mjolnir") {
            0
        } else if c.name.contains("Inferno") {
            1
        } else if c.name.contains("Scourge") {
            2
        } else if c.name.contains("Nova") {
            3
        } else {
            return Err("unknown candidate damage type");
        };
        let speed = a[0] * loaded.flight[0];
        let duration = a[1] * loaded.flight[1] / 1000.0 + loaded.ship_radius / speed;
        let range = |tick: f64| pyfa_tick_range(speed, a[2], a[3], loaded.ship_radius, tick);
        let raw = (0..4)
            .map(|j| a[7 + j] * loaded.damage[j] * (1.0 - resist[j]))
            .sum::<f64>()
            * loaded.launcher_damage;
        charges.push(Charge {
            name: c.name.clone(),
            priority,
            lower: range(duration.floor()),
            upper: range(duration.ceil()),
            chance: duration.fract(),
            raw,
            er: a[4] * loaded.application[0],
            ev: a[5] * loaded.application[1],
            drf: a[6] * loaded.application[2],
        });
    }
    if charges.iter().any(|c| {
        [c.lower, c.upper, c.chance, c.raw, c.er, c.ev, c.drf]
            .iter()
            .any(|x| !x.is_finite())
    }) {
        return Err("candidate arithmetic overflow");
    }
    Ok(charges)
}

/// Computes only the qualified no-crossover, four-launcher Kestrel slice.
pub fn kestrel_profile(
    candidates: &[Candidate],
    loaded: &LoadedModifiers,
    target: (f64, f64),
    resist: [f64; 4],
    markers: &[f64],
    include_plot: bool,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<Profile, &'static str> {
    if candidates.is_empty()
        || candidates.len() > 25
        || !target.0.is_finite()
        || target.0 < 0.0
        || !target.1.is_finite()
        || target.1 <= 0.0
        || resist.iter().any(|r| !(0.0..=1.0).contains(r))
        || markers.len() > 201
        || markers.iter().any(|x| !x.is_finite() || !(0.0..=100000.0).contains(x))
    {
        return Err("outside qualified Kestrel profile scope");
    }
    let factors = loaded
        .damage
        .into_iter()
        .chain(loaded.flight)
        .chain(loaded.application)
        .chain([loaded.launcher_damage, loaded.cycle_ms, loaded.ship_radius]);
    if factors.into_iter().any(|x| !x.is_finite() || x <= 0.0) {
        return Err("invalid loaded modifiers");
    }
    let charges = build_charges(candidates, loaded, resist)?;
    // This slice is qualified only where every candidate has identical range and
    // application geometry. That guarantees a single winner at all distances.
    let reference = &charges[0];
    if charges.iter().any(|c| {
        c.lower != reference.lower
            || c.upper != reference.upper
            || c.chance != reference.chance
            || c.er != reference.er
            || c.ev != reference.ev
            || c.drf != reference.drf
    }) {
        return Err("unqualified candidate geometry or crossover");
    }
    let mut best = None;
    let mut max = 0.0;
    let mut priority = 99;
    for (j, c) in charges.iter().enumerate() {
        let value = volley(c, 0.0, target.0, target.1);
        if value > max || (value == max && value > 0.0 && c.priority < priority) {
            best = Some(j);
            max = value;
            priority = c.priority;
        }
    }
    let c = &charges[best.ok_or("no positive ammunition")?];
    let sample = |distance: f64| volley(c, distance, target.0, target.1) / (loaded.cycle_ms / 1000.0) * 4.0;
    let mut segment = if include_plot {
        Vec::with_capacity(251)
    } else {
        Vec::new()
    };
    for j in 0..if include_plot { 251 } else { 0 } {
        if is_cancelled() {
            return Err("cancelled");
        }
        let distance = j as f64 * 400.0;
        let selected = volley(c, distance, target.0, target.1);
        if charges
            .iter()
            .any(|other| volley(other, distance, target.0, target.1) > selected + 1e-10)
        {
            return Err("unqualified ammunition crossover");
        }
        segment.push((distance / 1000.0, sample(distance)));
    }
    let mut marker_dps = Vec::with_capacity(markers.len());
    for distance in markers {
        if is_cancelled() {
            return Err("cancelled");
        }
        marker_dps.push(sample(distance / 1000.0 * 1000.0));
    }
    if segment.iter().any(|(_, y)| !y.is_finite()) || marker_dps.iter().any(|y| !y.is_finite()) {
        return Err("profile arithmetic overflow");
    }
    Ok(Profile {
        ammo: c.name.clone(),
        lower: c.lower,
        upper: c.upper,
        chance: c.chance,
        marker_dps,
        segment,
    })
}

/// One sampled ammunition transition for the bounded T2 crossover.
#[derive(Debug)]
pub struct CrossoverTransition {
    pub distance_m: f64,
    pub ammo: String,
    pub single_launcher_volley: f64,
}
#[derive(Debug)]
pub struct CrossoverMarker {
    pub distance_m: f64,
    pub dps: f64,
    pub ammo: String,
}
#[derive(Debug)]
pub struct CrossoverSegment {
    pub ammo: String,
    pub index: usize,
    pub points: Vec<(f64, f64)>,
}
#[derive(Debug)]
pub struct CrossoverProfile {
    pub transitions: Vec<CrossoverTransition>,
    pub markers: Vec<CrossoverMarker>,
    pub segments: Vec<CrossoverSegment>,
}

/// Prepared once for the qualified T2 fit, pilot and fixed target.
/// Retains at most 33 candidate records and two transitions.
pub struct PreparedT2Crossover {
    charges: Vec<Charge>,
    changes: Vec<(f64, usize, f64)>,
    cycle_ms: f64,
}

/// Resolve candidate bins and Pyfa's bounded sampled transitions once.
pub fn prepare_kestrel_t2_crossover(
    candidates: &[Candidate],
    loaded: &LoadedModifiers,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<PreparedT2Crossover, &'static str> {
    if candidates.is_empty() || candidates.len() > 33 {
        return Err("outside qualified T2 crossover scope");
    }
    if is_cancelled() {
        return Err("cancelled");
    }
    if loaded
        .damage
        .into_iter()
        .chain(loaded.flight)
        .chain(loaded.application)
        .chain([loaded.launcher_damage, loaded.cycle_ms, loaded.ship_radius])
        .any(|x| !x.is_finite() || x <= 0.0)
    {
        return Err("invalid loaded modifiers");
    }
    let mut charges = build_charges(candidates, loaded, [0.0; 4])?;
    // Pyfa sorts by descending higher range and then descending unresisted DPS.
    charges.sort_by(|a, b| {
        b.upper
            .total_cmp(&a.upper)
            .then_with(|| (b.raw / (loaded.cycle_ms / 1000.0)).total_cmp(&(a.raw / (loaded.cycle_ms / 1000.0))))
    });
    let scan_max = charges.first().ok_or("no candidates")?.upper.floor();
    if !scan_max.is_finite() || !(1.0..=300000.0).contains(&scan_max) {
        return Err("unqualified scan range");
    }
    let scan_step = if scan_max / 300.0 <= 100.0 {
        100.0
    } else {
        (scan_max / 30000.0).ceil() * 100.0
    };
    let best = |distance: f64| -> (f64, Option<usize>) {
        let mut value = 0.0;
        let mut selected = None;
        let mut priority = 99;
        for (i, c) in charges.iter().enumerate() {
            let v = volley(c, distance, 60.0, 400.0);
            if v > value || (v == value && v > 0.0 && c.priority < priority) {
                value = v;
                selected = Some(i);
                priority = c.priority;
            }
        }
        (value, selected)
    };
    let (initial, first) = best(0.0);
    let first = first.ok_or("no positive ammunition")?;
    let mut changes = vec![(0.0, first, initial)];
    let mut current = Some(first);
    let mut d = scan_step;
    while d <= scan_max {
        if is_cancelled() {
            return Err("cancelled");
        }
        let (mut value, next) = best(d);
        if next.map(|i| charges[i].name.as_str()) != current.map(|i| charges[i].name.as_str()) {
            let mut low = d - scan_step;
            let mut high = d;
            while high - low > 10.0 {
                if is_cancelled() {
                    return Err("cancelled");
                }
                let mid = ((low + high) / 2.0).floor();
                let (_, chosen) = best(mid);
                if chosen.map(|i| charges[i].name.as_str()) == current.map(|i| charges[i].name.as_str()) {
                    low = mid;
                } else {
                    high = mid;
                }
            }
            value = best(high).0;
            if let Some(i) = next {
                changes.push((high, i, value));
                if changes.len() > 2 {
                    return Err("outside qualified T2 crossover configuration");
                }
            } else {
                return Err("unqualified terminal transition");
            }
            current = next;
        }
        if value < 0.01 {
            return Err("unqualified terminal transition");
        }
        d += scan_step;
    }
    if changes.len() != 2 {
        return Err("outside qualified T2 crossover configuration");
    }
    Ok(PreparedT2Crossover {
        charges,
        changes,
        cycle_ms: loaded.cycle_ms,
    })
}

/// Sample the prepared T2 crossover without repeating transition discovery.
pub fn sample_kestrel_t2_crossover(
    prepared: &PreparedT2Crossover,
    markers: &[f64],
    include_plot: bool,
    is_cancelled: &dyn Fn() -> bool,
) -> Result<CrossoverProfile, &'static str> {
    if markers.is_empty()
        || markers.len() > 201
        || markers.iter().any(|x| !x.is_finite() || !(0.0..=100000.0).contains(x))
        || markers.windows(2).any(|w| w[0] >= w[1])
    {
        return Err("outside qualified T2 crossover scope");
    }
    if is_cancelled() {
        return Err("cancelled");
    }
    let charges = &prepared.charges;
    let changes = &prepared.changes;
    let cycle_ms = prepared.cycle_ms;
    let query = |distance: f64| {
        let chosen = if distance >= changes[1].0 {
            changes[1].1
        } else {
            changes[0].1
        };
        let c = &charges[chosen];
        (volley(c, distance, 60.0, 400.0) / (cycle_ms / 1000.0) * 4.0, chosen)
    };
    let mut result_markers = Vec::with_capacity(markers.len());
    for &distance in markers {
        if is_cancelled() {
            return Err("cancelled");
        }
        let (dps, chosen) = query(distance / 1000.0 * 1000.0);
        result_markers.push(CrossoverMarker {
            distance_m: distance,
            dps,
            ammo: charges[chosen].name.clone(),
        });
    }
    let mut segments = Vec::new();
    if include_plot {
        for i in 0..2 {
            let start = changes[i].0;
            let end = if i == 0 { changes[1].0 } else { 100000.0 };
            if start >= end {
                return Err("unqualified segment geometry");
            }
            let mut points = Vec::new();
            let mut x = start;
            while x <= end {
                if is_cancelled() {
                    return Err("cancelled");
                }
                points.push((x / 1000.0, query(x).0));
                x += 400.0;
            }
            if points.last().is_none_or(|(x, _)| x * 1000.0 < end) {
                points.push((end / 1000.0, query(end).0));
            }
            segments.push(CrossoverSegment {
                ammo: charges[changes[i].1].name.clone(),
                index: i,
                points,
            });
        }
    }
    Ok(CrossoverProfile {
        transitions: changes
            .iter()
            .map(|(d, i, v)| CrossoverTransition {
                distance_m: *d,
                ammo: charges[*i].name.clone(),
                single_launcher_volley: *v,
            })
            .collect(),
        markers: result_markers,
        segments,
    })
}

#[cfg(test)]
mod application_profile_tests {
    use std::cell::Cell;

    use super::*;

    fn inputs() -> (Vec<Candidate>, LoadedModifiers) {
        let candidate = Candidate {
            name: "Mjolnir Light Missile".to_owned(),
            attrs: [
                3750.0,
                5000.0,
                700.0,
                0.000144498003785,
                40.0,
                170.0,
                0.604,
                83.0,
                0.0,
                0.0,
                0.0,
            ],
        };
        (
            vec![candidate; 9],
            LoadedModifiers {
                damage: [1.8390625; 4],
                flight: [2.25, 1.5],
                application: [0.75, 1.5, 1.0],
                launcher_damage: 1.0,
                cycle_ms: 11260.8,
                ship_radius: 45.98,
            },
        )
    }

    #[test]
    fn actual_profile_uses_unrounded_flight_ticks_and_inclusive_cutoffs() {
        let (candidates, loaded) = inputs();
        let target = (350.0, 38.0);
        let baseline = kestrel_profile(&candidates, &loaded, target, [0.0; 4], &[0.0], false, &|| false).unwrap();
        assert!((baseline.lower - 59016.51957327933).abs() < 1e-9);
        assert!((baseline.upper - 67454.01957327934).abs() < 1e-9);
        assert!((baseline.chance - 0.5054494814814818).abs() < 1e-14);
        let points = [
            baseline.lower - 0.001,
            baseline.lower,
            baseline.lower + 0.001,
            baseline.upper - 0.001,
            baseline.upper,
            baseline.upper + 0.001,
        ];
        let result = kestrel_profile(&candidates, &loaded, target, [0.0; 4], &points, false, &|| false).unwrap();
        let full = baseline.marker_dps[0];
        assert_eq!(result.marker_dps[0], full);
        assert_eq!(result.marker_dps[1], full);
        for value in &result.marker_dps[2..5] {
            assert!((*value - full * baseline.chance).abs() < 1e-12);
        }
        assert_eq!(result.marker_dps[5], 0.0);
    }

    #[test]
    fn cancellation_interrupts_each_marker_and_plot_without_allocating_marker_only_plot() {
        let (candidates, loaded) = inputs();
        let (target, resist) = ((350.0, 38.0), [0.0; 4]);
        let marker_checks = Cell::new(0);
        let marker_cancel = || {
            marker_checks.set(marker_checks.get() + 1);
            marker_checks.get() == 2
        };
        assert_eq!(
            kestrel_profile(
                &candidates,
                &loaded,
                target,
                resist,
                &[0.0, 1000.0],
                false,
                &marker_cancel
            )
            .unwrap_err(),
            "cancelled"
        );
        assert_eq!(marker_checks.get(), 2);
        let plot_checks = Cell::new(0);
        let plot_cancel = || {
            plot_checks.set(plot_checks.get() + 1);
            plot_checks.get() == 3
        };
        assert_eq!(
            kestrel_profile(&candidates, &loaded, target, resist, &[0.0], true, &plot_cancel).unwrap_err(),
            "cancelled"
        );
        assert_eq!(plot_checks.get(), 3);
        let marker_only = kestrel_profile(&candidates, &loaded, target, resist, &[0.0], false, &|| false).unwrap();
        assert!(marker_only.segment.is_empty());
        assert_eq!(marker_only.segment.capacity(), 0);
    }
}
