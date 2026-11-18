use super::*;
use crate::{
    AddMode, ItemCommon, ItemTypeId, ModRack, ModuleState, SolOwned, Src,
    ed::{
        EAttr, EAttrId, EData, EEffect, EEffectCatId, EEffectId, EFloat, EItem, EItemAttr, EItemCatId, EItemEffect,
        EItemGroup, EItemGrpId, EItemId, EveDataHandlerCore, err::EveDataHandlerError,
    },
    stats::StatCapSimStagger,
};

const SHIP: EItemId = EItemId::from_i32(602);
const BOOSTER: EItemId = EItemId::from_i32(399);
const DURATION: EAttrId = EAttrId::from_i32(73);

#[derive(Debug)]
struct CapTestData {
    delay_ms: f64,
    capacity: f64,
    recharge_ms: f64,
    need: f64,
    duration_ms: f64,
}

impl Default for CapTestData {
    fn default() -> Self {
        Self {
            delay_ms: 0.0,
            capacity: 100.0,
            recharge_ms: 5000.0,
            need: 100.0,
            duration_ms: 1000.0,
        }
    }
}

impl EveDataHandlerCore for CapTestData {
    fn get_data(&self) -> Result<EData, EveDataHandlerError> {
        let mut data = EData::new();
        for (id, group, category) in [(SHIP, 25, EItemCatId::SHIP), (BOOSTER, 40, EItemCatId::MODULE)] {
            let group_id = EItemGrpId::from_i32(group);
            data.groups.data.push(EItemGroup {
                id: group_id,
                category_id: category,
            });
            data.items.data.push(EItem {
                id,
                group_id,
                is_dynamic: false,
                capacity: None,
                mass: None,
                radius: None,
                volume: None,
            });
        }
        for (item_id, attr_id, value) in [
            (SHIP, EAttrId::CAPACITOR_CAPACITY, self.capacity),
            (SHIP, EAttrId::RECHARGE_RATE, self.recharge_ms),
            (SHIP, EAttrId::MED_SLOTS, 4.0),
            (BOOSTER, EAttrId::CAPACITOR_NEED, self.need),
            (BOOSTER, DURATION, self.duration_ms),
            (BOOSTER, EAttrId::MOD_REACTIVATION_DELAY, self.delay_ms),
        ] {
            data.attrs.data.push(EAttr {
                id: attr_id,
                stackable: true,
                high_is_good: true,
                default_value: EFloat::from_f64(0.0),
                min_attr_id: None,
                max_attr_id: None,
            });
            data.item_attrs.data.push(EItemAttr {
                item_id,
                attr_id,
                value: EFloat::from_f64(value),
            });
        }
        for (id, category, activation) in [(EEffectId::MED_POWER, 0, false), (EEffectId::SHIELD_BOOSTING, 1, true)] {
            data.effects.data.push(EEffect {
                id,
                category_id: EEffectCatId::from_i32(category),
                is_assistance: false,
                is_offensive: false,
                discharge_attr_id: activation.then_some(EAttrId::CAPACITOR_NEED),
                duration_attr_id: activation.then_some(DURATION),
                range_attr_id: None,
                falloff_attr_id: None,
                tracking_attr_id: None,
                usage_chance_attr_id: None,
                resist_attr_id: None,
                mods: vec![],
            });
            data.item_effects.data.push(EItemEffect {
                item_id: BOOSTER,
                effect_id: id,
                is_default: activation,
            });
        }
        Ok(data)
    }
    fn get_data_version(&self) -> Result<String, EveDataHandlerError> {
        Ok("synthetic capacitor regression".to_owned())
    }
}

fn assert_stats(sol: &mut SolOwned, fit_id: &crate::FitId, active: bool) {
    let mut fit = sol.get_fit(fit_id).unwrap();
    let mut stats = fit.stats();
    let result = stats
        .get_stat_cap_sim_pyfa(
            false,
            Some(StatCapSimTraceRequest {
                start_cap_gj: 100.0,
                end_time_s: 5.0,
            }),
        )
        .unwrap();
    let inherited = stats
        .get_stat_cap_sim(UnitInterval::ONE, None, &StatCapSimStagger::new(false), None)
        .unwrap();
    if active {
        assert!(matches!(result.summary, StatCapSim::Time(time) if time.into_f64() == 1.0));
        assert!(matches!(inherited, StatCapSim::Time(time) if time.into_f64() == 1.0));
        assert_eq!(result.trace.unwrap(), [(0.0, 0.0), (1.0, 0.0)]);
    } else {
        assert!(matches!(result.summary, StatCapSim::Stable(fraction) if fraction == UnitInterval::ONE));
        assert!(matches!(inherited, StatCapSim::Stable(fraction) if fraction == UnitInterval::ONE));
        assert_eq!(result.trace.unwrap(), []);
    }
}

#[test]
fn both_simulators_follow_module_state_changes() {
    let source = Src::new(&CapTestData::default().into(), None).unwrap();
    let mut sol = SolOwned::new(&source);
    let mut fit = sol.add_fit();
    let fit_id = fit.get_fit_id();
    fit.set_ship(ItemTypeId::from_i32(SHIP.into_i32()), None, None);
    assert_stats(&mut sol, &fit_id, false);
    let module_id = sol
        .get_fit(&fit_id)
        .unwrap()
        .add_module(
            ModRack::Mid,
            AddMode::Equip,
            ItemTypeId::from_i32(BOOSTER.into_i32()),
            ModuleState::Online,
        )
        .unwrap()
        .get_item_id();
    assert_stats(&mut sol, &fit_id, false);
    sol.get_module(&module_id).unwrap().set_state(ModuleState::Active);
    assert_stats(&mut sol, &fit_id, true);
    sol.get_module(&module_id).unwrap().set_state(ModuleState::Offline);
    assert_stats(&mut sol, &fit_id, false);
}

#[test]
fn pyfa_simulator_adds_reactivation_delay_to_each_cycle() {
    // Pinned Pyfa Kestrel, All 5, active Warp Core Stabilizer I (10998).
    // Effective cycle is 10 s plus a 150 s reactivation delay.
    let source = Src::new(
        &CapTestData {
            delay_ms: 150_000.0,
            capacity: 412.5,
            recharge_ms: 123_750.0,
            need: 80.0,
            duration_ms: 10_000.0,
        }
        .into(),
        None,
    )
    .unwrap();
    let mut sol = SolOwned::new(&source);
    let mut fit = sol.add_fit();
    fit.set_ship(ItemTypeId::from_i32(SHIP.into_i32()), None, None);
    fit.add_module(
        ModRack::Mid,
        AddMode::Equip,
        ItemTypeId::from_i32(BOOSTER.into_i32()),
        ModuleState::Active,
    )
    .unwrap();
    let result = fit
        .stats()
        .get_stat_cap_sim_pyfa(
            false,
            Some(StatCapSimTraceRequest {
                start_cap_gj: 412.5,
                end_time_s: 600.0,
            }),
        )
        .unwrap();
    assert!(
        matches!(result.summary, StatCapSim::Stable(fraction) if (fraction.into_f64() - 0.9027114410338217).abs() < 1e-9)
    );
    let expected = [
        (0.0, 332.5),
        (160.0, 332.3686975952961),
        (320.0, 332.36846982229974),
        (480.0, 332.36846942713817),
    ];
    let actual = result.trace.unwrap();
    assert_eq!(actual.len(), expected.len());
    for ((time, cap), (expected_time, expected_cap)) in actual.into_iter().zip(expected) {
        assert_eq!(time, expected_time);
        assert!((cap - expected_cap).abs() < 1e-8);
    }
}
