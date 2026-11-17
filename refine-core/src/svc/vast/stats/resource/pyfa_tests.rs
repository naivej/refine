use crate::{
    AddMode, AttrId, EveAttrId, ItemCommon, ItemTypeId, ModRack, ModuleState,
    OptionExt::Enabled,
    SolOwned, Src,
    ed::{
        EAttr, EAttrId, EData, EEffect, EEffectCatId, EEffectId, EFloat, EItem, EItemAttr, EItemCatId, EItemEffect,
        EItemGroup, EItemGrpId, EItemId, EveDataHandlerCore, err::EveDataHandlerError,
    },
    val::{ValOptions, ValResult},
};

const SHIP: EItemId = EItemId::from_i32(602);
const MODULE: EItemId = EItemId::from_i32(210);

#[derive(Debug)]
struct FittingTestData;

impl EveDataHandlerCore for FittingTestData {
    fn get_data(&self) -> Result<EData, EveDataHandlerError> {
        let mut data = EData::new();
        for (id, group, category) in [(SHIP, 25, EItemCatId::SHIP), (MODULE, 509, EItemCatId::MODULE)] {
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
            (SHIP, EAttrId::POWER_OUTPUT, 65.625),
            (SHIP, EAttrId::CPU_OUTPUT, 2.675),
            (SHIP, EAttrId::HI_SLOTS, 1.0),
            (MODULE, EAttrId::POWER, 65.63),
            (MODULE, EAttrId::CPU, 2.675),
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
        data.effects.data.push(EEffect {
            id: EEffectId::HI_POWER,
            category_id: EEffectCatId::from_i32(0),
            is_assistance: false,
            is_offensive: false,
            discharge_attr_id: None,
            duration_attr_id: None,
            range_attr_id: None,
            falloff_attr_id: None,
            tracking_attr_id: None,
            usage_chance_attr_id: None,
            resist_attr_id: None,
            mods: vec![],
        });
        data.item_effects.data.push(EItemEffect {
            item_id: MODULE,
            effect_id: EEffectId::HI_POWER,
            is_default: false,
        });
        Ok(data)
    }

    fn get_data_version(&self) -> Result<String, EveDataHandlerError> {
        Ok("synthetic fitting rounding regression".to_owned())
    }
}

#[test]
fn opt_in_fitting_rounding_keeps_attributes_stats_and_validators_consistent() {
    let src = Src::new(&FittingTestData.into(), None).unwrap();
    for (mut sol, cpu, power, valid) in [
        (SolOwned::new(&src), 2.68, 65.63, true),
        (SolOwned::new_with_pyfa_fitting_rounding(&src), 2.67, 65.62, false),
    ] {
        let mut fit = sol.add_fit();
        fit.set_ship(ItemTypeId::from_i32(SHIP.into_i32()), None, None);
        fit.add_module(
            ModRack::High,
            AddMode::Equip,
            ItemTypeId::from_i32(MODULE.into_i32()),
            ModuleState::Online,
        )
        .unwrap();
        let options = ValOptions::with_override_capacity(false, 2)
            .with_cpu(Enabled)
            .with_powergrid(Enabled);
        assert_eq!(fit.validate_fast(&options), valid);
        let verbose = fit.validate_verbose(&options);
        assert!(!verbose.cpu.is_failed());
        assert_eq!(verbose.powergrid.is_failed(), !valid);
        if let ValResult::Failed(failure) = verbose.powergrid {
            assert_eq!(failure.used.into_f64(), 65.63);
            assert_eq!(failure.max.unwrap().into_f64(), power);
        }
        let mut ship = fit.get_ship().unwrap();
        let attr = ship
            .get_attr(&AttrId::Eve(EveAttrId::from_i32(EAttrId::POWER_OUTPUT.into_i32())))
            .unwrap();
        assert_eq!(attr.base.into_f64(), 65.625);
        assert_eq!(attr.modified.into_f64(), power);
        let mut stats = fit.stats();
        let cpu_stat = stats.get_stat_cpu();
        assert_eq!(cpu_stat.used.into_f64(), cpu);
        assert_eq!(cpu_stat.output.unwrap().into_f64(), cpu);
        let power_stat = stats.get_stat_powergrid();
        assert_eq!(power_stat.used.into_f64(), 65.63);
        assert_eq!(power_stat.output.unwrap().into_f64(), power);
    }
}
