use crate::{PValue, UnitInterval, misc::DmgKinds, stats::StatResistsLayer};

pub struct StatDmg {
    pub dps: StatDmgEntry,
    pub volley: StatDmgEntry,
}

pub struct StatDmgApplied {
    pub dps: StatDmgEntryApplied,
    pub volley: StatDmgEntryApplied,
}

pub struct StatDmgEntry {
    pub em: PValue,
    pub thermal: PValue,
    pub kinetic: PValue,
    pub explosive: PValue,
    pub breacher: StatDmgEntryBreacher,
}

pub struct StatDmgEntryApplied {
    pub em: PValue,
    pub thermal: PValue,
    pub kinetic: PValue,
    pub explosive: PValue,
    pub breacher: PValue,
}

pub struct StatDmgEntryBreacher {
    pub absolute_max: PValue,
    // This field is not unit interval since it is supposed to store breacher DPS as well, and DPS
    // can exceed value of 1 if server has more than 1 ticks per second
    pub relative_max: PValue,
}

impl StatDmg {
    /// Apply one target layer's resistances to paper DPS and volley damage.
    /// Breacher damage keeps its limits because it bypasses layer resistances.
    pub fn after_target_resists(&self, resists: StatResistsLayer) -> Self {
        Self {
            dps: self.dps.after_target_resists(resists),
            volley: self.volley.after_target_resists(resists),
        }
    }
}

impl StatDmgEntry {
    fn after_target_resists(&self, resists: StatResistsLayer) -> Self {
        let resonance = |resistance: UnitInterval| PValue::from_f64_unchecked(1.0 - resistance.into_f64());
        Self {
            em: self.em * resonance(resists.em),
            thermal: self.thermal * resonance(resists.thermal),
            kinetic: self.kinetic * resonance(resists.kinetic),
            explosive: self.explosive * resonance(resists.explosive),
            breacher: StatDmgEntryBreacher {
                absolute_max: self.breacher.absolute_max,
                relative_max: self.breacher.relative_max,
            },
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Conversions
////////////////////////////////////////////////////////////////////////////////////////////////////
impl StatDmgEntry {
    pub(super) fn from_dmgs(normal: DmgKinds<PValue>, breacher: StatDmgEntryBreacher) -> Self {
        Self {
            em: normal.em,
            thermal: normal.thermal,
            kinetic: normal.kinetic,
            explosive: normal.explosive,
            breacher,
        }
    }
}

impl StatDmgEntryApplied {
    pub(super) fn from_dmgs(normal: DmgKinds<PValue>, breacher: PValue) -> Self {
        Self {
            em: normal.em,
            thermal: normal.thermal,
            kinetic: normal.kinetic,
            explosive: normal.explosive,
            breacher,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layer_resistances_affect_each_damage_type_in_dps_and_volley() {
        let damage = |values: [f64; 6]| {
            let [em, thermal, kinetic, explosive, absolute_max, relative_max] = values.map(PValue::from_f64_unchecked);
            StatDmgEntry {
                em,
                thermal,
                kinetic,
                explosive,
                breacher: StatDmgEntryBreacher {
                    absolute_max,
                    relative_max,
                },
            }
        };
        let paper = StatDmg {
            dps: damage([16.0, 20.0, 24.0, 28.0, 4.0, 0.25]),
            volley: damage([32.0, 40.0, 48.0, 56.0, 8.0, 0.5]),
        };
        let result = paper.after_target_resists(StatResistsLayer {
            em: UnitInterval::ZERO,
            thermal: UnitInterval::from_f64_clamped(0.25),
            kinetic: UnitInterval::from_f64_clamped(0.5),
            explosive: UnitInterval::ONE,
        });
        let channels = |entry: StatDmgEntry| {
            [
                entry.em,
                entry.thermal,
                entry.kinetic,
                entry.explosive,
                entry.breacher.absolute_max,
                entry.breacher.relative_max,
            ]
            .map(PValue::into_f64)
        };
        assert_eq!(channels(result.dps), [16.0, 15.0, 12.0, 0.0, 4.0, 0.25]);
        assert_eq!(channels(result.volley), [32.0, 30.0, 24.0, 0.0, 8.0, 0.5]);
    }
}
