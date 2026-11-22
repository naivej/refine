use dary_heap::QuaternaryHeap;

use super::{
    merge::Merger,
    stagger::{StaggerKey, StatCapSimStaggerInt, process_staggers},
};
use crate::{
    OptionalReload, PValue,
    nd::NEffectGeneralOutputGetter,
    rd::{REffect, REffectId, REffectLocalOpcSpec, REffectProjOpcSpec},
    stats::StatCritOptions,
    svc::{
        Calc, SvcCtx, Vast,
        cycle::{
            CSeqHardDtFull, CseqCache, CycleDataFull, CycleOptionsSim, CycleSeq, CyclingOptions, get_item_cseq_map,
        },
        vast::{
            VastFitData,
            aggr::{AggrIterData, aggr_local_iter, aggr_proj_iter},
            stats::cap::{
                sim::{
                    event::{CapSimEvent, CapSimEventData, CapSimEventInjector},
                    shared::Direction,
                },
                use_pyfa_nosferatu_drain,
            },
        },
    },
    ud::UItemId,
    util::{PrefetchPeekable, RMapRMap, RMapVec},
};

type LocalSources = RMapRMap<UItemId, REffectId, REffectLocalOpcSpec<NEffectGeneralOutputGetter>>;
type ProjSources = RMapRMap<UItemId, REffectId, REffectProjOpcSpec<NEffectGeneralOutputGetter>>;

#[derive(Copy, Clone)]
pub(in crate::svc::vast::stats::cap::sim) enum CapSourceKind {
    Consumer,
    Nosferatu,
    IncomingNeut,
    IncomingTransfer,
    Injector,
}
impl CapSourceKind {
    pub(super) const ALL: [Self; 5] = [
        Self::Consumer,
        Self::Nosferatu,
        Self::IncomingNeut,
        Self::IncomingTransfer,
        Self::Injector,
    ];
    pub(super) fn direction(self) -> Direction {
        match self {
            Self::Consumer | Self::IncomingNeut => Direction::Loss,
            _ => Direction::Gain,
        }
    }
}

pub(in crate::svc::vast::stats::cap::sim) struct CapSources<'a> {
    pub(in crate::svc::vast::stats::cap::sim) vast: &'a Vast,
    pub(in crate::svc::vast::stats::cap::sim) fit_data: &'a VastFitData,
    pub(in crate::svc::vast::stats::cap::sim) cap_item_uid: UItemId,
    pub(in crate::svc::vast::stats::cap::sim) nosf_projectee_item_uid: Option<UItemId>,
    pub(in crate::svc::vast::stats::cap::sim) optional_reloads: Option<OptionalReload>,
    pub(in crate::svc::vast::stats::cap::sim) limit_instances: bool,
    pub(in crate::svc::vast::stats::cap::sim) pyfa_incoming_nosferatu: bool,
}
impl CapSources<'_> {
    pub(super) fn visit<E>(
        &self,
        kind: CapSourceKind,
        cseq_cache: &mut CseqCache,
        ctx: &SvcCtx,
        calc: &mut Calc,
        emit: impl FnMut(&mut Calc, UItemId, AggrIterData<PValue>, CycleSeq<CycleDataFull, CSeqHardDtFull>) -> Result<(), E>,
    ) -> Result<(), E> {
        let cycling_options = CyclingOptions::Sim(CycleOptionsSim {
            optional_reloads: self.optional_reloads,
            ..
        });
        let (sources, projectee_uid) = match kind {
            CapSourceKind::Consumer | CapSourceKind::Injector => {
                let sources: &LocalSources = match kind {
                    CapSourceKind::Consumer => &self.fit_data.cap_consumers,
                    _ => &self.fit_data.cap_injects,
                };
                return visit_sources(
                    cseq_cache,
                    ctx,
                    calc,
                    cycling_options,
                    sources,
                    emit,
                    |calc, item_uid, effect, cseq, ospec| {
                        let mut ospec = *ospec;
                        if !self.limit_instances {
                            ospec.limit_attr_rid = None;
                        }
                        aggr_local_iter(ctx, calc, item_uid, effect, cseq, &ospec, ())
                    },
                );
            }
            CapSourceKind::Nosferatu => (Some(&self.fit_data.cap_nosfs), self.nosf_projectee_item_uid),
            CapSourceKind::IncomingNeut => (self.vast.in_neuts.get_l1(&self.cap_item_uid), Some(self.cap_item_uid)),
            CapSourceKind::IncomingTransfer => (self.vast.in_cap.get_l1(&self.cap_item_uid), Some(self.cap_item_uid)),
        };
        if let Some(sources) = sources {
            let sources: &ProjSources = sources;
            visit_sources(
                cseq_cache,
                ctx,
                calc,
                cycling_options,
                sources,
                emit,
                |calc, item_uid, effect, cseq, ospec| {
                    let mut ospec = *ospec;
                    if self.pyfa_incoming_nosferatu && matches!(kind, CapSourceKind::IncomingNeut) {
                        use_pyfa_nosferatu_drain(&mut ospec);
                    }
                    if !self.limit_instances {
                        ospec.local_limit_attr_id = None;
                        ospec.remote_limit_attr_id = None;
                    }
                    aggr_proj_iter(
                        ctx,
                        calc,
                        item_uid,
                        effect,
                        cseq,
                        &ospec,
                        (),
                        StatCritOptions::default(),
                        projectee_uid,
                    )
                },
            )?;
        }
        Ok(())
    }
}

pub(in crate::svc::vast::stats::cap::sim) fn prepare_events(
    cseq_cache: &mut CseqCache,
    ctx: &SvcCtx,
    calc: &mut Calc,
    sources: &CapSources,
    stagger: &StatCapSimStaggerInt,
) -> (QuaternaryHeap<CapSimEvent>, Vec<CapSimEvent>) {
    let mut merger = Merger::new();
    let mut injector_events = Vec::new();
    for kind in CapSourceKind::ALL {
        let direction = kind.direction();
        let mut stagger_map = RMapVec::new();
        let Ok(()) = sources.visit(kind, cseq_cache, ctx, calc, |_, item_uid, iter_data, _| {
            if matches!(kind, CapSourceKind::Injector) {
                injector_events.push(CapSimEvent::new(
                    PValue::ZERO,
                    CapSimEventData::InjectorReady(Box::new(CapSimEventInjector {
                        cycle_iter: PrefetchPeekable::new(iter_data.iter()),
                    })),
                ));
            } else if stagger.is_staggered(item_uid) {
                stagger_map.add_entry(StaggerKey::new(&iter_data), iter_data);
            } else {
                merger.add_entry(PValue::ZERO, iter_data, direction);
            }
            Ok::<(), !>(())
        });
        process_staggers(stagger_map, &mut merger, direction);
    }
    let mut events = QuaternaryHeap::new();
    merger.into_sim_events(&mut events);
    (events, injector_events)
}

fn visit_sources<S, E>(
    cseq_cache: &mut CseqCache,
    ctx: &SvcCtx,
    calc: &mut Calc,
    cycling_options: CyclingOptions,
    sources: &RMapRMap<UItemId, REffectId, S>,
    mut emit: impl FnMut(&mut Calc, UItemId, AggrIterData<PValue>, CycleSeq<CycleDataFull, CSeqHardDtFull>) -> Result<(), E>,
    mut aggregate: impl FnMut(
        &mut Calc,
        UItemId,
        &REffect,
        &CycleSeq<CycleDataFull, CSeqHardDtFull>,
        &S,
    ) -> Option<AggrIterData<PValue>>,
) -> Result<(), E> {
    for (&item_uid, item_data) in sources.iter() {
        let Some(cseq_map) = get_item_cseq_map(cseq_cache, ctx, calc, item_uid, cycling_options) else {
            continue;
        };
        for (&effect_rid, ospec) in item_data.iter() {
            let Some(cseq) = cseq_map.get(effect_rid) else {
                continue;
            };
            let effect = ctx.u_data.r_data.get_effect_by_rid(effect_rid);
            if let Some(iter_data) = aggregate(calc, item_uid, effect, cseq, ospec) {
                emit(calc, item_uid, iter_data, *cseq)?;
            }
        }
    }
    Ok(())
}
