use smallvec::SmallVec;

use crate::{
    ItemId, PValue, ProjRange,
    sol::{SolCore, UndoStep},
    ud::UItemId,
};

////////////////////////////////////////////////////////////////////////////////////////////////////
// Public
////////////////////////////////////////////////////////////////////////////////////////////////////
/// Projection which allows to set range.
pub struct RangedProj<'s> {
    pub(super) sol: &'s mut SolCore,
    pub(super) projector_uid: UItemId,
    pub(super) projectee_uid: UItemId,
}
impl<'s> RangedProj<'s> {
    pub fn get_projectee_item_id(&self) -> ItemId {
        self.sol.u_data.items.ext_id_by_int_id(self.projectee_uid)
    }
    /// Get the effective ranges for this edge. Both fields contain the explicit distance if set.
    pub fn get_range(&self) -> ProjRange {
        let projectee_uid = &self.projectee_uid;
        // - unwrap #1 - projection itself is fetchable only for item with projection data on them
        // - unwrap #2 - projection itself is fetchable only for projections which exist
        // - unwrap #3 - ranged projection should be exposed only for items which have ranged
        //   projection, i.e. put some value into projection container
        let u_proj = self
            .sol
            .u_data
            .items
            .get(self.projector_uid)
            .get_projs()
            .unwrap()
            .get(*projectee_uid)
            .unwrap()
            .unwrap();
        ProjRange::from_u_proj_data(u_proj)
    }
    /// Set the distance in meters for range checks on this directed projection.
    ///
    /// The explicit distance replaces all geometry-based range values on the edge.
    /// Coordinates and movement still determine angular motion.
    pub fn set_range(&mut self, range: PValue) {
        self.sol
            .internal_set_proj_range(self.projector_uid, self.projectee_uid, Some(range));
    }
    /// Resume using the current coordinates and radii for range checks.
    pub fn clear_range(&mut self) {
        self.sol
            .internal_set_proj_range(self.projector_uid, self.projectee_uid, None);
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Private
////////////////////////////////////////////////////////////////////////////////////////////////////
impl<'s> RangedProj<'s> {
    pub(in crate::api) fn new(sol: &'s mut SolCore, projector_uid: UItemId, projectee_uid: UItemId) -> Self {
        Self {
            sol,
            projector_uid,
            projectee_uid,
        }
    }
}

impl SolCore {
    pub(in crate::api) fn internal_set_proj_range(
        &mut self,
        projector_uid: UItemId,
        projectee_uid: UItemId,
        range: Option<PValue>,
    ) {
        let projector = self.u_data.items.get_mut(projector_uid);
        let proj_data = projector
            .get_projs_mut()
            .unwrap()
            .get_proj_data_mut(projectee_uid)
            .unwrap();
        let previous_range = proj_data.get_range_override();
        if previous_range == range {
            return;
        }
        proj_data.set_range_override(range);
        let proj_data = *proj_data;
        let charge_uids: SmallVec<[UItemId; 1]> = projector.iter_charges().collect();
        for &charge_uid in &charge_uids {
            let charge = self.u_data.items.get_mut(charge_uid);
            *charge
                .get_projs_mut()
                .unwrap()
                .get_proj_data_mut(projectee_uid)
                .unwrap() = proj_data;
        }
        for uid in std::iter::once(projector_uid).chain(charge_uids) {
            SolCore::util_change_item_proj_data(&self.u_data, &mut self.svc, uid, projectee_uid, Some(proj_data));
        }
        if self.undo.is_recording() {
            self.undo
                .record(UndoStep::ProjRange(projector_uid, projectee_uid, previous_range));
        }
    }
}
