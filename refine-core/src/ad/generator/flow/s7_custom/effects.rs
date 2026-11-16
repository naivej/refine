use std::collections::hash_map::Entry;

use crate::{
    ad::ADataGenerator,
    ed::EveDataHandlerCore,
    nd::{N_EFFECT_MAP, NEffect},
};

impl ADataGenerator {
    pub(super) fn customize_effects(&mut self, handler: &dyn EveDataHandlerCore) {
        for n_effect in N_EFFECT_MAP.values() {
            if let Some(assigned) = self.assign_effect(n_effect) {
                match assigned {
                    true => self.add_effect(n_effect),
                    false => {
                        let warning = format!("effect {}: no items to assign effect to", n_effect.aid);
                        self.a_data.warnings.customization.push(warning);
                    }
                }
            }
            self.update_effect(n_effect, handler);
        }
    }
    fn add_effect(&mut self, n_effect: &NEffect) {
        if let Some(effect_maker) = n_effect.adg_make_effect_fn {
            let a_effect = effect_maker();
            match self.a_data.effects.data.entry(a_effect.id) {
                Entry::Occupied(..) => {
                    let warning = format!("effect {}: already exists, not replacing", a_effect.id);
                    self.a_data.warnings.customization.push(warning);
                }
                Entry::Vacant(entry) => {
                    entry.insert(a_effect);
                }
            }
        }
    }
    fn update_effect(&mut self, n_effect: &NEffect, handler: &dyn EveDataHandlerCore) {
        if let Some(effect_updater) = n_effect.adg_update_effect_fn {
            let Some(a_effect) = self.a_data.effects.data.get_mut(&n_effect.aid) else {
                let warning = format!("effect {}: not found for customization", n_effect.aid);
                self.a_data.warnings.customization.push(warning);
                return;
            };
            if n_effect
                .aid
                .dc_dogma_effect()
                .is_none_or(|id| handler.use_built_in_effect_update(id))
            {
                effect_updater(a_effect, &mut self.a_data.warnings.customization);
            }
        }
    }
    fn assign_effect(&mut self, n_effect: &NEffect) -> Option<bool> {
        let effect_assigner = n_effect.adg_assign_effect_fn?;
        Some(effect_assigner(&mut self.a_data.items.data))
    }
}
