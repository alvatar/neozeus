use super::render_surface::HudSurfaceId;

/// Identifies one bloom-authoring group owned by a specific HUD surface.
///
/// Groups are intentionally explicit and isolated: unrelated components must not silently collapse
/// into one shared global bloom bucket.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum HudBloomGroupId {
    AgentListSelection,
    AgentListAegis,
}

impl HudBloomGroupId {
    /// Returns the owning surface for this bloom group.
    pub(crate) fn surface(self) -> HudSurfaceId {
        match self {
            Self::AgentListSelection | Self::AgentListAegis => HudSurfaceId::MainHud,
        }
    }
}

/// Describes whether a draw operation belongs to the base HUD surface or to one bloom group owned
/// by that surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum HudRenderRoute {
    Base { surface: HudSurfaceId },
    Bloom { group: HudBloomGroupId },
}

impl HudRenderRoute {
    /// Returns the owning surface for this route.
    pub(crate) fn surface(self) -> HudSurfaceId {
        match self {
            Self::Base { surface } => surface,
            Self::Bloom { group } => group.surface(),
        }
    }
}
