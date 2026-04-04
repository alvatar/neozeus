use super::{OwnedTmuxSessionInfo, TerminalRuntimeSpawner};
use bevy::prelude::*;

const OWNED_TMUX_SYNC_INTERVAL_SECS: f32 = 0.5;

#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct OwnedTmuxSessionStore {
    pub(crate) sessions: Vec<OwnedTmuxSessionInfo>,
    pub(crate) last_error: Option<String>,
    last_sync_secs: Option<f32>,
}

impl OwnedTmuxSessionStore {
    pub(crate) fn session(&self, session_uid: &str) -> Option<&OwnedTmuxSessionInfo> {
        self.sessions
            .iter()
            .find(|session| session.session_uid == session_uid)
    }

    pub(crate) fn replace_sessions(&mut self, mut sessions: Vec<OwnedTmuxSessionInfo>) -> bool {
        sessions.sort_by(|left, right| {
            left.created_unix
                .cmp(&right.created_unix)
                .then_with(|| left.tmux_name.cmp(&right.tmux_name))
        });
        let changed = self.sessions != sessions || self.last_error.is_some();
        self.sessions = sessions;
        self.last_error = None;
        changed
    }

    pub(crate) fn record_refresh_error(&mut self, error: String) -> bool {
        let changed = self.last_error.as_deref() != Some(error.as_str());
        self.last_error = Some(error);
        changed
    }

    pub(crate) fn record_removed_session(&mut self, session_uid: &str) {
        self.sessions
            .retain(|session| session.session_uid != session_uid);
        self.last_error = None;
    }
}

pub(crate) fn refresh_owned_tmux_sessions_now(
    runtime_spawner: &TerminalRuntimeSpawner,
    store: &mut OwnedTmuxSessionStore,
) -> Result<bool, String> {
    match runtime_spawner.list_owned_tmux_sessions() {
        Ok(sessions) => Ok(store.replace_sessions(sessions)),
        Err(error) if error == "terminal runtime still connecting" => Err(error),
        Err(error) => {
            let _ = store.record_refresh_error(error.clone());
            Err(error)
        }
    }
}

pub(crate) fn sync_owned_tmux_sessions(
    time: Res<Time>,
    runtime_spawner: Res<TerminalRuntimeSpawner>,
    mut store: ResMut<OwnedTmuxSessionStore>,
    mut redraws: MessageWriter<bevy::window::RequestRedraw>,
) {
    let now_secs = time.elapsed_secs();
    if store
        .last_sync_secs
        .is_some_and(|last_sync_secs| now_secs - last_sync_secs < OWNED_TMUX_SYNC_INTERVAL_SECS)
    {
        return;
    }
    store.last_sync_secs = Some(now_secs);
    if matches!(
        refresh_owned_tmux_sessions_now(&runtime_spawner, &mut store),
        Ok(true)
    ) {
        redraws.write(bevy::window::RequestRedraw);
    }
}
