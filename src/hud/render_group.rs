use super::render_surface::HudSurfaceId;

/// Tags one Vello-authored scene entity as the source surface for one bloom group.
#[derive(bevy::prelude::Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HudBloomGroupMarker {
    pub(crate) group: HudBloomGroupId,
}

/// Records the authoritative scene entity for each bloom-authoring group.
#[derive(bevy::prelude::Resource, Clone, Debug, Default)]
pub(crate) struct HudBloomGroupRegistry {
    scenes: std::collections::BTreeMap<HudBloomGroupId, bevy::prelude::Entity>,
}

/// Tracks which bloom groups produced any Vello draw content in the latest HUD render pass.
#[derive(bevy::prelude::Resource, Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HudBloomGroupRenderState {
    active_groups: std::collections::BTreeSet<HudBloomGroupId>,
}

impl HudBloomGroupRenderState {
    /// Replaces the active group set with the provided contents.
    pub(crate) fn set_active_groups(
        &mut self,
        active_groups: std::collections::BTreeSet<HudBloomGroupId>,
    ) {
        self.active_groups = active_groups;
    }

    /// Returns whether the provided group produced draw content this frame.
    pub(crate) fn is_active(&self, group: HudBloomGroupId) -> bool {
        self.active_groups.contains(&group)
    }
}

impl HudBloomGroupRegistry {
    /// Registers one scene entity as the owner of the provided bloom group.
    pub(crate) fn register_scene(&mut self, group: HudBloomGroupId, entity: bevy::prelude::Entity) {
        self.scenes.insert(group, entity);
    }

    /// Returns the current scene entity for the requested bloom group, if registered.
    pub(crate) fn scene_entity(&self, group: HudBloomGroupId) -> Option<bevy::prelude::Entity> {
        self.scenes.get(&group).copied()
    }
}

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
    #[cfg(test)]
    pub(crate) fn surface(self) -> HudSurfaceId {
        match self {
            Self::AgentListSelection | Self::AgentListAegis => HudSurfaceId::MainHud,
        }
    }

    /// Returns the explicit bloom groups owned by the provided surface.
    pub(crate) fn ordered_for_surface(surface: HudSurfaceId) -> &'static [Self] {
        match surface {
            HudSurfaceId::MainHud => &[Self::AgentListSelection, Self::AgentListAegis],
            HudSurfaceId::ModalHud => &[],
        }
    }
}

/// Describes whether a draw operation belongs to the base HUD surface or to one bloom group owned
/// by that surface.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum HudRenderRoute {
    Base { surface: HudSurfaceId },
    Bloom { group: HudBloomGroupId },
}

#[cfg(test)]
impl HudRenderRoute {
    /// Returns the owning surface for this route.
    pub(crate) fn surface(self) -> HudSurfaceId {
        match self {
            Self::Base { surface } => surface,
            Self::Bloom { group } => group.surface(),
        }
    }
}
