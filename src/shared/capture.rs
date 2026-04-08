#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaptureRequestState {
    frames_until_capture: u32,
    requested: bool,
    completed: bool,
}

impl CaptureRequestState {
    pub fn new(frames_until_capture: u32) -> Self {
        Self {
            frames_until_capture,
            requested: false,
            completed: false,
        }
    }

    pub fn requested(&self) -> bool {
        self.requested
    }

    pub fn completed(&self) -> bool {
        self.completed
    }

    pub fn delay_pending(&self) -> bool {
        self.frames_until_capture > 0
    }

    pub fn wait_delay(&mut self) -> bool {
        if self.frames_until_capture > 0 {
            self.frames_until_capture -= 1;
            true
        } else {
            false
        }
    }

    pub fn mark_requested(&mut self) {
        self.requested = true;
    }

    pub fn mark_completed(&mut self) {
        self.completed = true;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArmedCaptureProgress {
    WaitingDelay,
    ArmedThisFrame,
    ReadyToRequest,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ArmedCaptureRequestState {
    request: CaptureRequestState,
    armed: bool,
}

impl ArmedCaptureRequestState {
    pub fn new(frames_until_capture: u32) -> Self {
        Self {
            request: CaptureRequestState::new(frames_until_capture),
            armed: false,
        }
    }

    pub fn requested(&self) -> bool {
        self.request.requested()
    }

    pub fn completed(&self) -> bool {
        self.request.completed()
    }

    pub fn progress(&mut self) -> ArmedCaptureProgress {
        if self.request.wait_delay() {
            return ArmedCaptureProgress::WaitingDelay;
        }
        if !self.armed {
            self.armed = true;
            return ArmedCaptureProgress::ArmedThisFrame;
        }
        ArmedCaptureProgress::ReadyToRequest
    }

    pub fn mark_requested(&mut self) {
        self.request.mark_requested();
    }

    pub fn mark_completed(&mut self) {
        self.request.mark_completed();
    }
}

#[cfg(test)]
mod tests {
    use super::{ArmedCaptureProgress, ArmedCaptureRequestState, CaptureRequestState};

    #[test]
    fn capture_request_state_counts_down_before_ready() {
        let mut state = CaptureRequestState::new(2);
        assert!(state.wait_delay());
        assert!(state.wait_delay());
        assert!(!state.wait_delay());
        assert!(!state.requested());
        assert!(!state.completed());
        state.mark_requested();
        assert!(state.requested());
        state.mark_completed();
        assert!(state.completed());
    }

    #[test]
    fn armed_capture_request_state_waits_then_arms_then_becomes_ready() {
        let mut state = ArmedCaptureRequestState::new(1);
        assert_eq!(state.progress(), ArmedCaptureProgress::WaitingDelay);
        assert_eq!(state.progress(), ArmedCaptureProgress::ArmedThisFrame);
        assert_eq!(state.progress(), ArmedCaptureProgress::ReadyToRequest);
        state.mark_requested();
        assert!(state.requested());
        state.mark_completed();
        assert!(state.completed());
    }
}
