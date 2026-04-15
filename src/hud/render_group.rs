use bevy::prelude::*;

use super::{HudLayerId, HudRect};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum HudBloomGroupId {
    AgentListSelection,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HudBloomRectSpec {
    pub(crate) layer_id: HudLayerId,
    pub(crate) group_id: HudBloomGroupId,
    pub(crate) rect: HudRect,
    pub(crate) color: Color,
}

#[derive(Resource, Clone, Debug, Default)]
pub(crate) struct HudBloomGroupAuthoring {
    rects: Vec<HudBloomRectSpec>,
}

impl HudBloomGroupAuthoring {
    pub(crate) fn clear_layer(&mut self, layer_id: HudLayerId) {
        self.rects.retain(|spec| spec.layer_id != layer_id);
    }

    pub(crate) fn writer(
        &mut self,
        layer_id: HudLayerId,
        group_id: HudBloomGroupId,
    ) -> HudBloomGroupWriter<'_> {
        HudBloomGroupWriter {
            layer_id,
            group_id,
            authoring: self,
        }
    }

    pub(crate) fn rects_for(
        &self,
        layer_id: HudLayerId,
        group_id: HudBloomGroupId,
    ) -> impl Iterator<Item = &HudBloomRectSpec> {
        self.rects
            .iter()
            .filter(move |spec| spec.layer_id == layer_id && spec.group_id == group_id)
    }
}

pub(crate) struct HudBloomGroupWriter<'a> {
    layer_id: HudLayerId,
    group_id: HudBloomGroupId,
    authoring: &'a mut HudBloomGroupAuthoring,
}

impl HudBloomGroupWriter<'_> {
    pub(crate) fn fill_rect(&mut self, rect: HudRect, color: Color) {
        self.authoring.rects.push(HudBloomRectSpec {
            layer_id: self.layer_id,
            group_id: self.group_id,
            rect,
            color,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::{HudBloomGroupAuthoring, HudBloomGroupId};
    use crate::hud::{HudLayerId, HudRect};
    use bevy::prelude::Color;

    #[test]
    fn bloom_group_authoring_routes_rects_explicitly_by_layer_and_group() {
        let mut authoring = HudBloomGroupAuthoring::default();
        authoring
            .writer(HudLayerId::Main, HudBloomGroupId::AgentListSelection)
            .fill_rect(
                HudRect {
                    x: 1.0,
                    y: 2.0,
                    w: 3.0,
                    h: 4.0,
                },
                Color::WHITE,
            );

        let rects = authoring
            .rects_for(HudLayerId::Main, HudBloomGroupId::AgentListSelection)
            .collect::<Vec<_>>();
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].rect.w, 3.0);
    }

    #[test]
    fn bloom_group_authoring_scopes_groups_per_layer() {
        let mut authoring = HudBloomGroupAuthoring::default();
        authoring
            .writer(HudLayerId::Main, HudBloomGroupId::AgentListSelection)
            .fill_rect(
                HudRect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                Color::WHITE,
            );
        authoring
            .writer(HudLayerId::Overlay, HudBloomGroupId::AgentListSelection)
            .fill_rect(
                HudRect {
                    x: 10.0,
                    y: 10.0,
                    w: 20.0,
                    h: 20.0,
                },
                Color::BLACK,
            );

        assert_eq!(
            authoring
                .rects_for(HudLayerId::Main, HudBloomGroupId::AgentListSelection)
                .count(),
            1
        );
        assert_eq!(
            authoring
                .rects_for(HudLayerId::Overlay, HudBloomGroupId::AgentListSelection)
                .count(),
            1
        );
    }

    #[test]
    fn bloom_group_authoring_safely_returns_no_rects_for_unwritten_groups() {
        let authoring = HudBloomGroupAuthoring::default();
        assert_eq!(
            authoring
                .rects_for(HudLayerId::Modal, HudBloomGroupId::AgentListSelection)
                .count(),
            0
        );
    }
}
