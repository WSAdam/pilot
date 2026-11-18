//! pilot: one browser-started `beforeunload` step per page target.
//!
//! Chrome asks a page about leaving once at a time. A browser-started
//! navigation (Page.navigate, Page.reload, Page.navigateToHistoryEntry) that
//! arrives while another one's `beforeunload` runs, for example from a second
//! CDP session while the beforeunload dialog is open, does not ask the page
//! again: it waits for that answer.

use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use tokio::sync::oneshot;

type BeforeUnloadWaiters = Mutex<Vec<oneshot::Sender<bool>>>;

/// The page target's handle on the step in progress. It does not keep the
/// step alive: once the navigation that asked the page settles it, or is
/// dropped, no step is in progress.
#[derive(Clone, Debug, Default)]
pub(crate) struct TargetBeforeUnloadStep(Weak<BeforeUnloadWaiters>);

/// Held by the navigation whose `beforeunload` runs in the renderer. It
/// answers the navigations that arrived while it ran. Dropping it unsettled
/// drops their answers too, and they ask the page again.
#[derive(Default)]
pub(crate) struct BeforeUnloadStepLeader(Arc<BeforeUnloadWaiters>);

impl TargetBeforeUnloadStep {
    /// Records a new step in progress, answered by the returned leader.
    pub(crate) fn begin(&mut self) -> BeforeUnloadStepLeader {
        let waiters = Arc::new(BeforeUnloadWaiters::default());
        self.0 = Arc::downgrade(&waiters);
        BeforeUnloadStepLeader(waiters)
    }

    /// Waits on the step in progress, if there is one. The receiver yields
    /// whether this navigation may go on.
    pub(crate) fn join(&self) -> Option<oneshot::Receiver<bool>> {
        let waiters = self.0.upgrade()?;
        let (answer, answered) = oneshot::channel();
        waiters.lock().push(answer);
        Some(answered)
    }
}

impl BeforeUnloadStepLeader {
    /// Hands the page's answer to the navigations that waited on this step
    /// and returns whether the leading navigation itself may go on.
    ///
    /// When the user chose to stay, every one of them is aborted, as in
    /// Chrome. Otherwise the leading navigation goes on and the ones that
    /// arrived while it asked are aborted.
    pub(crate) fn settle(self, proceed: bool) -> bool {
        let waiters = std::mem::take(&mut *self.0.lock());
        for waiter in waiters {
            let _ = waiter.send(false);
        }
        proceed
    }
}

#[cfg(test)]
mod tests {
    use super::TargetBeforeUnloadStep;

    // 2026-11-18: a navigation arriving during another's beforeunload asked the renderer again behind it, so a stay answer never reached it.
    #[test]
    fn a_navigation_waiting_on_a_dismissed_beforeunload_is_aborted() {
        let mut step = TargetBeforeUnloadStep::default();
        let leader = step.begin();
        let mut waiting = step.join().expect("a step is in progress");

        assert!(!leader.settle(false), "the leading navigation stays");
        assert_eq!(waiting.try_recv(), Ok(false), "the waiting one is aborted");
        assert!(step.join().is_none(), "no step is in progress once settled");
    }

    // 2026-11-18: guard for the fix above: with no beforeunload in progress, or once its asker is dropped unanswered, a navigation must ask the page itself instead of waiting forever.
    #[test]
    fn no_step_is_in_progress_before_one_begins_or_after_its_leader_is_dropped() {
        let mut step = TargetBeforeUnloadStep::default();
        assert!(step.join().is_none());

        let leader = step.begin();
        let mut waiting = step.join().expect("a step is in progress");
        drop(leader);

        assert!(step.join().is_none());
        assert!(
            waiting.try_recv().is_err(),
            "a dropped leader leaves its waiters unanswered, so they ask again"
        );
    }
}
