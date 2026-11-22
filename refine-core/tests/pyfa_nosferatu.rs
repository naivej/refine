//! Public capacitor APIs for the Kestrel NOS witness and the ordinary combat override.
use refine_core::{
    AddMode, AttrId, EveAttrId, FitId, ItemCommon, ItemTypeId, ModRack, ModuleState, PValue, SolOwned, Src,
    ed::{
        EAttr, EAttrId, EData, EEffect, EEffectCatId, EEffectId, EFloat, EItem, EItemAttr, EItemCatId, EItemEffect,
        EItemGroup, EItemGrpId, EItemId, EveDataHandlerCore, err::EveDataHandlerError,
    },
    stats::{StatCapBlcSrcKinds, StatCapSimTraceRequest},
};

const CAPACITOR_NEED: AttrId = AttrId::Eve(EveAttrId::from_i32(6));

#[derive(Debug)]
struct NosferatuData {
    nos_override: bool,
}

impl EveDataHandlerCore for NosferatuData {
    fn get_data(&self) -> Result<EData, EveDataHandlerError> {
        let mut data = EData::new();
        let ship = EItemId::from_i32(602);
        let module = EItemId::from_i32(13001);
        for (id, group, category) in [(ship, 25, 6), (module, 68, 7)] {
            let group_id = EItemGrpId::from_i32(group);
            data.groups.data.push(EItemGroup {
                id: group_id,
                category_id: EItemCatId::from_i32(category),
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
        for (item_id, attr, value) in [
            (ship, 14, 4.0),
            (ship, 482, 412.5),
            (ship, 55, 123_750.0),
            (ship, 552, 38.0),
            (module, 6, 0.0),
            (module, 54, 5_000.0),
            (module, 73, 2_500.0),
            (module, 90, 10.0),
            (module, 1945, if self.nos_override { 1.0 } else { 0.0 }),
            (module, 2044, 2_500.0),
        ] {
            let attr_id = EAttrId::from_i32(attr);
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
        for (id, active) in [(12, false), (6197, true)] {
            let id = EEffectId::from_i32(id);
            data.effects.data.push(EEffect {
                id,
                category_id: EEffectCatId::from_i32(if active { 2 } else { 0 }),
                is_assistance: false,
                is_offensive: active,
                discharge_attr_id: None,
                duration_attr_id: active.then_some(EAttrId::from_i32(73)),
                range_attr_id: active.then_some(EAttrId::from_i32(54)),
                falloff_attr_id: active.then_some(EAttrId::from_i32(2044)),
                tracking_attr_id: None,
                usage_chance_attr_id: None,
                resist_attr_id: None,
                mods: vec![],
            });
            data.item_effects.data.push(EItemEffect {
                item_id: module,
                effect_id: id,
                is_default: active,
            });
        }
        Ok(data)
    }

    fn get_data_version(&self) -> Result<String, EveDataHandlerError> {
        Ok("synthetic NOS override regression".into())
    }
}

fn drain(sol: &mut SolOwned, fit_id: FitId, pyfa: bool) -> f64 {
    let sources = StatCapBlcSrcKinds::new(false)
        .with_incoming_neuts(true)
        .with_pyfa_incoming_nosferatu(pyfa);
    -sol.get_fit(&fit_id)
        .unwrap()
        .stats()
        .get_stat_cap_balance(sources, Default::default())
        .unwrap()
        .into_f64()
}

fn first_events(sol: &mut SolOwned, fit_id: FitId) -> Vec<(f64, f64)> {
    sol.get_fit(&fit_id)
        .unwrap()
        .stats()
        .get_stat_cap_sim_pyfa(
            false,
            Some(StatCapSimTraceRequest {
                start_cap_gj: 100.0,
                end_time_s: 0.0,
            }),
        )
        .unwrap()
        .trace
        .unwrap()
}

#[test]
fn pyfa_nosferatu_projects_at_optimal_and_falloff_with_or_without_combat_override() {
    for nos_override in [false, true] {
        let src = Src::new(&NosferatuData { nos_override }.into(), None).unwrap();
        let mut sol = SolOwned::new(&src);
        let mut target = sol.add_fit();
        let target_fit = target.get_fit_id();
        let target_ship = target.set_ship(ItemTypeId::from_i32(602), None, None).get_item_id();
        let mut source = sol.add_fit();
        let source_fit = source.get_fit_id();
        source.set_ship(ItemTypeId::from_i32(602), None, None);
        let mut nos = source
            .add_module(
                ModRack::High,
                AddMode::Equip,
                ItemTypeId::from_i32(13001),
                ModuleState::Active,
            )
            .unwrap();
        let nos_id = nos.get_item_id();
        assert_eq!(nos.get_pyfa_capacitor_need().unwrap().into_f64(), -10.0);
        assert_eq!(nos.get_attr(&CAPACITOR_NEED).unwrap().modified.into_f64(), 0.0);
        assert_eq!(first_events(&mut sol, source_fit), [(0.0, 110.0)]);
        assert_eq!(drain(&mut sol, target_fit, true), 0.0);
        assert!(first_events(&mut sol, target_fit).is_empty());

        let mut nos = sol.get_module(&nos_id).unwrap();
        nos.add_proj(&target_ship).unwrap();
        for (range, amount) in [(5_000.0, 10.0), (7_500.0, 5.0)] {
            sol.get_module(&nos_id)
                .unwrap()
                .get_proj(&target_ship)
                .unwrap()
                .set_range(PValue::from_f64_checked(range).unwrap());
            assert_eq!(drain(&mut sol, target_fit, true), amount / 2.5);
            let ordinary = if nos_override { amount / 2.5 } else { 0.0 };
            assert_eq!(drain(&mut sol, target_fit, false), ordinary);
            assert_eq!(first_events(&mut sol, target_fit), [(0.0, 100.0 - amount)]);
        }

        let mut nos = sol.get_module(&nos_id).unwrap();
        nos.set_state(ModuleState::Online);
        assert_eq!(nos.get_pyfa_capacitor_need().unwrap().into_f64(), 0.0);
        assert_eq!(drain(&mut sol, target_fit, true), 0.0);
        assert!(first_events(&mut sol, target_fit).is_empty());
    }
}
