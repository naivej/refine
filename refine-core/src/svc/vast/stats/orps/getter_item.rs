use crate::{
    Count, OptionalReload, PValue,
    ad::AEffectId,
    nd::NEffectGeneralOutputGetter,
    pyfa_graph::{RemoteRepairCycle, RemoteRepairReload, RemoteRepairSchedule},
    rd::{REffect, REffectProjOpcSpec},
    stats::{StatCritOptions, StatOutReps, StatTimeOptions, StatTimeOptionsSim},
    svc::{
        Calc, SvcCtx, Vast,
        cycle::{CseqCache, CycleSeq, CyclingOptions, get_item_cseq_map},
        err::IntStatItemError,
        vast::{
            aggr::{AggrIterData, SeqAccum, aggr_proj_burst, aggr_proj_iter, aggr_proj_looped, aggr_proj_time},
            stats::item_checks::check_drone_fighter_module,
        },
    },
    ud::UItemId,
};

impl Vast {
    pub(in crate::svc) fn get_stat_item_outgoing_rps(
        cseq_cache: &mut CseqCache,
        ctx: &SvcCtx,
        calc: &mut Calc,
        item_uid: UItemId,
        time_options: StatTimeOptions,
        projectee_uid: Option<UItemId>,
    ) -> Result<StatOutReps, IntStatItemError<!>> {
        check_drone_fighter_module(ctx.u_data, item_uid)?;
        let orps = StatOutReps {
            shield: get_orps(
                cseq_cache,
                ctx,
                calc,
                item_uid,
                time_options,
                projectee_uid,
                get_getter_shield,
            ),
            armor: get_orps(
                cseq_cache,
                ctx,
                calc,
                item_uid,
                time_options,
                projectee_uid,
                get_getter_armor,
            ),
            hull: get_orps(
                cseq_cache,
                ctx,
                calc,
                item_uid,
                time_options,
                projectee_uid,
                get_getter_hull,
            ),
        };
        Ok(orps)
    }
}

impl Vast {
    /// Resolve an immediate ordinary cycle or a fully charged, equal-cycle ancillary magazine.
    pub(in crate::svc) fn get_item_pyfa_shield_schedule(
        cseq_cache: &mut CseqCache,
        ctx: &SvcCtx,
        calc: &mut Calc,
        item_uid: UItemId,
        ancillary: bool,
    ) -> Option<RemoteRepairSchedule> {
        let cseq_map = get_item_cseq_map(
            cseq_cache,
            ctx,
            calc,
            item_uid,
            CyclingOptions::from_time_options(StatTimeOptions::Sim(StatTimeOptionsSim {
                optional_reloads: ancillary.then_some(OptionalReload::OnEmpty),
                ..Default::default()
            })),
        )?;
        let mut result = None;
        for (effect_rid, cseq) in cseq_map.iter() {
            let effect = ctx.u_data.r_data.get_effect_by_rid(effect_rid);
            let Some(ospec) = effect.outgoing_shield_rep else {
                continue;
            };
            let reload = match cseq {
                CycleSeq::LoopSin(cycle)
                    if !ancillary
                        && cycle.hard_dt.is_none()
                        && cycle.data.soft_dt.is_none()
                        && cycle.data.active.chargedness.is_none() =>
                {
                    None
                }
                CycleSeq::LoopLimSin(cycle)
                    if ancillary && effect.aid == AEffectId::SHIP_MOD_ANCILLARY_REMOTE_SHIELD_BOOSTER =>
                {
                    let (_, magazine_cycles, duration_s) = cycle.full_magazine()?;
                    Some(RemoteRepairReload {
                        magazine_cycles,
                        duration_s,
                    })
                }
                _ => return None,
            };
            let data @ AggrIterData::Regular(_) = aggr_proj_iter(
                ctx,
                calc,
                item_uid,
                effect,
                cseq,
                &ospec,
                (),
                StatCritOptions::default(),
                None,
            )?
            else {
                return None;
            };
            let first = data.iter().next()?;
            if first.output_duration_limit.is_some()
                || first.output.get_instance_count() != Count::ONE
                || result.is_some()
            {
                return None;
            }
            let amount = first.output.get_immediate_instance()?;
            result = Some(RemoteRepairSchedule {
                cycle: RemoteRepairCycle {
                    duration_s: first.cycle_duration.into_f64(),
                    amount_hp: amount.into_f64(),
                },
                reload,
            });
        }
        result
    }
}

fn get_orps<F>(
    cseq_cache: &mut CseqCache,
    ctx: &SvcCtx,
    calc: &mut Calc,
    item_uid: UItemId,
    time_options: StatTimeOptions,
    projectee_uid: Option<UItemId>,
    rep_ospec_getter: F,
) -> PValue
where
    F: Fn(&REffect) -> Option<REffectProjOpcSpec<NEffectGeneralOutputGetter>>,
{
    let mut orps = PValue::ZERO;
    let cycling_options = CyclingOptions::from_time_options(time_options);
    let Some(cseq_map) = get_item_cseq_map(cseq_cache, ctx, calc, item_uid, cycling_options) else {
        return orps;
    };
    for (effect_rid, cseq) in cseq_map.iter() {
        let effect = ctx.u_data.r_data.get_effect_by_rid(effect_rid);
        let Some(ospec) = rep_ospec_getter(effect) else {
            continue;
        };
        if let Some(accum) = match time_options {
            StatTimeOptions::Burst(burst_opts) => aggr_proj_burst(
                ctx,
                calc,
                item_uid,
                effect,
                cseq,
                &ospec,
                (),
                StatCritOptions::default(),
                projectee_uid,
                burst_opts.spool,
                SeqAccum::new_stack(),
            ),
            StatTimeOptions::Sim(sim_options) => match sim_options.time.map(|v| v.into_pvalue()) {
                Some(time) if time > PValue::ZERO => aggr_proj_time(
                    ctx,
                    calc,
                    item_uid,
                    effect,
                    cseq,
                    &ospec,
                    (),
                    StatCritOptions::default(),
                    projectee_uid,
                    SeqAccum::new_stack(),
                    time,
                ),
                _ => aggr_proj_looped(
                    ctx,
                    calc,
                    item_uid,
                    effect,
                    cseq,
                    &ospec,
                    (),
                    StatCritOptions::default(),
                    projectee_uid,
                    SeqAccum::new_stack(),
                ),
            },
        } {
            orps += accum.get_per_second();
        }
    }
    orps
}

fn get_getter_shield(effect: &REffect) -> Option<REffectProjOpcSpec<NEffectGeneralOutputGetter>> {
    effect.outgoing_shield_rep
}

fn get_getter_armor(effect: &REffect) -> Option<REffectProjOpcSpec<NEffectGeneralOutputGetter>> {
    effect.outgoing_armor_rep
}

fn get_getter_hull(effect: &REffect) -> Option<REffectProjOpcSpec<NEffectGeneralOutputGetter>> {
    effect.outgoing_hull_rep
}
