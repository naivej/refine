//! Per-charge missile preparation for Pyfa's Application Profile.
use crate::pyfa_graph::pyfa_tick_range;

#[derive(Clone, Copy, Debug)]
pub struct ChargeAttributes {
    pub velocity: f64,
    pub flight_time_ms: f64,
    pub mass: f64,
    pub agility: f64,
    pub explosion_radius: f64,
    pub explosion_velocity: f64,
    pub damage_reduction_factor: f64,
    /// Base damage in EM, thermal, kinetic and explosive order.
    pub damage: [f64; 4],
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoadedModifiers {
    pub damage: [f64; 4],
    pub flight: [f64; 2],
    pub application: [f64; 3],
    pub launcher_damage: f64,
    pub ship_radius: f64,
}

/// Effective flight and application geometry after applying loaded coefficients.
#[derive(Debug, PartialEq)]
pub struct ChargeGeometry {
    pub lower_range_m: f64,
    pub upper_range_m: f64,
    pub partial_hit_chance: f64,
    explosion_radius: f64,
    explosion_velocity: f64,
    damage_reduction_factor: f64,
}

/// A charge evaluated for one target and resistance profile.
pub struct PreparedCharge {
    geometry: ChargeGeometry,
    volley_after_resists: f64,
    application: f64,
}

impl PreparedCharge {
    pub fn geometry(&self) -> &ChargeGeometry {
        &self.geometry
    }

    /// One launcher's volley after resistance, before range and application.
    pub fn volley_after_resists(&self) -> f64 {
        self.volley_after_resists
    }

    /// One launcher's applied HP at a finite, nonnegative distance in metres.
    pub fn applied_volley(&self, distance_m: f64) -> f64 {
        let range = if distance_m <= self.geometry.lower_range_m {
            1.0
        } else if distance_m <= self.geometry.upper_range_m {
            self.geometry.partial_hit_chance
        } else {
            0.0
        };
        self.volley_after_resists * range * self.application
    }
}

/// Validates the shared launcher and target inputs once before preparing charges.
pub struct ChargePreparation<'a> {
    loaded: &'a LoadedModifiers,
    target_speed: f64,
    target_signature: f64,
    resistances: [f64; 4],
}

impl<'a> ChargePreparation<'a> {
    pub fn new(
        loaded: &'a LoadedModifiers,
        target_speed: f64,
        target_signature: f64,
        resistances: [f64; 4],
    ) -> Result<Self, &'static str> {
        if !target_speed.is_finite()
            || target_speed < 0.0
            || !target_signature.is_finite()
            || target_signature <= 0.0
            || resistances.iter().any(|r| !(0.0..=1.0).contains(r))
        {
            return Err("invalid target or resistances");
        }
        let factors = loaded
            .damage
            .into_iter()
            .chain(loaded.flight)
            .chain(loaded.application)
            .chain([loaded.launcher_damage, loaded.ship_radius]);
        if factors.into_iter().any(|x| !x.is_finite() || x <= 0.0) {
            return Err("invalid loaded modifiers");
        }
        Ok(Self {
            loaded,
            target_speed,
            target_signature,
            resistances,
        })
    }

    pub fn prepare(&self, charge: &ChargeAttributes) -> Result<PreparedCharge, &'static str> {
        if [
            charge.velocity,
            charge.flight_time_ms,
            charge.explosion_radius,
            charge.damage_reduction_factor,
        ]
        .iter()
        .any(|x| !x.is_finite() || *x <= 0.0)
            || [charge.mass, charge.agility, charge.explosion_velocity]
                .into_iter()
                .chain(charge.damage)
                .any(|x| !x.is_finite() || x < 0.0)
        {
            return Err("invalid candidate attributes");
        }

        let loaded = self.loaded;
        let speed = charge.velocity * loaded.flight[0];
        // Application Profile takes floor and ceil without Damage Stats' preceding time rounding.
        let duration = charge.flight_time_ms * loaded.flight[1] / 1000.0 + loaded.ship_radius / speed;
        let range = |tick| pyfa_tick_range(speed, charge.mass, charge.agility, loaded.ship_radius, tick);
        let geometry = ChargeGeometry {
            lower_range_m: range(duration.floor()),
            upper_range_m: range(duration.ceil()),
            partial_hit_chance: duration.fract(),
            explosion_radius: charge.explosion_radius * loaded.application[0],
            explosion_velocity: charge.explosion_velocity * loaded.application[1],
            damage_reduction_factor: charge.damage_reduction_factor * loaded.application[2],
        };
        let volley_after_resists = (0..4)
            .map(|index| charge.damage[index] * loaded.damage[index] * (1.0 - self.resistances[index]))
            .sum::<f64>()
            * loaded.launcher_damage;
        if [
            geometry.lower_range_m,
            geometry.upper_range_m,
            geometry.partial_hit_chance,
            geometry.explosion_radius,
            geometry.explosion_velocity,
            geometry.damage_reduction_factor,
            volley_after_resists,
        ]
        .iter()
        .any(|x| !x.is_finite())
        {
            return Err("candidate arithmetic overflow");
        }

        let mut application = 1.0_f64;
        if geometry.explosion_radius > 0.0 {
            application = application.min(self.target_signature / geometry.explosion_radius);
            if self.target_speed > 0.0 {
                let speed_ratio = (geometry.explosion_velocity * self.target_signature)
                    / (geometry.explosion_radius * self.target_speed);
                application = application.min(speed_ratio.powf(geometry.damage_reduction_factor));
            }
        }
        Ok(PreparedCharge {
            geometry,
            volley_after_resists,
            application,
        })
    }
}

#[cfg(test)]
mod application_profile_tests {
    use super::*;

    #[test]
    fn actual_profile_uses_unrounded_flight_ticks_and_inclusive_cutoffs() {
        let attrs = ChargeAttributes {
            velocity: 3750.0,
            flight_time_ms: 5000.0,
            mass: 700.0,
            agility: 0.000144498003785,
            explosion_radius: 40.0,
            explosion_velocity: 170.0,
            damage_reduction_factor: 0.604,
            damage: [83.0, 0.0, 0.0, 0.0],
        };
        let loaded = LoadedModifiers {
            damage: [1.8390625; 4],
            flight: [2.25, 1.5],
            application: [0.75, 1.5, 1.0],
            launcher_damage: 1.0,
            ship_radius: 45.98,
        };
        let charge = ChargePreparation::new(&loaded, 350.0, 38.0, [0.0; 4])
            .unwrap()
            .prepare(&attrs)
            .unwrap();
        let geometry = charge.geometry();
        let lower = geometry.lower_range_m;
        let upper = geometry.upper_range_m;
        let chance = geometry.partial_hit_chance;
        assert!((lower - 59016.51957327933).abs() < 1e-9);
        assert!((upper - 67454.01957327934).abs() < 1e-9);
        assert!((chance - 0.5054494814814818).abs() < 1e-14);
        let full = charge.applied_volley(0.0);
        assert_eq!(charge.applied_volley(lower.next_down()), full);
        assert_eq!(charge.applied_volley(lower), full);
        for distance in [lower.next_up(), upper.next_down(), upper] {
            assert!((charge.applied_volley(distance) - full * chance).abs() < 1e-12);
        }
        assert_eq!(charge.applied_volley(upper.next_up()), 0.0);
    }
}
