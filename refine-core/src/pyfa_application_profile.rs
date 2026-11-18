//! Draft bounded Kestrel application-profile calculation; not a published contract.
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
pub(crate) fn pyfa_tick_range(speed: f64, mass: f64, agility: f64, radius: f64, tick: f64) -> f64 {
    let t = tick.min(mass * agility / 1e6);
    (speed / 2.0 * t + speed * (tick - t) - radius).max(0.0)
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
    if !matches!(candidates.len(), 9 | 13 | 25)
        || !matches!(target, (350.0, 38.0) | (60.0, 400.0))
        || !matches!(
            resist,
            [0.0, 0.0, 0.0, 0.0] | [0.6, 0.1, 0.0, 0.9] | [0.0, 0.0, 1.0, 0.0]
        )
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
