use crate::{misc::DefOption, ud::UItem};

/// Items which will be included in outgoing rep stats.
#[cfg_attr(feature = "serde", derive(serde::Deserialize))]
#[derive(Copy, Clone)]
pub struct StatOutRepItemKinds {
    #[cfg_attr(feature = "serde", serde(default = "custom_serde::kind_default"))]
    default: bool = true,
    #[cfg_attr(feature = "serde", serde(default))]
    module: DefOption = DefOption::Default,
    #[cfg_attr(feature = "serde", serde(default))]
    minion: DefOption = DefOption::Default,
    #[cfg_attr(feature = "serde", serde(default))]
    pyfa_graph_range_rounding: bool = false,
}
impl StatOutRepItemKinds {
    /// True to have all supported item kinds enabled by default, false to have them disabled.
    pub fn new(default: bool) -> Self {
        Self { default, .. }
    }
    pub fn with_module(mut self, enabled: bool) -> Self {
        self.module = enabled.into();
        self
    }
    /// Use Pyfa's per-source range factor for Remote Repairs graph points.
    /// This affects applied outgoing repair stats only and defaults to false.
    pub fn with_pyfa_graph_range_rounding(mut self, enabled: bool) -> Self {
        self.pyfa_graph_range_rounding = enabled;
        self
    }
    pub fn with_minion(mut self, enabled: bool) -> Self {
        self.minion = enabled.into();
        self
    }
}
const impl Default for StatOutRepItemKinds {
    fn default() -> Self {
        Self { .. }
    }
}
impl StatOutRepItemKinds {
    pub(in crate::svc::vast::stats) fn pyfa_graph_range_rounding(self) -> bool {
        self.pyfa_graph_range_rounding
    }
    pub(in crate::svc::vast::stats) fn resolve(self, u_item: &UItem) -> bool {
        match u_item {
            UItem::Drone(..) => self.minion.is_enabled(self.default),
            UItem::Fighter(..) => self.minion.is_enabled(self.default),
            // Just consider everything else as modules
            _ => self.module.is_enabled(self.default),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Custom de/serialization
////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "serde")]
mod custom_serde {
    pub(super) fn kind_default() -> bool {
        true
    }
}
