use crate::{
    FitId, ItemCommon, ItemId,
    api::ItemSealed,
    sol::SolCore,
    ud::{UCharge, UItemId},
};

////////////////////////////////////////////////////////////////////////////////////////////////////
// Public
////////////////////////////////////////////////////////////////////////////////////////////////////
pub struct Charge<'s> {
    pub(super) sol: &'s mut SolCore,
    pub(super) uid: UItemId,
}
impl<'s> Charge<'s> {
    /// Return the loaded-charge dogma coefficient used by Pyfa Application Profile.
    /// Zero-base attributes use the same modifiers with a temporary base of one.
    pub fn pyfa_ammo_coefficient(
        &mut self,
        attr_id: &crate::AttrId,
    ) -> Result<crate::Value, crate::err::ItemAttrGetError> {
        let item_uid = self.uid;
        let sol = &mut self.sol;
        let attr_aid = attr_id.into_aid();
        let Some(attr_rid) = sol.u_data.r_data.get_attr_rid_by_aid(&attr_aid) else {
            return Err(crate::err::basic::AttrFoundError { attr_id: *attr_id }.into());
        };
        sol.svc
            .pyfa_ammo_coefficient(&sol.u_data, item_uid, attr_rid)
            .map_err(|error| {
                crate::err::basic::ItemLoadedError {
                    item_id: sol.u_data.items.ext_id_by_int_id(error.item_uid),
                }
                .into()
            })
    }
    pub fn get_fit_id(&self) -> FitId {
        let fit_uid = get_u_charge(self.sol, self.uid).get_fit_uid();
        self.sol.u_data.fits.ext_id_by_int_id(fit_uid)
    }
    pub fn get_cont_item_id(&self) -> ItemId {
        let cont_uid = get_u_charge(self.sol, self.uid).get_cont_item_uid();
        self.sol.u_data.items.ext_id_by_int_id(cont_uid)
    }
    pub fn get_state(&self) -> bool {
        !get_u_charge(self.sol, self.uid).get_force_disabled()
    }
}
impl<'s> ItemCommon for Charge<'s> {}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Private
////////////////////////////////////////////////////////////////////////////////////////////////////
impl<'s> Charge<'s> {
    pub(in crate::api) fn new(sol: &'s mut SolCore, uid: UItemId) -> Self {
        Self { sol, uid }
    }
}
impl<'s> ItemSealed for Charge<'s> {
    fn get_uid(&self) -> UItemId {
        self.uid
    }
    fn get_sol(&self) -> &SolCore {
        self.sol
    }
    fn get_sol_mut(&mut self) -> &mut SolCore {
        self.sol
    }
}

fn get_u_charge(sol: &SolCore, charge_uid: UItemId) -> &UCharge {
    sol.u_data.items.get(charge_uid).dc_charge().unwrap()
}
