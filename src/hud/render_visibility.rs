use crate::{app::AppSessionState, startup::DaemonConnectionState};
use bevy::prelude::*;

/// Centralized HUD-pass visibility policy derived from authoritative app/startup state.
///
/// The policy exists so all HUD passes obey one consistent contract instead of scattering startup/
/// modal checks across render systems.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HudRenderVisibilityPolicy {
    pub(crate) main_visible: bool,
    pub(crate) overlay_visible: bool,
    pub(crate) bloom_visible: bool,
    pub(crate) modal_visible: bool,
}

impl Default for HudRenderVisibilityPolicy {
    fn default() -> Self {
        Self {
            main_visible: true,
            overlay_visible: true,
            bloom_visible: true,
            modal_visible: true,
        }
    }
}

impl HudRenderVisibilityPolicy {
    /// Derives the current HUD-pass visibility contract from startup and app modal state.
    pub(crate) fn derive(
        app_session: &AppSessionState,
        startup_connect: Option<&DaemonConnectionState>,
    ) -> Self {
        let startup_blocking = startup_connect.is_some_and(DaemonConnectionState::modal_visible);
        Self {
            main_visible: !startup_blocking,
            overlay_visible: !startup_blocking,
            bloom_visible: !startup_blocking && !app_session.modal_visible(),
            modal_visible: true,
        }
    }

    /// Returns whether one HUD layer may be presented in the final compositor output.
    pub(crate) fn layer_visible(self, layer: crate::hud::HudLayerId) -> bool {
        match layer {
            crate::hud::HudLayerId::Main => self.main_visible,
            crate::hud::HudLayerId::Overlay => self.overlay_visible,
            crate::hud::HudLayerId::Modal => self.modal_visible,
        }
    }
}

/// Refreshes the derived HUD visibility policy from authoritative state.
pub(crate) fn sync_hud_render_visibility_policy(
    app_session: Res<AppSessionState>,
    startup_connect: Option<Res<DaemonConnectionState>>,
    mut policy: ResMut<HudRenderVisibilityPolicy>,
) {
    *policy = HudRenderVisibilityPolicy::derive(&app_session, startup_connect.as_deref());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::startup::{DaemonConnectionState, StartupConnectPhase};

    #[test]
    fn derive_blocks_main_overlay_and_bloom_while_startup_modal_is_visible() {
        let app_session = AppSessionState::default();
        let startup = DaemonConnectionState::with_phase_for_test(
            StartupConnectPhase::Connecting,
            "Connecting",
        );

        let policy = HudRenderVisibilityPolicy::derive(&app_session, Some(&startup));

        assert!(!policy.main_visible);
        assert!(!policy.overlay_visible);
        assert!(!policy.bloom_visible);
        assert!(policy.modal_visible);
    }

    #[test]
    fn derive_keeps_main_and_overlay_visible_while_app_modal_only_blocks_bloom() {
        let mut app_session = AppSessionState::default();
        app_session.composer.message_editor.visible = true;

        let policy = HudRenderVisibilityPolicy::derive(&app_session, None);

        assert!(policy.main_visible);
        assert!(policy.overlay_visible);
        assert!(!policy.bloom_visible);
        assert!(policy.modal_visible);
    }
}
