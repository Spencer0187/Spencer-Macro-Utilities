//! Windows Anti-AFK compatibility state and key-press timing.
//!
//! The Windows runtime owns the surrounding process/window activation flow. This module freezes the
//! platform-independent timer transition and bound-key sequence so those semantics can be tested on
//! every CI host against the C++ oracle.

use crate::WaitKind;
use crate::trace::{InputPath, KeyTransition, LaneId, TraceRecorder};
use std::time::Duration;

/// Result of one Anti-AFK timer evaluation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AntiAfkTimerStepResult {
    pub armed: bool,
    pub last_keyboard_serial_seen: u64,
    pub reset_last_activity_to_now: bool,
    pub due: bool,
    pub threshold_minutes: i32,
}

/// Mirrors the timer transition performed by the current Windows C++ runtime.
#[must_use]
pub fn evaluate_anti_afk_timer_step(
    armed: bool,
    last_keyboard_serial_seen: u64,
    keyboard_serial: u64,
    roblox_foreground: bool,
    configured_minutes: i32,
    elapsed_since_last_activity: Duration,
) -> AntiAfkTimerStepResult {
    let threshold_minutes = configured_minutes.max(1);
    if !armed {
        return AntiAfkTimerStepResult {
            armed: true,
            last_keyboard_serial_seen: keyboard_serial,
            reset_last_activity_to_now: true,
            due: false,
            threshold_minutes,
        };
    }

    if roblox_foreground && keyboard_serial != last_keyboard_serial_seen {
        return AntiAfkTimerStepResult {
            armed: true,
            last_keyboard_serial_seen: keyboard_serial,
            reset_last_activity_to_now: true,
            due: false,
            threshold_minutes,
        };
    }

    let elapsed_minutes = elapsed_since_last_activity.as_secs() / 60;
    AntiAfkTimerStepResult {
        armed: true,
        last_keyboard_serial_seen,
        reset_last_activity_to_now: false,
        due: elapsed_minutes >= u64::try_from(threshold_minutes).unwrap_or(u64::MAX),
        threshold_minutes,
    }
}

/// Emits the current controller-lane Anti-AFK bound-key sequence.
pub fn execute_anti_afk_keypress(
    trace: &mut TraceRecorder,
    lane: LaneId,
    key: u32,
    double_press: bool,
) {
    trace.setting_bool_read(lane, "double_press", double_press);
    trace.key(lane, key, KeyTransition::Down, InputPath::Bound);
    trace.wait(lane, Duration::from_millis(20), WaitKind::Coarse);
    trace.key(lane, key, KeyTransition::Up, InputPath::Bound);

    if double_press {
        trace.wait(lane, Duration::from_millis(20), WaitKind::Coarse);
        trace.key(lane, key, KeyTransition::Down, InputPath::Bound);
        trace.wait(lane, Duration::from_millis(20), WaitKind::Coarse);
        trace.key(lane, key, KeyTransition::Up, InputPath::Bound);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{evaluate_anti_afk_timer_step, execute_anti_afk_keypress};
    use crate::WaitKind;
    use crate::trace::{InputPath, KeyTransition, LaneId, TraceAction, TraceRecorder};
    use serde::Deserialize;
    use serde_json::{Value, json};
    use std::time::Duration;

    const RUNTIME_FIXTURE: &str = include_str!("../../../../compat/timing/runtime_cases.json");

    #[derive(Debug, Deserialize)]
    struct Fixture {
        anti_afk_timer_cases: Vec<AntiAfkTimerCase>,
        anti_afk_keypress_cases: Vec<AntiAfkKeypressCase>,
    }

    #[derive(Debug, Deserialize)]
    #[allow(clippy::struct_excessive_bools)]
    struct AntiAfkTimerCase {
        name: String,
        armed: bool,
        last_serial: u64,
        keyboard_serial: u64,
        foreground: bool,
        configured_minutes: i32,
        elapsed_ms: u64,
        next_armed: bool,
        next_last_serial: u64,
        reset_activity: bool,
        due: bool,
        threshold_minutes: i32,
    }

    #[derive(Debug, Deserialize)]
    struct AntiAfkKeypressCase {
        name: String,
        key: u32,
        double_press: bool,
        events: Vec<Value>,
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
                TraceAction::SettingBoolRead { name, value } => Some(json!({
                    "action": "setting_read",
                    "name": name,
                    "value": value,
                })),
                TraceAction::Key { key, transition, path } => {
                    let action = match transition {
                        KeyTransition::Down => "key_down",
                        KeyTransition::Up => "key_up",
                    };
                    let path = match path {
                        InputPath::Direct => "direct",
                        InputPath::Bound => "bound",
                    };
                    Some(json!({"action": action, "key": key, "path": path}))
                }
                TraceAction::Wait { duration, kind } => {
                    let kind = match kind {
                        WaitKind::Coarse => "coarse",
                        WaitKind::Precise => "precise",
                        WaitKind::Poll => "poll",
                    };
                    Some(json!({
                        "action": "wait",
                        "duration_ms": duration.as_millis(),
                        "kind": kind,
                    }))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn cpp_anti_afk_timer_matrix_matches() {
        for case in fixture().anti_afk_timer_cases {
            let observed = evaluate_anti_afk_timer_step(
                case.armed,
                case.last_serial,
                case.keyboard_serial,
                case.foreground,
                case.configured_minutes,
                Duration::from_millis(case.elapsed_ms),
            );
            assert_eq!(observed.armed, case.next_armed, "case {} armed", case.name);
            assert_eq!(
                observed.last_keyboard_serial_seen, case.next_last_serial,
                "case {} serial",
                case.name
            );
            assert_eq!(
                observed.reset_last_activity_to_now, case.reset_activity,
                "case {} activity reset",
                case.name
            );
            assert_eq!(observed.due, case.due, "case {} due", case.name);
            assert_eq!(
                observed.threshold_minutes, case.threshold_minutes,
                "case {} threshold",
                case.name
            );
        }
    }

    #[test]
    fn cpp_anti_afk_keypress_timing_matches() {
        for case in fixture().anti_afk_keypress_cases {
            let mut trace = TraceRecorder::default();
            execute_anti_afk_keypress(&mut trace, LaneId::CONTROLLER, case.key, case.double_press);
            assert_eq!(events_json(&trace), case.events, "case {} events", case.name);
            let expected_ms = if case.double_press { 60 } else { 20 };
            assert_eq!(
                trace.lane_now(LaneId::CONTROLLER).map(crate::MonotonicMicros::as_micros),
                Some(expected_ms * 1_000),
                "case {} controller duration",
                case.name
            );
        }
    }
}
