use smu_process::linux::FreezeHelperAuthorization;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Duration;

const DEFAULT_AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct AuthorizationState {
    pending: bool,
    resolved: bool,
    approved: bool,
}

#[derive(Debug)]
pub struct FreezeHelperAuthorizationGate {
    state: Mutex<AuthorizationState>,
    changed: Condvar,
    timeout: Duration,
}

impl Default for FreezeHelperAuthorizationGate {
    fn default() -> Self {
        Self::new(DEFAULT_AUTHORIZATION_TIMEOUT)
    }
}

impl FreezeHelperAuthorizationGate {
    #[must_use]
    pub const fn new(timeout: Duration) -> Self {
        Self {
            state: Mutex::new(AuthorizationState {
                pending: false,
                resolved: false,
                approved: false,
            }),
            changed: Condvar::new(),
            timeout,
        }
    }

    fn state(&self) -> MutexGuard<'_, AuthorizationState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    #[must_use]
    pub fn is_pending(&self) -> bool {
        let state = self.state();
        state.pending && !state.resolved
    }

    pub fn resolve(&self, approved: bool) {
        {
            let mut state = self.state();
            state.approved = approved;
            state.resolved = true;
            state.pending = false;
        }
        self.changed.notify_all();
    }
}

impl FreezeHelperAuthorization for FreezeHelperAuthorizationGate {
    #[allow(clippy::significant_drop_tightening)] // The mutex guard is the Condvar wait token.
    fn wait_until_authorized(&self) -> bool {
        let mut state = self.state();
        if state.resolved {
            let approved = state.approved;
            drop(state);
            return approved;
        }

        state.pending = true;
        let waited = self.changed.wait_timeout_while(state, self.timeout, |state| !state.resolved);
        let (mut state, timeout) = match waited {
            Ok(value) => value,
            Err(poisoned) => poisoned.into_inner(),
        };
        if timeout.timed_out() && !state.resolved {
            state.pending = false;
            drop(state);
            return false;
        }
        let approved = state.approved;
        drop(state);
        approved
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use std::sync::Arc;
    use std::time::Instant;

    #[test]
    fn resolved_decision_is_sticky_for_the_session() {
        let gate = FreezeHelperAuthorizationGate::default();
        gate.resolve(true);
        assert!(gate.wait_until_authorized());
        assert!(!gate.is_pending());
        gate.resolve(false);
        assert!(!gate.wait_until_authorized());
    }

    #[test]
    fn pending_request_is_visible_to_ui_and_resolve_wakes_waiter() {
        let gate = Arc::new(FreezeHelperAuthorizationGate::default());
        let waiter = Arc::clone(&gate);
        let thread = std::thread::spawn(move || waiter.wait_until_authorized());
        let deadline = Instant::now() + Duration::from_secs(1);
        while !gate.is_pending() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(gate.is_pending());
        gate.resolve(true);
        assert!(thread.join().expect("authorization waiter should finish"));
        assert!(!gate.is_pending());
    }

    #[test]
    fn timeout_clears_pending_without_resolving_future_requests() {
        let gate = FreezeHelperAuthorizationGate::new(Duration::from_millis(1));
        assert!(!gate.wait_until_authorized());
        assert!(!gate.is_pending());
        gate.resolve(false);
        assert!(!gate.wait_until_authorized());
    }
}
