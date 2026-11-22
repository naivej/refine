#[cfg(feature = "dev")]
pub(crate) use balance::StatCapBlcNosfsOptionsInt;
pub(crate) use balance::StatCapBlcSrcKindsInt;
pub use balance::{StatCapBlcNosfs, StatCapBlcRegen, StatCapBlcSrcKinds};
pub(crate) use sim::StatCapSimStaggerInt;
pub use sim::{StatCapSim, StatCapSimPyfa, StatCapSimPyfaError, StatCapSimStagger, StatCapSimTraceRequest};

use crate::{nd::NEffectGeneralOutputGetter, rd::REffectProjOpcSpec};

fn use_pyfa_nosferatu_drain(spec: &mut REffectProjOpcSpec<NEffectGeneralOutputGetter>) {
    if matches!(spec.base, NEffectGeneralOutputGetter::NeutNosf) {
        spec.base = NEffectGeneralOutputGetter::NeutNosfPyfa;
        spec.remote_limit_attr_id = None;
        spec.exact_resistance = true;
    }
}

mod balance;
mod cap_amount;
mod neut_resist;
mod sim;
