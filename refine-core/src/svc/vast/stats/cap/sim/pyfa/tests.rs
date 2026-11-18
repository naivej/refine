use super::*;

mod fit_stats;

fn cycle(duration_ms: u64, cap_need: f64) -> PyfaCycle {
    PyfaCycle {
        duration_ms,
        cap_need: Value::from_f64_unchecked(cap_need),
        clip_size: 0,
        reload_ms: PValue::ZERO,
        injector: false,
        stagger: true,
    }
}

fn request(start_cap_gj: f64, end_time_s: f64) -> StatCapSimTraceRequest {
    StatCapSimTraceRequest {
        start_cap_gj,
        end_time_s,
    }
}

fn simulate(cycles: Vec<PyfaCycle>, trace: StatCapSimTraceRequest) -> StatCapSimPyfa {
    PyfaEvents::simulate(cycles, PValue::from_f64_unchecked(100.0), 5000.0, Some(trace)).unwrap()
}

fn assert_close(actual: f64, expected: f64) {
    let tolerance = 1e-8_f64.max(1e-9 * actual.abs().max(expected.abs()));
    assert!((actual - expected).abs() <= tolerance, "{actual} != {expected}");
}

#[test]
fn kestrel_summary_stops_at_repeat_but_requested_trace_continues() {
    let cycles = vec![cycle(7500, 5.0), cycle(2000, 18.0)];
    for (end, count, last) in [
        (60.0, 37, (60.0, 140.79393381286314)),
        (3600.0, 2160, (3598.0, 120.62845074273565)),
    ] {
        let result = PyfaEvents::simulate(
            cycles.clone(),
            PValue::from_f64_unchecked(412.5),
            105187.5,
            Some(request(200.0, end)),
        )
        .unwrap();
        let StatCapSim::Stable(fraction) = result.summary else {
            panic!("Kestrel should be stable")
        };
        assert_close(fraction.into_f64(), 0.29526422200921043);
        let trace = result.trace.unwrap();
        assert_eq!(trace.len(), count);
        assert_eq!(trace[0], (0.0, 177.0));
        assert_close(trace[1].1, 176.30965274966445);
        assert_eq!(trace[count - 1].0, last.0);
        assert_close(trace[count - 1].1, last.1);
    }
    let full_trace = PyfaEvents::simulate(
        cycles,
        PValue::from_f64_unchecked(412.5),
        105187.5,
        Some(request(412.5, 3600.0)),
    )
    .unwrap()
    .trace
    .unwrap();
    assert_eq!(full_trace.len(), 2160);
    assert_eq!(full_trace[396].0, 660.0);
    assert_close(full_trace[396].1, 117.01127445638815);
    assert_eq!(full_trace[2159].0, 3598.0);
    assert_close(full_trace[2159].1, 120.62845074273565);
}

#[test]
fn equal_time_activations_stop_at_depletion_before_slower_transfer() {
    let result = simulate(
        vec![cycle(3000, -50.0), cycle(2000, 15.0), cycle(1000, 90.0)],
        request(100.0, 5.0),
    );
    assert!(matches!(result.summary, StatCapSim::Time(time) if time.into_f64() == 0.0));
    assert_eq!(result.trace.unwrap(), [(0.0, 0.0)]);
}

#[test]
fn zero_capacitor_does_not_stop_activations() {
    let result = simulate(vec![cycle(1000, 100.0)], request(100.0, 5.0));
    assert!(matches!(result.summary, StatCapSim::Time(time) if time.into_f64() == 1.0));
    assert_eq!(result.trace.unwrap(), [(0.0, 0.0), (1.0, 0.0)]);
    let initially_empty = simulate(vec![cycle(1000, 100.0)], request(0.0, 5.0));
    assert_eq!(initially_empty.trace.unwrap(), [(0.0, 0.0)]);
}

#[test]
fn no_cap_changes_are_stable_and_have_no_trace_points() {
    let result = simulate(vec![], request(0.0, 60.0));
    assert!(matches!(result.summary, StatCapSim::Stable(fraction) if fraction == UnitInterval::ONE));
    assert_eq!(result.trace.unwrap(), []);
}

#[test]
fn identical_sources_stagger_by_reload_cycle_or_truncated_interval() {
    let base = cycle(5001, 8.0);
    let reloading = PyfaCycle {
        clip_size: 3,
        reload_ms: PValue::from_f64_unchecked(2500.0),
        ..base
    };
    let trace = simulate(vec![reloading; 2], request(50.0, 18.0)).trace.unwrap();
    let expected = [
        (0.0, 42.0),
        (2.9171666666666667, 88.22932771232125),
        (5.001, 90.4949721453525),
        (7.918166666666666, 91.47376461774405),
        (10.002, 90.9181956181836),
        (12.919166666666666, 91.49773757941723),
        (17.503, 91.91123436981758),
    ];
    assert_eq!(trace.len(), expected.len());
    for ((time, cap), (expected_time, expected_cap)) in trace.into_iter().zip(expected) {
        assert_close(time, expected_time);
        assert_close(cap, expected_cap);
    }
    let trace = simulate(vec![base; 2], request(50.0, 5.0)).trace.unwrap();
    assert_eq!(trace.len(), 3);
    assert_eq!(trace[1].0, 2.5);
    assert_close(trace[1].1, 86.3058828284897);
}

#[test]
fn injector_comes_available_after_forced_reload() {
    let injector = PyfaCycle {
        clip_size: 1,
        reload_ms: PValue::from_f64_unchecked(3000.0),
        injector: true,
        ..cycle(2000, -30.0)
    };
    let result = simulate(vec![injector, cycle(2000, 22.0)], request(50.0, 18.0));
    let trace = result.trace.unwrap();
    assert_eq!(trace.len(), 10);
    assert_eq!(trace[0], (0.0, 58.0));
    assert_close(trace[1].1, 71.6507155027441);
    assert_close(trace[9].1, 74.29875621947784);
}

#[test]
fn full_start_uses_the_summary_history_when_repeats_are_disabled() {
    // Measured through the pinned Fit API with Kestrel 602, modules 399/377/1195,
    // and Small Capacitor Booster II 3568 fitted with Cap Booster 100 3554.
    let injector = PyfaCycle {
        clip_size: 3,
        reload_ms: PValue::from_f64_unchecked(10000.0),
        injector: true,
        ..cycle(12000, -100.0)
    };
    let cycles = vec![cycle(2000, 18.0), injector];
    let capacity = PValue::from_f64_unchecked(412.5);
    let full = PyfaEvents::simulate(cycles.clone(), capacity, 105187.5, Some(request(412.5, 3600.0))).unwrap();
    let StatCapSim::Stable(fraction) = full.summary else {
        panic!("Kestrel should be stable");
    };
    assert_close(fraction.into_f64(), 0.6942981136195327);
    let full = full.trace.unwrap();
    assert_eq!(full.len(), 1801);
    assert_eq!(full[1800].0, 3600.0);
    assert_close(full[1800].1, 377.5474363120808);
    let partial = PyfaEvents::simulate(cycles, capacity, 105187.5, Some(request(412.49, 3600.0)))
        .unwrap()
        .trace
        .unwrap();
    assert_eq!(partial.len(), 1800);
    assert_eq!(partial[1799].0, 3598.0);
    assert_close(partial[1799].1, 393.8761960083696);
}

#[test]
fn railgun_capacitor_reload_matches_pinned_pyfa() {
    // Merlin 603, All 5, 125mm Railgun I with Iron Charge S; the pinned
    // fixture records 80 shots per clip and a 5 second reload.
    let gun = PyfaCycle {
        clip_size: 80,
        reload_ms: PValue::from_f64_unchecked(5000.0),
        stagger: false,
        ..cycle(2340, 0.7901249999999999)
    };
    let result = PyfaEvents::simulate(
        vec![gun],
        PValue::from_f64_unchecked(437.5),
        131250.0,
        Some(request(300.0, 600.0)),
    )
    .unwrap();
    assert!(
        matches!(result.summary, StatCapSim::Stable(fraction) if (fraction.into_f64() - 0.9796254542275523).abs() < 1e-9)
    );
    let trace = result.trace.unwrap();
    assert_eq!(trace.len(), 251);
    assert_close(trace[79].0, 184.86);
    assert_close(trace[80].0, 192.2);
    assert_close(trace[trace.len() - 1].0, 600.0);
    assert_close(trace[trace.len() - 1].1, 428.79889507895837);
}

#[test]
fn simultaneous_local_and_projected_cycles_match_pinned_pyfa() {
    // Pinned Kestrel with shield booster, incoming small neut and cap transfer.
    let result = PyfaEvents::simulate(
        vec![cycle(2000, 18.0), cycle(6000, 45.0), cycle(5000, -30.0)],
        PValue::from_f64_unchecked(412.5),
        123750.0,
        Some(request(300.0, 600.0)),
    )
    .unwrap();
    assert!(matches!(result.summary, StatCapSim::Time(time) if time.into_f64() == 100.0));
    let trace = result.trace.unwrap();
    let first = [
        (0.0, 267.0),
        (2.0, 259.16655816518715),
        (4.0, 251.7997565604648),
        (5.0, 287.4129129843657),
        (6.0, 228.93765338372015),
    ];
    for ((time, cap), (expected_time, expected_cap)) in trace.iter().copied().zip(first) {
        assert_close(time, expected_time);
        assert_close(cap, expected_cap);
    }
    assert_eq!(trace.last().unwrap().0, 84.0);
    assert_eq!(trace.last().unwrap().1, 0.0);
}

#[test]
fn invalid_trace_bounds_are_rejected_and_records_are_bounded() {
    let capacity = PValue::from_f64_unchecked(100.0);
    for start in [-1.0, 101.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            request(start, 0.0).validate(capacity),
            Err(StatCapSimPyfaError::StartCharge)
        ));
    }
    for end in [-1.0, 3601.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            request(100.0, end).validate(capacity),
            Err(StatCapSimPyfaError::TraceDuration)
        ));
    }
    assert!(matches!(
        PyfaEvents::simulate(vec![cycle(1, -1.0)], capacity, 5000.0, Some(request(99.0, 3600.0))),
        Err(StatCapSimPyfaError::TraceLimit)
    ));
    let unoptimizable = PyfaCycle {
        clip_size: 1,
        ..cycle(1, -1.0)
    };
    assert!(matches!(
        PyfaEvents::simulate(vec![unoptimizable], capacity, 5000.0, None),
        Err(StatCapSimPyfaError::WorkLimit)
    ));
}
