use bevy::prelude::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum HudLayerId {
    Main,
    Overlay,
    Modal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HudLayerSpec {
    pub(crate) id: HudLayerId,
    pub(crate) order: isize,
    pub(crate) composite_render_layer: usize,
}

const HUD_LAYER_SPECS: [HudLayerSpec; 3] = [
    HudLayerSpec {
        id: HudLayerId::Main,
        order: 50,
        composite_render_layer: 28,
    },
    HudLayerSpec {
        id: HudLayerId::Overlay,
        order: 70,
        composite_render_layer: 35,
    },
    HudLayerSpec {
        id: HudLayerId::Modal,
        order: 90,
        composite_render_layer: 36,
    },
];

impl HudLayerId {
    pub(crate) fn spec(self) -> &'static HudLayerSpec {
        HUD_LAYER_SPECS
            .iter()
            .find(|spec| spec.id == self)
            .expect("every HUD layer id must have a static spec")
    }

    pub(crate) fn order(self) -> isize {
        self.spec().order
    }

    pub(crate) fn bloom_order(self) -> isize {
        self.order() + 1
    }

    pub(crate) fn composite_render_layer(self) -> usize {
        self.spec().composite_render_layer
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct HudLayerRuntime {
    pub(crate) scene_entity: Option<Entity>,
    pub(crate) camera_entity: Option<Entity>,
    pub(crate) surface_image: Option<Handle<Image>>,
}

#[derive(Resource, Clone, Debug, PartialEq, Eq)]
pub(crate) struct HudLayerRegistry {
    layers: BTreeMap<HudLayerId, HudLayerRuntime>,
}

impl Default for HudLayerRegistry {
    fn default() -> Self {
        Self {
            layers: HUD_LAYER_SPECS
                .into_iter()
                .map(|spec| (spec.id, HudLayerRuntime::default()))
                .collect(),
        }
    }
}

impl HudLayerRegistry {
    pub(crate) fn ordered_specs(&self) -> &'static [HudLayerSpec; 3] {
        &HUD_LAYER_SPECS
    }

    #[cfg(test)]
    pub(crate) fn ordered_ids(&self) -> &'static [HudLayerId; 3] {
        const IDS: [HudLayerId; 3] = [HudLayerId::Main, HudLayerId::Overlay, HudLayerId::Modal];
        &IDS
    }

    pub(crate) fn layer(&self, id: HudLayerId) -> Option<&HudLayerRuntime> {
        self.layers.get(&id)
    }

    pub(crate) fn layer_mut(&mut self, id: HudLayerId) -> Option<&mut HudLayerRuntime> {
        self.layers.get_mut(&id)
    }

    pub(crate) fn set_scene_entity(&mut self, id: HudLayerId, entity: Entity) {
        self.layers.entry(id).or_default().scene_entity = Some(entity);
    }

    pub(crate) fn set_camera_entity(&mut self, id: HudLayerId, entity: Entity) {
        self.layers.entry(id).or_default().camera_entity = Some(entity);
    }

    pub(crate) fn set_surface_image(&mut self, id: HudLayerId, image: Handle<Image>) {
        self.layers.entry(id).or_default().surface_image = Some(image);
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
