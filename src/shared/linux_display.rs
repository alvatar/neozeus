#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinuxDisplayEnvironment<'a> {
    session_type: Option<&'a str>,
    wayland_display: Option<&'a str>,
    display: Option<&'a str>,
}

impl<'a> LinuxDisplayEnvironment<'a> {
    pub fn new(
        session_type: Option<&'a str>,
        wayland_display: Option<&'a str>,
        display: Option<&'a str>,
    ) -> Self {
        Self {
            session_type,
            wayland_display,
            display,
        }
    }

    pub fn prefers_wayland(self) -> bool {
        self.wayland_display_present()
            || self
                .session_type
                .map(str::trim)
                .is_some_and(|value| value.eq_ignore_ascii_case("wayland"))
    }

    pub fn prefers_x11(self) -> bool {
        self.x11_display_present()
            || self
                .session_type
                .map(str::trim)
                .is_some_and(|value| value.eq_ignore_ascii_case("x11"))
    }

    pub fn wayland_display_present(self) -> bool {
        self.wayland_display
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
    }

    pub fn x11_display_present(self) -> bool {
        self.display
            .map(str::trim)
            .is_some_and(|value| !value.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::LinuxDisplayEnvironment;

    #[test]
    fn linux_display_environment_matches_wayland_and_x11_inference_for_bootstrap_and_clipboard() {
        let env = LinuxDisplayEnvironment::new(Some("wayland"), Some("wayland-1"), Some(":0"));
        assert!(env.prefers_wayland());
        assert!(env.prefers_x11());

        let env = LinuxDisplayEnvironment::new(Some("x11"), None, Some(":0"));
        assert!(!env.prefers_wayland());
        assert!(env.prefers_x11());

        let env = LinuxDisplayEnvironment::new(None, Some("wayland-1"), None);
        assert!(env.prefers_wayland());
        assert!(!env.prefers_x11());

        let env = LinuxDisplayEnvironment::new(None, None, None);
        assert!(!env.prefers_wayland());
        assert!(!env.prefers_x11());
    }
}
