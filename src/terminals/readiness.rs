use super::{presentation_state::PresentedTerminal, registry::ManagedTerminal, TerminalId};
use bevy::math::UVec2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum TerminalReadiness {
    #[default]
    Missing,
    StartupPending,
    Loading,
    Presentable,
    ReadyForCapture,
}

impl TerminalReadiness {
    pub(crate) fn is_ready_for_capture(self) -> bool {
        matches!(self, Self::ReadyForCapture)
    }

    pub(crate) fn is_startup_pending(self) -> bool {
        matches!(self, Self::StartupPending)
    }
}

fn non_placeholder_texture_state(presented_terminal: &PresentedTerminal) -> bool {
    presented_terminal.texture_state.texture_size != UVec2::ONE
        && presented_terminal.texture_state.cell_size != UVec2::ZERO
}

pub(crate) fn terminal_readiness(
    terminal: Option<&ManagedTerminal>,
    presented_terminal: Option<&PresentedTerminal>,
    override_revision: Option<u64>,
    startup_pending: bool,
) -> TerminalReadiness {
    let base = {
        let Some(terminal) = terminal else {
            return TerminalReadiness::Missing;
        };
        let has_surface_source = override_revision.is_some() || terminal.snapshot.surface.is_some();
        if !has_surface_source {
            if startup_pending {
                TerminalReadiness::StartupPending
            } else {
                TerminalReadiness::Missing
            }
        } else {
            let Some(presented_terminal) = presented_terminal else {
                return if startup_pending {
                    TerminalReadiness::StartupPending
                } else {
                    TerminalReadiness::Loading
                };
            };
            let uploaded_matches = match override_revision {
                Some(override_revision) => {
                    presented_terminal.uploaded_active_override_revision == Some(override_revision)
                }
                None => presented_terminal.uploaded_revision == terminal.surface_revision,
            };
            if !uploaded_matches {
                if startup_pending {
                    TerminalReadiness::StartupPending
                } else {
                    TerminalReadiness::Loading
                }
            } else if non_placeholder_texture_state(presented_terminal) {
                TerminalReadiness::ReadyForCapture
            } else {
                TerminalReadiness::Presentable
            }
        }
    };

    if startup_pending && !base.is_ready_for_capture() {
        TerminalReadiness::StartupPending
    } else {
        base
    }
}

pub(crate) fn terminal_readiness_for_id(
    terminal_id: TerminalId,
    terminal_manager: &super::TerminalManager,
    presentation_store: &super::TerminalPresentationStore,
    override_revision: Option<u64>,
) -> TerminalReadiness {
    terminal_readiness(
        terminal_manager.get(terminal_id),
        presentation_store.get(terminal_id),
        override_revision,
        presentation_store.is_startup_pending(terminal_id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        terminals::{TerminalManager, TerminalPresentationStore, TerminalTextureState},
        tests::test_bridge,
    };
    use bevy::prelude::*;

    fn setup_terminal() -> (TerminalManager, TerminalId, TerminalPresentationStore) {
        let (bridge, _) = test_bridge();
        let mut terminal_manager = TerminalManager::default();
        let terminal_id = terminal_manager.create_terminal(bridge);
        terminal_manager
            .get_mut(terminal_id)
            .expect("terminal exists")
            .snapshot
            .surface = Some(crate::tests::surface_with_text(10, 3, 0, "ready"));
        terminal_manager
            .get_mut(terminal_id)
            .expect("terminal exists")
            .surface_revision = 4;
        let mut presentation_store = TerminalPresentationStore::default();
        presentation_store.register(
            terminal_id,
            crate::terminals::presentation_state::PresentedTerminal {
                image: Handle::default(),
                texture_state: TerminalTextureState {
                    texture_size: UVec2::ONE,
                    cell_size: UVec2::new(8, 16),
                },
                desired_texture_state: TerminalTextureState {
                    texture_size: UVec2::ONE,
                    cell_size: UVec2::new(8, 16),
                },
                display_mode: crate::terminals::TerminalDisplayMode::Smooth,
                uploaded_revision: 0,
                uploaded_active_override_revision: None,
                uploaded_text_selection_revision: None,
                panel_entity: Entity::PLACEHOLDER,
                frame_entity: Entity::PLACEHOLDER,
            },
        );
        (terminal_manager, terminal_id, presentation_store)
    }

    #[test]
    fn terminal_readiness_reports_loading_until_matching_upload_arrives() {
        let (terminal_manager, terminal_id, mut presentation_store) = setup_terminal();
        assert_eq!(
            terminal_readiness_for_id(terminal_id, &terminal_manager, &presentation_store, None),
            TerminalReadiness::Loading
        );

        let presented = presentation_store.get_mut(terminal_id).unwrap();
        presented.uploaded_revision = 4;
        assert_eq!(
            terminal_readiness_for_id(terminal_id, &terminal_manager, &presentation_store, None),
            TerminalReadiness::Presentable
        );
    }

    #[test]
    fn terminal_readiness_reports_startup_pending_until_ready_for_capture() {
        let (terminal_manager, terminal_id, mut presentation_store) = setup_terminal();
        presentation_store.mark_startup_pending(terminal_id);
        assert_eq!(
            terminal_readiness_for_id(terminal_id, &terminal_manager, &presentation_store, None),
            TerminalReadiness::StartupPending
        );

        let presented = presentation_store.get_mut(terminal_id).unwrap();
        presented.uploaded_revision = 4;
        assert_eq!(
            terminal_readiness_for_id(terminal_id, &terminal_manager, &presentation_store, None),
            TerminalReadiness::StartupPending
        );

        let presented = presentation_store.get_mut(terminal_id).unwrap();
        presented.texture_state.texture_size = UVec2::new(640, 480);
        assert_eq!(
            terminal_readiness_for_id(terminal_id, &terminal_manager, &presentation_store, None),
            TerminalReadiness::ReadyForCapture
        );
    }

    #[test]
    fn terminal_readiness_reports_ready_for_capture_for_non_placeholder_upload() {
        let (terminal_manager, terminal_id, mut presentation_store) = setup_terminal();
        let presented = presentation_store.get_mut(terminal_id).unwrap();
        presented.uploaded_revision = 4;
        presented.texture_state.texture_size = UVec2::new(640, 480);
        assert_eq!(
            terminal_readiness_for_id(terminal_id, &terminal_manager, &presentation_store, None),
            TerminalReadiness::ReadyForCapture
        );
    }
}
