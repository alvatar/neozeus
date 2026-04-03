use super::{OwnedTmuxSessionInfo, TerminalRuntimeSpawner};
use bevy::prelude::*;

const OWNED_TMUX_SYNC_INTERVAL_SECS: f32 = 0.5;

#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub(crate) struct OwnedTmuxSessionStore {
    pub(crate) sessions: Vec<OwnedTmuxSessionInfo>,
    pub(crate) last_error: Option<String>,
    last_sync_secs: Option<f32>,
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
