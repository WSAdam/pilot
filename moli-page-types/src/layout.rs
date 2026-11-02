/// Selects whether layout-dependent operations use Moli's deterministic
/// compatibility geometry or construct a real one-shot layout pass.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum LayoutPolicy {
    /// Preserve the pre-layout compatibility behavior and reject operations
    /// such as renderer screenshots that require a real layout backend.
    #[default]
    Mock,
    /// Only screenshot, screencast, and PDF output build layout.
    /// Screenshots and screencasts publish layout; print projections are temporary.
    /// Geometry queries and input read the last published layout, if any.
    OnDemand,
    /// pilot: `OnDemand`, except that a geometry query or input event after a
    /// DOM change rebuilds the latest layout instead of reading the frozen one,
    /// as Chromium does. Playwright needs this to act on nodes added later.
    Live,
}

impl LayoutPolicy {
    pub const fn uses_real_layout(self) -> bool {
        matches!(self, Self::OnDemand | Self::Live)
    }

    /// Whether a latest layout built before the current DOM version is stale.
    pub const fn rebuilds_stale_layout(self) -> bool {
        matches!(self, Self::Live)
    }
}

/// Immutable browser layout configuration, sealed before its first Page.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LayoutConfiguration {
    pub policy: LayoutPolicy,
    pub scrollbars_hidden: bool,
}

impl LayoutConfiguration {
    pub const fn scrollbars_hidden_for(self, emulated_hidden: bool) -> bool {
        self.scrollbars_hidden || emulated_hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_policy_defaults_to_mock() {
        assert_eq!(LayoutPolicy::default(), LayoutPolicy::Mock);
        assert!(!LayoutPolicy::default().uses_real_layout());
        assert!(LayoutPolicy::OnDemand.uses_real_layout());
        assert!(!LayoutPolicy::Mock.uses_real_layout());
    }

    #[test]
    fn only_the_live_policy_rebuilds_a_stale_layout() {
        assert!(LayoutPolicy::Live.uses_real_layout());
        assert!(LayoutPolicy::Live.rebuilds_stale_layout());
        assert!(!LayoutPolicy::OnDemand.rebuilds_stale_layout());
        assert!(!LayoutPolicy::Mock.rebuilds_stale_layout());
    }
}
