use super::StatDmgItemKinds;
use crate::{
    MinionState, PValue, RearmMinion, Value,
    ad::{AAttrId, AEffectId, AItemId},
    misc::DmgKinds,
    nd::NEffectOutputGetter,
    stats::StatTimeOptions,
    svc::{Calc, SvcCtx},
    ud::{UItem, UItemId},
};

const FIRBOLG_I: AItemId = AItemId::from_i32(23059);

pub(super) fn get_pyfa_fighter_damage(
    ctx: &SvcCtx,
    calc: &mut Calc,
    item_uid: UItemId,
    item: &UItem,
    time_options: StatTimeOptions,
    item_kinds: Option<StatDmgItemKinds>,
) -> Option<(DmgKinds<PValue>, DmgKinds<PValue>)> {
    let (true, UItem::Fighter(fighter), StatTimeOptions::Sim(sim)) =
        (ctx.u_data.pyfa_damage_compatibility, item, time_options)
    else {
        return None;
    };
    if fighter.get_fighter_state() != MinionState::Engaging
        || sim.time.is_some_and(|time| time.into_f64() > 0.0)
        || ctx.u_data.get_item_rearm_minion(item_uid, sim.rearm_minions) != RearmMinion::OnFirstEmpty
    {
        return None;
    }
    let base = fighter.get_r_item_base()?;
    if base.aid != FIRBOLG_I || !base.is_light_fighter {
        return None;
    }
    let r_data = &ctx.u_data.r_data;
    let turret_rid = r_data.get_effect_rid_by_aid(&AEffectId::FTR_ABIL_ATTACK_M)?;
    let missile_rid = r_data.get_effect_rid_by_aid(&AEffectId::FTR_ABIL_MISSILES)?;
    if !base.effect_rids.contains(&turret_rid) || !base.effect_rids.contains(&missile_rid) {
        return None;
    }
    // This covers the light attack loadout with two attacks and an uncharged MWD.
    // Other cycling abilities may change the recall schedule and need their own qualification.
    if base.effect_rids.iter().any(|&rid| {
        let effect = r_data.get_effect_by_rid(rid);
        effect.is_active_with_duration
            && rid != turret_rid
            && rid != missile_rid
            && !(effect.aid == AEffectId::FTR_ABIL_MICRO_WARP_DRIVE
                && base.get_effect_data(rid).ability_charge_count.is_none())
    }) || base.get_effect_data(turret_rid).ability_charge_count.is_some()
    {
        return None;
    }
    let missile_data = base.get_effect_data(missile_rid);
    let charges = missile_data.ability_charge_count?;
    let turret = r_data.get_effect_by_rid(turret_rid);
    let missile = r_data.get_effect_by_rid(missile_rid);
    let [Some(turret_cycle_ms), Some(missile_cycle_ms)] = [turret, missile].map(|effect| {
        calc.get_item_oattr_oextra(ctx, item_uid, effect.duration_attr_rid)
            .map(Value::into_f64)
    }) else {
        return None;
    };
    // Qualify the pinned Firbolg attack timings. Modified attack cycles may
    // require Pyfa's rounding of ratios and partial-cycle durations.
    if turret_cycle_ms != 5000.0
        || missile_cycle_ms != 14000.0
        || charges.into_u32() != 12
        || missile_data.ability_charge_reload_duration.into_f64() != 4.0
    {
        return None;
    }
    let turret_cycle = PValue::from_f64_unchecked(turret_cycle_ms / 1000.0);
    let missile_cycle = PValue::from_f64_unchecked(missile_cycle_ms / 1000.0);
    let refuel_attr = r_data.get_attr_rid_by_aid(&AAttrId::FTR_REFUELING_TIME);
    let refuel_ms = calc.get_item_oattr_oextra(ctx, item_uid, refuel_attr)?;
    let running = fighter.get_reffs()?;

    let mut volleys = [DmgKinds::default(); 2];
    for (volley, (rid, effect)) in volleys.iter_mut().zip([(turret_rid, turret), (missile_rid, missile)]) {
        if running.contains(rid)
            && item_kinds.is_none_or(|kinds| kinds.resolve(item, effect))
            && let Some(output) = effect
                .normal_dmg
                .and_then(|spec| spec.base.get(ctx, calc, item_uid, effect, ()))
        {
            *volley = output.get_instance();
        }
    }
    let [turret_volley, missile_volley] = volleys;

    // Disabled abilities still determine the Pyfa recall schedule. An attack fires at the
    // start of its cycle, so a partial final turret cycle counts as a full volley.
    let charges = charges.into_pvalue();
    let in_space = missile_cycle * charges;
    let refuel = PValue::from_finite_value_clamped(refuel_ms / Value::THOUSAND)
        + missile_data.ability_charge_reload_duration * charges;
    let turret_shots = (in_space / turret_cycle).ceil_unerr();
    let mut coordinated_damage = turret_volley * turret_shots;
    coordinated_damage += missile_volley * charges;
    let coordinated_dps = coordinated_damage / (in_space + refuel);
    let unlimited_dps = turret_volley / turret_cycle;
    let dps = if unlimited_dps.get_total() >= coordinated_dps.get_total() {
        unlimited_dps
    } else {
        coordinated_dps
    };
    let mut volley = turret_volley;
    volley += missile_volley;
    Some((dps, volley))
}
