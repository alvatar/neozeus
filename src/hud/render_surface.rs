/// Identifies one explicit HUD surface in compositor/render ordering.
///
/// The current HUD has two logical surfaces:
/// - `MainHud` for the retained module scene
/// - `ModalHud` for overlays that must render above the main HUD surface
///
/// Bloom ownership is defined relative to these surfaces; there is intentionally no catch-all
/// "global bloom surface" fallback because that would reintroduce the layering drift this contract is
/// meant to prevent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum HudSurfaceId {
    MainHud,
    ModalHud,
}

/// Tags one HUD-authored scene entity with its owning surface identity.
#[derive(bevy::prelude::Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HudSurfaceMarker {
    pub(crate) id: HudSurfaceId,
}

/// Records the authoritative scene entity for each explicit HUD surface.
#[derive(bevy::prelude::Resource, Clone, Debug, Default)]
pub(crate) struct HudSurfaceRegistry {
    scenes: std::collections::BTreeMap<HudSurfaceId, bevy::prelude::Entity>,
}

impl HudSurfaceRegistry {
    /// Registers one scene entity as the owner of the provided surface.
    pub(crate) fn register_scene(&mut self, surface: HudSurfaceId, entity: bevy::prelude::Entity) {
        self.scenes.insert(surface, entity);
    }

    /// Returns the current scene entity for the requested surface, if registered.
    pub(crate) fn scene_entity(&self, surface: HudSurfaceId) -> Option<bevy::prelude::Entity> {
        self.scenes.get(&surface).copied()
    }
}

impl HudSurfaceId {
    /// Returns the explicit HUD surface stack from back to front.
    ///
    /// This ordering is the architectural contract the compositor/effect systems build on.
    #[cfg(test)]
    pub(crate) fn ordered() -> [Self; 2] {
        [Self::MainHud, Self::ModalHud]
    }
}

#[cfg(test)]
mod tests {
    use super::HudSurfaceId;

    #[test]
    fn hud_surface_order_lists_main_before_modal() {
        assert_eq!(HudSurfaceId::ordered(), [HudSurfaceId::MainHud, HudSurfaceId::ModalHud]);
    }
}
