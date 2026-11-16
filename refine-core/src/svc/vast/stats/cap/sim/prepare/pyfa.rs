use super::{CapSourceKind, CapSources};
use crate::{
    PValue, Value,
    svc::{
        Calc, SvcCtx,
        cycle::{CseqCache, CycleSeq},
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
        sources.visit(kind, cseq_cache, ctx, calc, |item_uid, data| {
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
            if let Some(cycle) = prepare_cycle(
                data,
                kind.direction(),
                injector,
                stagger,
                local && (factor_reload || injector),
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
    direction: Direction,
    injector: bool,
    stagger: bool,
    factor_reload: bool,
) -> Result<Option<PyfaCycle>, StatCapSimPyfaError> {
    let AggrIterData::Regular(data) = data else {
        return Err(StatCapSimPyfaError::Schedule);
    };
    let sequence = data.get_cseq();
    let first = sequence.get_first_cycle();
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
    let duration_ms = to_ms(first.cycle_main_duration);
    if !duration_ms.is_finite() || duration_ms < 1.0 || duration_ms >= (1_u64 << 53) as f64 {
        return Err(StatCapSimPyfaError::Schedule);
    }
    let (clip_size, reload_ms) = match (factor_reload, sequence.get_hard_dt()) {
        (true, Some(downtime)) => {
            let clip_size = match sequence {
                CycleSeq::LoopSin(_) => 1,
                CycleSeq::LoopLimSin(looped) => looped
                    .p1_repeat_count
                    .into_u32()
                    .checked_add(1)
                    .ok_or(StatCapSimPyfaError::Schedule)?,
                _ => unreachable!("limited sequences have no reload downtime"),
            };
            let reload_ms =
                PValue::from_f64_checked(to_ms(downtime.duration)).map_err(|_| StatCapSimPyfaError::Schedule)?;
            (clip_size, reload_ms)
        }
        _ => (0, PValue::ZERO),
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
