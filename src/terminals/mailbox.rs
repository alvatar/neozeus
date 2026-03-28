use super::types::{
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
    /// Stores the newest frame update and reports whether the runtime should be woken.
    ///
    /// The mailbox coalesces frames aggressively: if an older frame was still waiting, it is dropped
    /// and the drop counter is incremented. That keeps the consumer focused on the most recent visual
    /// state instead of replaying stale intermediate frames.
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

    /// Stores the newest runtime/status update and reports whether the consumer should be woken.
    ///
    /// Status updates are also coalesced, but unlike frames there is no drop counter because only the
    /// latest status matters semantically.
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

    /// Routes a generic terminal update into the appropriate coalescing path.
    ///
    /// This is just the enum-dispatch convenience wrapper around [`push_frame`] and [`push_status`],
    /// keeping callers from having to duplicate the match themselves.
    pub(crate) fn push(&self, update: TerminalUpdate) -> MailboxPush {
        match update {
            TerminalUpdate::Frame(frame) => self.push_frame(frame),
            TerminalUpdate::Status { runtime, surface } => self.push_status((runtime, surface)),
        }
    }

    /// Takes the currently queued latest frame/status pair and clears the wake flag.
    ///
    /// Draining is destructive by design: the consumer receives the newest coalesced frame, the
    /// newest coalesced status, and the accumulated dropped-frame count, and the mailbox returns to an
    /// empty waiting state.
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

/// Sets the mailbox's one-bit wake latch and tells the caller whether this push should trigger a
/// wake-up.
///
/// The first update after a drain returns `true`; subsequent pushes while the latch is already set
/// return `false` so the runtime is not spammed with redundant wake signals.
fn mark_wake_pending(pending: &mut PendingTerminalUpdates) -> bool {
    if pending.wake_pending {
        false
    } else {
        pending.wake_pending = true;
        true
    }
}
