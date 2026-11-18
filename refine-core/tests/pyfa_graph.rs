use refine_core::pyfa_graph::{BumpSample, EcmBurstScenario, MotionSample, lock_time_s, warp_time_s};

#[test]
#[ignore = "requires the independent pinned Pyfa motion graph replay"]
fn graph_primitives_match_pinned_pyfa_kinematics() {
    let file = std::env::var_os("RUFA_PYFA_MOTION_GRAPH_CASES").expect("set the Pyfa motion graph fixture");
    let contents = std::fs::read_to_string(file).expect("read the Pyfa motion graph fixture");
    let mut counts = [0; 4];
    for (index, row) in contents.lines().enumerate() {
        let (kind, fields) = row.split_once('\t').expect("fixture row kind");
        let fields: Vec<f64> = fields.split('\t').map(|field| field.parse().unwrap()).collect();
        let compare = |actual: f64, expected: f64| {
            assert!(
                (actual - expected).abs() <= 1e-8_f64.max(1e-9 * actual.abs().max(expected.abs())),
                "row {} {kind}: Pyfa {expected}, Refine {actual}",
                index + 1,
            );
        };
        match (kind, fields.as_slice()) {
            (
                "motion",
                &[
                    time,
                    speed,
                    mass,
                    inertia,
                    target_mass,
                    target_inertia,
                    distance,
                    sped,
                    momentum,
                    bump_speed,
                    bump_distance,
                ],
            ) => {
                let sample = MotionSample::at(time, speed, mass, inertia).unwrap();
                compare(sample.distance_m, distance);
                compare(sample.speed_m_per_s, sped);
                compare(sample.momentum_kg_m_per_s, momentum);
                let bump = BumpSample::at(sample.speed_m_per_s, mass, target_mass, target_inertia).unwrap();
                compare(bump.target_speed_m_per_s, bump_speed);
                compare(bump.target_distance_m, bump_distance);
                counts[0] += 1;
            }
            ("lock", &[scan_res, fallback, signature, expected]) => {
                compare(lock_time_s(scan_res, fallback, signature).unwrap(), expected);
                counts[1] += 1;
            }
            ("warp", &[distance, warp_speed, subwarp_speed, expected]) => {
                compare(warp_time_s(distance, warp_speed, subwarp_speed).unwrap(), expected);
                counts[2] += 1;
            }
            (
                "ecm",
                &[
                    signature,
                    ehp,
                    weapon,
                    drone,
                    scan_res,
                    scan_mult,
                    opponent_dps,
                    adjustment,
                    max_cycles,
                    lock,
                    uptime,
                    damage,
                ],
            ) => {
                let sample = EcmBurstScenario {
                    source_signature_m: signature,
                    source_ehp: ehp,
                    source_weapon_dps: weapon,
                    source_drone_dps: drone,
                    opponent_scan_resolution_mm: scan_res,
                    opponent_scan_resolution_multiplier: scan_mult,
                    opponent_dps,
                    uptime_adjustment_s: adjustment,
                    max_cycles: max_cycles as u32,
                }
                .sample()
                .unwrap();
                compare(sample.opponent_lock_time_s, lock);
                compare(sample.opponent_lock_uptime_s, uptime);
                compare(sample.inflicted_damage_hp, damage);
                counts[3] += 1;
            }
            _ => panic!("unknown fixture row {row}"),
        }
    }
    assert_eq!(counts, [26, 24, 24, 27]);
}
