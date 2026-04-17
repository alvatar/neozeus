use crate::{
    hud::{HudLayerId, HudLayerRegistry},
    startup::DaemonConnectionState,
};
use bevy::prelude::*;

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AppPresentationMode {
    #[default]
    StartupBlackout,
    StartupOverlay,
    Normal,
}

impl AppPresentationMode {
    pub(crate) fn derive(
        startup_connect: Option<&DaemonConnectionState>,
        layers: Option<&HudLayerRegistry>,
    ) -> Self {
        if !startup_connect.is_some_and(DaemonConnectionState::modal_visible) {
            return Self::Normal;
        }

        let startup_surface_ready = layers
            .and_then(|layers| layers.layer(HudLayerId::Modal))
            .and_then(|runtime| runtime.surface_image.as_ref())
            .is_some();
        if startup_surface_ready {
            Self::StartupOverlay
        } else {
            Self::StartupBlackout
        }
    }

    pub(crate) fn blocks_normal_presentation(self) -> bool {
        !matches!(self, Self::Normal)
    }

    pub(crate) fn shows_startup_overlay(self) -> bool {
        matches!(self, Self::StartupOverlay)
    }
}

pub(crate) fn sync_app_presentation_mode(
    startup_connect: Option<Res<DaemonConnectionState>>,
    layers: Option<Res<HudLayerRegistry>>,
    mut mode: ResMut<AppPresentationMode>,
) {
    *mode = AppPresentationMode::derive(startup_connect.as_deref(), layers.as_deref());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::startup::{DaemonConnectionState, StartupConnectPhase};

    #[test]
    fn derive_returns_blackout_while_startup_blocks_before_modal_surface_exists() {
        let startup = DaemonConnectionState::with_phase_for_test(
            StartupConnectPhase::Connecting,
            "Connecting",
        );

        assert_eq!(
            AppPresentationMode::derive(Some(&startup), Some(&HudLayerRegistry::default())),
            AppPresentationMode::StartupBlackout
        );
    }

    #[test]
    fn derive_returns_overlay_once_modal_surface_exists() {
        let startup = DaemonConnectionState::with_phase_for_test(
            StartupConnectPhase::Connecting,
            "Connecting",
        );
        let mut layers = HudLayerRegistry::default();
        layers.set_surface_image(HudLayerId::Modal, Handle::default());

        assert_eq!(
            AppPresentationMode::derive(Some(&startup), Some(&layers)),
            AppPresentationMode::StartupOverlay
        );
    }

    #[test]
    fn derive_returns_normal_when_startup_no_longer_blocks() {
        assert_eq!(
            AppPresentationMode::derive(None, Some(&HudLayerRegistry::default())),
            AppPresentationMode::Normal
        );
    }
}
