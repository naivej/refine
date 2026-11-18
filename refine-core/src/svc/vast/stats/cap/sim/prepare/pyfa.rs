use super::{CapSourceKind, CapSources};
use crate::{
    PValue, Value,
    svc::{
        Calc, SvcCtx,
        cycle::{CSeqHardDtFull, CseqCache, CycleDataFull, CycleSeq},
        output::Output,
        vast::{
            aggr::AggrIterData,
            stats::cap::sim::{
                pyfa::{PyfaCycle, StatCapSimPyfaError},
                shared::Direction,
            },
        },
    },
};

const MAX_SOURCES: usize = 4096;

pub(in crate::svc::vast::stats::cap::sim) fn prepare_pyfa_cycles(
    cseq_cache: &mut CseqCache,
    ctx: &SvcCtx,
    calc: &mut Calc,
    sources: &CapSources,
    factor_reload: bool,
) -> Result<Vec<PyfaCycle>, StatCapSimPyfaError> {
    let mut cycles = Vec::new();
    for kind in CapSourceKind::ALL {
        sources.visit(kind, cseq_cache, ctx, calc, |calc, item_uid, data, sequence| {
            let local = matches!(
                kind,
                CapSourceKind::Consumer | CapSourceKind::Nosferatu | CapSourceKind::Injector
            );
            let injector = matches!(kind, CapSourceKind::Injector);
            let stagger = !local
                || !ctx
                    .u_data
                    .items
                    .get(item_uid)
                    .get_r_item_base()
                    .is_some_and(|base| base.takes_turret_hardpoint);
            // Pyfa adds the modified reactivation delay to every local activation,
            // even when the game cycle model treats it as downtime only on stopping.
            let delay_ms = local.then(|| {
                calc.get_item_oattr_ffb_extra(ctx, item_uid, ctx.ac().mod_reactivation_delay, Value::ZERO)
                    .into_f64()
            });
            let uses_reload = local && (factor_reload || injector);
            let reload_ms = if uses_reload {
                calc.get_item_oattr_oextra(ctx, item_uid, ctx.ac().reload_time)
                    .map(|value| value.into_f64())
            } else {
                None
            };
            if let Some(cycle) = prepare_cycle(
                data,
                sequence,
                kind.direction(),
                injector,
                stagger,
                uses_reload,
                delay_ms,
                reload_ms,
            )? {
                if cycles.len() == MAX_SOURCES {
                    return Err(StatCapSimPyfaError::WorkLimit);
                }
                cycles.push(cycle);
            }
            Ok(())
        })?;
    }
    Ok(cycles)
}

fn prepare_cycle(
    data: AggrIterData<PValue>,
    sequence: CycleSeq<CycleDataFull, CSeqHardDtFull>,
    direction: Direction,
    injector: bool,
    stagger: bool,
    factor_reload: bool,
    delay_ms: Option<f64>,
    reload_ms: Option<f64>,
) -> Result<Option<PyfaCycle>, StatCapSimPyfaError> {
    let AggrIterData::Regular(data) = data else {
        return Err(StatCapSimPyfaError::Schedule);
    };
    let first = data.get_cseq().get_first_cycle();
    let Output::Simple(output) = first.output else {
        return Err(StatCapSimPyfaError::Schedule);
    };
    let amount = output.instance.into_f64();
    if amount == 0.0 {
        return Ok(None);
    }
    let cap_need = match direction {
        Direction::Gain => -amount,
        Direction::Loss => amount,
    };
    let cap_need = Value::from_f64_checked(cap_need).map_err(|_| StatCapSimPyfaError::Schedule)?;
    // Local modules use their active duration plus reactivation delay. Projected
    // effects inherit the source cycle, including reactivation for neut bombs.
    let duration_ms = match delay_ms {
        Some(delay_ms) => to_ms(sequence.get_first_cycle().active.duration) + delay_ms,
        None => to_ms(first.cycle_main_duration),
    };
    if !duration_ms.is_finite() || duration_ms < 1.0 || duration_ms >= (1_u64 << 53) as f64 {
        return Err(StatCapSimPyfaError::Schedule);
    }
    let reloads = |cycle: CycleDataFull| cycle.soft_dt.is_some_and(|dt| dt.reasons.reload);
    let clip_size = if factor_reload {
        match sequence {
            CycleSeq::LoopSin(looped) if reloads(looped.data) => 1,
            CycleSeq::LoopLimSin(looped) if reloads(looped.p2_data) => {
                if looped.p1_data.active.chargedness != looped.p2_data.active.chargedness {
                    return Err(StatCapSimPyfaError::Schedule);
                }
                looped
                    .p1_repeat_count
                    .into_u32()
                    .checked_add(1)
                    .ok_or(StatCapSimPyfaError::Schedule)?
            }
            CycleSeq::LoopSin(_) => 0,
            // Other sequences can contain a partial or finite clip that does
            // not map directly to Pyfa's integer numShots.
            _ => return Err(StatCapSimPyfaError::Schedule),
        }
    } else {
        0
    };
    let reload_ms = if clip_size != 0 {
        let raw = reload_ms.ok_or(StatCapSimPyfaError::Schedule)?;
        PValue::from_f64_checked(raw).map_err(|_| StatCapSimPyfaError::Schedule)?
    } else {
        PValue::ZERO
    };
    Ok(Some(PyfaCycle {
        duration_ms: duration_ms as u64,
        cap_need,
        clip_size,
        reload_ms,
        injector,
        stagger,
    }))
}

// The aggregation layer stores seconds. Snap away conversion error at integral milliseconds
// before the caller truncates duration as Pyfa does.
fn to_ms(seconds: PValue) -> f64 {
    let milliseconds = seconds.into_f64() * 1000.0;
    let rounded = milliseconds.round();
    if (rounded - milliseconds).abs() <= milliseconds * f64::EPSILON {
        rounded
    } else {
        milliseconds
    }
}
