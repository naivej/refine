use crate::{
    err::basic::ProjFoundError,
    sol::{SolCore, UndoStep},
    ud::{UItem, UItemId},
};

impl SolCore {
    pub(in crate::api) fn internal_remove_projection(
        &mut self,
        projector_uid: UItemId,
        projectee_uid: UItemId,
    ) -> Result<(), ProjFoundError> {
        let projector_u_item = self.u_data.items.get(projector_uid);
        let recording = self.undo.is_recording();
        let range = if recording {
            projector_u_item
                .get_projs()
                .unwrap()
                .get(projectee_uid)
                .flatten()
                .and_then(|data| data.get_range_override())
        } else {
            None
        };
        match projector_u_item {
            UItem::Drone(..) => self.internal_remove_drone_proj(projector_uid, projectee_uid),
            UItem::Fighter(..) => self.internal_remove_fighter_proj(projector_uid, projectee_uid),
            UItem::Module(..) => self.internal_remove_module_proj(projector_uid, projectee_uid),
            // Still need to handle projected effect, even if projected effect is not using ranged
            // projections - this method is used not just by ranged projection removal
            UItem::ProjEffect(..) => self.internal_remove_proj_effect_proj(projector_uid, projectee_uid),
            _ => unreachable!("unprojectable item kind is used in projection"),
        }?;
        if recording {
            self.undo
                .record(UndoStep::ProjRemoved(projector_uid, projectee_uid, range));
        }
        Ok(())
    }
}
