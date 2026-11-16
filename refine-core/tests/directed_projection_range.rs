use refine_core::{
    AddMode, AttrId, Coordinates, DogmaEffectId, EffectId, EffectMode, EveAttrId, ItemCommon, ItemId, ItemTypeId,
    ModRack, ModuleState, PValue, ProjRange, RangedProj, Sol, SolOwned, Src, Value,
    ed::{
        EAttr, EAttrId, EData, EEffect, EEffectCatId, EEffectId, EFloat, EItem, EItemAttr, EItemCatId, EItemEffect,
        EItemGroup, EItemGrpId, EItemId, EveDataHandlerCore, err::EveDataHandlerError,
    },
};

const KESTREL: i32 = 602;
const WIDER_SHIP: i32 = 603;
const WEB: i32 = 527;
const WEAKER_WEB: i32 = 528;
const CHARGED_WEB: i32 = 529;
const PAINT_CHARGE: i32 = 530;
const STRONGER_PAINT_CHARGE: i32 = 531;
const WEB_EFFECT: i32 = 6426;
const PAINT_EFFECT: i32 = 6425;
const ACTIVATE_CHARGE_EFFECT: i32 = 101;
const VELOCITY: i32 = 37;
const SPEED_FACTOR: i32 = 20;
const OPTIMAL: i32 = 54;
const RADIUS: i32 = 162;
const SIG_RADIUS: i32 = 552;
const SIG_BONUS: i32 = 554;

#[test]
fn cyclic_webs_use_independent_ranges_and_clear_to_current_geometry() {
    let mut sol = new_sol();
    let (left_ship, left_web) = add_fit(&mut sol, 0.0, WEB);
    let (right_ship, right_web) = add_fit(&mut sol, 12_000.0, WEB);
    sol.get_module(&left_web).unwrap().add_proj(&right_ship).unwrap();
    sol.get_module(&right_web).unwrap().add_proj(&left_ship).unwrap();

    assert_range(
        with_projection(&mut sol, &left_web, &right_ship, |proj| proj.get_range()),
        12_000.0,
        11_900.0,
    );
    assert_attr(&mut sol, &left_ship, VELOCITY, 406.25);
    assert_attr(&mut sol, &right_ship, VELOCITY, 406.25);
    with_projection(&mut sol, &left_web, &right_ship, |mut proj| {
        proj.set_range(distance(1000.0))
    });
    with_projection(&mut sol, &right_web, &left_ship, |mut proj| {
        proj.set_range(distance(12_000.0))
    });
    assert_attr(&mut sol, &left_ship, VELOCITY, 406.25);
    assert_attr(&mut sol, &right_ship, VELOCITY, 162.5);

    let mut left = sol.get_ship(&left_ship).unwrap();
    left.set_coordinates(coordinates(100.0));
    left.set_type_id(ItemTypeId::from_i32(WIDER_SHIP));
    let mut right = sol.get_ship(&right_ship).unwrap();
    right.set_coordinates(coordinates(10_300.0));
    right.set_type_id(ItemTypeId::from_i32(WIDER_SHIP));
    assert_range(
        with_projection(&mut sol, &left_web, &right_ship, |proj| proj.get_range()),
        1000.0,
        1000.0,
    );
    assert_attr(&mut sol, &left_ship, VELOCITY, 406.25);
    assert_attr(&mut sol, &right_ship, VELOCITY, 162.5);

    with_projection(&mut sol, &right_web, &left_ship, |mut proj| proj.clear_range());
    assert_range(
        with_projection(&mut sol, &right_web, &left_ship, |proj| proj.get_range()),
        10_200.0,
        9900.0,
    );
    assert_attr(&mut sol, &left_ship, VELOCITY, 162.5);
    sol.get_ship(&right_ship)
        .unwrap()
        .set_coordinates(coordinates(13_000.0));
    assert_attr(&mut sol, &left_ship, VELOCITY, 406.25);
    assert_attr(&mut sol, &right_ship, VELOCITY, 162.5);
    assert_consistent(&sol);
}

#[test]
fn changing_range_invalidates_cached_module_and_charge_effects() {
    let mut sol = new_sol();
    let (_, web) = add_fit(&mut sol, 0.0, CHARGED_WEB);
    let (target, _) = add_fit(&mut sol, 12_000.0, WEB);
    let mut module = sol.get_module(&web).unwrap();
    module.set_effect_mode(
        &EffectId::Dogma(DogmaEffectId::from_i32(WEB_EFFECT)),
        EffectMode::StateCompliance,
    );
    module.add_proj(&target).unwrap().set_range(distance(1000.0));
    module.set_charge_type_id(ItemTypeId::from_i32(PAINT_CHARGE));
    assert_attr(&mut sol, &target, VELOCITY, 162.5);
    assert_attr(&mut sol, &target, SIG_RADIUS, 60.0);

    with_projection(&mut sol, &web, &target, |mut proj| proj.set_range(distance(12_000.0)));
    assert_attr(&mut sol, &target, VELOCITY, 406.25);
    assert_attr(&mut sol, &target, SIG_RADIUS, 50.0);
    with_projection(&mut sol, &web, &target, |mut proj| proj.set_range(distance(1000.0)));
    assert_attr(&mut sol, &target, VELOCITY, 162.5);
    assert_attr(&mut sol, &target, SIG_RADIUS, 60.0);

    sol.get_module(&web).unwrap().set_state(ModuleState::Online);
    assert_attr(&mut sol, &target, VELOCITY, 406.25);
    assert_attr(&mut sol, &target, SIG_RADIUS, 50.0);
    with_projection(&mut sol, &web, &target, |mut proj| proj.set_range(distance(0.0)));
    sol.get_module(&web).unwrap().set_state(ModuleState::Active);
    sol.get_module(&web)
        .unwrap()
        .set_charge_type_id(ItemTypeId::from_i32(STRONGER_PAINT_CHARGE));
    assert_attr(&mut sol, &target, VELOCITY, 162.5);
    assert_attr(&mut sol, &target, SIG_RADIUS, 70.0);

    sol.get_module(&web)
        .unwrap()
        .set_type_id(ItemTypeId::from_i32(WEAKER_WEB));
    assert_range(
        with_projection(&mut sol, &web, &target, |proj| proj.get_range()),
        0.0,
        0.0,
    );
    assert_attr(&mut sol, &target, VELOCITY, 203.125);
    assert_attr(&mut sol, &target, SIG_RADIUS, 50.0);
    with_projection(&mut sol, &web, &target, |mut proj| proj.clear_range());
    assert_attr(&mut sol, &target, VELOCITY, 406.25);
    assert_consistent(&sol);
}

#[test]
fn rollback_restores_range_edits_and_removed_edges() {
    let mut sol = new_sol();
    let (_, web) = add_fit(&mut sol, 0.0, WEB);
    let (target, _) = add_fit(&mut sol, 12_000.0, WEB);
    sol.get_module(&web)
        .unwrap()
        .add_proj(&target)
        .unwrap()
        .set_range(distance(0.0));
    assert_attr(&mut sol, &target, VELOCITY, 162.5);

    let mut tx = sol.begin_tx();
    with_projection(&mut tx, &web, &target, |mut proj| proj.clear_range());
    assert_attr(&mut tx, &target, VELOCITY, 406.25);
    tx.rollback();
    assert_attr(&mut sol, &target, VELOCITY, 162.5);

    let mut tx = sol.begin_tx();
    with_projection(&mut tx, &web, &target, |proj| proj.remove());
    assert_attr(&mut tx, &target, VELOCITY, 406.25);
    tx.rollback();
    assert_range(
        with_projection(&mut sol, &web, &target, |proj| proj.get_range()),
        0.0,
        0.0,
    );
    assert_attr(&mut sol, &target, VELOCITY, 162.5);
    assert_consistent(&sol);
}

fn with_projection<T>(sol: &mut Sol, source: &ItemId, target: &ItemId, f: impl FnOnce(RangedProj<'_>) -> T) -> T {
    let mut module = sol.get_module(source).unwrap();
    f(module.get_proj(target).unwrap())
}

fn assert_range(range: ProjRange, c2c: f64, s2s: f64) {
    assert_eq!(range.c2c.into_f64(), c2c);
    assert_eq!(range.s2s.into_f64(), s2s);
}

fn assert_attr(sol: &mut Sol, item: &ItemId, attr: i32, expected: f64) {
    let attr = AttrId::Eve(EveAttrId::from_i32(attr));
    let actual = sol.get_ship(item).unwrap().get_attr(&attr).unwrap().modified.into_f64();
    assert!(
        (actual - expected).abs() < 1e-10,
        "{attr}: got {actual}, expected {expected}"
    );
}

fn assert_consistent(_sol: &Sol) {
    #[cfg(feature = "dev")]
    assert!(_sol.consistency_check());
}

fn distance(value: f64) -> PValue {
    PValue::from_f64_checked(value).unwrap()
}

fn coordinates(x: f64) -> Coordinates {
    let zero = Value::from_f64_checked(0.0).unwrap();
    Coordinates::new(Value::from_f64_checked(x).unwrap(), zero, zero)
}

fn add_fit(sol: &mut Sol, x: f64, web: i32) -> (ItemId, ItemId) {
    let mut fit = sol.add_fit();
    let ship = fit
        .set_ship(ItemTypeId::from_i32(KESTREL), Some(coordinates(x)), None)
        .get_item_id();
    let web = fit
        .add_module(
            ModRack::Mid,
            AddMode::Append,
            ItemTypeId::from_i32(web),
            ModuleState::Active,
        )
        .unwrap()
        .get_item_id();
    (ship, web)
}

fn new_sol() -> SolOwned {
    SolOwned::new(&Src::new(&ProjectionData.into(), None).unwrap())
}

#[derive(Debug)]
struct ProjectionData;
impl EveDataHandlerCore for ProjectionData {
    fn get_data_version(&self) -> Result<String, EveDataHandlerError> {
        Ok("directed projection range tests".into())
    }

    fn get_data(&self) -> Result<EData, EveDataHandlerError> {
        let mut data = EData::new();
        for (group, category) in [(25, 6), (65, 7), (384, 8)] {
            data.groups.data.push(EItemGroup {
                id: EItemGrpId::from_i32(group),
                category_id: EItemCatId::from_i32(category),
            });
        }
        for attr in [VELOCITY, SPEED_FACTOR, OPTIMAL, RADIUS, SIG_RADIUS, SIG_BONUS] {
            data.attrs.data.push(EAttr {
                id: EAttrId::from_i32(attr),
                stackable: true,
                high_is_good: true,
                default_value: EFloat::from_f64(0.0),
                min_attr_id: None,
                max_attr_id: None,
            });
        }
        for (effect, category, range) in [
            (WEB_EFFECT, 2, true),
            (PAINT_EFFECT, 2, true),
            (ACTIVATE_CHARGE_EFFECT, 1, false),
        ] {
            data.effects.data.push(EEffect {
                id: EEffectId::from_i32(effect),
                category_id: EEffectCatId::from_i32(category),
                is_assistance: false,
                is_offensive: range,
                discharge_attr_id: None,
                duration_attr_id: None,
                range_attr_id: range.then(|| EAttrId::from_i32(OPTIMAL)),
                falloff_attr_id: None,
                tracking_attr_id: None,
                usage_chance_attr_id: None,
                resist_attr_id: None,
                mods: Vec::new(),
            });
        }

        let ship_attrs = [(VELOCITY, 406.25), (SIG_RADIUS, 50.0)];
        let web_attrs = [(SPEED_FACTOR, -60.0), (OPTIMAL, 10_000.0)];
        for (id, group, radius, attrs, effects) in [
            (KESTREL, 25, 50.0, ship_attrs, &[][..]),
            (WIDER_SHIP, 25, 150.0, ship_attrs, &[][..]),
            (WEB, 65, 0.0, web_attrs, &[WEB_EFFECT][..]),
            (
                WEAKER_WEB,
                65,
                0.0,
                [(SPEED_FACTOR, -50.0), (OPTIMAL, 10_000.0)],
                &[WEB_EFFECT][..],
            ),
            (
                CHARGED_WEB,
                65,
                0.0,
                web_attrs,
                &[ACTIVATE_CHARGE_EFFECT, WEB_EFFECT][..],
            ),
            (
                PAINT_CHARGE,
                384,
                0.0,
                [(SIG_BONUS, 20.0), (OPTIMAL, 10_000.0)],
                &[PAINT_EFFECT][..],
            ),
            (
                STRONGER_PAINT_CHARGE,
                384,
                0.0,
                [(SIG_BONUS, 40.0), (OPTIMAL, 10_000.0)],
                &[PAINT_EFFECT][..],
            ),
        ] {
            let item_id = EItemId::from_i32(id);
            data.items.data.push(EItem {
                id: item_id,
                group_id: EItemGrpId::from_i32(group),
                is_dynamic: false,
                capacity: None,
                mass: None,
                radius: Some(EFloat::from_f64(radius)),
                volume: None,
            });
            data.item_attrs.data.extend(attrs.map(|(attr, value)| EItemAttr {
                item_id,
                attr_id: EAttrId::from_i32(attr),
                value: EFloat::from_f64(value),
            }));
            data.item_effects
                .data
                .extend(effects.iter().enumerate().map(|(index, &effect)| EItemEffect {
                    item_id,
                    effect_id: EEffectId::from_i32(effect),
                    is_default: index == 0,
                }));
        }
        Ok(data)
    }
}
