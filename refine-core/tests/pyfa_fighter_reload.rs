use std::{collections::BTreeSet, env, fs};

use refine_core::{
    AbilityId, AddMode, AttrId, Coordinates, CountNz, Direction, DogmaEffectId, EffectId, EffectMode, EveAttrId, Fit,
    ItemCommon, ItemId, ItemTypeId, MinionState, ModRack, ModuleState, Movement, PValue, RearmMinion, SkillLevel,
    SolOwned, Src, Value,
    ed::{
        EAbil, EAbilId, EAttr, EAttrId, EData, EEffect, EEffectCatId, EEffectId, EEffectMod, EEffectModArg, EFloat,
        EInt, EItem, EItemAbil, EItemAttr, EItemCatId, EItemEffect, EItemGroup, EItemGrpId, EItemId, EPrimitive,
        EveDataHandlerCore, err::EveDataHandlerError,
    },
    stats::{
        ItemStatsCommon, StatCritOptions, StatDmg, StatDmgItemKinds, StatItemChargeOptions, StatTime, StatTimeOptions,
        StatTimeOptionsSim,
    },
};

const THANATOS: i32 = 23911;
const KESTREL: i32 = 602;
const FIRBOLG: i32 = 23059;
const AMPLIFIER: i32 = 4405;
const GALLENTE_CARRIER: i32 = 24313;
const FIGHTERS: i32 = 23069;
const DRONE_INTERFACING: i32 = 3442;
const MISSILES: i32 = 6431;
const MWD: i32 = 6441;
const TURRET: i32 = 6465;

#[derive(Debug, Default)]
struct FirbolgData {
    slow_refuel: bool,
    faster_turret: bool,
}

impl EveDataHandlerCore for FirbolgData {
    fn get_data_version(&self) -> Result<String, EveDataHandlerError> {
        Ok("Pyfa v2.70.0 Firbolg damage inputs".into())
    }

    fn get_data(&self) -> Result<EData, EveDataHandlerError> {
        let mut data = EData::new();
        for (group, category) in [(547, 6), (25, 6), (1652, 87), (645, 7), (257, 16), (273, 16)] {
            data.groups.data.push(EItemGroup {
                id: EItemGrpId::from_i32(group),
                category_id: EItemCatId::from_i32(category),
            });
        }
        for (item, group) in [
            (THANATOS, 547),
            (KESTREL, 25),
            (FIRBOLG, 1652),
            (AMPLIFIER, 645),
            (GALLENTE_CARRIER, 257),
            (FIGHTERS, 273),
            (DRONE_INTERFACING, 273),
        ] {
            data.items.data.push(EItem {
                id: EItemId::from_i32(item),
                group_id: EItemGrpId::from_i32(group),
                is_dynamic: false,
                capacity: None,
                mass: None,
                radius: None,
                volume: None,
            });
        }
        for attr in [
            12, 37, 182, 277, 280, 292, 552, 1255, 2125, 2126, 2127, 2128, 2130, 2132, 2149, 2157, 2182, 2212, 2215,
            2226, 2228, 2231, 2232, 2233, 2234, 2235, 2236, 2237, 2367, 2426,
        ] {
            data.attrs.data.push(EAttr {
                id: EAttrId::from_i32(attr),
                stackable: !matches!(attr, 2130 | 2226),
                high_is_good: true,
                default_value: EFloat::from_f64(0.0),
                min_attr_id: None,
                max_attr_id: None,
            });
        }
        for (item, attr, value) in [
            (THANATOS, 12, 6.0),
            (THANATOS, 2367, 5.0),
            (KESTREL, 37, 325.0),
            (KESTREL, 552, 38.0),
            (FIRBOLG, 182, f64::from(FIGHTERS)),
            (FIRBOLG, 277, 1.0),
            (FIRBOLG, 2125, 400.0),
            (FIRBOLG, 2126, 70.0),
            (FIRBOLG, 2127, 5.0),
            (FIRBOLG, 2128, 5.5),
            (FIRBOLG, 2130, 1.0),
            (FIRBOLG, 2132, 207.0),
            (FIRBOLG, 2149, 10000.0),
            (FIRBOLG, 2157, 20000.0),
            (FIRBOLG, 2182, 14000.0),
            (FIRBOLG, 2212, 1.0),
            (FIRBOLG, 2215, 6.0),
            (FIRBOLG, 2226, 1.0),
            (FIRBOLG, 2228, 112.5),
            (FIRBOLG, 2231, 3.0),
            (FIRBOLG, 2232, 5.5),
            (FIRBOLG, 2233, if self.faster_turret { 3990.0 } else { 5000.0 }),
            (FIRBOLG, 2234, 185.0),
            (FIRBOLG, 2235, 105.0),
            (FIRBOLG, 2236, 4000.0),
            (FIRBOLG, 2237, 5000.0),
            (FIRBOLG, 2426, 5000.0),
            (AMPLIFIER, 1255, 20.5),
            (DRONE_INTERFACING, 292, 10.0),
            (FIGHTERS, 292, 5.0),
        ] {
            data.item_attrs.data.push(EItemAttr {
                item_id: EItemId::from_i32(item),
                attr_id: EAttrId::from_i32(attr),
                value: EFloat::from_f64(value),
            });
        }
        let fighter_bonus = |source, operation| {
            [2130, 2226].map(|target| {
                modifier(
                    "OwnerRequiredSkillModifier",
                    "charID",
                    source,
                    target,
                    operation,
                    Some(FIGHTERS),
                )
            })
        };
        let skill_damage = || {
            std::iter::once(modifier("ItemModifier", "itemID", 280, 292, 0, None))
                .chain(fighter_bonus(292, 8))
                .collect()
        };
        let mut hull_bonuses: Vec<_> = fighter_bonus(2367, 8).into();
        if self.slow_refuel {
            hull_bonuses.push(modifier(
                "OwnerRequiredSkillModifier",
                "charID",
                2367,
                2426,
                4,
                Some(FIGHTERS),
            ));
        }
        for (item, id, category, duration, mods) in [
            (FIRBOLG, MISSILES, 2, Some(2182), vec![]),
            (FIRBOLG, MWD, 1, Some(2157), vec![]),
            (FIRBOLG, TURRET, 2, Some(2233), vec![]),
            (AMPLIFIER, 11, 0, None, vec![]),
            (AMPLIFIER, 16, 0, None, vec![]),
            (AMPLIFIER, 6556, 4, None, fighter_bonus(1255, 6).into()),
            (DRONE_INTERFACING, 6663, 0, None, skill_damage()),
            (FIGHTERS, 6560, 0, None, skill_damage()),
            (THANATOS, 6601, 0, None, hull_bonuses),
            (
                GALLENTE_CARRIER,
                6585,
                0,
                None,
                vec![modifier("ItemModifier", "shipID", 280, 2367, 0, None)],
            ),
        ] {
            data.effects.data.push(EEffect {
                id: EEffectId::from_i32(id),
                category_id: EEffectCatId::from_i32(category),
                is_assistance: false,
                is_offensive: matches!(id, MISSILES | TURRET),
                discharge_attr_id: None,
                duration_attr_id: duration.map(EAttrId::from_i32),
                range_attr_id: match id {
                    MISSILES => Some(EAttrId::from_i32(2149)),
                    TURRET => Some(EAttrId::from_i32(2236)),
                    _ => None,
                },
                falloff_attr_id: (id == TURRET).then_some(EAttrId::from_i32(2237)),
                tracking_attr_id: None,
                usage_chance_attr_id: None,
                resist_attr_id: None,
                mods,
            });
            data.item_effects.data.push(EItemEffect {
                item_id: EItemId::from_i32(item),
                effect_id: EEffectId::from_i32(id),
                is_default: id == TURRET,
            });
        }
        // Pyfa's light attack role supplies 12 charges and 4 seconds of rearm per charge.
        for (slot, id, charged) in [(0, 24, false), (1, 4, false), (2, 34, true)] {
            let id = EAbilId::from_i32(id);
            data.abils.data.push(EAbil {
                id,
                disallow_hisec: false,
                disallow_lowsec: false,
            });
            data.item_abils.data.push(EItemAbil {
                item_id: EItemId::from_i32(FIRBOLG),
                abil_id: id,
                slot: EInt::from_i32(slot),
                cooldown: None,
                charge_count: charged.then_some(EInt::from_i32(12)),
                charge_rearm_duration: charged.then_some(EFloat::from_f64(4.0)),
            });
        }
        Ok(data)
    }
}

fn modifier(kind: &str, domain: &str, source: i32, target: i32, operation: i32, skill: Option<i32>) -> EEffectMod {
    let mut args = vec![EEffectModArg {
        name: "domain".into(),
        value: EPrimitive::String(domain.into()),
    }];
    for (name, value) in [
        ("modifyingAttributeID", source),
        ("modifiedAttributeID", target),
        ("operation", operation),
    ]
    .into_iter()
    .chain(skill.map(|id| ("skillTypeID", id)))
    {
        args.push(EEffectModArg {
            name: name.into(),
            value: EPrimitive::Int(value),
        });
    }
    EEffectMod {
        func: kind.into(),
        args,
    }
}

fn create_fit(sol: &mut SolOwned, amplified: bool) -> Fit<'_> {
    let mut fit = sol.add_fit();
    fit.set_ship(ItemTypeId::from_i32(THANATOS), None, None);
    for id in [GALLENTE_CARRIER, FIGHTERS, DRONE_INTERFACING] {
        fit.add_skill(ItemTypeId::from_i32(id), SkillLevel::from_i32_checked(5).unwrap())
            .unwrap();
    }
    if amplified {
        fit.add_module(
            ModRack::Low,
            AddMode::Equip,
            ItemTypeId::from_i32(AMPLIFIER),
            ModuleState::Online,
        )
        .unwrap();
    }
    fit
}

fn set_abilities(fighter: &mut refine_core::Fighter<'_>, active_effect_ids: &[i32]) {
    fighter.set_effect_modes([MISSILES, MWD, TURRET].map(|id| {
        let mode = if active_effect_ids.contains(&id) {
            EffectMode::StateCompliance
        } else {
            EffectMode::ForceStop
        };
        (EffectId::Dogma(DogmaEffectId::from_i32(id)), mode)
    }));
}

fn time_options(time: Option<f64>, rearm: RearmMinion) -> StatTimeOptions {
    StatTimeOptions::Sim(StatTimeOptionsSim {
        time: time.map(|time| StatTime::from_f64_checked(time).unwrap()),
        rearm_minions: Some(rearm),
        ..StatTimeOptionsSim::default()
    })
}

fn damage_pair(damage: StatDmg) -> [f64; 2] {
    [damage.dps, damage.volley].map(|value| {
        assert_eq!(
            [value.em, value.kinetic, value.explosive].map(|v| v.into_f64()),
            [0.0; 3]
        );
        value.thermal.into_f64()
    })
}

fn fit_damage(fit: &mut Fit<'_>, options: StatTimeOptions) -> [f64; 2] {
    damage_pair(
        fit.stats()
            .get_stat_dmg(StatDmgItemKinds::default(), options, StatCritOptions::Exclude),
    )
}

fn check_damage(case: &str, actual: [f64; 2], expected: [f64; 2]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        let tolerance = 1e-8_f64.max(1e-9 * actual.abs().max(expected.abs()));
        assert!(
            (actual - expected).abs() <= tolerance,
            "{case}: expected {expected}, got {actual}"
        );
    }
}

fn projected_firbolg<'a>(sol: &'a mut SolOwned, effects: &[i32]) -> (Fit<'a>, ItemId, ItemId) {
    let origin = Coordinates::new(Value::default(), Value::default(), Value::default());
    let target = sol
        .add_fit()
        .set_ship(ItemTypeId::from_i32(KESTREL), Some(origin), None)
        .get_item_id();
    sol.get_ship(&target).unwrap().set_movement(Movement {
        direction: Direction {
            azimuth: Value::default(),
            elevation: Value::default(),
        },
        speed: PValue::from_f64_checked(350.0 / 325.0).unwrap(),
    });
    let mut fit = create_fit(sol, false);
    let mut fighter = fit.add_fighter(ItemTypeId::from_i32(FIRBOLG), MinionState::Engaging, Some(origin), None);
    fighter.set_count_override(Some(CountNz::from_u32_checked(6).unwrap()));
    set_abilities(&mut fighter, effects);
    fighter.add_proj(&target).unwrap();
    let fighter_id = fighter.get_item_id();
    (fit, target, fighter_id)
}

#[test]
fn fighter_application_rounding_is_per_attack_and_opt_in() {
    let source = Src::new(&FirbolgData::default().into(), None).unwrap();
    // Pyfa v2.70.0, six Firbolg I following a 38 m target at 350 m/s.
    // Each pair is DPS and volley, first in compatibility mode and then using native application.
    for (effect, pyfa, native) in [
        (
            TURRET,
            [52.516698046875, 262.583490234375],
            [52.516711509605486, 262.58355754802744],
        ),
        (
            MISSILES,
            [4.930500194196429, 69.02700271875],
            [4.930500693250402, 69.02700970550563],
        ),
    ] {
        for (mut sol, label, expected) in [
            (SolOwned::new_with_pyfa_compatibility(&source), "Pyfa", pyfa),
            (SolOwned::new_with_pyfa_fitting_rounding(&source), "native", native),
        ] {
            let (mut fit, target, _) = projected_firbolg(&mut sol, &[effect]);
            let applied = fit
                .stats()
                .get_stat_dmg_applied(
                    Default::default(),
                    Default::default(),
                    StatCritOptions::Exclude,
                    &target,
                )
                .unwrap();
            check_damage(
                &format!("{label} effect {effect}"),
                [applied.dps.thermal.into_f64(), applied.volley.thermal.into_f64()],
                expected,
            );
        }
    }
}

#[test]
fn fighter_reload_chooses_the_schedule_before_application() {
    let source = Src::new(&FirbolgData::default().into(), None).unwrap();
    let reload = time_options(None, RearmMinion::OnFirstEmpty);
    // Stock Pyfa v2.70.0 Damage Stats at 350 m/s and signature 38 m. The coordinated
    // schedule wins on raw DPS even though turret-only deals more after application.
    for (label, effects, expected) in [
        ("both", &[TURRET, MISSILES][..], [44.145532581872175, 331.610492953125]),
        ("turret", &[TURRET], [52.516698046875, 262.583490234375]),
        ("missile", &[MISSILES], [3.74807254581448, 69.02700271875]),
    ] {
        let mut sol = SolOwned::new_with_pyfa_compatibility(&source);
        let (mut fit, target, fighter_id) = projected_firbolg(&mut sol, effects);
        let fitted = fit
            .stats()
            .get_stat_dmg_applied(Default::default(), reload, StatCritOptions::Exclude, &target)
            .unwrap();
        let fighter = sol
            .get_fighter(&fighter_id)
            .unwrap()
            .stats()
            .get_stat_dmg_applied(
                reload,
                StatCritOptions::Exclude,
                StatItemChargeOptions::Exclude,
                &target,
            )
            .unwrap();
        for applied in [fitted, fighter] {
            check_damage(
                label,
                [applied.dps.thermal.into_f64(), applied.volley.thermal.into_f64()],
                expected,
            );
        }
    }
}

#[test]
#[ignore = "requires the pinned Pyfa fighter reload replay exported by the parity runner"]
fn native_item_and_fit_paper_damage_match_pinned_pyfa_reload_choices() {
    let path = env::var_os("RUFA_PYFA_FIGHTER_RELOAD_CASES").expect("set RUFA_PYFA_FIGHTER_RELOAD_CASES");
    let cases = fs::read_to_string(path).unwrap();
    let mut remaining = BTreeSet::from([
        "half-both",
        "half-turret-only",
        "full-both",
        "full-turret-only",
        "full-missile-only",
        "full-no-abilities",
        "full-stowed-missile-only",
        "zero-quantity-both",
        "full-dda-both",
        "full-dda-turret-only",
    ]);
    let source = Src::new(&FirbolgData::default().into(), None).unwrap();
    let mut sol = SolOwned::new_with_pyfa_compatibility(&source);
    let reload = time_options(None, RearmMinion::OnFirstEmpty);
    let burst = StatTimeOptions::default();

    for line in cases.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        let [
            case,
            quantity,
            amplified,
            launched,
            effect_ids,
            burst_dps,
            sustained_dps,
            volley,
            cycle_ms,
            salvos,
            reload_ms,
        ] = fields.as_slice()
        else {
            panic!("invalid fighter reload row: {line}")
        };
        assert!(
            remaining.remove(case),
            "unexpected or duplicate fighter reload case: {case}"
        );
        let quantity = quantity.parse().unwrap();
        let mut fit = create_fit(&mut sol, *amplified == "1");
        let (burst_dps, sustained_dps, volley) = (
            burst_dps.parse().unwrap(),
            sustained_dps.parse().unwrap(),
            volley.parse().unwrap(),
        );

        if let Ok(quantity) = CountNz::from_u32_checked(quantity) {
            let state = if *launched == "1" {
                MinionState::Engaging
            } else {
                MinionState::InBay
            };
            let effects: Vec<_> = effect_ids
                .split(',')
                .filter(|id| !id.is_empty())
                .map(|id| id.parse().unwrap())
                .collect();
            let mut fighter = fit.add_fighter(ItemTypeId::from_i32(FIRBOLG), state, None, None);
            fighter.set_count_override(Some(quantity));
            set_abilities(&mut fighter, &effects);
            let charges = fighter
                .get_ability(&AbilityId::from_i32(34))
                .unwrap()
                .get_charge_count()
                .unwrap()
                .into_u32();
            assert_eq!(charges, salvos.parse::<u32>().unwrap(), "{case} charges");
            let attr = |id| AttrId::Eve(EveAttrId::from_i32(id));
            let duration = fighter.get_attr(&attr(2182)).unwrap().modified.into_f64();
            assert_eq!(
                duration,
                cycle_ms.parse::<f64>().unwrap(),
                "{case} missile cycle length"
            );
            let refuel_ms = fighter.get_attr(&attr(2426)).unwrap().modified.into_f64();
            assert_eq!(
                refuel_ms + f64::from(charges) * 4000.0,
                reload_ms.parse::<f64>().unwrap(),
                "{case} full reload length"
            );
            for (options, expected) in [(burst, burst_dps), (reload, sustained_dps)] {
                let damage = fighter
                    .stats()
                    .get_stat_dmg(options, StatCritOptions::Exclude, StatItemChargeOptions::Exclude)
                    .unwrap();
                check_damage(case, damage_pair(damage), [expected, volley]);
            }
        }
        check_damage(case, fit_damage(&mut fit, burst), [burst_dps, volley]);
        check_damage(case, fit_damage(&mut fit, reload), [sustained_dps, volley]);
    }
    assert!(remaining.is_empty(), "missing fighter reload cases: {remaining:?}");
}

#[test]
fn paper_reload_compatibility_is_opt_in_and_retains_native_finite_damage() {
    let source = Src::new(&FirbolgData::default().into(), None).unwrap();
    let mut standard = SolOwned::new(&source);
    let mut compatible = SolOwned::new_with_pyfa_compatibility(&source);
    let mut standard_fit = create_fit(&mut standard, false);
    let mut compatible_fit = create_fit(&mut compatible, false);
    for fit in [&mut standard_fit, &mut compatible_fit] {
        let mut fighter = fit.add_fighter(ItemTypeId::from_i32(FIRBOLG), MinionState::Engaging, None, None);
        set_abilities(&mut fighter, &[MISSILES, TURRET]);
    }
    for options in [
        StatTimeOptions::default(),
        time_options(None, RearmMinion::Disabled),
        time_options(Some(200.0), RearmMinion::OnFirstEmpty),
    ] {
        assert_eq!(
            fit_damage(&mut compatible_fit, options),
            fit_damage(&mut standard_fit, options)
        );
    }
    let rearm = time_options(None, RearmMinion::OnFirstEmpty);
    check_damage(
        "native recall includes launch and landing",
        fit_damage(&mut standard_fit, rearm),
        [397.8489352017937, 4492.96875],
    );
    check_damage(
        "Pyfa coordinated recall",
        fit_damage(&mut compatible_fit, rearm),
        [401.4493778280543, 4492.96875],
    );
    let excluded = compatible_fit
        .stats()
        .get_stat_dmg(StatDmgItemKinds::new(false), rearm, StatCritOptions::Exclude);
    assert_eq!(damage_pair(excluded), [0.0; 2]);
}

#[test]
fn slow_modified_refuel_can_favor_unlimited_dps_with_both_attacks_enabled() {
    let source = Src::new(
        &FirbolgData {
            slow_refuel: true,
            ..FirbolgData::default()
        }
        .into(),
        None,
    )
    .unwrap();
    let mut sol = SolOwned::new_with_pyfa_compatibility(&source);
    let mut fit = create_fit(&mut sol, false);
    let mut fighter = fit.add_fighter(ItemTypeId::from_i32(FIRBOLG), MinionState::Engaging, None, None);
    set_abilities(&mut fighter, &[MISSILES, TURRET]);
    let refuel_ms = fighter
        .get_attr(&AttrId::Eve(EveAttrId::from_i32(2426)))
        .unwrap()
        .modified
        .into_f64();
    assert_eq!(refuel_ms, 125_000.0);
    let reload = time_options(None, RearmMinion::OnFirstEmpty);
    check_damage(
        "unlimited DPS keeps both enabled volleys",
        fit_damage(&mut fit, reload),
        [316.40625, 4492.96875],
    );
}

#[test]
fn unqualified_attack_timing_keeps_native_rearm_damage() {
    let source = Src::new(
        &FirbolgData {
            faster_turret: true,
            ..FirbolgData::default()
        }
        .into(),
        None,
    )
    .unwrap();
    let mut standard = SolOwned::new(&source);
    let mut compatible = SolOwned::new_with_pyfa_compatibility(&source);
    let reload = time_options(None, RearmMinion::OnFirstEmpty);
    let actual = [&mut standard, &mut compatible].map(|sol| {
        let mut fit = create_fit(sol, false);
        let mut fighter = fit.add_fighter(ItemTypeId::from_i32(FIRBOLG), MinionState::Engaging, None, None);
        set_abilities(&mut fighter, &[MISSILES, TURRET]);
        fit_damage(&mut fit, reload)
    });
    assert!(actual[0][0] > 0.0);
    assert_eq!(actual[0], actual[1]);
}
