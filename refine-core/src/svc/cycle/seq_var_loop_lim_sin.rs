use super::{
    data::{CSeqHardDtFull, CycleDataFull, GetMainDuration},
    seq::CycleSeq,
    seq_looped::CycleSeqLooped,
    seq_split::CycleSeqSplit,
    seq_var_loop_sin::CSeqLoopSin,
};
use crate::{Count, PValue, UnitInterval, util::LibConverter};

////////////////////////////////////////////////////////////////////////////////////////////////////
// Following parts are lopped:
// Part 1: runs specified number of times
// Part 2: runs once
// Optional hard downtime
////////////////////////////////////////////////////////////////////////////////////////////////////
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub(in crate::svc) struct CSeqLoopLimSin<D, HDT> {
    pub(in crate::svc) p1_data: D,
    pub(in crate::svc) p1_repeat_count: Count,
    pub(in crate::svc) p2_data: D,
    /// Optional hard downtime every loop
    pub(in crate::svc) hard_dt: Option<HDT>,
}
impl<D, HDT> CSeqLoopLimSin<D, HDT> {
    pub(super) fn get_first_cycle(&self) -> &D {
        &self.p1_data
    }
    pub(in crate::svc) fn get_main_duration(&self) -> PValue
    where
        D: GetMainDuration,
    {
        self.p1_data
            .get_main_duration()
            .mul_add(self.p1_repeat_count.into_pvalue(), self.p2_data.get_main_duration())
    }
    pub(super) fn get_hard_dt(&self) -> Option<&HDT> {
        self.hard_dt.as_ref()
    }
}

impl CSeqLoopLimSin<CycleDataFull, CSeqHardDtFull> {
    /// A magazine made of equal, fully charged cycles followed by one reload.
    pub(in crate::svc) fn full_magazine(&self) -> Option<(f64, u32, f64)> {
        let regular = self.p1_data.active;
        let last = self.p2_data.active;
        if self.hard_dt.is_some()
            || self.p1_data.soft_dt.is_some()
            || regular.duration != last.duration
            || regular.chargedness != Some(UnitInterval::ONE)
            || last.chargedness != Some(UnitInterval::ONE)
        {
            return None;
        }
        self.p2_data.soft_dt.filter(|downtime| downtime.reasons.reload)?;
        let cycles = self.p1_repeat_count.into_u32().checked_add(1)?;
        let reload = self.p2_data.get_main_duration() - last.duration;
        Some((regular.duration.into_f64(), cycles, reload.into_f64()))
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Conversions
////////////////////////////////////////////////////////////////////////////////////////////////////
impl<D, HDT> CSeqLoopLimSin<D, HDT> {
    pub(super) fn split_lim_loop(self) -> CycleSeqSplit<D, HDT> {
        CycleSeqSplit {
            limited: None,
            looped: Some(CycleSeqLooped::LoopLimSin(self)),
        }
    }
    pub(super) fn convert<D2, HDT2>(self) -> CSeqLoopLimSin<D2, HDT2>
    where
        D2: From<D>,
        HDT2: From<HDT>,
    {
        CSeqLoopLimSin {
            p1_data: D2::from(self.p1_data),
            p1_repeat_count: self.p1_repeat_count,
            p2_data: D2::from(self.p2_data),
            hard_dt: self.hard_dt.map(Into::into),
        }
    }
    pub(in crate::svc) fn convert_with<C, D2, HDT2>(self, converter: &mut C) -> CSeqLoopLimSin<D2, HDT2>
    where
        C: LibConverter<D, D2>,
        HDT2: From<HDT>,
    {
        CSeqLoopLimSin {
            p1_data: converter.lib_convert(self.p1_data),
            p1_repeat_count: self.p1_repeat_count,
            p2_data: converter.lib_convert(self.p2_data),
            hard_dt: self.hard_dt.map(Into::into),
        }
    }
    pub(in crate::svc) fn optimize(self) -> CycleSeq<D, HDT>
    where
        D: Eq,
    {
        // Have to check for no hard downtime not to make it occur every cycle instead of every N
        // cycles; if we had a LoopLim container, could optimize into it, but just this optimization
        // is not sufficient to introduce it and complicate things elsewhere.
        match self.p1_data == self.p2_data && self.hard_dt.is_none() {
            true => CycleSeq::LoopSin(CSeqLoopSin {
                data: self.p1_data,
                hard_dt: None,
            }),
            false => CycleSeq::LoopLimSin(self),
        }
    }
    pub(super) fn optimize_looped(self) -> CycleSeqLooped<D, HDT>
    where
        D: Eq,
    {
        match self.p1_data == self.p2_data && self.hard_dt.is_none() {
            true => CycleSeqLooped::LoopSin(CSeqLoopSin {
                data: self.p1_data,
                hard_dt: None,
            }),
            false => CycleSeqLooped::LoopLimSin(self),
        }
    }
}
