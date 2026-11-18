//! Regression for Pyfa Remote Repairs graph range rounding.
use refine_core::{
    AddMode, ItemCommon, ItemTypeId, ModRack, ModuleState, PValue, SolOwned, Src,
    ed::{
        EAttr, EAttrId, EData, EEffect, EEffectCatId, EEffectId, EFloat, EItem, EItemAttr, EItemCatId, EItemEffect,
        EItemGroup, EItemGrpId, EItemId, EveDataHandlerCore, err::EveDataHandlerError,
    },
    stats::StatOutRepItemKinds,
};

#[derive(Debug)]
struct Data;
impl EveDataHandlerCore for Data {
    fn get_data(&self) -> Result<EData, EveDataHandlerError> {
        let mut data = EData::new();
        let ship = EItemId::from_i32(1001);
        let module = EItemId::from_i32(1002);
        for (id, group, category) in [
            (ship, 25, EItemCatId::from_i32(6)),
            (module, 40, EItemCatId::from_i32(7)),
        ] {
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
        for (item_id, attr, value) in [
            (ship, 263, 10000.0),
            (ship, 14, 4.0),
            (module, 68, 425.0),
            (module, 73, 5000.0),
            (module, 54, 8040.0),
            (module, 2044, 12000.0),
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
        for (id, activation) in [(12, false), (6186, true)] {
            let id = EEffectId::from_i32(id);
            data.effects.data.push(EEffect {
                id,
                category_id: EEffectCatId::from_i32(if activation { 1 } else { 0 }),
                is_assistance: false,
                is_offensive: false,
                discharge_attr_id: None,
                duration_attr_id: activation.then_some(EAttrId::from_i32(73)),
                range_attr_id: activation.then_some(EAttrId::from_i32(54)),
                falloff_attr_id: activation.then_some(EAttrId::from_i32(2044)),
                tracking_attr_id: None,
                usage_chance_attr_id: None,
                resist_attr_id: None,
                mods: vec![],
            });
            data.item_effects.data.push(EItemEffect {
                item_id: module,
                effect_id: id,
                is_default: activation,
            });
        }
        Ok(data)
    }
    fn get_data_version(&self) -> Result<String, EveDataHandlerError> {
        Ok("remote repair graph regression".into())
    }
}

#[test]
fn pyfa_graph_rounds_native_falloff_before_aggregating_each_source() {
    let source = Src::new(&Data.into(), None).unwrap();
    let mut sol = SolOwned::new(&source);
    let mut target = sol.add_fit();
    let ship = target.set_ship(ItemTypeId::from_i32(1001), None, None).get_item_id();
    let mut emitter = sol.add_fit();
    let emitter_id = emitter.get_fit_id();
    emitter.set_ship(ItemTypeId::from_i32(1001), None, None);
    let mut module = emitter
        .add_module(
            ModRack::High,
            AddMode::Equip,
            ItemTypeId::from_i32(1002),
            ModuleState::Active,
        )
        .unwrap();
    let module_id = module.get_item_id();
    module.add_proj(&ship).unwrap();
    for (range, expected) in [
        (28855.818961822133, 10.558802),
        (29027.625676544947, 10.2000085),
        (32120.310130906986, 5.21467095),
        (35105.77721314129, 2.500462),
    ] {
        sol.get_module(&module_id)
            .unwrap()
            .get_proj(&ship)
            .unwrap()
            .set_range(PValue::from_f64_checked(range).unwrap());
        let mut fit = sol.get_fit(&emitter_id).unwrap();
        let mut stats = fit.stats();
        let ordinary = stats
            .get_stat_outgoing_rps_applied(StatOutRepItemKinds::default(), Default::default(), &ship)
            .unwrap()
            .shield
            .into_f64();
        let pyfa = stats
            .get_stat_outgoing_rps_applied(
                StatOutRepItemKinds::default().with_pyfa_graph_range_rounding(true),
                Default::default(),
                &ship,
            )
            .unwrap()
            .shield
            .into_f64();
        assert!(
            (pyfa - expected).abs() < 1e-10,
            "{range}: got {pyfa}, expected {expected}"
        );
        assert!(
            (ordinary - expected).abs() > 1e-8,
            "ordinary stat unexpectedly rounded: {ordinary}"
        );
    }
}
