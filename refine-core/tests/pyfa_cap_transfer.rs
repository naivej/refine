//! Regression for Pyfa's uncapped incoming capacitor transfer stat.
use refine_core::{
    AddMode, ItemCommon, ItemTypeId, ModRack, ModuleState, PValue, SolOwned, Src,
    ed::{
        EAttr, EAttrId, EData, EEffect, EEffectCatId, EEffectId, EFloat, EItem, EItemAttr, EItemCatId, EItemEffect,
        EItemGroup, EItemGrpId, EItemId, EveDataHandlerCore, err::EveDataHandlerError,
    },
    stats::StatCapBlcSrcKinds,
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
            (ship, 482, 330.0),
            (ship, 14, 4.0),
            (module, 90, 351.0),
            (module, 73, 5000.0),
            (module, 54, 100000.0),
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
        for (id, activation) in [(12, false), (6184, true)] {
            let id = EEffectId::from_i32(id);
            data.effects.data.push(EEffect {
                id,
                category_id: EEffectCatId::from_i32(if activation { 1 } else { 0 }),
                is_assistance: false,
                is_offensive: false,
                discharge_attr_id: None,
                duration_attr_id: activation.then_some(EAttrId::from_i32(73)),
                range_attr_id: activation.then_some(EAttrId::from_i32(54)),
                falloff_attr_id: None,
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
        Ok("incoming transfer regression".into())
    }
}

#[test]
fn pyfa_incoming_transfer_does_not_clip_a_cycle_to_recipient_capacity() {
    let source = Src::new(&Data.into(), None).unwrap();
    let mut sol = SolOwned::new(&source);
    let mut target = sol.add_fit();
    let target_id = target.get_fit_id();
    let ship = target.set_ship(ItemTypeId::from_i32(1001), None, None).get_item_id();
    let mut emitter = sol.add_fit();
    emitter.set_ship(ItemTypeId::from_i32(1001), None, None);
    let mut module = emitter
        .add_module(
            ModRack::High,
            AddMode::Equip,
            ItemTypeId::from_i32(1002),
            ModuleState::Active,
        )
        .unwrap();
    let mut projection = module.add_proj(&ship).unwrap();
    projection.set_range(PValue::from_f64_checked(0.0).unwrap());
    let mut fit = sol.get_fit(&target_id).unwrap();
    let mut stats = fit.stats();
    let base = StatCapBlcSrcKinds::new(false).with_incoming_transfers(true);
    let ordinary = stats.get_stat_cap_balance(base, Default::default()).unwrap().into_f64();
    let pyfa = stats
        .get_stat_cap_balance(base.with_uncapped_incoming_transfers(true), Default::default())
        .unwrap()
        .into_f64();
    assert_eq!(ordinary, 66.0);
    assert_eq!(pyfa, 70.2);
}
