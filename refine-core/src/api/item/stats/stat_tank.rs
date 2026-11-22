use crate::{
    DpsProfile, UnitInterval,
    stats::{ItemStats, StatEhp, StatErps, StatHp, StatResists, StatRps, StatTimeOptions, err::StatItemError},
};

////////////////////////////////////////////////////////////////////////////////////////////////////
// Public
////////////////////////////////////////////////////////////////////////////////////////////////////
impl<'s> ItemStats<'s> {
    pub fn get_stat_resists(&mut self) -> Result<StatResists, StatItemError<!>> {
        let item_uid = self.uid;
        let sol = &mut *self.sol;
        sol.svc
            .get_stat_item_resists(&sol.u_data, item_uid)
            .map_err(|e| StatItemError::from_svc_err(e, &sol.u_data.items))
    }
    pub fn get_stat_hp(&mut self) -> Result<StatHp, StatItemError<!>> {
        let item_uid = self.uid;
        let sol = &mut *self.sol;
        sol.svc
            .get_stat_item_hp(&mut sol.reuse.cseq_cache, &sol.u_data, item_uid)
            .map_err(|e| StatItemError::from_svc_err(e, &sol.u_data.items))
    }
    pub fn get_stat_ehp(&mut self, incoming_dps: Option<DpsProfile>) -> Result<StatEhp, StatItemError<!>> {
        let item_uid = self.uid;
        let sol = &mut *self.sol;
        sol.svc
            .get_stat_item_ehp(&mut sol.reuse.cseq_cache, &sol.u_data, item_uid, incoming_dps)
            .map_err(|e| StatItemError::from_svc_err(e, &sol.u_data.items))
    }
    pub fn get_stat_wc_ehp(&mut self) -> Result<StatEhp, StatItemError<!>> {
        let item_uid = self.uid;
        let sol = &mut *self.sol;
        sol.svc
            .get_stat_item_wc_ehp(&mut sol.reuse.cseq_cache, &sol.u_data, item_uid)
            .map_err(|e| StatItemError::from_svc_err(e, &sol.u_data.items))
    }
    pub fn get_stat_rps(
        &mut self,
        time_options: StatTimeOptions,
        shield_perc: UnitInterval,
    ) -> Result<StatRps, StatItemError<!>> {
        self.get_stat_rps_impl(time_options, shield_perc, false)
    }
    /// Read repair rates as Pyfa does, with incoming repair before recipient
    /// capacity and repair impedance. Range, spool, diminishing returns,
    /// and the fitted recipient's `disallowAssistance` flag still apply.
    /// With the pinned Pyfa v2.70.0 data, the recipient's active local Warp
    /// Disruption Field Generator effect (3380) also blocks incoming repair.
    /// The applied repair readout remains available through [Self::get_stat_rps].
    pub fn get_stat_rps_pyfa(
        &mut self,
        time_options: StatTimeOptions,
        shield_perc: UnitInterval,
    ) -> Result<StatRps, StatItemError<!>> {
        self.get_stat_rps_impl(time_options, shield_perc, true)
    }
    fn get_stat_rps_impl(
        &mut self,
        time_options: StatTimeOptions,
        shield_perc: UnitInterval,
        pyfa_incoming: bool,
    ) -> Result<StatRps, StatItemError<!>> {
        let item_uid = self.uid;
        let sol = &mut *self.sol;
        sol.svc
            .get_stat_item_rps(
                &mut sol.reuse.cseq_cache,
                &sol.u_data,
                item_uid,
                time_options,
                shield_perc,
                pyfa_incoming,
            )
            .map_err(|e| StatItemError::from_svc_err(e, &sol.u_data.items))
    }
    pub fn get_stat_erps(
        &mut self,
        incoming_dps: Option<DpsProfile>,
        time_options: StatTimeOptions,
        shield_perc: UnitInterval,
    ) -> Result<StatErps, StatItemError<!>> {
        let item_uid = self.uid;
        let sol = &mut *self.sol;
        sol.svc
            .get_stat_item_erps(
                &mut sol.reuse.cseq_cache,
                &sol.u_data,
                item_uid,
                incoming_dps,
                time_options,
                shield_perc,
            )
            .map_err(|e| StatItemError::from_svc_err(e, &sol.u_data.items))
    }
    pub fn get_stat_breach_resist(&mut self) -> Result<UnitInterval, StatItemError<!>> {
        let item_uid = self.uid;
        let sol = &mut *self.sol;
        sol.svc
            .get_stat_item_breach_resist(&sol.u_data, item_uid)
            .map_err(|e| StatItemError::from_svc_err(e, &sol.u_data.items))
    }
}
