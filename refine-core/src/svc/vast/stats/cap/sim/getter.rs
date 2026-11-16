use crate::{
    OptionalReload, PValue, UnitInterval, Value,
    svc::{
        Calc, SvcCtx, Vast,
        cycle::CseqCache,
        err::IntStatItemError,
        vast::stats::{
            cap::sim::{
                prepare::{CapSources, StatCapSimStaggerInt, prepare_events, prepare_pyfa_cycles},
                pyfa::{PyfaEvents, StatCapSimPyfa, StatCapSimPyfaError, StatCapSimTraceRequest},
                sim::{CapSim, StatCapSim},
            },
            item_checks::check_ship,
        },
    },
    ud::UItemId,
};

impl Vast {
    pub(in crate::svc) fn get_stat_item_cap_sim(
        &self,
        cseq_cache: &mut CseqCache,
        ctx: &SvcCtx,
        calc: &mut Calc,
        item_uid: UItemId,
        cap_perc: UnitInterval,
        optional_reloads: Option<OptionalReload>,
        stagger: &StatCapSimStaggerInt,
        nosf_projectee_item_uid: Option<UItemId>,
    ) -> Result<StatCapSim, IntStatItemError<!>> {
        let ship = check_ship(ctx.u_data, item_uid)?;
        let max_cap = Self::get_stat_item_cap_amount(ctx, calc, item_uid).unwrap();
        let recharge_time_ms = calc.get_item_oattr_ffb_extra(ctx, item_uid, ctx.ac().recharge_rate, Value::ZERO);
        let recharge_time_s = match recharge_time_ms < Value::FLOAT_TOLERANCE {
            true => None,
            false => Some(PValue::from_finite_value_clamped(recharge_time_ms / Value::THOUSAND)),
        };
        let start_cap = max_cap * cap_perc.into_pvalue();
        let sources = CapSources {
            vast: self,
            fit_data: self.get_fit_data(ship.get_fit_uid()),
            cap_item_uid: item_uid,
            nosf_projectee_item_uid,
            optional_reloads,
            limit_instances: true,
        };
        let (events, injector_events) = prepare_events(cseq_cache, ctx, calc, &sources, stagger);
        let mut sim = CapSim::new(start_cap, max_cap, recharge_time_s, events, injector_events);
        Ok(sim.run())
    }

    pub(in crate::svc) fn get_stat_item_cap_sim_pyfa(
        &self,
        cseq_cache: &mut CseqCache,
        ctx: &SvcCtx,
        calc: &mut Calc,
        item_uid: UItemId,
        factor_reload: bool,
        trace: Option<StatCapSimTraceRequest>,
    ) -> Result<StatCapSimPyfa, IntStatItemError<StatCapSimPyfaError>> {
        let ship = check_ship(ctx.u_data, item_uid)?;
        let capacity = Self::internal_get_stat_item_cap_amount_unchecked(ctx, calc, item_uid);
        if let Some(request) = trace {
            request.validate(capacity).map_err(IntStatItemError::StatSpecific)?;
        }
        let recharge_ms = calc.get_item_oattr_ffb_extra(ctx, item_uid, ctx.ac().recharge_rate, Value::ZERO);
        let sources = CapSources {
            vast: self,
            fit_data: self.get_fit_data(ship.get_fit_uid()),
            cap_item_uid: item_uid,
            nosf_projectee_item_uid: None,
            optional_reloads: Some(if factor_reload {
                OptionalReload::OnEmpty
            } else {
                OptionalReload::Disabled
            }),
            // Pyfa limits total cap after each event. Injector selection uses its full output.
            limit_instances: false,
        };
        let cycles = prepare_pyfa_cycles(cseq_cache, ctx, calc, &sources, factor_reload)
            .map_err(IntStatItemError::StatSpecific)?;
        PyfaEvents::simulate(cycles, capacity, recharge_ms.into_f64(), trace).map_err(IntStatItemError::StatSpecific)
    }
}
