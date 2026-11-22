use crate::{
    ad::AEffectId,
    rd::RAttrId,
    svc::{SvcCtx, calc::Calc},
    ud::{UItem, UItemId},
};

pub(in crate::svc) fn is_oattr_flag_set(
    ctx: &SvcCtx,
    calc: &mut Calc,
    item_uid: UItemId,
    attr_rid: Option<RAttrId>,
) -> Option<bool> {
    is_attr_flag_set(ctx, calc, item_uid, attr_rid?)
}

pub(in crate::svc) fn is_attr_flag_set(
    ctx: &SvcCtx,
    calc: &mut Calc,
    item_uid: UItemId,
    attr_rid: RAttrId,
) -> Option<bool> {
    calc.get_item_attr_oextra(ctx, item_uid, attr_rid)
        .map(|v| v.is_flag_set())
}

pub(in crate::svc) fn is_pyfa_assistance_blocked(ctx: &SvcCtx, calc: &mut Calc, recipient_uid: UItemId) -> bool {
    if is_oattr_flag_set(ctx, calc, recipient_uid, ctx.ac().disallow_assistance).unwrap_or(false) {
        return true;
    }
    let UItem::Ship(ship) = ctx.u_data.items.get(recipient_uid) else {
        return false;
    };
    let Some(field_effect) = ctx.u_data.r_data.get_effect_rid_by_aid(&AEffectId::WARP_DISRUPT_SPHERE) else {
        return false;
    };
    // Pyfa Effect3380 blocks incoming assistance while the local effect
    // runs. Refine's native field updater does not write that ship flag.
    ctx.u_data
        .fits
        .get(ship.get_fit_uid())
        .iter_module_uids()
        .any(|module_uid| {
            ctx.u_data
                .items
                .get(module_uid)
                .get_reffs()
                .is_some_and(|effects| effects.contains(field_effect))
        })
}
