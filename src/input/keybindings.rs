use crate::hud::HudWidgetKey;
use bevy::prelude::KeyCode;

/// Physical keyboard chord used for non-text shortcut matching.
///
/// The model is intentionally keyed by `KeyCode` plus explicit modifier booleans so NeoZeus can
/// reason about shortcut ownership independently of text payloads or keyboard layout casing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct KeyChord {
    pub(crate) code: KeyCode,
    pub(crate) ctrl: bool,
    pub(crate) alt: bool,
    pub(crate) shift: Option<bool>,
    pub(crate) super_key: bool,
}

/// High-level route/domain that owns one class of keyboard shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum KeybindingDomain {
    Primary,
    DirectInput,
    CreateAgentDialog,
    CloneAgentDialog,
    RenameAgentDialog,
    ResetDialog,
    AegisDialog,
    MessageDialog,
    TaskDialog,
}

/// Routed shortcut action emitted after a binding match.
///
/// This enum intentionally models only shortcut-like actions. Raw text insertion and PTY byte
/// translation remain route-local logic and are not represented here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum KeybindingAction {
    SpawnTerminal,
    OpenCloneDialog,
    KillSelected,
    OpenResetDialog,
    ExitApplication,
    OpenMessageEditor,
    OpenTaskEditor,
    OpenRenameDialog,
    ToggleAegis,
    ConsumeNextTask,
    TogglePaused,
    ToggleAgentContext,
    ClearDoneTasks,
    ScrollPageUp,
    ScrollPageDown,
    HudNextRow,
    HudPrevRow,
    ToggleWidget(HudWidgetKey),
    ResetWidget(HudWidgetKey),
    ToggleDirectInput,
    DirectInputScrollToBottom,
    DialogEscape,
    DialogTabForward,
    DialogTabBackward,
    MessageDialogSubmit,
    TaskDialogClearDone,
}

/// One authoritative keybinding definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct KeybindingSpec {
    pub(crate) domain: KeybindingDomain,
    pub(crate) chord: KeyChord,
    pub(crate) action: KeybindingAction,
}

const fn chord(
    code: KeyCode,
    ctrl: bool,
    alt: bool,
    shift: Option<bool>,
    super_key: bool,
) -> KeyChord {
    KeyChord {
        code,
        ctrl,
        alt,
        shift,
        super_key,
    }
}

const fn binding(
    domain: KeybindingDomain,
    action: KeybindingAction,
    chord: KeyChord,
) -> KeybindingSpec {
    KeybindingSpec {
        domain,
        action,
        chord,
    }
}

pub(crate) const fn plain(action: KeybindingAction, code: KeyCode) -> KeybindingSpec {
    binding(
        KeybindingDomain::Primary,
        action,
        chord(code, false, false, Some(false), false),
    )
}

pub(crate) const fn plain_letter(action: KeybindingAction, code: KeyCode) -> KeybindingSpec {
    plain(action, code)
}

pub(crate) const fn ctrl(action: KeybindingAction, code: KeyCode) -> KeybindingSpec {
    binding(
        KeybindingDomain::Primary,
        action,
        chord(code, true, false, None, false),
    )
}

pub(crate) const fn ctrl_alt(action: KeybindingAction, code: KeyCode) -> KeybindingSpec {
    binding(
        KeybindingDomain::Primary,
        action,
        chord(code, true, true, None, false),
    )
}

pub(crate) const fn alt(action: KeybindingAction, code: KeyCode) -> KeybindingSpec {
    binding(
        KeybindingDomain::Primary,
        action,
        chord(code, false, true, None, false),
    )
}

pub(crate) const fn alt_shift(action: KeybindingAction, code: KeyCode) -> KeybindingSpec {
    binding(
        KeybindingDomain::Primary,
        action,
        chord(code, false, true, Some(true), false),
    )
}

const fn route_plain(
    domain: KeybindingDomain,
    action: KeybindingAction,
    code: KeyCode,
) -> KeybindingSpec {
    binding(
        domain,
        action,
        chord(code, false, false, Some(false), false),
    )
}

const fn route_shift(
    domain: KeybindingDomain,
    action: KeybindingAction,
    code: KeyCode,
) -> KeybindingSpec {
    binding(domain, action, chord(code, false, false, Some(true), false))
}

const fn route_ctrl(
    domain: KeybindingDomain,
    action: KeybindingAction,
    code: KeyCode,
) -> KeybindingSpec {
    binding(domain, action, chord(code, true, false, None, false))
}

/// Returns whether one key event matches a central shortcut chord under explicit modifier state.
pub(crate) fn chord_matches(
    event: &bevy::input::keyboard::KeyboardInput,
    chord: KeyChord,
    ctrl: bool,
    alt: bool,
    shift: bool,
    super_key: bool,
) -> bool {
    event.state == bevy::input::ButtonState::Pressed
        && event.key_code == chord.code
        && ctrl == chord.ctrl
        && alt == chord.alt
        && super_key == chord.super_key
        && chord
            .shift
            .is_none_or(|expected_shift| expected_shift == shift)
}

/// Returns the matched action from one binding table under explicit modifier state.
pub(crate) fn binding_action_for_event(
    bindings: &[KeybindingSpec],
    event: &bevy::input::keyboard::KeyboardInput,
    ctrl: bool,
    alt: bool,
    shift: bool,
    super_key: bool,
) -> Option<KeybindingAction> {
    bindings
        .iter()
        .find(|binding| chord_matches(event, binding.chord, ctrl, alt, shift, super_key))
        .map(|binding| binding.action)
}

/// Authoritative non-modal shortcut inventory.
pub(crate) const PRIMARY_KEYBINDINGS: &[KeybindingSpec] = &[
    plain_letter(KeybindingAction::SpawnTerminal, KeyCode::KeyZ),
    plain_letter(KeybindingAction::OpenCloneDialog, KeyCode::KeyC),
    ctrl(KeybindingAction::KillSelected, KeyCode::KeyK),
    ctrl_alt(KeybindingAction::OpenResetDialog, KeyCode::KeyR),
    plain(KeybindingAction::ExitApplication, KeyCode::F10),
    plain(KeybindingAction::OpenMessageEditor, KeyCode::Enter),
    plain_letter(KeybindingAction::OpenTaskEditor, KeyCode::KeyT),
    plain_letter(KeybindingAction::OpenRenameDialog, KeyCode::KeyR),
    plain_letter(KeybindingAction::ToggleAegis, KeyCode::KeyA),
    plain_letter(KeybindingAction::ConsumeNextTask, KeyCode::KeyN),
    plain_letter(KeybindingAction::TogglePaused, KeyCode::KeyP),
    plain_letter(KeybindingAction::ToggleAgentContext, KeyCode::KeyI),
    ctrl(KeybindingAction::ClearDoneTasks, KeyCode::KeyT),
    ctrl(KeybindingAction::ScrollPageDown, KeyCode::KeyV),
    alt(KeybindingAction::ScrollPageUp, KeyCode::KeyV),
    plain_letter(KeybindingAction::HudNextRow, KeyCode::KeyJ),
    plain_letter(KeybindingAction::HudPrevRow, KeyCode::KeyK),
    plain(KeybindingAction::HudNextRow, KeyCode::ArrowDown),
    plain(KeybindingAction::HudPrevRow, KeyCode::ArrowUp),
    plain(
        KeybindingAction::ToggleWidget(HudWidgetKey::InfoBar),
        KeyCode::Digit0,
    ),
    plain(
        KeybindingAction::ToggleWidget(HudWidgetKey::AgentList),
        KeyCode::Digit1,
    ),
    plain(
        KeybindingAction::ToggleWidget(HudWidgetKey::ConversationList),
        KeyCode::Digit2,
    ),
    plain(
        KeybindingAction::ToggleWidget(HudWidgetKey::ThreadPane),
        KeyCode::Digit3,
    ),
    alt_shift(
        KeybindingAction::ResetWidget(HudWidgetKey::InfoBar),
        KeyCode::Digit0,
    ),
    alt_shift(
        KeybindingAction::ResetWidget(HudWidgetKey::AgentList),
        KeyCode::Digit1,
    ),
    alt_shift(
        KeybindingAction::ResetWidget(HudWidgetKey::ConversationList),
        KeyCode::Digit2,
    ),
    alt_shift(
        KeybindingAction::ResetWidget(HudWidgetKey::ThreadPane),
        KeyCode::Digit3,
    ),
];

/// Direct-input route-local shortcut inventory.
pub(crate) const DIRECT_INPUT_KEYBINDINGS: &[KeybindingSpec] = &[
    route_ctrl(
        KeybindingDomain::DirectInput,
        KeybindingAction::ToggleDirectInput,
        KeyCode::Enter,
    ),
    route_plain(
        KeybindingDomain::DirectInput,
        KeybindingAction::DirectInputScrollToBottom,
        KeyCode::End,
    ),
    route_plain(
        KeybindingDomain::DirectInput,
        KeybindingAction::ScrollPageUp,
        KeyCode::PageUp,
    ),
    route_plain(
        KeybindingDomain::DirectInput,
        KeybindingAction::ScrollPageDown,
        KeyCode::PageDown,
    ),
];

pub(crate) const CREATE_AGENT_DIALOG_KEYBINDINGS: &[KeybindingSpec] = &[
    route_plain(
        KeybindingDomain::CreateAgentDialog,
        KeybindingAction::DialogEscape,
        KeyCode::Escape,
    ),
    route_plain(
        KeybindingDomain::CreateAgentDialog,
        KeybindingAction::DialogTabForward,
        KeyCode::Tab,
    ),
    route_shift(
        KeybindingDomain::CreateAgentDialog,
        KeybindingAction::DialogTabBackward,
        KeyCode::Tab,
    ),
];

pub(crate) const CLONE_AGENT_DIALOG_KEYBINDINGS: &[KeybindingSpec] = &[
    route_plain(
        KeybindingDomain::CloneAgentDialog,
        KeybindingAction::DialogEscape,
        KeyCode::Escape,
    ),
    route_plain(
        KeybindingDomain::CloneAgentDialog,
        KeybindingAction::DialogTabForward,
        KeyCode::Tab,
    ),
    route_shift(
        KeybindingDomain::CloneAgentDialog,
        KeybindingAction::DialogTabBackward,
        KeyCode::Tab,
    ),
];

pub(crate) const RENAME_AGENT_DIALOG_KEYBINDINGS: &[KeybindingSpec] = &[
    route_plain(
        KeybindingDomain::RenameAgentDialog,
        KeybindingAction::DialogEscape,
        KeyCode::Escape,
    ),
    route_plain(
        KeybindingDomain::RenameAgentDialog,
        KeybindingAction::DialogTabForward,
        KeyCode::Tab,
    ),
    route_shift(
        KeybindingDomain::RenameAgentDialog,
        KeybindingAction::DialogTabBackward,
        KeyCode::Tab,
    ),
];

pub(crate) const RESET_DIALOG_KEYBINDINGS: &[KeybindingSpec] = &[
    route_plain(
        KeybindingDomain::ResetDialog,
        KeybindingAction::DialogEscape,
        KeyCode::Escape,
    ),
    route_plain(
        KeybindingDomain::ResetDialog,
        KeybindingAction::DialogTabForward,
        KeyCode::Tab,
    ),
    route_shift(
        KeybindingDomain::ResetDialog,
        KeybindingAction::DialogTabBackward,
        KeyCode::Tab,
    ),
];

pub(crate) const AEGIS_DIALOG_KEYBINDINGS: &[KeybindingSpec] = &[
    route_plain(
        KeybindingDomain::AegisDialog,
        KeybindingAction::DialogEscape,
        KeyCode::Escape,
    ),
    route_plain(
        KeybindingDomain::AegisDialog,
        KeybindingAction::DialogTabForward,
        KeyCode::Tab,
    ),
    route_shift(
        KeybindingDomain::AegisDialog,
        KeybindingAction::DialogTabBackward,
        KeyCode::Tab,
    ),
];

pub(crate) const MESSAGE_DIALOG_KEYBINDINGS: &[KeybindingSpec] = &[
    route_ctrl(
        KeybindingDomain::MessageDialog,
        KeybindingAction::MessageDialogSubmit,
        KeyCode::KeyS,
    ),
    route_plain(
        KeybindingDomain::MessageDialog,
        KeybindingAction::DialogEscape,
        KeyCode::Escape,
    ),
    route_plain(
        KeybindingDomain::MessageDialog,
        KeybindingAction::DialogTabForward,
        KeyCode::Tab,
    ),
    route_shift(
        KeybindingDomain::MessageDialog,
        KeybindingAction::DialogTabBackward,
        KeyCode::Tab,
    ),
];

pub(crate) const TASK_DIALOG_KEYBINDINGS: &[KeybindingSpec] = &[
    route_ctrl(
        KeybindingDomain::TaskDialog,
        KeybindingAction::TaskDialogClearDone,
        KeyCode::KeyT,
    ),
    route_plain(
        KeybindingDomain::TaskDialog,
        KeybindingAction::DialogEscape,
        KeyCode::Escape,
    ),
    route_plain(
        KeybindingDomain::TaskDialog,
        KeybindingAction::DialogTabForward,
        KeyCode::Tab,
    ),
    route_shift(
        KeybindingDomain::TaskDialog,
        KeybindingAction::DialogTabBackward,
        KeyCode::Tab,
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn assert_no_duplicate_chords(bindings: &[KeybindingSpec]) {
        let mut seen = BTreeMap::new();
        for binding in bindings {
            let previous = seen.insert(binding.chord, binding.action);
            assert!(
                previous.is_none(),
                "duplicate chord {:?} for {:?} and {:?}",
                binding.chord,
                previous.unwrap_or(binding.action),
                binding.action
            );
        }
    }

    #[test]
    fn plain_letter_helper_encodes_unmodified_letter_chord() {
        let binding = plain_letter(KeybindingAction::TogglePaused, KeyCode::KeyP);
        assert_eq!(binding.domain, KeybindingDomain::Primary);
        assert_eq!(binding.chord.code, KeyCode::KeyP);
        assert!(!binding.chord.ctrl);
        assert!(!binding.chord.alt);
        assert_eq!(binding.chord.shift, Some(false));
        assert!(!binding.chord.super_key);
    }

    #[test]
    fn ctrl_alt_helper_encodes_modifier_chord_without_shift_requirement() {
        let binding = ctrl_alt(KeybindingAction::OpenResetDialog, KeyCode::KeyR);
        assert_eq!(binding.chord.code, KeyCode::KeyR);
        assert!(binding.chord.ctrl);
        assert!(binding.chord.alt);
        assert_eq!(binding.chord.shift, None);
        assert!(!binding.chord.super_key);
    }

    #[test]
    fn primary_keybindings_have_no_duplicate_chords() {
        assert_no_duplicate_chords(PRIMARY_KEYBINDINGS);
    }

    #[test]
    fn direct_input_keybindings_have_no_duplicate_chords() {
        assert_no_duplicate_chords(DIRECT_INPUT_KEYBINDINGS);
    }

    #[test]
    fn primary_keybindings_include_historical_high_risk_shortcuts() {
        let chords = PRIMARY_KEYBINDINGS
            .iter()
            .map(|binding| binding.chord)
            .collect::<Vec<_>>();
        assert!(chords.contains(&chord(KeyCode::KeyP, false, false, Some(false), false)));
        assert!(chords.contains(&chord(KeyCode::KeyK, true, false, None, false)));
        assert!(chords.contains(&chord(KeyCode::KeyR, true, true, None, false)));
    }
}
