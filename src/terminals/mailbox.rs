use crate::terminals::{
    DrainedTerminalUpdates, LatestTerminalStatus, TerminalFrameUpdate, TerminalUpdate,
};
use std::sync::Mutex;

#[derive(Default)]
struct PendingTerminalUpdates {
    latest_frame: Option<TerminalFrameUpdate>,
    latest_status: Option<LatestTerminalStatus>,
    dropped_frames: u64,
    wake_pending: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MailboxPush {
    pub(crate) should_wake: bool,
}

#[derive(Default)]
pub(crate) struct TerminalUpdateMailbox {
    inner: Mutex<PendingTerminalUpdates>,
}

impl TerminalUpdateMailbox {
    pub(crate) fn push_frame(&self, frame: TerminalFrameUpdate) -> MailboxPush {
        let mut pending = match self.inner.lock() {
            Ok(pending) => pending,
            Err(poisoned) => poisoned.into_inner(),
        };
        if pending.latest_frame.replace(frame).is_some() {
            pending.dropped_frames += 1;
        }
        MailboxPush {
            should_wake: mark_wake_pending(&mut pending),
        }
    }

    pub(crate) fn push_status(&self, status: LatestTerminalStatus) -> MailboxPush {
        let mut pending = match self.inner.lock() {
            Ok(pending) => pending,
            Err(poisoned) => poisoned.into_inner(),
        };
        pending.latest_status = Some(status);
        MailboxPush {
            should_wake: mark_wake_pending(&mut pending),
        }
    }

    pub(crate) fn push(&self, update: TerminalUpdate) -> MailboxPush {
        match update {
            TerminalUpdate::Frame(frame) => self.push_frame(frame),
            TerminalUpdate::Status { runtime, surface } => self.push_status((runtime, surface)),
        }
    }

    pub(crate) fn drain(&self) -> DrainedTerminalUpdates {
        let mut pending = match self.inner.lock() {
            Ok(pending) => pending,
            Err(poisoned) => poisoned.into_inner(),
        };
        pending.wake_pending = false;
        (
            pending.latest_frame.take(),
            pending.latest_status.take(),
            std::mem::take(&mut pending.dropped_frames),
        )
    }
}

fn mark_wake_pending(pending: &mut PendingTerminalUpdates) -> bool {
    if pending.wake_pending {
        false
    } else {
        pending.wake_pending = true;
        true
    }
}
