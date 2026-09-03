//! Freeze process-control compatibility semantics.
//!
//! This layer intentionally preserves the current C++ partial-success and refresh/retry rules.

use crate::trace::{LaneId, TraceRecorder};

/// Runtime state owned by Freeze's process-control path.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct TargetSuspendState {
    pub target_pids: Vec<u32>,
    pub frozen_pids: Vec<u32>,
    pub freeze_suspended: bool,
}

/// Observable result of one suspend/resume request.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TargetSuspendOutcome {
    pub any_success: bool,
    pub warning: bool,
    pub reset_freeze_start: bool,
}

/// Executes one request with the exact current C++ refresh and partial-success semantics.
#[must_use]
pub fn execute_target_suspend_request<RefreshFn, SetPidSuspendedFn>(
    trace: &mut TraceRecorder,
    lane: LaneId,
    state: &mut TargetSuspendState,
    suspended: bool,
    mut refresh: RefreshFn,
    mut set_pid_suspended: SetPidSuspendedFn,
) -> TargetSuspendOutcome
where
    RefreshFn: FnMut() -> Vec<u32>,
    SetPidSuspendedFn: FnMut(u32, bool) -> bool,
{
    if suspended {
        if state.target_pids.is_empty() {
            state.target_pids = refresh();
            trace.process_target_refresh(lane, state.target_pids.clone());
        }
        if state.target_pids.is_empty() {
            return TargetSuspendOutcome::default();
        }
        state.frozen_pids.clone_from(&state.target_pids);
    }

    let mut any_success = false;
    for &pid in &state.frozen_pids {
        let success = set_pid_suspended(pid, suspended);
        trace.process_suspend_attempt(lane, pid, suspended, success);
        any_success = success || any_success;
    }

    if !any_success && suspended {
        let previous_pids = state.frozen_pids.clone();
        state.target_pids = refresh();
        trace.process_target_refresh(lane, state.target_pids.clone());
        if !state.target_pids.is_empty() && state.target_pids != previous_pids {
            state.frozen_pids.clone_from(&state.target_pids);
            for &pid in &state.frozen_pids {
                let success = set_pid_suspended(pid, true);
                trace.process_suspend_attempt(lane, pid, true, success);
                any_success = success || any_success;
            }
        }
    }

    let mut outcome = TargetSuspendOutcome { any_success, ..TargetSuspendOutcome::default() };
    if any_success {
        state.freeze_suspended = suspended;
        outcome.reset_freeze_start = suspended;
        if !suspended {
            state.frozen_pids.clear();
        }
    } else if !state.frozen_pids.is_empty() {
        outcome.warning = true;
    }
    outcome
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{TargetSuspendState, execute_target_suspend_request};
    use crate::trace::{LaneId, TraceAction, TraceRecorder};
    use serde::Deserialize;
    use serde_json::{Value, json};
    use std::collections::VecDeque;

    const RUNTIME_FIXTURE: &str = include_str!("../../../../compat/timing/runtime_cases.json");

    #[derive(Debug, Deserialize)]
    struct Fixture {
        target_suspend_cases: Vec<TargetSuspendCase>,
    }

    #[derive(Debug, Deserialize)]
    #[allow(clippy::struct_excessive_bools)]
    struct TargetSuspendCase {
        name: String,
        spec: TargetSuspendSpec,
        any_success: bool,
        warning: bool,
        reset_freeze_start: bool,
        target_pids: Vec<u32>,
        frozen_pids: Vec<u32>,
        freeze_suspended: bool,
        events: Vec<Value>,
    }

    #[derive(Debug, Deserialize)]
    struct TargetSuspendSpec {
        target_pids: Vec<u32>,
        frozen_pids: Vec<u32>,
        freeze_suspended: bool,
        request_suspended: bool,
        refresh_results: Vec<Vec<u32>>,
        attempt_results: Vec<bool>,
    }

    fn fixture() -> Fixture {
        serde_json::from_str(RUNTIME_FIXTURE)
            .expect("C++ generated runtime compatibility fixture should parse")
    }

    fn events_json(trace: &TraceRecorder) -> Vec<Value> {
        trace
            .events()
            .iter()
            .filter_map(|event| match &event.action {
                TraceAction::ProcessTargetRefresh { pids } => {
                    Some(json!({"action": "refresh_targets", "pids": pids}))
                }
                TraceAction::ProcessSuspendAttempt { pid, suspended, success } => Some(json!({
                    "action": "set_pid_suspended",
                    "pid": pid,
                    "suspended": suspended,
                    "success": success,
                })),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn cpp_target_suspend_process_control_cases_match() {
        for case in fixture().target_suspend_cases {
            let mut trace = TraceRecorder::default();
            let mut state = TargetSuspendState {
                target_pids: case.spec.target_pids,
                frozen_pids: case.spec.frozen_pids,
                freeze_suspended: case.spec.freeze_suspended,
            };
            let mut refresh_results: VecDeque<Vec<u32>> = case.spec.refresh_results.into();
            let mut attempt_results: VecDeque<bool> = case.spec.attempt_results.into();

            let outcome = execute_target_suspend_request(
                &mut trace,
                LaneId::CONTROLLER,
                &mut state,
                case.spec.request_suspended,
                || refresh_results.pop_front().unwrap_or_default(),
                |_pid, _suspended| attempt_results.pop_front().unwrap_or(false),
            );

            assert_eq!(outcome.any_success, case.any_success, "case {} success", case.name);
            assert_eq!(outcome.warning, case.warning, "case {} warning", case.name);
            assert_eq!(
                outcome.reset_freeze_start, case.reset_freeze_start,
                "case {} timer reset",
                case.name
            );
            assert_eq!(state.target_pids, case.target_pids, "case {} targets", case.name);
            assert_eq!(state.frozen_pids, case.frozen_pids, "case {} frozen", case.name);
            assert_eq!(
                state.freeze_suspended, case.freeze_suspended,
                "case {} suspended state",
                case.name
            );
            assert_eq!(events_json(&trace), case.events, "case {} events", case.name);
        }
    }
}
