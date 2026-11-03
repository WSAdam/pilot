//! Whether the agent is driving the browser right now, for the window's
//! indicator: it is while its command count keeps moving.

use std::time::{Duration, Instant};

/// How long the indicator stays on after the agent's last command.
pub const ACTIVE_FOR: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy, Debug, Default)]
pub struct AgentActivity {
    count: u64,
    last_change: Option<Instant>,
}

impl AgentActivity {
    /// Records the agent's command count at `now`; returns whether the agent
    /// counts as active.
    pub fn observe(&mut self, count: u64, now: Instant) -> bool {
        if count != self.count {
            self.count = count;
            self.last_change = Some(now);
        }
        self.last_change
            .is_some_and(|changed| now.saturating_duration_since(changed) < ACTIVE_FOR)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agent_is_active_while_its_commands_keep_coming() {
        let start = Instant::now();
        let mut activity = AgentActivity::default();
        assert!(!activity.observe(0, start), "no command yet");
        assert!(activity.observe(3, start));
        assert!(activity.observe(3, start + Duration::from_millis(1000)));
        assert!(!activity.observe(3, start + ACTIVE_FOR));
        assert!(activity.observe(4, start + Duration::from_secs(5)));
    }
}
