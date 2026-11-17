use crate::{
    Value,
    rd::{RAttrConsts, REffectConsts},
    svc::{calc::RawModTemplates, eff_projs::EffProjs},
    ud::UData,
    util::round_pyfa_fitting,
};

#[derive(Copy, Clone)]
pub(crate) struct SvcCtx<'u, 'p> {
    pub(crate) u_data: &'u UData,
    pub(super) eff_projs: &'p EffProjs,
}
impl<'u, 'p> SvcCtx<'u, 'p> {
    pub(in crate::svc) fn new(u_data: &'u UData, eff_projs: &'p EffProjs) -> Self {
        Self { u_data, eff_projs }
    }
    pub(crate) fn ac(&self) -> &RAttrConsts {
        self.u_data.r_data.get_attr_consts()
    }
    pub(crate) fn ec(&self) -> &REffectConsts {
        self.u_data.r_data.get_effect_consts()
    }
    pub(crate) fn rmt(&self) -> &RawModTemplates {
        self.u_data.r_data.get_rmod_templates()
    }
    pub(crate) fn round_fitting_resource(&self, value: Value) -> Value {
        if self.u_data.pyfa_fitting_rounding {
            Value::from_f64_unchecked(round_pyfa_fitting(value.into_f64()))
        } else {
            value.rounded_to_digits(2)
        }
    }
}
