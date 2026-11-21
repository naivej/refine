use crate::{
    PValue, Value,
    rd::{RAttrId, REffect},
    svc::{SvcCtx, calc::Calc},
    ud::{UItemId, UProjData},
};

////////////////////////////////////////////////////////////////////////////////////////////////////
// Regular optimal/falloff range calculation
////////////////////////////////////////////////////////////////////////////////////////////////////
pub(in crate::nd::effect::specs::proj_mult_getter) fn get_std_simple_c2s_range_mult(
    ctx: &SvcCtx,
    calc: &mut Calc,
    projector_uid: UItemId,
    effect: &REffect,
    proj_data: UProjData,
) -> PValue {
    let affector_optimal = get_effect_range(ctx, calc, projector_uid, effect.range_attr_rid);
    match proj_data.get_range_c2s() <= affector_optimal {
        true => PValue::ONE,
        false => PValue::ZERO,
    }
}
pub(in crate::nd::effect::specs::proj_mult_getter) fn get_std_simple_s2s_range_mult(
    ctx: &SvcCtx,
    calc: &mut Calc,
    projector_uid: UItemId,
    effect: &REffect,
    proj_data: UProjData,
) -> PValue {
    get_simple_s2s_range_mult(ctx, calc, projector_uid, proj_data, effect.range_attr_rid)
}
pub(super) fn get_simple_s2s_range_mult(
    ctx: &SvcCtx,
    calc: &mut Calc,
    projector_uid: UItemId,
    proj_data: UProjData,
    range_attr_rid: Option<RAttrId>,
) -> PValue {
    let affector_optimal = get_effect_range(ctx, calc, projector_uid, range_attr_rid);
    match proj_data.get_range_s2s() <= affector_optimal {
        true => PValue::ONE,
        false => PValue::ZERO,
    }
}

pub(in crate::nd::effect::specs::proj_mult_getter) fn get_std_full_restricted_range_mult(
    ctx: &SvcCtx,
    calc: &mut Calc,
    projector_uid: UItemId,
    effect: &REffect,
    proj_data: UProjData,
) -> PValue {
    get_std_full_range_mult(ctx, calc, projector_uid, effect, proj_data.get_range_s2s(), true, false)
}
pub(in crate::nd::effect::specs::proj_mult_getter) fn get_pyfa_repair_graph_range_mult(
    ctx: &SvcCtx,
    calc: &mut Calc,
    projector_uid: UItemId,
    effect: &REffect,
    proj_data: UProjData,
) -> PValue {
    get_std_full_range_mult(ctx, calc, projector_uid, effect, proj_data.get_range_s2s(), true, true)
}
pub(super) fn get_std_full_unrestricted_range_mult(
    ctx: &SvcCtx,
    calc: &mut Calc,
    projector_uid: UItemId,
    effect: &REffect,
    proj_data: UProjData,
) -> PValue {
    get_std_full_range_mult(
        ctx,
        calc,
        projector_uid,
        effect,
        proj_data.get_range_s2s(),
        false,
        false,
    )
}
fn get_std_full_range_mult(
    ctx: &SvcCtx,
    calc: &mut Calc,
    projector_uid: UItemId,
    effect: &REffect,
    proj_range: PValue,
    restricted: bool,
    pyfa_graph: bool,
) -> PValue {
    let affector_optimal = get_effect_range(ctx, calc, projector_uid, effect.range_attr_rid);
    let affector_falloff = get_effect_range(ctx, calc, projector_uid, effect.falloff_attr_rid);
    // Calculate actual range multiplier after collecting all the data
    match affector_falloff > PValue::FLOAT_TOLERANCE {
        true => {
            match restricted && proj_range > affector_optimal + PValue::from_f64_unchecked(3.0) * affector_falloff {
                true => PValue::ZERO,
                false => {
                    let distance_ratio =
                        PValue::from_finite_value_clamped(proj_range - affector_optimal) / affector_falloff;
                    if pyfa_graph {
                        // Pyfa uses CPython pow for both powers before rounding to
                        // seven significant digits. Rewrites to exp2 or multiplication
                        // can change the rounded factor at a tie.
                        let exponent = distance_ratio.into_f64().powf(std::hint::black_box(2.0));
                        let base = std::hint::black_box(0.5_f64);
                        pyfa_missile_round(PValue::from_f64_unchecked(base.powf(exponent)))
                    } else {
                        (-distance_ratio.powi(2)).exp2()
                    }
                }
            }
        }
        false => match proj_range <= affector_optimal {
            true => PValue::ONE,
            false => PValue::ZERO,
        },
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Missile-alike effect range calculation
////////////////////////////////////////////////////////////////////////////////////////////////////
pub(in crate::nd::effect::specs::proj_mult_getter) fn get_missile_range_mult(
    ctx: &SvcCtx,
    calc: &mut Calc,
    projector_uid: UItemId,
    proj_data: UProjData,
) -> PValue {
    let MissileFlight {
        max_velocity,
        flight_duration,
        mass,
        agility,
    } = MissileFlight::from_charge(ctx, calc, projector_uid);
    let inertia_factor = calc_inertia_factor(mass, agility);
    let flight_duration_lower = flight_duration.floor_tick();
    // Missiles appear in center of attacking ship and explode on surface of target ship
    let proj_range = proj_data.get_range_c2s();
    match flight_duration_lower == flight_duration {
        // When flight duration is aligned to ticks, need to do fewer calculations
        true => {
            let flight_range = calc_flight_range(max_velocity, flight_duration, inertia_factor);
            match proj_range <= flight_range {
                true => PValue::ONE,
                false => PValue::ZERO,
            }
        }
        // When flight duration is not aligned to ticks, any range which lies within lower flight
        // duration receives full effect, any range past higher flight duration receives no effect,
        // and anything in-between receives partial effect corresponding to flight duration fraction
        // part
        false => {
            let flight_range_lower = calc_flight_range(max_velocity, flight_duration_lower, inertia_factor);
            match proj_range <= flight_range_lower {
                true => PValue::ONE,
                false => {
                    let flight_duration_higher = flight_duration.ceil_tick();
                    let flight_range_higher = calc_flight_range(max_velocity, flight_duration_higher, inertia_factor);
                    match proj_range > flight_range_higher {
                        true => PValue::ZERO,
                        false => (flight_duration * PValue::SERVER_TICK_HZ).fract(),
                    }
                }
            }
        }
    }
}

struct MissileFlight {
    max_velocity: PValue,
    flight_duration: PValue,
    mass: PValue,
    agility: PValue,
}

impl MissileFlight {
    fn from_charge(ctx: &SvcCtx, calc: &mut Calc, charge_uid: UItemId) -> Self {
        let mut attr = |rid| calc.get_item_oattr_ffb_extra(ctx, charge_uid, rid, Value::ZERO);
        Self {
            max_velocity: PValue::from_finite_value_clamped(attr(ctx.ac().max_velocity)),
            // The missile flight-time effect already adds the time across the source radius.
            flight_duration: PValue::from_finite_value_clamped(attr(ctx.ac().explosion_delay) / Value::THOUSAND),
            mass: PValue::from_finite_value_clamped(attr(ctx.ac().mass)),
            agility: PValue::from_finite_value_clamped(attr(ctx.ac().agility)),
        }
    }

    fn pyfa_range_mult(&self, overview_m: f64, ship_radius_m: f64) -> PValue {
        let speed = self.max_velocity.into_f64();
        if speed == 0.0 {
            return PValue::ZERO;
        }
        // Pyfa Module.missileMaxRangeData rounds flight time before splitting it into ticks.
        let duration = pyfa_missile_round(self.flight_duration).into_f64();
        self.pyfa_range_mult_for_duration(overview_m, ship_radius_m, duration)
    }

    fn pyfa_range_mult_for_duration(&self, overview_m: f64, ship_radius_m: f64, duration: f64) -> PValue {
        let speed = self.max_velocity.into_f64();
        let range_at = |tick: f64| {
            crate::pyfa_graph::pyfa_tick_range(
                speed,
                self.mass.into_f64(),
                self.agility.into_f64(),
                ship_radius_m,
                tick,
            )
        };
        if overview_m <= range_at(duration.floor()) {
            PValue::ONE
        } else if overview_m <= range_at(duration.ceil()) {
            PValue::from_f64_unchecked(duration.fract())
        } else {
            PValue::ZERO
        }
    }
}

pub(in crate::nd::effect::specs::proj_mult_getter) fn get_pyfa_missile_range_mult(
    ctx: &SvcCtx,
    calc: &mut Calc,
    charge_uid: UItemId,
    proj_data: UProjData,
) -> PValue {
    let radius = ctx.u_data.items.get(charge_uid).get_carrier_radius(ctx.u_data);
    MissileFlight::from_charge(ctx, calc, charge_uid)
        .pyfa_range_mult(proj_data.get_range_s2s().into_f64(), radius.into_f64())
}

/// Pyfa eos.utils.float.floatUnerr keeps seven significant decimal digits, with ties to even.
pub(in crate::nd::effect::specs::proj_mult_getter) fn pyfa_missile_round(value: PValue) -> PValue {
    let raw = value.into_f64();
    if raw == 0.0 || !raw.is_finite() {
        return value;
    }
    let digits = 7 - raw.log10().ceil() as i32;
    // Powers through 10^22 are exactly representable. For distant exponents or an apparent
    // decimal tie, the float formatter rounds the exact binary value before converting back.
    if (0..=22).contains(&digits) {
        let scale = 10.0_f64.powi(digits);
        let scaled = raw * scale;
        if scaled.fract() != 0.5 {
            return PValue::from_f64_unchecked(scaled.round() / scale);
        }
    }
    PValue::from_f64_unchecked(
        format!("{raw:.6e}")
            .parse::<f64>()
            .expect("formatted a floating-point value"),
    )
}

pub(in crate::nd::effect::specs::proj_mult_getter) fn get_fof_missile_range_mult(
    ctx: &SvcCtx,
    calc: &mut Calc,
    projector_uid: UItemId,
    proj_data: UProjData,
) -> PValue {
    // FoF missiles are limited by c2s range
    if let Some(range_limit) = calc.get_item_oattr_oextra(ctx, projector_uid, ctx.ac().max_fof_tgt_range)
        && proj_data.get_range_c2s() > PValue::from_finite_value_clamped(range_limit)
    {
        return PValue::ZERO;
    }
    get_missile_range_mult(ctx, calc, projector_uid, proj_data)
}

pub(in crate::nd::effect::specs::proj_mult_getter) fn get_bomb_range_mult(
    ctx: &SvcCtx,
    calc: &mut Calc,
    projector_uid: UItemId,
    proj_data: UProjData,
) -> PValue {
    // Bomb is similar to missile, but they have fixed flight range and AoE effect
    let max_velocity = PValue::from_finite_value_clamped(calc.get_item_oattr_ffb_extra(
        ctx,
        projector_uid,
        ctx.ac().max_velocity,
        Value::ZERO,
    ));
    let flight_duration = PValue::from_finite_value_clamped(
        calc.get_item_oattr_ffb_extra(ctx, projector_uid, ctx.ac().explosion_delay, Value::ZERO) / Value::THOUSAND,
    );
    let mass = PValue::from_finite_value_clamped(calc.get_item_oattr_ffb_extra(
        ctx,
        projector_uid,
        ctx.ac().mass,
        Value::ZERO,
    ));
    let agility = PValue::from_finite_value_clamped(calc.get_item_oattr_ffb_extra(
        ctx,
        projector_uid,
        ctx.ac().agility,
        Value::ZERO,
    ));
    let inertia_factor = calc_inertia_factor(mass, agility);
    let aoe_range = PValue::from_finite_value_clamped(calc.get_item_oattr_ffb_extra(
        ctx,
        projector_uid,
        ctx.ac().emp_field_range,
        Value::ZERO,
    ));
    let aoe_reach_range = aoe_range + proj_data.get_tgt_radius();
    let flight_duration_lower = flight_duration.floor_tick();
    // Bombs appear in center of attacking ship
    let proj_range = proj_data.get_range_c2c();
    match flight_duration_lower == flight_duration {
        // When flight duration is aligned to ticks, need to do fewer calculations
        true => {
            let flight_range = calc_flight_range(max_velocity, flight_duration, inertia_factor);
            let short_range = PValue::from_finite_value_clamped(flight_range - aoe_reach_range);
            let long_range = flight_range + aoe_reach_range;
            match proj_range >= short_range && proj_range <= long_range {
                true => PValue::ONE,
                false => PValue::ZERO,
            }
        }
        // When flight duration is not aligned to ticks, calculate 2 outcomes separately, and sum
        // their results up
        false => {
            let flight_duration_higher = flight_duration.ceil_tick();
            let flight_range_lower = calc_flight_range(max_velocity, flight_duration_lower, inertia_factor);
            let flight_range_higher = calc_flight_range(max_velocity, flight_duration_higher, inertia_factor);
            let chance_higher = (flight_duration * PValue::SERVER_TICK_HZ).fract();
            let chance_lower = PValue::from_value_unchecked(PValue::ONE - chance_higher);
            let lower_short_range = PValue::from_finite_value_clamped(flight_range_lower - aoe_reach_range);
            let lower_long_range = flight_range_lower + aoe_reach_range;
            let higher_short_range = PValue::from_finite_value_clamped(flight_range_higher - aoe_reach_range);
            let higher_long_range = flight_range_higher + aoe_reach_range;
            let mut mult = PValue::ZERO;
            if proj_range >= lower_short_range && proj_range <= lower_long_range {
                mult += chance_lower;
            };
            if proj_range >= higher_short_range && proj_range <= higher_long_range {
                mult += chance_higher;
            };
            mult
        }
    }
}

fn calc_flight_range(max_velocity: PValue, flight_duration: PValue, inertia_factor: PValue) -> PValue {
    // Missiles use the regular object acceleration formula:
    // https://wiki.eveuniversity.org/Acceleration#Mathematics_and_formulae
    // Here, a definite integral of this formula is calculated
    if flight_duration == PValue::ZERO {
        return PValue::ZERO;
    }
    if !inertia_factor.is_finite() {
        return max_velocity * flight_duration;
    }
    let inertia_factor = inertia_factor.into_f64();
    let flight_duration = flight_duration.into_f64();
    let range_units = flight_duration + (f64::exp(-inertia_factor * flight_duration) - 1.0) / inertia_factor;
    PValue::from_f64_unchecked(max_velocity.into_f64() * range_units)
}

fn calc_inertia_factor(mass: PValue, agility: PValue) -> PValue {
    PValue::MILLION / (mass * agility)
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Misc effects
////////////////////////////////////////////////////////////////////////////////////////////////////
pub(in crate::nd::effect::specs::proj_mult_getter) fn get_aoe_burst_range_mult(
    ctx: &SvcCtx,
    calc: &mut Calc,
    projector_uid: UItemId,
    proj_data: UProjData,
) -> PValue {
    // Doomsday projectiles are launched from center of the ship, and range is extended by aoe range
    let affector_optimal = get_effect_range(ctx, calc, projector_uid, ctx.ac().max_range);
    let affector_aoe = get_effect_range(ctx, calc, projector_uid, ctx.ac().doomsday_aoe_range);
    match proj_data.get_range_c2s() <= affector_optimal + affector_aoe {
        true => PValue::ONE,
        false => PValue::ZERO,
    }
}

pub(super) fn get_aoe_dd_sharp_range_mult(
    ctx: &SvcCtx,
    calc: &mut Calc,
    projector_uid: UItemId,
    proj_data: UProjData,
) -> PValue {
    // AoE doomsdays' effects do not specify range attribute ID, so it is hardcoded here
    let affector_optimal = get_effect_range(ctx, calc, projector_uid, ctx.ac().max_range);
    match proj_data.get_range_s2s() <= affector_optimal {
        true => PValue::ONE,
        false => PValue::ZERO,
    }
}

pub(in crate::nd::effect::specs::proj_mult_getter) fn get_aoe_dd_round_range_mult(
    ctx: &SvcCtx,
    calc: &mut Calc,
    projector_uid: UItemId,
    proj_data: UProjData,
) -> PValue {
    // AoE doomsdays' effects do not specify range attribute IDs, so they are hardcoded here
    let affector_optimal = get_effect_range(ctx, calc, projector_uid, ctx.ac().max_range);
    let affector_dmg_radius = get_effect_range(ctx, calc, projector_uid, ctx.ac().doomsday_dmg_radius);
    match proj_data.get_range_s2s() <= affector_optimal + affector_dmg_radius {
        true => PValue::ONE,
        false => PValue::ZERO,
    }
}

pub(super) fn get_dd_neut_range_mult(
    ctx: &SvcCtx,
    calc: &mut Calc,
    projector_uid: UItemId,
    proj_data: UProjData,
) -> PValue {
    let neut_optimal = get_effect_range(ctx, calc, projector_uid, ctx.ac().doomsday_energy_neut_radius);
    match proj_data.get_range_s2s() <= neut_optimal {
        true => PValue::ONE,
        false => PValue::ZERO,
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Utility
////////////////////////////////////////////////////////////////////////////////////////////////////
fn get_effect_range(ctx: &SvcCtx, calc: &mut Calc, projector_uid: UItemId, attr_rid: Option<RAttrId>) -> PValue {
    match attr_rid {
        Some(attr_rid) => match calc.get_item_attr_rfull(ctx, projector_uid, attr_rid) {
            Ok(val) => PValue::from_finite_value_clamped(val.extra),
            _ => PValue::ZERO,
        },
        None => PValue::ZERO,
    }
}

#[cfg(test)]
mod pyfa_missile_tests {
    use super::*;

    #[test]
    fn rounds_flight_time_and_application_like_pyfa_at_decimal_boundaries() {
        for (value, expected) in [
            (0.0, 0.0),
            (1.0, 1.0),
            (0.10000005, 0.1),
            (0.12345675, 0.1234568),
            (0.12345665, 0.1234567),
            (7.505449481481482, 7.505449),
            (7.99999999, 8.0),
            (8.0000001, 8.0),
            (0.00000012345675, 0.0000001234568),
            (1e-308, 1e-308),
            (10000005.0, 10000000.0),
            (10000015.0, 10000020.0),
        ] {
            assert_eq!(
                pyfa_missile_round(PValue::from_f64_unchecked(value)).into_f64(),
                expected
            );
        }
    }

    #[test]
    fn integer_flight_tick_includes_its_upper_range_and_a_stationary_missile_has_no_range() {
        let mut flight = MissileFlight {
            max_velocity: PValue::from_f64_unchecked(1000.0),
            flight_duration: PValue::from_f64_unchecked(7.99999999),
            mass: PValue::from_f64_unchecked(1_000_000.0),
            agility: PValue::ONE,
        };
        let radius = 40.0;
        let cutoff = 7_460.0_f64;
        assert_eq!(flight.pyfa_range_mult(cutoff, radius), PValue::ONE);
        assert_eq!(flight.pyfa_range_mult(cutoff.next_up(), radius), PValue::ZERO);
        flight.max_velocity = PValue::ZERO;
        assert_eq!(flight.pyfa_range_mult(0.0, radius), PValue::ZERO);
    }
}
