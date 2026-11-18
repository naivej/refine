use refine_core::pyfa_graph::PassiveChargeSample;

#[test]
#[ignore = "requires the independent pinned Pyfa passive time graph replay"]
fn passive_time_graph_matches_registered_pyfa_plots_and_markers() {
    let file = std::env::var_os("RUFA_PYFA_PASSIVE_TIME_CASES").expect("set the Pyfa passive time fixture");
    let contents = std::fs::read_to_string(file).expect("read the Pyfa passive time fixture");
    let mut count = 0;
    for (index, row) in contents.lines().enumerate() {
        let fields: Vec<_> = row.split('\t').collect();
        assert_eq!(fields.len(), 10);
        let parse = |i: usize| fields[i].parse::<f64>().unwrap();
        let sample = PassiveChargeSample::at(parse(8), parse(5), parse(6), parse(7)).unwrap();
        let actual = if fields[3].ends_with("Amount") {
            sample.amount
        } else {
            sample.recharge_per_s
        };
        let expected = parse(9);
        assert!(
            (actual - expected).abs() <= 1e-8_f64.max(1e-9 * actual.abs().max(expected.abs())),
            "row {}: Pyfa {expected}, Refine {actual}: {row}",
            index + 1
        );
        count += 1;
    }
    assert_eq!(count, 1728);
}

#[test]
fn passive_time_rejects_invalid_inputs_and_preserves_full_charge() {
    for inputs in [
        (-1.0, 100.0, 10.0, 0.0),
        (0.0, 0.0, 10.0, 0.0),
        (0.0, 100.0, 0.0, 0.0),
        (0.0, 100.0, 10.0, -1.0),
        (0.0, 100.0, 10.0, 101.0),
        (f64::NAN, 100.0, 10.0, 0.0),
    ] {
        assert!(PassiveChargeSample::at(inputs.0, inputs.1, inputs.2, inputs.3).is_none());
    }
    let full = PassiveChargeSample::at(300.0, 100.0, 10.0, 100.0).unwrap();
    assert_eq!(full.amount, 100.0);
    assert_eq!(full.recharge_per_s, 0.0);
}
