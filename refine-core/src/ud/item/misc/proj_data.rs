use crate::{PValue, misc::Xyz, rd::RItemBase, ud::UPhysics};

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
enum UProjRange {
    Geometric { surface_to_surface: PValue },
    Explicit(PValue),
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd)]
pub(crate) struct UProjData {
    src_physics: UPhysics,
    tgt_physics: UPhysics,
    geometric_range_c2c: PValue,
    range: UProjRange,
    src_radius: PValue,
    tgt_radius: PValue,
}
impl UProjData {
    pub(crate) fn from_physics_with_rib(
        src_physics: UPhysics,
        tgt_physics: UPhysics,
        src_rib: Option<&RItemBase>,
        tgt_rib: Option<&RItemBase>,
    ) -> Self {
        UProjData::from_physics_with_radii(
            src_physics,
            tgt_physics,
            src_rib.map(|v| v.radius),
            tgt_rib.map(|v| v.radius),
        )
    }
    pub(crate) fn from_physics_with_radii(
        src_physics: UPhysics,
        tgt_physics: UPhysics,
        src_radius: Option<PValue>,
        tgt_radius: Option<PValue>,
    ) -> Self {
        let src_radius = src_radius.unwrap_or(PValue::ZERO);
        let tgt_radius = tgt_radius.unwrap_or(PValue::ZERO);
        let range_c2c = calc_range_c2c(src_physics.coordinates, tgt_physics.coordinates);
        Self {
            src_physics,
            tgt_physics,
            geometric_range_c2c: range_c2c,
            range: UProjRange::Geometric {
                surface_to_surface: calc_range_s2s(range_c2c, src_radius, tgt_radius),
            },
            src_radius,
            tgt_radius,
        }
    }
    pub(crate) fn get_range_c2c(&self) -> PValue {
        self.get_range_override().unwrap_or(self.geometric_range_c2c)
    }
    pub(crate) fn get_range_s2s(&self) -> PValue {
        match self.range {
            UProjRange::Geometric { surface_to_surface } => surface_to_surface,
            UProjRange::Explicit(range) => range,
        }
    }
    pub(crate) fn get_range_c2s(&self) -> PValue {
        self.get_range_override()
            .unwrap_or_else(|| calc_range_c2s(self.geometric_range_c2c, self.tgt_radius))
    }
    pub(crate) fn get_geometric_range_c2c(&self) -> PValue {
        self.geometric_range_c2c
    }
    pub(crate) fn get_range_override(&self) -> Option<PValue> {
        match self.range {
            UProjRange::Geometric { .. } => None,
            UProjRange::Explicit(range) => Some(range),
        }
    }
    pub(crate) fn set_range_override(&mut self, range: Option<PValue>) {
        self.range = match range {
            Some(range) => UProjRange::Explicit(range),
            None => UProjRange::Geometric {
                surface_to_surface: calc_range_s2s(self.geometric_range_c2c, self.src_radius, self.tgt_radius),
            },
        };
    }
    pub(crate) fn get_tgt_radius(&self) -> PValue {
        self.tgt_radius
    }
    pub(crate) fn get_src_coordinates(&self) -> Xyz {
        self.src_physics.coordinates
    }
    pub(crate) fn get_tgt_coordinates(&self) -> Xyz {
        self.tgt_physics.coordinates
    }
    pub(crate) fn get_src_direction(&self) -> Xyz {
        self.src_physics.direction
    }
    pub(crate) fn get_tgt_direction(&self) -> Xyz {
        self.tgt_physics.direction
    }
    pub(crate) fn get_src_speed(&self) -> PValue {
        self.src_physics.speed
    }
    pub(crate) fn get_tgt_speed(&self) -> PValue {
        self.tgt_physics.speed
    }
    pub(crate) fn update_src_physics(&mut self, src_physics: UPhysics) {
        self.src_physics = src_physics;
        self.geometric_range_c2c = calc_range_c2c(self.src_physics.coordinates, self.tgt_physics.coordinates);
        self.update_s2s_range();
    }
    pub(crate) fn update_tgt_physics(&mut self, tgt_physics: UPhysics) {
        self.tgt_physics = tgt_physics;
        self.geometric_range_c2c = calc_range_c2c(self.src_physics.coordinates, self.tgt_physics.coordinates);
        self.update_s2s_range();
    }
    pub(crate) fn update_src_radius(&mut self, src_radius: PValue) -> bool {
        if self.src_radius == src_radius {
            return false;
        };
        self.src_radius = src_radius;
        self.update_s2s_range();
        true
    }
    pub(crate) fn update_tgt_radius(&mut self, tgt_radius: PValue) -> bool {
        if self.tgt_radius == tgt_radius {
            return false;
        };
        self.tgt_radius = tgt_radius;
        self.update_s2s_range();
        true
    }
    pub(crate) fn update_radii(&mut self, src_radius: PValue, tgt_radius: PValue) {
        self.src_radius = src_radius;
        self.tgt_radius = tgt_radius;
        self.update_s2s_range();
    }
    fn update_s2s_range(&mut self) {
        if let UProjRange::Geometric { surface_to_surface } = &mut self.range {
            *surface_to_surface = calc_range_s2s(self.geometric_range_c2c, self.src_radius, self.tgt_radius);
        }
    }
}

fn calc_range_c2c(src_coordinates: Xyz, tgt_coordinates: Xyz) -> PValue {
    // Do not check anything because result will always be positive
    PValue::from_f64_unchecked(
        ((tgt_coordinates.x - src_coordinates.x).powi(2)
            + (tgt_coordinates.y - src_coordinates.y).powi(2)
            + (tgt_coordinates.z - src_coordinates.z).powi(2))
        .sqrt()
        .into_f64(),
    )
}

fn calc_range_c2s(range_c2c: PValue, tgt_radius: PValue) -> PValue {
    PValue::from_finite_value_clamped(range_c2c - tgt_radius)
}

fn calc_range_s2s(range_c2c: PValue, src_radius: PValue, tgt_radius: PValue) -> PValue {
    PValue::from_finite_value_clamped(range_c2c - src_radius - tgt_radius)
}

////////////////////////////////////////////////////////////////////////////////////////////////////
// Debugging
////////////////////////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "dev")]
mod debug {
    use super::*;
    use crate::dbg::DebugResult;

    impl UProjData {
        pub(crate) fn get_src_radius(&self) -> PValue {
            self.src_radius
        }
        pub(crate) fn consistency_check(&self) -> DebugResult {
            if let UProjRange::Geometric { surface_to_surface } = self.range
                && surface_to_surface != calc_range_s2s(self.geometric_range_c2c, self.src_radius, self.tgt_radius)
            {
                return Err(Default::default());
            }
            Ok(())
        }
    }
}
