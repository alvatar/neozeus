use super::{OwnedTmuxSessionInfo, TerminalRuntimeSpawner};
use bevy::prelude::*;

const OWNED_TMUX_SYNC_INTERVAL_SECS: f32 = 0.5;
const OWNED_TMUX_CAPTURE_LINES: usize = 200;

#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct OwnedTmuxSessionStore {
    pub(crate) sessions: Vec<OwnedTmuxSessionInfo>,
    pub(crate) last_error: Option<String>,
    last_sync_secs: Option<f32>,
}

#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct OwnedTmuxInspectState {
    pub(crate) selected_session_uid: Option<String>,
    pub(crate) text: String,
    pub(crate) last_error: Option<String>,
    last_sync_secs: Option<f32>,
}

impl OwnedTmuxInspectState {
    pub(crate) fn select(&mut self, session_uid: String) {
        self.selected_session_uid = Some(session_uid);
        self.text.clear();
        self.last_error = None;
        self.last_sync_secs = None;
    }

    pub(crate) fn clear(&mut self) {
        self.selected_session_uid = None;
        self.text.clear();
        self.last_error = None;
        self.last_sync_secs = None;
    }
}

pub(crate) fn sync_owned_tmux_sessions(
    time: Res<Time>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    mut store: ResMut<OwnedTmuxSessionStore>,
) {
    let now_secs = time.elapsed_secs();
    if store
        .last_sync_secs
        .is_some_and(|last_sync_secs| now_secs - last_sync_secs < OWNED_TMUX_SYNC_INTERVAL_SECS)
    {
        return;
    }
    store.last_sync_secs = Some(now_secs);
    match runtime_spawner.list_owned_tmux_sessions() {
        Ok(mut sessions) => {
            sessions.sort_by(|left, right| {
                left.created_unix
                    .cmp(&right.created_unix)
                    .then_with(|| left.tmux_name.cmp(&right.tmux_name))
            });
            store.sessions = sessions;
            store.last_error = None;
        }
        Err(error) if error == "terminal runtime still connecting" => {}
        Err(error) => store.last_error = Some(error),
    }
}

pub(crate) fn sync_owned_tmux_inspect(
    time: Res<Time>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    session_store: Res<OwnedTmuxSessionStore>,
    mut inspect: ResMut<OwnedTmuxInspectState>,
) {
    let Some(session_uid) = inspect.selected_session_uid.clone() else {
        return;
    };
    let now_secs = time.elapsed_secs();
    if inspect
        .last_sync_secs
        .is_some_and(|last_sync_secs| now_secs - last_sync_secs < OWNED_TMUX_SYNC_INTERVAL_SECS)
    {
        return;
    }
    inspect.last_sync_secs = Some(now_secs);

    if !session_store
        .sessions
        .iter()
        .any(|session| session.session_uid == session_uid)
    {
        inspect.text.clear();
        inspect.last_error = Some("Owned tmux session is no longer available".to_owned());
        return;
    }

    match runtime_spawner.capture_owned_tmux_session(&session_uid, OWNED_TMUX_CAPTURE_LINES) {
        Ok(text) => {
            inspect.text = text;
            inspect.last_error = None;
        }
        Err(error) => {
            inspect.text.clear();
            inspect.last_error = Some(error);
        }
    }
}
