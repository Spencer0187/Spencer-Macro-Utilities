//! Windows Anti-AFK compatibility state and key-press timing.
//!
//! The Windows runtime owns the Win32 API calls. This module freezes the timer transition, bound-key
//! action, center-click, and background window-restoration sequence so those semantics can be tested
//! on every CI host against the C++ oracle.

use crate::WaitKind;
use crate::trace::{AntiAfkWindowAction, InputPath, KeyTransition, LaneId, TraceRecorder};
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

/// Emits the Anti-AFK bound-key sequence on the supplied execution lane.
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

/// Inputs to the nested center-click operation used by background Anti-AFK.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AntiAfkTapInput {
    pub rect_available: bool,
    pub center_x: i32,
    pub center_y: i32,
    pub has_original_mouse_pos: bool,
    pub original_mouse_x: i32,
    pub original_mouse_y: i32,
}

/// Mirrors `TapWindowCenter`, including its optional 100 ms inner cursor restore.
#[must_use]
pub fn execute_anti_afk_tap_window_center(
    trace: &mut TraceRecorder,
    lane: LaneId,
    input: AntiAfkTapInput,
) -> bool {
    if !input.rect_available {
        return false;
    }

    trace.cursor_position(lane, input.center_x, input.center_y);
    trace.mouse_button(lane, 1, KeyTransition::Down);
    trace.wait(lane, Duration::from_millis(50), WaitKind::Coarse);
    trace.mouse_button(lane, 1, KeyTransition::Up);
    if input.has_original_mouse_pos {
        trace.wait(lane, Duration::from_millis(100), WaitKind::Coarse);
        trace.cursor_position(lane, input.original_mouse_x, input.original_mouse_y);
    }
    true
}

/// Inputs to the outer background-window Anti-AFK sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct AntiAfkBackgroundInput {
    pub has_original_foreground: bool,
    pub has_outer_mouse_pos: bool,
    pub tap: AntiAfkTapInput,
    pub outer_mouse_x: i32,
    pub outer_mouse_y: i32,
    pub key: u32,
    pub double_press: bool,
}

/// Emits the exact background activation/restoration sequence on the supplied execution lane.
pub fn execute_anti_afk_background_sequence(
    trace: &mut TraceRecorder,
    lane: LaneId,
    input: AntiAfkBackgroundInput,
) {
    trace.anti_afk_window(lane, AntiAfkWindowAction::ShowTargetRestore);
    trace.anti_afk_window(lane, AntiAfkWindowAction::TargetTopmost { active: true });
    trace.anti_afk_window(lane, AntiAfkWindowAction::TargetTopmost { active: false });
    let tap_success = execute_anti_afk_tap_window_center(trace, lane, input.tap);
    trace.anti_afk_window(lane, AntiAfkWindowAction::TapResult { success: tap_success });
    trace.wait(lane, Duration::from_millis(500), WaitKind::Coarse);
    execute_anti_afk_keypress(trace, lane, input.key, input.double_press);
    trace.wait(lane, Duration::from_millis(200), WaitKind::Coarse);

    if input.has_original_foreground {
        trace.anti_afk_window(lane, AntiAfkWindowAction::OriginalTopmost { active: true });
        trace.anti_afk_window(lane, AntiAfkWindowAction::OriginalTopmost { active: false });
        trace.wait(lane, Duration::from_millis(50), WaitKind::Coarse);
        trace.anti_afk_window(lane, AntiAfkWindowAction::SetOriginalForeground);
        trace.wait(lane, Duration::from_millis(1000), WaitKind::Coarse);
    }
    if input.has_outer_mouse_pos {
        trace.cursor_position(lane, input.outer_mouse_x, input.outer_mouse_y);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{
        AntiAfkBackgroundInput, AntiAfkTapInput, evaluate_anti_afk_timer_step,
        execute_anti_afk_background_sequence, execute_anti_afk_keypress,
        execute_anti_afk_tap_window_center,
    };
    use crate::WaitKind;
    use crate::trace::{
        AntiAfkWindowAction, InputPath, KeyTransition, LaneId, TraceAction, TraceRecorder,
    };
    use serde::Deserialize;
    use serde_json::{Value, json};
    use std::time::Duration;

    const RUNTIME_FIXTURE: &str = include_str!("../../../../compat/timing/runtime_cases.json");

    #[derive(Debug, Deserialize)]
    #[allow(clippy::struct_field_names)]
    struct Fixture {
        anti_afk_timer_cases: Vec<AntiAfkTimerCase>,
        anti_afk_keypress_cases: Vec<AntiAfkKeypressCase>,
        anti_afk_tap_cases: Vec<AntiAfkTapCase>,
        anti_afk_background_cases: Vec<AntiAfkBackgroundCase>,
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

    #[derive(Debug, Deserialize)]
    struct AntiAfkTapCase {
        name: String,
        rect_available: bool,
        has_mouse_pos: bool,
        success: bool,
        events: Vec<Value>,
    }

    #[derive(Debug, Deserialize)]
    #[allow(clippy::struct_excessive_bools)]
    struct AntiAfkBackgroundCase {
        name: String,
        original_foreground: bool,
        outer_mouse_pos: bool,
        tap_rect: bool,
        tap_mouse_pos: bool,
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
                TraceAction::CursorPosition { x, y } => Some(json!({
                    "action": "cursor_move",
                    "x": x,
                    "y": y,
                })),
                TraceAction::MouseButton { button, transition } => {
                    let action = match transition {
                        KeyTransition::Down => "mouse_down",
                        KeyTransition::Up => "mouse_up",
                    };
                    Some(json!({"action": action, "button": button}))
                }
                TraceAction::AntiAfkWindow(action) => Some(match action {
                    AntiAfkWindowAction::ShowTargetRestore => json!({
                        "action": "show_target_restore",
                    }),
                    AntiAfkWindowAction::TargetTopmost { active } => json!({
                        "action": "target_topmost",
                        "active": active,
                    }),
                    AntiAfkWindowAction::TapResult { success } => json!({
                        "action": "tap_result",
                        "success": success,
                    }),
                    AntiAfkWindowAction::OriginalTopmost { active } => json!({
                        "action": "original_topmost",
                        "active": active,
                    }),
                    AntiAfkWindowAction::SetOriginalForeground => json!({
                        "action": "set_original_foreground",
                    }),
                }),
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
    fn cpp_anti_afk_keypress_timing_matches_on_dedicated_worker() {
        for case in fixture().anti_afk_keypress_cases {
            let mut trace = TraceRecorder::default();
            let worker = trace
                .spawn_dedicated_worker(LaneId::CONTROLLER)
                .expect("controller lane should exist");
            execute_anti_afk_keypress(&mut trace, worker, case.key, case.double_press);
            assert_eq!(events_json(&trace), case.events, "case {} events", case.name);
            let expected_ms = if case.double_press { 60 } else { 20 };
            assert_eq!(
                trace.lane_now(LaneId::CONTROLLER).map(crate::MonotonicMicros::as_micros),
                Some(0),
                "case {} controller must not stall",
                case.name
            );
            assert_eq!(
                trace.lane_now(worker).map(crate::MonotonicMicros::as_micros),
                Some(expected_ms * 1_000),
                "case {} worker duration",
                case.name
            );
        }
    }

    #[test]
    fn cpp_anti_afk_tap_window_center_matches_on_dedicated_worker() {
        for case in fixture().anti_afk_tap_cases {
            let mut trace = TraceRecorder::default();
            let worker = trace
                .spawn_dedicated_worker(LaneId::CONTROLLER)
                .expect("controller lane should exist");
            let success = execute_anti_afk_tap_window_center(
                &mut trace,
                worker,
                AntiAfkTapInput {
                    rect_available: case.rect_available,
                    center_x: 500,
                    center_y: 400,
                    has_original_mouse_pos: case.has_mouse_pos,
                    original_mouse_x: 10,
                    original_mouse_y: 20,
                },
            );
            assert_eq!(success, case.success, "case {} success", case.name);
            assert_eq!(events_json(&trace), case.events, "case {} events", case.name);
            let expected_ms =
                if case.rect_available { if case.has_mouse_pos { 150 } else { 50 } } else { 0 };
            assert_eq!(
                trace.lane_now(LaneId::CONTROLLER).map(crate::MonotonicMicros::as_micros),
                Some(0),
                "case {} controller must not stall",
                case.name
            );
            assert_eq!(
                trace.lane_now(worker).map(crate::MonotonicMicros::as_micros),
                Some(expected_ms * 1_000),
                "case {} worker duration",
                case.name
            );
        }
    }

    #[test]
    fn cpp_anti_afk_background_sequence_matches_on_dedicated_worker() {
        for case in fixture().anti_afk_background_cases {
            let mut trace = TraceRecorder::default();
            let worker = trace
                .spawn_dedicated_worker(LaneId::CONTROLLER)
                .expect("controller lane should exist");
            execute_anti_afk_background_sequence(
                &mut trace,
                worker,
                AntiAfkBackgroundInput {
                    has_original_foreground: case.original_foreground,
                    has_outer_mouse_pos: case.outer_mouse_pos,
                    tap: AntiAfkTapInput {
                        rect_available: case.tap_rect,
                        center_x: 500,
                        center_y: 400,
                        has_original_mouse_pos: case.tap_mouse_pos,
                        original_mouse_x: 10,
                        original_mouse_y: 20,
                    },
                    outer_mouse_x: 30,
                    outer_mouse_y: 40,
                    key: 112,
                    double_press: case.double_press,
                },
            );
            assert_eq!(events_json(&trace), case.events, "case {} events", case.name);

            let tap_ms = if case.tap_rect { if case.tap_mouse_pos { 150 } else { 50 } } else { 0 };
            let key_ms = if case.double_press { 60 } else { 20 };
            let restore_ms = if case.original_foreground { 1_050 } else { 0 };
            let expected_ms = tap_ms + 500 + key_ms + 200 + restore_ms;
            assert_eq!(
                trace.lane_now(LaneId::CONTROLLER).map(crate::MonotonicMicros::as_micros),
                Some(0),
                "case {} controller must not stall",
                case.name
            );
            assert_eq!(
                trace.lane_now(worker).map(crate::MonotonicMicros::as_micros),
                Some(expected_ms * 1_000),
                "case {} worker duration",
                case.name
            );
        }
    }
}
