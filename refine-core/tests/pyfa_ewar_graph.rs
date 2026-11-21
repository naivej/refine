use refine_core::{
    AddMode, AttrId, EveAttrId, ItemCommon, ItemTypeId, ModRack, ModuleState, PValue, SolOwned, Src,
    ed::{
        EAttr, EAttrId, EData, EEffect, EEffectCatId, EEffectId, EFloat, EItem, EItemAttr, EItemCatId, EItemEffect,
        EItemGroup, EItemGrpId, EItemId, EveDataHandlerCore, err::EveDataHandlerError,
    },
    pyfa_graph::{PyfaEwarSource, pyfa_ewar_neut_gj_per_s, pyfa_ewar_web_percentage},
    stats::StatNeutItemKinds,
};

#[derive(Debug)]
struct KestrelEwarData;

impl EveDataHandlerCore for KestrelEwarData {
    fn get_data(&self) -> Result<EData, EveDataHandlerError> {
        let mut data = EData::new();
        for (item, group, category) in [(602, 25, 6), (527, 65, 7), (12267, 71, 7)] {
            let group_id = EItemGrpId::from_i32(group);
            data.groups.data.push(EItemGroup {
                id: group_id,
                category_id: EItemCatId::from_i32(category),
            });
            data.items.data.push(EItem {
                id: EItemId::from_i32(item),
                group_id,
                is_dynamic: false,
                capacity: None,
                mass: None,
                radius: None,
                volume: None,
            });
        }
        for attr in [14, 20, 54, 73, 76, 97, 482, 552, 2044, 2451] {
            data.attrs.data.push(EAttr {
                id: EAttrId::from_i32(attr),
                stackable: true,
                high_is_good: true,
                default_value: EFloat::from_f64(0.0),
                min_attr_id: None,
                max_attr_id: None,
            });
        }
        for (item, attr, value) in [
            (602, 14, 4.0),
            (602, 76, 50000.0),
            (602, 482, 10.0),
            (602, 552, 40.0),
            (527, 20, -60.0),
            (527, 54, 10000.0),
            (527, 73, 5000.0),
            (527, 2044, 0.0),
            (12267, 54, 10000.0),
            (12267, 73, 12000.0),
            (12267, 97, 180.0),
            (12267, 2044, 5000.0),
            (12267, 2451, 40.0),
        ] {
            data.item_attrs.data.push(EItemAttr {
                item_id: EItemId::from_i32(item),
                attr_id: EAttrId::from_i32(attr),
                value: EFloat::from_f64(value),
            });
        }
        for (item, effect, activation) in [
            (527, 13, false),
            (527, 6426, true),
            (12267, 12, false),
            (12267, 6187, true),
        ] {
            let id = EEffectId::from_i32(effect);
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
                item_id: EItemId::from_i32(item),
                effect_id: id,
                is_default: activation,
            });
        }
        Ok(data)
    }
    fn get_data_version(&self) -> Result<String, EveDataHandlerError> {
        Ok("EWAR pinned item values".into())
    }
}

#[test]
fn resolved_native_module_attributes_feed_both_ewar_graph_series() {
    let source = Src::new(&KestrelEwarData.into(), None).unwrap();
    let mut sol = SolOwned::new(&source);
    let mut fit = sol.add_fit();
    fit.set_ship(ItemTypeId::from_i32(602), None, None);
    let fit_id = fit.get_fit_id();
    let mut web = fit
        .add_module(
            ModRack::Mid,
            AddMode::Equip,
            ItemTypeId::from_i32(527),
            ModuleState::Active,
        )
        .unwrap();
    let get = |m: &mut refine_core::Module<'_>, id| {
        m.get_attr(&AttrId::Eve(EveAttrId::from_i32(id)))
            .unwrap()
            .modified
            .into_f64()
    };
    let w = PyfaEwarSource {
        strength: get(&mut web, 20),
        optimal_m: get(&mut web, 54),
        falloff_m: get(&mut web, 2044),
        cycle_s: get(&mut web, 73) / 1000.0,
    };
    assert!((pyfa_ewar_web_percentage(&[w, w], 0.0, 50000.0, false, 0.25).unwrap() - 66.51071952480983).abs() < 1e-10);
    let mut fit = sol.get_fit(&fit_id).unwrap();
    let mut neut = fit
        .add_module(
            ModRack::High,
            AddMode::Equip,
            ItemTypeId::from_i32(12267),
            ModuleState::Active,
        )
        .unwrap();
    let n = PyfaEwarSource {
        strength: get(&mut neut, 97),
        optimal_m: get(&mut neut, 54),
        falloff_m: get(&mut neut, 2044),
        cycle_s: get(&mut neut, 73) / 1000.0,
    };
    let neut_id = neut.get_item_id();
    assert_eq!(pyfa_ewar_neut_gj_per_s(&[n], 15000.0, 50000.0, false, 0.0), Some(7.5));
    let native = sol
        .get_fit(&fit_id)
        .unwrap()
        .stats()
        .get_stat_outgoing_nps(StatNeutItemKinds::default(), Default::default());
    assert_eq!(native.into_f64(), 15.0);
    let target = sol
        .add_fit()
        .set_ship(ItemTypeId::from_i32(602), None, None)
        .get_item_id();
    sol.get_module(&neut_id)
        .unwrap()
        .add_proj(&target)
        .unwrap()
        .set_range(PValue::from_f64_checked(0.0).unwrap());
    let applied = sol
        .get_fit(&fit_id)
        .unwrap()
        .stats()
        .get_stat_outgoing_nps_applied(StatNeutItemKinds::default(), Default::default(), &target)
        .unwrap();
    assert!((applied.into_f64() - 10.0 / 12.0).abs() < 1e-10);
    assert_eq!(pyfa_ewar_neut_gj_per_s(&[n], 0.0, 50000.0, false, 0.0), Some(15.0));
}

#[test]
fn ewar_lock_guard_suppresses_in_range_sources_only_when_enabled() {
    let web = PyfaEwarSource {
        optimal_m: 100_000.0,
        falloff_m: 0.0,
        strength: -60.0,
        cycle_s: 1.0,
    };
    let neut = PyfaEwarSource {
        optimal_m: 100_000.0,
        falloff_m: 5000.0,
        strength: 180.0,
        cycle_s: 12.0,
    };
    for (distance, expected_web, expected_neut) in
        [(49_999.0, 60.0, 15.0), (50_000.0, 60.0, 15.0), (50_001.0, 0.0, 0.0)]
    {
        assert!(
            (pyfa_ewar_web_percentage(&[web], distance, 50_000.0, false, 0.0).unwrap() - expected_web).abs() < 1e-12
        );
        assert_eq!(
            pyfa_ewar_neut_gj_per_s(&[neut], distance, 50_000.0, false, 0.0).unwrap(),
            expected_neut
        );
        assert!((pyfa_ewar_web_percentage(&[web], distance, 50_000.0, true, 0.0).unwrap() - 60.0).abs() < 1e-12);
        assert_eq!(
            pyfa_ewar_neut_gj_per_s(&[neut], distance, 50_000.0, true, 0.0).unwrap(),
            15.0
        );
    }
}

#[test]
#[ignore = "requires independent pinned Pyfa EWAR graph replay"]
fn ewar_graph_sources_match_pinned_pyfa_points() {
    let path = std::env::var_os("RUFA_PYFA_EWAR_GRAPH_CASES").expect("set Pyfa EWAR graph fixture");
    let file = std::fs::read_to_string(path).unwrap();
    let mut count = 0;
    for (index, line) in file.lines().enumerate() {
        let fields: Vec<_> = line.split('\t').collect();
        let kind = fields[0];
        let numbers: Vec<f64> = fields[1..].iter().map(|s| s.parse().unwrap()).collect();
        let [
            distance,
            lock,
            ignore,
            resist,
            optimal,
            falloff,
            strength,
            cycle,
            active,
            expected,
        ]: [f64; 10] = numbers.try_into().unwrap();
        let source = PyfaEwarSource {
            optimal_m: optimal,
            falloff_m: falloff,
            strength,
            cycle_s: cycle,
        };
        let sources = vec![source; active as usize];
        let actual = match kind {
            "web" => pyfa_ewar_web_percentage(&sources, distance, lock, ignore == 1.0, resist),
            "neutralizer" => pyfa_ewar_neut_gj_per_s(&sources, distance, lock, ignore == 1.0, resist),
            _ => panic!("unexpected kind"),
        }
        .unwrap();
        assert!(
            (actual - expected).abs() <= 1e-8_f64.max(1e-9 * actual.abs().max(expected.abs())),
            "line {} {} distance {} resistance {}: actual {}, expected {}",
            index + 1,
            kind,
            distance,
            resist,
            actual,
            expected
        );
        count += 1;
    }
    assert_eq!(count, 1056, "incomplete fixture");
}
