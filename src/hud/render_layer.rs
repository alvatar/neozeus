use bevy::prelude::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum HudLayerId {
    Main,
    Overlay,
    Modal,
}

const HUD_LAYER_ORDER: [HudLayerId; 3] = [HudLayerId::Main, HudLayerId::Overlay, HudLayerId::Modal];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HudLayerRuntime {
    pub(crate) scene_entity: Option<Entity>,
    pub(crate) camera_entity: Option<Entity>,
}

#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub(crate) struct HudLayerRegistry {
    layers: BTreeMap<HudLayerId, HudLayerRuntime>,
}

impl Default for HudLayerRegistry {
    fn default() -> Self {
        Self {
            layers: HUD_LAYER_ORDER
                .into_iter()
                .map(|id| (id, HudLayerRuntime::default()))
                .collect(),
        }
    }
}

impl HudLayerRegistry {
    pub(crate) fn ordered_ids(&self) -> &'static [HudLayerId; 3] {
        &HUD_LAYER_ORDER
    }

    pub(crate) fn layer(&self, id: HudLayerId) -> Option<&HudLayerRuntime> {
        self.layers.get(&id)
    }

    pub(crate) fn set_scene_entity(&mut self, id: HudLayerId, entity: Entity) {
        self.layers.entry(id).or_default().scene_entity = Some(entity);
    }

    pub(crate) fn set_camera_entity(&mut self, id: HudLayerId, entity: Entity) {
        self.layers.entry(id).or_default().camera_entity = Some(entity);
    }
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct HudLayerSceneMarker {
    pub(crate) id: HudLayerId,
}

#[cfg(test)]
mod tests {
    use super::{HudLayerId, HudLayerRegistry};

    #[test]
    fn hud_layer_registry_exposes_stable_order() {
        let registry = HudLayerRegistry::default();
        assert_eq!(
            registry.ordered_ids(),
            &[HudLayerId::Main, HudLayerId::Overlay, HudLayerId::Modal]
        );
    }

    #[test]
    fn hud_layer_registry_contains_all_default_layers() {
        let registry = HudLayerRegistry::default();
        for id in registry.ordered_ids() {
            assert!(registry.layer(*id).is_some(), "missing layer {id:?}");
        }
    }
}
