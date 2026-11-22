//! Pyfa's projected armor and Siege assistance readouts on fitted inputs.
use refine_core::{
    AddMode, AttrId, EveAttrId, FitId, ItemCommon, ItemTypeId, ModRack, ModuleState, PValue, SolOwned, Spool, Src,
    UnitInterval,
    ed::{err::EveDataHandlerError, *},
    stats::StatCapBlcSrcKinds,
};

const KESTREL: i32 = 602;
const ZARMAZD: i32 = 49713;
const MUTADAPTIVE: i32 = 49770;
const MOROS: i32 = 19724;
const SIEGE: i32 = 20280;
const BASILISK: i32 = 11985;
const SHIELD_BOOSTER: i32 = 3608;
const CAP_TRANSMITTER: i32 = 12102;
const ONYX: i32 = 11995;
const WARP_DISRUPTION_FIELD_GENERATOR: i32 = 28654;

#[derive(Debug)]
struct Data;

impl EveDataHandlerCore for Data {
    fn get_data(&self) -> Result<EData, EveDataHandlerError> {
        let mut data = EData::new();
        for (types, group, category) in [
            (&[KESTREL][..], 25, 6),
            (&[ZARMAZD, BASILISK][..], 832, 6),
            (&[MOROS][..], 485, 6),
            (&[ONYX][..], 894, 6),
            (&[MUTADAPTIVE][..], 2018, 7),
            (&[SIEGE][..], 515, 7),
            (&[WARP_DISRUPTION_FIELD_GENERATOR][..], 899, 7),
            (&[SHIELD_BOOSTER][..], 41, 7),
            (&[CAP_TRANSMITTER][..], 67, 7),
        ] {
            let group_id = EItemGrpId::from_i32(group);
            data.groups.data.push(EItemGroup {
                id: group_id,
                category_id: EItemCatId::from_i32(category),
            });
            data.items.data.extend(types.iter().map(|&id| EItem {
                id: EItemId::from_i32(id),
                group_id,
                is_dynamic: false,
                capacity: None,
                mass: None,
                radius: None,
                volume: None,
            }));
        }
        // Keep the fitted amount, cycle and range from the matching pinned
        // Pyfa fits. The test isolates projection from the source skill adapter.
        for (item, attrs) in [
            (KESTREL, &[(265, 437.5)][..]),
            (ZARMAZD, &[(14, 1.0)][..]),
            (
                MUTADAPTIVE,
                &[(84, 784.0), (73, 6000.0), (54, 63000.0), (2796, 0.1), (2797, 1.5)][..],
            ),
            (MOROS, &[(14, 1.0), (263, 50_000.0), (482, 68_125.0)][..]),
            (SIEGE, &[(73, 300_000.0), (2342, -99.9999), (6464, -99.9999)][..]),
            (BASILISK, &[(14, 2.0), (263, 2500.0), (482, 1500.0)][..]),
            (ONYX, &[(14, 4.0), (265, 3000.0), (263, 3600.0), (482, 1600.0)][..]),
            (WARP_DISRUPTION_FIELD_GENERATOR, &[(73, 30000.0), (854, 1.0)][..]),
            (
                SHIELD_BOOSTER,
                &[(68, 680.0), (73, 8000.0), (54, 32160.0), (2044, 48000.0)][..],
            ),
            (CAP_TRANSMITTER, &[(90, 351.0), (73, 5000.0), (54, 82875.0)][..]),
        ] {
            for &(attr, value) in attrs {
                data.item_attrs.data.push(EItemAttr {
                    item_id: EItemId::from_i32(item),
                    attr_id: EAttrId::from_i32(attr),
                    value: EFloat::from_f64(value),
                });
            }
        }
        let mut attr_ids: Vec<_> = data.item_attrs.data.iter().map(|row| row.attr_id.into_i32()).collect();
        attr_ids.extend([2116, 6463]);
        attr_ids.sort_unstable();
        attr_ids.dedup();
        data.attrs.data.extend(attr_ids.into_iter().map(|id| EAttr {
            id: EAttrId::from_i32(id),
            stackable: true,
            high_is_good: true,
            default_value: EFloat::from_f64(if matches!(id, 2116 | 6463) { 1.0 } else { 0.0 }),
            min_attr_id: None,
            max_attr_id: None,
        }));
        for (id, category, range, falloff, resistance) in [
            (12, 0, None, None, None),
            (7166, 2, Some(54), None, Some(2116)),
            (6582, 1, None, None, None),
            (3380, 1, None, None, None),
            (6186, 2, Some(54), Some(2044), Some(2116)),
            (6184, 2, Some(54), None, Some(6463)),
        ] {
            // The production 3380 record supplies no modifiers. Let Refine's
            // built-in updater resolve the fitted field generator normally.
            let modifiers = if id == 6582 {
                &[(2342, 2116, 6), (6464, 6463, 6)][..]
            } else {
                &[]
            };
            let mods = modifiers
                .iter()
                .map(|&(source, target, operation)| EEffectMod {
                    func: "ItemModifier".into(),
                    args: [
                        ("domain", EPrimitive::String("shipID".into())),
                        ("modifyingAttributeID", EPrimitive::Int(source)),
                        ("modifiedAttributeID", EPrimitive::Int(target)),
                        ("operation", EPrimitive::Int(operation)),
                    ]
                    .into_iter()
                    .map(|(name, value)| EEffectModArg {
                        name: name.into(),
                        value,
                    })
                    .collect(),
                })
                .collect();
            data.effects.data.push(EEffect {
                id: EEffectId::from_i32(id),
                category_id: EEffectCatId::from_i32(category),
                is_assistance: category == 2,
                is_offensive: false,
                discharge_attr_id: None,
                duration_attr_id: (id != 12).then_some(EAttrId::from_i32(73)),
                range_attr_id: range.map(EAttrId::from_i32),
                falloff_attr_id: falloff.map(EAttrId::from_i32),
                tracking_attr_id: None,
                usage_chance_attr_id: None,
                resist_attr_id: resistance.map(EAttrId::from_i32),
                mods,
            });
        }
        for (item, effect) in [
            (MUTADAPTIVE, 7166),
            (SIEGE, 6582),
            (SHIELD_BOOSTER, 6186),
            (CAP_TRANSMITTER, 6184),
            (WARP_DISRUPTION_FIELD_GENERATOR, 3380),
        ] {
            for id in [12, effect] {
                data.item_effects.data.push(EItemEffect {
                    item_id: EItemId::from_i32(item),
                    effect_id: EEffectId::from_i32(id),
                    is_default: id == effect,
                });
            }
        }
        Ok(data)
    }

    fn get_data_version(&self) -> Result<String, EveDataHandlerError> {
        Ok("Pyfa v2.70.0 incoming assistance witness".into())
    }
}

fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
}

fn incoming(sol: &mut SolOwned, fit: &FitId, pyfa: bool) -> [f64; 3] {
    let mut fit = sol.get_fit(fit).unwrap();
    let mut stats = fit.stats();
    let threshold = UnitInterval::from_f64_clamped(0.25);
    let hp = if pyfa {
        stats.get_stat_rps_pyfa(Default::default(), threshold)
    } else {
        stats.get_stat_rps(Default::default(), threshold)
    }
    .unwrap();
    let cap_sources = StatCapBlcSrcKinds::new(false)
        .with_incoming_transfers(true)
        .with_exact_incoming_transfer_resistance(pyfa);
    let cap = stats.get_stat_cap_balance(cap_sources, Default::default()).unwrap();
    [
        hp.shield.remote_penalized.into_f64(),
        hp.armor.remote_penalized.into_f64(),
        cap.into_f64(),
    ]
}

#[test]
fn pyfa_assistance_block_applies_to_the_recipient_and_reacts_to_module_state() {
    let source = Src::new(&Data.into(), None).unwrap();
    let mut sol = SolOwned::new(&source);
    let mut onyx = sol.add_fit();
    let onyx_id = onyx.get_fit_id();
    let onyx_ship = onyx.set_ship(ItemTypeId::from_i32(ONYX), None, None).get_item_id();
    let field_id = onyx
        .add_module(
            ModRack::High,
            AddMode::Append,
            ItemTypeId::from_i32(WARP_DISRUPTION_FIELD_GENERATOR),
            ModuleState::Online,
        )
        .unwrap()
        .get_item_id();
    let mut basilisk = sol.add_fit();
    let basilisk_id = basilisk.get_fit_id();
    let basilisk_ship = basilisk
        .set_ship(ItemTypeId::from_i32(BASILISK), None, None)
        .get_item_id();
    assert_eq!(incoming(&mut sol, &onyx_id, true), [0.0; 3]);
    assert_eq!(incoming(&mut sol, &basilisk_id, true), [0.0; 3]);

    let mut incoming_modules = Vec::new();
    for type_id in [SHIELD_BOOSTER, CAP_TRANSMITTER] {
        let mut fit = sol.get_fit(&basilisk_id).unwrap();
        let mut module = fit
            .add_module(
                ModRack::High,
                AddMode::Append,
                ItemTypeId::from_i32(type_id),
                ModuleState::Active,
            )
            .unwrap();
        module
            .add_proj(&onyx_ship)
            .unwrap()
            .set_range(PValue::from_f64_checked(5000.0).unwrap());
        incoming_modules.push(module.get_item_id());
    }
    let mut zarmazd = sol.add_fit();
    zarmazd.set_ship(ItemTypeId::from_i32(ZARMAZD), None, None);
    let mut module = zarmazd
        .add_module(
            ModRack::High,
            AddMode::Append,
            ItemTypeId::from_i32(MUTADAPTIVE),
            ModuleState::Active,
        )
        .unwrap();
    module
        .add_proj(&onyx_ship)
        .unwrap()
        .set_range(PValue::from_f64_checked(7500.0).unwrap());
    incoming_modules.push(module.get_item_id());

    let ordinary_online = incoming(&mut sol, &onyx_id, false);
    for (actual, expected) in ordinary_online.into_iter().zip([85.0, 1960.0 / 6.0, 70.2]) {
        close(actual, expected);
    }
    for (actual, expected) in incoming(&mut sol, &onyx_id, true)
        .into_iter()
        .zip([85.0, 1960.0 / 6.0, 70.2])
    {
        close(actual, expected);
    }
    assert_eq!(incoming(&mut sol, &basilisk_id, true), [0.0; 3]);
    sol.get_module(&field_id).unwrap().set_state(ModuleState::Active);
    let mut onyx = sol.get_fit(&onyx_id).unwrap();
    let active_flag = onyx
        .get_ship()
        .unwrap()
        .get_attr(&AttrId::Eve(EveAttrId::from_i32(854)))
        .unwrap()
        .modified
        .into_f64();
    assert_eq!(active_flag, 0.0);
    // The ordinary rate API retains its original behavior.
    assert_eq!(incoming(&mut sol, &onyx_id, false), ordinary_online);
    assert_eq!(incoming(&mut sol, &onyx_id, true), [0.0; 3]);
    sol.get_module(&field_id).unwrap().set_state(ModuleState::Online);
    for (actual, expected) in incoming(&mut sol, &onyx_id, true)
        .into_iter()
        .zip([85.0, 1960.0 / 6.0, 70.2])
    {
        close(actual, expected);
    }

    sol.get_module(&field_id).unwrap().set_state(ModuleState::Active);
    for module in incoming_modules {
        sol.get_module(&module).unwrap().get_proj(&onyx_ship).unwrap().remove();
    }
    for type_id in [SHIELD_BOOSTER, CAP_TRANSMITTER] {
        let mut fit = sol.get_fit(&onyx_id).unwrap();
        let mut module = fit
            .add_module(
                ModRack::High,
                AddMode::Append,
                ItemTypeId::from_i32(type_id),
                ModuleState::Active,
            )
            .unwrap();
        module
            .add_proj(&basilisk_ship)
            .unwrap()
            .set_range(PValue::from_f64_checked(5000.0).unwrap());
    }
    assert_eq!(incoming(&mut sol, &onyx_id, true), [0.0; 3]);
    for (actual, expected) in incoming(&mut sol, &basilisk_id, true)
        .into_iter()
        .zip([85.0, 0.0, 70.2])
    {
        close(actual, expected);
    }
}

#[test]
fn pyfa_incoming_mutadaptive_repair_preserves_spool_before_target_armor_cap() {
    let source = Src::new(&Data.into(), None).unwrap();
    for (spool, range, applied, displayed) in [
        (0.0, None, 0.0, 0.0),
        (0.0, Some(7500.0), 437.5 / 6.0, 784.0 / 6.0),
        (1.0, Some(7500.0), 437.5 / 6.0, 1960.0 / 6.0),
        (1.0, Some(64_000.0), 0.0, 0.0),
    ] {
        let mut sol = SolOwned::new(&source);
        let mut target = sol.add_fit();
        let target_id = target.get_fit_id();
        let ship = target.set_ship(ItemTypeId::from_i32(KESTREL), None, None).get_item_id();
        let mut emitter = sol.add_fit();
        emitter.set_ship(ItemTypeId::from_i32(ZARMAZD), None, None);
        let mut module = emitter
            .add_module(
                ModRack::High,
                AddMode::Append,
                ItemTypeId::from_i32(MUTADAPTIVE),
                ModuleState::Active,
            )
            .unwrap();
        module.set_spool_override(Some(Spool::SpoolScale(UnitInterval::from_f64_clamped(spool))));
        if let Some(range) = range {
            module
                .add_proj(&ship)
                .unwrap()
                .set_range(PValue::from_f64_checked(range).unwrap());
        }
        let mut fit = sol.get_fit(&target_id).unwrap();
        let mut stats = fit.stats();
        let threshold = UnitInterval::from_f64_clamped(0.25);
        close(
            stats
                .get_stat_rps(Default::default(), threshold)
                .unwrap()
                .armor
                .remote_penalized
                .into_f64(),
            applied,
        );
        close(
            stats
                .get_stat_rps_pyfa(Default::default(), threshold)
                .unwrap()
                .armor
                .remote_penalized
                .into_f64(),
            displayed,
        );
    }
}

#[test]
fn pyfa_incoming_siege_shield_and_capacitor_display_use_distinct_impedance_rules() {
    let source = Src::new(&Data.into(), None).unwrap();
    for (state, active, ordinary_shield, display_shield, ordinary_cap, display_cap) in [
        (ModuleState::Online, true, 85.0, 85.0, 70.2, 70.2),
        (ModuleState::Active, true, 0.0, 85.0, 0.0, 0.0000702),
        (ModuleState::Active, false, 0.0, 0.0, 0.0, 0.0),
    ] {
        let mut sol = SolOwned::new(&source);
        let mut target = sol.add_fit();
        let target_id = target.get_fit_id();
        let ship = target.set_ship(ItemTypeId::from_i32(MOROS), None, None).get_item_id();
        target
            .add_module(ModRack::High, AddMode::Append, ItemTypeId::from_i32(SIEGE), state)
            .unwrap();
        let mut emitter = sol.add_fit();
        emitter.set_ship(ItemTypeId::from_i32(BASILISK), None, None);
        for type_id in [SHIELD_BOOSTER, CAP_TRANSMITTER] {
            let mut module = emitter
                .add_module(
                    ModRack::High,
                    AddMode::Append,
                    ItemTypeId::from_i32(type_id),
                    ModuleState::Active,
                )
                .unwrap();
            if active {
                module
                    .add_proj(&ship)
                    .unwrap()
                    .set_range(PValue::from_f64_checked(5000.0).unwrap());
            }
        }
        let mut fit = sol.get_fit(&target_id).unwrap();
        let mut stats = fit.stats();
        let threshold = UnitInterval::from_f64_clamped(0.25);
        close(
            stats
                .get_stat_rps(Default::default(), threshold)
                .unwrap()
                .shield
                .remote_penalized
                .into_f64(),
            ordinary_shield,
        );
        close(
            stats
                .get_stat_rps_pyfa(Default::default(), threshold)
                .unwrap()
                .shield
                .remote_penalized
                .into_f64(),
            display_shield,
        );
        let cap = StatCapBlcSrcKinds::new(false).with_incoming_transfers(true);
        close(
            stats.get_stat_cap_balance(cap, Default::default()).unwrap().into_f64(),
            ordinary_cap,
        );
        close(
            stats
                .get_stat_cap_balance(cap.with_exact_incoming_transfer_resistance(true), Default::default())
                .unwrap()
                .into_f64(),
            display_cap,
        );
    }
}
