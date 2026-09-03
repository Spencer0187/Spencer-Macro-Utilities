//! Compatibility-frozen native macro slices.
//!
//! These helpers intentionally preserve the current C++ controller/worker topology. They are small
//! enough to test differentially before the full runtime controller is ported.

use crate::WaitKind;
use crate::trace::{InputPath, KeyTransition, LaneId, TraceRecorder};
use smu_core::KeyCode;
use std::time::Duration;

const FRAME_DELAY_EPSILON: f32 = 0.008;

fn milliseconds_from_nonnegative(value: i32) -> Duration {
    Duration::from_millis(u64::try_from(value).unwrap_or_default())
}

fn microseconds_from_nonnegative(value: i32) -> Duration {
    Duration::from_micros(u64::try_from(value).unwrap_or_default())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RisingEdgeResult {
    pub rising: bool,
    pub next_was_pressed: bool,
}

#[must_use]
pub const fn evaluate_rising_edge(pressed: bool, was_pressed: bool) -> RisingEdgeResult {
    RisingEdgeResult { rising: pressed && !was_pressed, next_was_pressed: pressed }
}

#[must_use]
pub const fn should_toggle_freeze(configured_toggle: bool, freeze_key: u32) -> bool {
    configured_toggle
        || freeze_key == KeyCode::MOUSE_WHEEL_UP.raw()
        || freeze_key == KeyCode::MOUSE_WHEEL_DOWN.raw()
}

#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn freeze_timeout_threshold_ms(max_freeze_time_seconds: f32) -> i64 {
    // Compatibility cast: production multiplies in float32, then casts to `long long`.
    (max_freeze_time_seconds * 1000.0_f32) as i64
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct FreezeStepInput {
    pub section_enabled: bool,
    pub foreground_allowed: bool,
    pub pressed: bool,
    pub was_pressed: bool,
    pub configured_toggle: bool,
    pub freeze_key: u32,
    pub suspended: bool,
    pub elapsed_ms: i64,
    pub max_seconds: f32,
    pub override_ms: i32,
    pub running_after_override: bool,
    pub pressed_after_override: bool,
    pub suspend_success: bool,
    pub resume_success: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FreezeStepResult {
    pub toggle_freeze: bool,
    pub next_was_pressed: bool,
    pub final_suspended: bool,
    pub final_elapsed_ms: i64,
}

fn attempt_target_suspend(
    trace: &mut TraceRecorder,
    lane: LaneId,
    suspended: &mut bool,
    elapsed_ms: &mut i64,
    requested: bool,
    success: bool,
) {
    trace.target_suspend_attempt(lane, requested, success);
    if success {
        *suspended = requested;
        if requested {
            *elapsed_ms = 0;
        }
    }
}

#[must_use]
pub fn execute_freeze_step(
    trace: &mut TraceRecorder,
    lane: LaneId,
    input: FreezeStepInput,
) -> FreezeStepResult {
    let toggle_freeze = should_toggle_freeze(input.configured_toggle, input.freeze_key);
    let mut suspended = input.suspended;
    let mut elapsed_ms = input.elapsed_ms;

    if !input.section_enabled {
        if suspended {
            attempt_target_suspend(
                trace,
                lane,
                &mut suspended,
                &mut elapsed_ms,
                false,
                input.resume_success,
            );
        }
        return FreezeStepResult {
            toggle_freeze,
            next_was_pressed: false,
            final_suspended: suspended,
            final_elapsed_ms: elapsed_ms,
        };
    }

    if toggle_freeze {
        if input.pressed && !input.was_pressed && input.foreground_allowed {
            let requested = !suspended;
            let success = if requested { input.suspend_success } else { input.resume_success };
            attempt_target_suspend(
                trace,
                lane,
                &mut suspended,
                &mut elapsed_ms,
                requested,
                success,
            );
        }
    } else if input.pressed && input.foreground_allowed {
        if !suspended {
            attempt_target_suspend(
                trace,
                lane,
                &mut suspended,
                &mut elapsed_ms,
                true,
                input.suspend_success,
            );
        }
    } else if suspended {
        attempt_target_suspend(
            trace,
            lane,
            &mut suspended,
            &mut elapsed_ms,
            false,
            input.resume_success,
        );
    }

    if suspended && elapsed_ms >= freeze_timeout_threshold_ms(input.max_seconds) {
        attempt_target_suspend(
            trace,
            lane,
            &mut suspended,
            &mut elapsed_ms,
            false,
            input.resume_success,
        );
        let delay_ms = input.override_ms.max(0);
        trace.wait(lane, milliseconds_from_nonnegative(delay_ms), WaitKind::Coarse);
        elapsed_ms = elapsed_ms.saturating_add(i64::from(delay_ms));
        trace.running_check(lane, input.running_after_override);
        if input.running_after_override {
            let should_resuspend = if toggle_freeze {
                true
            } else {
                trace.hotkey_recheck(lane, input.pressed_after_override);
                input.pressed_after_override
            };
            if should_resuspend {
                attempt_target_suspend(
                    trace,
                    lane,
                    &mut suspended,
                    &mut elapsed_ms,
                    true,
                    input.suspend_success,
                );
            }
        }
    }

    FreezeStepResult {
        toggle_freeze,
        next_was_pressed: input.pressed,
        final_suspended: suspended,
        final_elapsed_ms: elapsed_ms,
    }
}

#[must_use]
pub fn inventory_slot_key(slot: i32) -> u32 {
    u32::try_from(slot.clamp(0, 9)).unwrap_or_default() + KeyCode::DIGIT_0.raw()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemDesyncResult {
    pub pressed: bool,
    pub loop_state: bool,
    pub slot_key: Option<u32>,
}

#[must_use]
pub fn execute_item_desync(
    trace: &mut TraceRecorder,
    lane: LaneId,
    foreground_allowed: bool,
    section_enabled: bool,
    hotkey_pressed: bool,
    slot: i32,
) -> ItemDesyncResult {
    let pressed = foreground_allowed && section_enabled && hotkey_pressed;
    if !pressed {
        return ItemDesyncResult { pressed: false, loop_state: false, slot_key: None };
    }

    let key = inventory_slot_key(slot);
    trace.key(lane, key, KeyTransition::Down, InputPath::Direct);
    trace.key(lane, key, KeyTransition::Up, InputPath::Direct);
    trace.key(lane, key, KeyTransition::Down, InputPath::Direct);
    trace.key(lane, key, KeyTransition::Up, InputPath::Direct);
    ItemDesyncResult { pressed: true, loop_state: true, slot_key: Some(key) }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToggleLoopStateAction {
    None,
    Toggle,
    Clear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToggleLoopEdgeResult {
    pub pressed: bool,
    pub next_was_pressed: bool,
    pub state_action: ToggleLoopStateAction,
}

#[must_use]
#[allow(clippy::fn_params_excessive_bools)]
pub const fn evaluate_toggle_loop_edge(
    foreground_allowed: bool,
    section_enabled: bool,
    hotkey_pressed: bool,
    toggle_switch: bool,
    was_pressed: bool,
) -> ToggleLoopEdgeResult {
    let pressed = foreground_allowed && section_enabled && hotkey_pressed;
    let state_action = if pressed && !was_pressed {
        ToggleLoopStateAction::Toggle
    } else if !pressed && toggle_switch {
        ToggleLoopStateAction::Clear
    } else {
        ToggleLoopStateAction::None
    };
    ToggleLoopEdgeResult { pressed, next_was_pressed: pressed, state_action }
}

#[must_use]
pub const fn apply_toggle_loop_state_action(action: ToggleLoopStateAction, active: bool) -> bool {
    match action {
        ToggleLoopStateAction::Toggle => !active,
        ToggleLoopStateAction::Clear => false,
        ToggleLoopStateAction::None => active,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ItemClipCyclePlan {
    pub half_delay_ms: i32,
    pub slot_key: u32,
}

#[must_use]
pub fn itemclip_half_delay_ms(clip_delay: i32) -> i32 {
    (clip_delay / 2).max(1)
}

#[must_use]
pub fn build_itemclip_cycle_plan(clip_delay: i32, clip_slot: i32) -> ItemClipCyclePlan {
    ItemClipCyclePlan {
        half_delay_ms: itemclip_half_delay_ms(clip_delay),
        slot_key: inventory_slot_key(clip_slot),
    }
}

pub fn execute_itemclip_cycle(trace: &mut TraceRecorder, lane: LaneId, plan: ItemClipCyclePlan) {
    trace.key(lane, plan.slot_key, KeyTransition::Down, InputPath::Direct);
    trace.wait(lane, milliseconds_from_nonnegative(plan.half_delay_ms), WaitKind::Coarse);
    trace.key(lane, plan.slot_key, KeyTransition::Up, InputPath::Direct);
    trace.wait(lane, milliseconds_from_nonnegative(plan.half_delay_ms), WaitKind::Coarse);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ItemClipStepInput {
    pub foreground_allowed: bool,
    pub section_press: bool,
    pub section_execute: bool,
    pub hotkey_pressed: bool,
    pub toggle_switch: bool,
    pub was_pressed: bool,
    pub is_loop: bool,
    pub clip_delay: i32,
    pub clip_slot: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct ItemClipStepResult {
    pub pressed: bool,
    pub next_was_pressed: bool,
    pub state_action: ToggleLoopStateAction,
    pub next_is_loop: bool,
    pub execute_cycle: bool,
    pub plan: Option<ItemClipCyclePlan>,
}

#[must_use]
pub fn execute_itemclip_step(
    trace: &mut TraceRecorder,
    lane: LaneId,
    input: ItemClipStepInput,
) -> ItemClipStepResult {
    let edge = evaluate_toggle_loop_edge(
        input.foreground_allowed,
        input.section_press,
        input.hotkey_pressed,
        input.toggle_switch,
        input.was_pressed,
    );
    let next_is_loop = apply_toggle_loop_state_action(edge.state_action, input.is_loop);
    let execute_cycle = input.foreground_allowed && input.section_execute && next_is_loop;
    let plan = execute_cycle.then(|| build_itemclip_cycle_plan(input.clip_delay, input.clip_slot));
    if let Some(plan) = plan {
        execute_itemclip_cycle(trace, lane, plan);
    }
    ItemClipStepResult {
        pressed: edge.pressed,
        next_was_pressed: edge.next_was_pressed,
        state_action: edge.state_action,
        next_is_loop,
        execute_cycle,
        plan,
    }
}

#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn wallwalk_frame_delay_ms(fps: u32) -> i32 {
    let safe_fps = fps.max(1);
    ((((1000.0_f32 / safe_fps as f32) + 0.5_f32) * 1.1_f32) as i32).max(1)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WallWalkCyclePlan {
    pub first_move: i32,
    pub second_move: i32,
    pub frame_delay_ms: i32,
}

#[must_use]
pub fn build_wallwalk_cycle_plan(
    fps: u32,
    side_first: bool,
    strength_x: i32,
    side_second: bool,
    strength_y: i32,
) -> WallWalkCyclePlan {
    WallWalkCyclePlan {
        first_move: if side_first { strength_x.wrapping_neg() } else { strength_x },
        second_move: if side_second { strength_y.wrapping_neg() } else { strength_y },
        frame_delay_ms: wallwalk_frame_delay_ms(fps),
    }
}

pub fn execute_wallwalk_cycle<TailDelayFn>(
    trace: &mut TraceRecorder,
    lane: LaneId,
    plan: WallWalkCyclePlan,
    mut tail_delay_us: TailDelayFn,
) where
    TailDelayFn: FnMut() -> i32,
{
    trace.mouse_move(lane, plan.first_move, 0);
    trace.wait(lane, milliseconds_from_nonnegative(plan.frame_delay_ms), WaitKind::Coarse);
    trace.mouse_move(lane, plan.second_move, 0);
    let raw_tail_delay_us = tail_delay_us();
    trace.wallwalk_tail_delay_read(lane, raw_tail_delay_us);
    trace.wait(lane, microseconds_from_nonnegative(raw_tail_delay_us.max(0)), WaitKind::Coarse);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct WallWalkStepInput {
    pub foreground_allowed: bool,
    pub section_press: bool,
    pub section_execute: bool,
    pub hotkey_pressed: bool,
    pub toggle_switch: bool,
    pub was_pressed: bool,
    pub is_loop: bool,
    pub fps: u32,
    pub side_first: bool,
    pub strength_x: i32,
    pub side_second: bool,
    pub strength_y: i32,
    pub tail_delay_us: i32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct WallWalkStepResult {
    pub pressed: bool,
    pub next_was_pressed: bool,
    pub state_action: ToggleLoopStateAction,
    pub next_is_loop: bool,
    pub execute_cycle: bool,
    pub plan: Option<WallWalkCyclePlan>,
}

#[must_use]
pub fn execute_wallwalk_step(
    trace: &mut TraceRecorder,
    lane: LaneId,
    input: WallWalkStepInput,
) -> WallWalkStepResult {
    let edge = evaluate_toggle_loop_edge(
        input.foreground_allowed,
        input.section_press,
        input.hotkey_pressed,
        input.toggle_switch,
        input.was_pressed,
    );
    let next_is_loop = apply_toggle_loop_state_action(edge.state_action, input.is_loop);
    let execute_cycle = input.foreground_allowed && input.section_execute && next_is_loop;
    let plan = execute_cycle.then(|| {
        build_wallwalk_cycle_plan(
            input.fps,
            input.side_first,
            input.strength_x,
            input.side_second,
            input.strength_y,
        )
    });
    if let Some(plan) = plan {
        execute_wallwalk_cycle(trace, lane, plan, || input.tail_delay_us);
    }
    WallWalkStepResult {
        pressed: edge.pressed,
        next_was_pressed: edge.next_was_pressed,
        state_action: edge.state_action,
        next_is_loop,
        execute_cycle,
        plan,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct WallhopWorkerPlan {
    pub dx: i32,
    pub dy: i32,
    pub vertical: i32,
    pub delay_ms: i32,
    pub bonus_delay_ms: i32,
    pub left: bool,
    pub jump: bool,
    pub flick_back: bool,
    pub jump_key: u32,
}

#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn build_wallhop_worker_plan(
    dx: i32,
    dy: i32,
    vertical: i32,
    delay_ms: i32,
    bonus_delay_ms: i32,
    left: bool,
    jump: bool,
    flick_back: bool,
    jump_key: u32,
) -> WallhopWorkerPlan {
    WallhopWorkerPlan {
        dx,
        dy,
        vertical,
        delay_ms: delay_ms.max(1),
        bonus_delay_ms: bonus_delay_ms.max(0),
        left,
        jump,
        flick_back,
        jump_key,
    }
}

pub fn execute_wallhop_worker(trace: &mut TraceRecorder, lane: LaneId, plan: WallhopWorkerPlan) {
    let first_dx = if plan.left { plan.dx.wrapping_neg() } else { plan.dx };
    trace.mouse_move(lane, first_dx, plan.vertical);

    if plan.flick_back {
        if plan.bonus_delay_ms > 0 && plan.bonus_delay_ms < plan.delay_ms {
            trace.wait(lane, milliseconds_from_nonnegative(plan.bonus_delay_ms), WaitKind::Coarse);
            if plan.jump {
                trace.key(lane, plan.jump_key, KeyTransition::Down, InputPath::Bound);
            }
            trace.wait(
                lane,
                milliseconds_from_nonnegative(plan.delay_ms - plan.bonus_delay_ms),
                WaitKind::Coarse,
            );
        } else {
            if plan.jump {
                trace.key(lane, plan.jump_key, KeyTransition::Down, InputPath::Bound);
            }
            trace.wait(lane, milliseconds_from_nonnegative(plan.delay_ms), WaitKind::Coarse);
        }
        let second_dx = if plan.left { plan.dy.wrapping_neg() } else { plan.dy };
        trace.mouse_move(lane, second_dx, plan.vertical.wrapping_neg());
    } else if plan.jump {
        trace.key(lane, plan.jump_key, KeyTransition::Down, InputPath::Bound);
    }

    if plan.jump {
        let release_delay = 100 - plan.delay_ms;
        if release_delay > 0 {
            trace.wait(lane, milliseconds_from_nonnegative(release_delay), WaitKind::Coarse);
        }
        trace.key(lane, plan.jump_key, KeyTransition::Up, InputPath::Bound);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct WallessLhjWorkerInput {
    pub pids: Vec<u32>,
    pub side_left: bool,
    pub global_zoom_action: bool,
    pub shift_key_action: u32,
    pub zoom_reverse: bool,
    pub global_zoom_release: bool,
    pub shift_key_release: u32,
}

pub fn execute_walless_lhj_worker(
    trace: &mut TraceRecorder,
    lane: LaneId,
    input: &WallessLhjWorkerInput,
) {
    trace.setting_bool_read(lane, "side_left", input.side_left);
    let side_key = if input.side_left { KeyCode::A.raw() } else { KeyCode::new(0x44).raw() };
    trace.key(lane, side_key, KeyTransition::Down, InputPath::Direct);
    trace.wait(lane, Duration::from_millis(15), WaitKind::Coarse);
    trace.key(lane, KeyCode::SPACE.raw(), KeyTransition::Down, InputPath::Direct);
    trace.wait(lane, Duration::from_millis(30), WaitKind::Coarse);

    trace.process_suspended(lane, input.pids.clone(), true);
    trace.wait(lane, Duration::from_millis(50), WaitKind::Coarse);
    trace.key(lane, side_key, KeyTransition::Up, InputPath::Direct);
    trace.wait(lane, Duration::from_millis(500), WaitKind::Coarse);

    trace.setting_bool_read(lane, "global_zoom_action", input.global_zoom_action);
    if input.global_zoom_action {
        trace.setting_bool_read(lane, "zoom_reverse", input.zoom_reverse);
        let wheel = if input.zoom_reverse {
            KeyCode::MOUSE_WHEEL_DOWN.raw()
        } else {
            KeyCode::MOUSE_WHEEL_UP.raw()
        };
        trace.key(lane, wheel, KeyTransition::Down, InputPath::Bound);
    } else {
        trace.setting_u32_read(lane, "shift_key_action", input.shift_key_action);
        trace.key(lane, input.shift_key_action, KeyTransition::Down, InputPath::Bound);
    }

    trace.process_suspended(lane, input.pids.clone(), false);
    trace.wait(lane, Duration::from_millis(50), WaitKind::Coarse);
    trace.setting_bool_read(lane, "global_zoom_release", input.global_zoom_release);
    if !input.global_zoom_release {
        trace.setting_u32_read(lane, "shift_key_release", input.shift_key_release);
        trace.key(lane, input.shift_key_release, KeyTransition::Up, InputPath::Bound);
    }
    trace.key(lane, KeyCode::SPACE.raw(), KeyTransition::Up, InputPath::Direct);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PressKeyWorkerPlan {
    pub trigger: u32,
    pub output: u32,
    pub bonus_delay_ms: i32,
    pub press_delay_ms: i32,
    pub release_trigger_first: bool,
}

#[must_use]
pub fn build_presskey_worker_plan(
    trigger: u32,
    output: u32,
    bonus_delay_ms: i32,
    press_delay_ms: i32,
) -> PressKeyWorkerPlan {
    PressKeyWorkerPlan {
        trigger,
        output,
        bonus_delay_ms: bonus_delay_ms.max(0),
        press_delay_ms: press_delay_ms.max(1),
        release_trigger_first: trigger == output,
    }
}

pub fn execute_presskey_worker(
    trace: &mut TraceRecorder,
    lane: LaneId,
    plan: PressKeyWorkerPlan,
    running_after_bonus: bool,
) {
    if plan.release_trigger_first {
        trace.key(lane, plan.trigger, KeyTransition::Up, InputPath::Bound);
    }
    if plan.bonus_delay_ms > 0 {
        trace.wait(lane, milliseconds_from_nonnegative(plan.bonus_delay_ms), WaitKind::Coarse);
    }
    trace.running_check(lane, running_after_bonus);
    if !running_after_bonus {
        return;
    }
    trace.key(lane, plan.output, KeyTransition::Down, InputPath::Bound);
    trace.wait(lane, milliseconds_from_nonnegative(plan.press_delay_ms), WaitKind::Coarse);
    trace.key(lane, plan.output, KeyTransition::Up, InputPath::Bound);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FrameDelayPair {
    pub first_ms: i32,
    pub second_ms: i32,
}

#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
pub fn frame_delays_for_fps(fps: u32) -> FrameDelayPair {
    // These casts intentionally mirror the production C++ float32 arithmetic exactly.
    let safe_fps = fps.max(1);
    let delay_float = 1000.0_f32 / safe_fps as f32;
    let delay_floor = (delay_float as i32).max(1);
    let delay_ceil = delay_floor + 1;
    let fractional = delay_float - delay_floor as f32;

    if fractional < 0.33 - FRAME_DELAY_EPSILON {
        FrameDelayPair { first_ms: delay_floor, second_ms: delay_floor }
    } else if fractional > 0.66 + FRAME_DELAY_EPSILON {
        FrameDelayPair { first_ms: delay_ceil, second_ms: delay_ceil }
    } else {
        FrameDelayPair { first_ms: delay_floor, second_ms: delay_ceil }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpeedglitchStateAction {
    None,
    Toggle,
    Clear,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpeedglitchEdgeResult {
    pub pressed: bool,
    pub next_was_pressed: bool,
    pub state_action: SpeedglitchStateAction,
}

#[must_use]
#[allow(clippy::fn_params_excessive_bools)]
pub const fn evaluate_speedglitch_edge(
    foreground_allowed: bool,
    section_enabled: bool,
    hotkey_pressed: bool,
    toggle_switch: bool,
    was_pressed: bool,
) -> SpeedglitchEdgeResult {
    let pressed = foreground_allowed && section_enabled && hotkey_pressed;
    let state_action = if pressed && !was_pressed {
        SpeedglitchStateAction::Toggle
    } else if !pressed && toggle_switch {
        SpeedglitchStateAction::Clear
    } else {
        SpeedglitchStateAction::None
    };
    SpeedglitchEdgeResult { pressed, next_was_pressed: pressed, state_action }
}

#[must_use]
pub const fn apply_speedglitch_state_action(
    action: SpeedglitchStateAction,
    is_speed: bool,
) -> bool {
    match action {
        SpeedglitchStateAction::Toggle => !is_speed,
        SpeedglitchStateAction::Clear => false,
        SpeedglitchStateAction::None => is_speed,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpeedglitchCyclePlan {
    pub strength_x: i32,
    pub strength_y: i32,
    pub delays: FrameDelayPair,
}

#[must_use]
pub fn build_speedglitch_cycle_plan(
    fps: u32,
    strength_x: i32,
    strength_y: i32,
) -> SpeedglitchCyclePlan {
    SpeedglitchCyclePlan { strength_x, strength_y, delays: frame_delays_for_fps(fps) }
}

pub fn execute_speedglitch_cycle<FirstStrengthFn, SecondStrengthFn>(
    trace: &mut TraceRecorder,
    lane: LaneId,
    delays: FrameDelayPair,
    mut first_strength: FirstStrengthFn,
    mut second_strength: SecondStrengthFn,
) where
    FirstStrengthFn: FnMut() -> i32,
    SecondStrengthFn: FnMut() -> i32,
{
    trace.mouse_move(lane, first_strength(), 0);
    trace.wait(lane, milliseconds_from_nonnegative(delays.first_ms), WaitKind::Coarse);
    trace.mouse_move(lane, second_strength(), 0);
    trace.wait(lane, milliseconds_from_nonnegative(delays.second_ms), WaitKind::Coarse);
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::struct_excessive_bools, clippy::struct_field_names)]

    use super::{
        FreezeStepInput, ItemClipStepInput, SpeedglitchStateAction, ToggleLoopStateAction,
        WallWalkStepInput, WallessLhjWorkerInput, apply_speedglitch_state_action,
        build_presskey_worker_plan, build_speedglitch_cycle_plan, build_wallhop_worker_plan,
        evaluate_rising_edge, evaluate_speedglitch_edge, execute_freeze_step, execute_item_desync,
        execute_itemclip_step, execute_presskey_worker, execute_speedglitch_cycle,
        execute_walless_lhj_worker, execute_wallhop_worker, execute_wallwalk_step,
        frame_delays_for_fps, freeze_timeout_threshold_ms, wallwalk_frame_delay_ms,
    };
    use crate::WaitKind;
    use crate::trace::{InputPath, KeyTransition, LaneId, TraceAction, TraceEvent, TraceRecorder};
    use serde::Deserialize;
    use serde_json::{Value, json};

    const FIXTURE: &str = include_str!("../../../../compat/timing/runtime_cases.json");

    #[derive(Debug, Deserialize)]
    struct Fixture {
        frame_delay_cases: Vec<FrameDelayCase>,
        rising_edge_cases: Vec<RisingEdgeCase>,
        freeze_threshold_cases: Vec<FreezeThresholdCase>,
        freeze_cases: Vec<FreezeCase>,
        item_desync_cases: Vec<ItemDesyncCase>,
        itemclip_cases: Vec<ItemClipCase>,
        wallwalk_delay_cases: Vec<WallWalkDelayCase>,
        wallwalk_cases: Vec<WallWalkCase>,
        wallhop_cases: Vec<WallhopCase>,
        walless_cases: Vec<WallessCase>,
        presskey_cases: Vec<PressKeyCase>,
        speedglitch_cases: Vec<SpeedglitchCase>,
    }

    #[derive(Debug, Deserialize)]
    struct FrameDelayCase {
        fps: u32,
        first_ms: i32,
        second_ms: i32,
    }

    #[derive(Debug, Deserialize)]
    struct RisingEdgeCase {
        pressed: bool,
        was_pressed: bool,
        rising: bool,
        next_was_pressed: bool,
    }

    #[derive(Debug, Deserialize)]
    struct FreezeThresholdCase {
        seconds: f32,
        threshold_ms: i64,
    }

    #[derive(Debug, Deserialize)]
    struct FreezeCase {
        name: String,
        section_enabled: bool,
        foreground_allowed: bool,
        pressed: bool,
        was_pressed: bool,
        configured_toggle: bool,
        freeze_key: u32,
        suspended: bool,
        elapsed_ms: i64,
        max_seconds: f32,
        override_ms: i32,
        running_after_override: bool,
        pressed_after_override: bool,
        suspend_success: bool,
        resume_success: bool,
        toggle_freeze: bool,
        next_was_pressed: bool,
        final_suspended: bool,
        events: Vec<Value>,
    }

    #[derive(Debug, Deserialize)]
    struct ItemDesyncCase {
        name: String,
        foreground_allowed: bool,
        section_enabled: bool,
        hotkey_pressed: bool,
        slot: i32,
        pressed: bool,
        loop_state: bool,
        slot_key: Option<u32>,
        events: Vec<Value>,
    }

    #[derive(Debug, Deserialize)]
    struct ItemClipCase {
        name: String,
        foreground_allowed: bool,
        section_press: bool,
        section_execute: bool,
        hotkey_pressed: bool,
        toggle_switch: bool,
        was_pressed: bool,
        is_loop: bool,
        clip_delay: i32,
        clip_slot: i32,
        pressed: bool,
        next_was_pressed: bool,
        state_action: String,
        next_is_loop: bool,
        execute_cycle: bool,
        plan: Value,
        events: Vec<Value>,
    }

    #[derive(Debug, Deserialize)]
    struct WallWalkDelayCase {
        fps: u32,
        delay_ms: i32,
    }

    #[derive(Debug, Deserialize)]
    struct WallWalkCase {
        name: String,
        foreground_allowed: bool,
        section_press: bool,
        section_execute: bool,
        hotkey_pressed: bool,
        toggle_switch: bool,
        was_pressed: bool,
        is_loop: bool,
        fps: u32,
        side_first: bool,
        strength_x: i32,
        side_second: bool,
        strength_y: i32,
        tail_delay_us: i32,
        pressed: bool,
        next_was_pressed: bool,
        state_action: String,
        next_is_loop: bool,
        execute_cycle: bool,
        plan: Value,
        events: Vec<Value>,
    }

    #[derive(Debug, Deserialize)]
    struct WallhopCase {
        name: String,
        dx: i32,
        dy: i32,
        vertical: i32,
        delay_ms: i32,
        bonus_delay_ms: i32,
        left: bool,
        jump: bool,
        flick_back: bool,
        jump_key: u32,
        plan: Value,
        events: Vec<Value>,
    }

    #[derive(Debug, Deserialize)]
    struct WallessCase {
        name: String,
        spec: WallessSpec,
        events: Vec<Value>,
    }

    #[derive(Debug, Deserialize)]
    #[allow(clippy::struct_excessive_bools)]
    struct WallessSpec {
        pids: Vec<u32>,
        side_left: bool,
        global_zoom_action: bool,
        shift_key_action: u32,
        zoom_reverse: bool,
        global_zoom_release: bool,
        shift_key_release: u32,
    }

    #[derive(Debug, Deserialize)]
    struct PressKeyCase {
        name: String,
        trigger: u32,
        output: u32,
        bonus_delay_ms: i32,
        press_delay_ms: i32,
        running_after_bonus: bool,
        plan: Value,
        events: Vec<Value>,
    }

    #[derive(Debug, Deserialize)]
    struct SpeedglitchCase {
        name: String,
        foreground_allowed: bool,
        section_enabled: bool,
        hotkey_pressed: bool,
        toggle_switch: bool,
        was_pressed: bool,
        is_speed: bool,
        fps: u32,
        strength_x: i32,
        strength_y: i32,
        pressed: bool,
        next_was_pressed: bool,
        state_action: String,
        next_is_speed: bool,
        execute_cycle: bool,
        plan: Value,
        events: Vec<Value>,
    }

    fn fixture() -> Fixture {
        serde_json::from_str(FIXTURE).expect("C++ generated runtime fixture should parse")
    }

    fn wait_kind_name(kind: WaitKind) -> &'static str {
        match kind {
            WaitKind::Coarse => "coarse",
            WaitKind::Precise => "precise",
            WaitKind::Poll => "poll",
        }
    }

    fn event_json(event: &TraceEvent) -> Option<Value> {
        match &event.action {
            TraceAction::Wait { duration, kind } => {
                let micros = duration.as_micros();
                if micros % 1000 == 0 {
                    Some(json!({
                        "action": "wait",
                        "duration_ms": micros / 1000,
                        "kind": wait_kind_name(*kind),
                    }))
                } else {
                    Some(json!({
                        "action": "wait",
                        "duration_us": micros,
                        "kind": wait_kind_name(*kind),
                    }))
                }
            }
            TraceAction::Key { key, transition, path } => Some(json!({
                "action": match transition {
                    KeyTransition::Down => "key_down",
                    KeyTransition::Up => "key_up",
                },
                "key": key,
                "path": match path {
                    InputPath::Direct => "direct",
                    InputPath::Bound => "bound",
                },
            })),
            TraceAction::MouseMove { dx, dy } => Some(json!({
                "action": "mouse_move",
                "dx": dx,
                "dy": dy,
            })),
            TraceAction::TargetSuspendAttempt { suspended, success } => Some(json!({
                "action": "set_suspended",
                "suspended": suspended,
                "success": success,
            })),
            TraceAction::SettingBoolRead { name, value } => Some(json!({
                "action": "setting_read",
                "name": name,
                "value": value,
            })),
            TraceAction::SettingU32Read { name, value } => Some(json!({
                "action": "setting_read",
                "name": name,
                "value": value,
            })),
            TraceAction::HotkeyRecheck { pressed } => Some(json!({
                "action": "hotkey_recheck",
                "pressed": pressed,
            })),
            TraceAction::WallWalkTailDelayRead { delay_us } => Some(json!({
                "action": "wallwalk_tail_delay_read",
                "delay_us": delay_us,
            })),
            TraceAction::RunningCheck { running } => Some(json!({
                "action": "running_check",
                "running": running,
            })),
            TraceAction::ProcessSuspended { pids, suspended } => Some(json!({
                "action": "process_suspended",
                "pids": pids,
                "suspended": suspended,
            })),
            TraceAction::SpawnWorker { .. }
            | TraceAction::ProcessTargetRefresh { .. }
            | TraceAction::ProcessSuspendAttempt { .. }
            | TraceAction::LagBackendInit { .. }
            | TraceAction::LagConfig(_)
            | TraceAction::LagBlocking { .. } => None,
        }
    }

    fn trace_events(trace: &TraceRecorder) -> Vec<Value> {
        trace.events().iter().filter_map(event_json).collect()
    }

    fn toggle_loop_action_name(action: ToggleLoopStateAction) -> &'static str {
        match action {
            ToggleLoopStateAction::None => "none",
            ToggleLoopStateAction::Toggle => "toggle",
            ToggleLoopStateAction::Clear => "clear",
        }
    }

    #[test]
    fn cpp_frame_delay_table_matches_for_every_fps_through_1000() {
        for case in fixture().frame_delay_cases {
            let observed = frame_delays_for_fps(case.fps);
            assert_eq!(observed.first_ms, case.first_ms, "fps {} first", case.fps);
            assert_eq!(observed.second_ms, case.second_ms, "fps {} second", case.fps);
        }
    }

    #[test]
    fn cpp_rising_edge_matrix_matches() {
        for case in fixture().rising_edge_cases {
            let observed = evaluate_rising_edge(case.pressed, case.was_pressed);
            assert_eq!(observed.rising, case.rising);
            assert_eq!(observed.next_was_pressed, case.next_was_pressed);
        }
    }

    #[test]
    fn cpp_freeze_timeout_thresholds_match() {
        for case in fixture().freeze_threshold_cases {
            assert_eq!(
                freeze_timeout_threshold_ms(case.seconds),
                case.threshold_ms,
                "seconds {}",
                case.seconds
            );
        }
    }

    #[test]
    fn cpp_freeze_steps_match() {
        for case in fixture().freeze_cases {
            let mut trace = TraceRecorder::default();
            let observed = execute_freeze_step(
                &mut trace,
                LaneId::CONTROLLER,
                FreezeStepInput {
                    section_enabled: case.section_enabled,
                    foreground_allowed: case.foreground_allowed,
                    pressed: case.pressed,
                    was_pressed: case.was_pressed,
                    configured_toggle: case.configured_toggle,
                    freeze_key: case.freeze_key,
                    suspended: case.suspended,
                    elapsed_ms: case.elapsed_ms,
                    max_seconds: case.max_seconds,
                    override_ms: case.override_ms,
                    running_after_override: case.running_after_override,
                    pressed_after_override: case.pressed_after_override,
                    suspend_success: case.suspend_success,
                    resume_success: case.resume_success,
                },
            );
            assert_eq!(observed.toggle_freeze, case.toggle_freeze, "case {} toggle", case.name);
            assert_eq!(
                observed.next_was_pressed, case.next_was_pressed,
                "case {} edge state",
                case.name
            );
            assert_eq!(
                observed.final_suspended, case.final_suspended,
                "case {} suspended state",
                case.name
            );
            assert_eq!(trace_events(&trace), case.events, "case {} events", case.name);
        }
    }

    #[test]
    fn cpp_item_desync_cases_match() {
        for case in fixture().item_desync_cases {
            let mut trace = TraceRecorder::default();
            let observed = execute_item_desync(
                &mut trace,
                LaneId::CONTROLLER,
                case.foreground_allowed,
                case.section_enabled,
                case.hotkey_pressed,
                case.slot,
            );
            assert_eq!(observed.pressed, case.pressed, "case {} pressed", case.name);
            assert_eq!(observed.loop_state, case.loop_state, "case {} loop", case.name);
            assert_eq!(observed.slot_key, case.slot_key, "case {} slot", case.name);
            assert_eq!(trace_events(&trace), case.events, "case {} events", case.name);
        }
    }

    #[test]
    fn cpp_itemclip_matrix_timing_and_slots_match() {
        for case in fixture().itemclip_cases {
            let mut trace = TraceRecorder::default();
            let observed = execute_itemclip_step(
                &mut trace,
                LaneId::CONTROLLER,
                ItemClipStepInput {
                    foreground_allowed: case.foreground_allowed,
                    section_press: case.section_press,
                    section_execute: case.section_execute,
                    hotkey_pressed: case.hotkey_pressed,
                    toggle_switch: case.toggle_switch,
                    was_pressed: case.was_pressed,
                    is_loop: case.is_loop,
                    clip_delay: case.clip_delay,
                    clip_slot: case.clip_slot,
                },
            );
            assert_eq!(observed.pressed, case.pressed, "case {} pressed", case.name);
            assert_eq!(
                observed.next_was_pressed, case.next_was_pressed,
                "case {} next edge",
                case.name
            );
            assert_eq!(
                toggle_loop_action_name(observed.state_action),
                case.state_action,
                "case {} action",
                case.name
            );
            assert_eq!(observed.next_is_loop, case.next_is_loop, "case {} state", case.name);
            assert_eq!(observed.execute_cycle, case.execute_cycle, "case {} execute", case.name);
            let plan = observed.plan.map_or(Value::Null, |plan| {
                json!({
                    "half_delay_ms": plan.half_delay_ms,
                    "slot_key": plan.slot_key,
                })
            });
            assert_eq!(plan, case.plan, "case {} plan", case.name);
            assert_eq!(trace_events(&trace), case.events, "case {} events", case.name);
        }
    }

    #[test]
    fn cpp_wallwalk_delay_table_matches() {
        for case in fixture().wallwalk_delay_cases {
            assert_eq!(wallwalk_frame_delay_ms(case.fps), case.delay_ms, "fps {}", case.fps);
        }
    }

    #[test]
    fn cpp_wallwalk_matrix_moves_and_tail_waits_match() {
        for case in fixture().wallwalk_cases {
            let mut trace = TraceRecorder::default();
            let observed = execute_wallwalk_step(
                &mut trace,
                LaneId::CONTROLLER,
                WallWalkStepInput {
                    foreground_allowed: case.foreground_allowed,
                    section_press: case.section_press,
                    section_execute: case.section_execute,
                    hotkey_pressed: case.hotkey_pressed,
                    toggle_switch: case.toggle_switch,
                    was_pressed: case.was_pressed,
                    is_loop: case.is_loop,
                    fps: case.fps,
                    side_first: case.side_first,
                    strength_x: case.strength_x,
                    side_second: case.side_second,
                    strength_y: case.strength_y,
                    tail_delay_us: case.tail_delay_us,
                },
            );
            assert_eq!(observed.pressed, case.pressed, "case {} pressed", case.name);
            assert_eq!(
                observed.next_was_pressed, case.next_was_pressed,
                "case {} next edge",
                case.name
            );
            assert_eq!(
                toggle_loop_action_name(observed.state_action),
                case.state_action,
                "case {} action",
                case.name
            );
            assert_eq!(observed.next_is_loop, case.next_is_loop, "case {} state", case.name);
            assert_eq!(observed.execute_cycle, case.execute_cycle, "case {} execute", case.name);
            let plan = observed.plan.map_or(Value::Null, |plan| {
                json!({
                    "frame_delay_ms": plan.frame_delay_ms,
                    "first_move": plan.first_move,
                    "second_move": plan.second_move,
                })
            });
            assert_eq!(plan, case.plan, "case {} plan", case.name);
            assert_eq!(trace_events(&trace), case.events, "case {} events", case.name);
        }
    }

    #[test]
    fn cpp_wallhop_worker_plans_events_and_lane_topology_match() {
        for case in fixture().wallhop_cases {
            let plan = build_wallhop_worker_plan(
                case.dx,
                case.dy,
                case.vertical,
                case.delay_ms,
                case.bonus_delay_ms,
                case.left,
                case.jump,
                case.flick_back,
                case.jump_key,
            );
            assert_eq!(
                json!({
                    "dx": plan.dx,
                    "dy": plan.dy,
                    "vertical": plan.vertical,
                    "delay_ms": plan.delay_ms,
                    "bonus_delay_ms": plan.bonus_delay_ms,
                    "left": plan.left,
                    "jump": plan.jump,
                    "flick_back": plan.flick_back,
                    "jump_key": plan.jump_key,
                }),
                case.plan,
                "case {} plan",
                case.name
            );

            let mut trace = TraceRecorder::default();
            let worker = trace
                .spawn_generic_worker(LaneId::CONTROLLER)
                .expect("controller lane should exist");
            execute_wallhop_worker(&mut trace, worker, plan);
            assert_eq!(
                trace.lane_now(LaneId::CONTROLLER),
                Some(crate::MonotonicMicros::default()),
                "case {} controller must not advance",
                case.name
            );
            assert_eq!(trace_events(&trace), case.events, "case {} events", case.name);
        }
    }

    #[test]
    fn cpp_walless_worker_live_reads_events_and_lane_topology_match() {
        for case in fixture().walless_cases {
            let input = WallessLhjWorkerInput {
                pids: case.spec.pids,
                side_left: case.spec.side_left,
                global_zoom_action: case.spec.global_zoom_action,
                shift_key_action: case.spec.shift_key_action,
                zoom_reverse: case.spec.zoom_reverse,
                global_zoom_release: case.spec.global_zoom_release,
                shift_key_release: case.spec.shift_key_release,
            };

            let mut trace = TraceRecorder::default();
            let worker = trace
                .spawn_generic_worker(LaneId::CONTROLLER)
                .expect("controller lane should exist");
            execute_walless_lhj_worker(&mut trace, worker, &input);

            assert_eq!(
                trace.lane_now(LaneId::CONTROLLER),
                Some(crate::MonotonicMicros::default()),
                "case {} controller must not advance",
                case.name
            );
            assert_eq!(
                trace.lane_now(worker),
                Some(crate::MonotonicMicros::from_micros(645_000)),
                "case {} worker duration",
                case.name
            );
            assert_eq!(trace_events(&trace), case.events, "case {} events", case.name);
        }
    }

    #[test]
    fn cpp_presskey_worker_plans_and_events_match() {
        for case in fixture().presskey_cases {
            let plan = build_presskey_worker_plan(
                case.trigger,
                case.output,
                case.bonus_delay_ms,
                case.press_delay_ms,
            );
            assert_eq!(
                json!({
                    "trigger": plan.trigger,
                    "output": plan.output,
                    "bonus_delay_ms": plan.bonus_delay_ms,
                    "press_delay_ms": plan.press_delay_ms,
                    "release_trigger_first": plan.release_trigger_first,
                }),
                case.plan,
                "case {} plan",
                case.name
            );

            let mut trace = TraceRecorder::default();
            execute_presskey_worker(&mut trace, LaneId::CONTROLLER, plan, case.running_after_bonus);
            assert_eq!(trace_events(&trace), case.events, "case {} events", case.name);
        }
    }

    #[test]
    fn speedglitch_second_strength_is_read_after_the_first_move() {
        use std::cell::Cell;

        let second_strength = Cell::new(-20);
        let mut trace = TraceRecorder::default();
        let delays = frame_delays_for_fps(200);
        execute_speedglitch_cycle(
            &mut trace,
            LaneId::CONTROLLER,
            delays,
            || {
                second_strength.set(-30);
                10
            },
            || second_strength.get(),
        );

        let events = trace_events(&trace);
        assert_eq!(events[0], json!({"action":"mouse_move","dx":10,"dy":0}));
        assert_eq!(events[2], json!({"action":"mouse_move","dx":-30,"dy":0}));
    }

    #[test]
    fn cpp_speedglitch_matrix_and_cycles_match() {
        for case in fixture().speedglitch_cases {
            let edge = evaluate_speedglitch_edge(
                case.foreground_allowed,
                case.section_enabled,
                case.hotkey_pressed,
                case.toggle_switch,
                case.was_pressed,
            );
            let next_is_speed = apply_speedglitch_state_action(edge.state_action, case.is_speed);
            let execute_cycle = case.foreground_allowed && case.section_enabled && next_is_speed;
            assert_eq!(edge.pressed, case.pressed, "case {} pressed", case.name);
            assert_eq!(
                edge.next_was_pressed, case.next_was_pressed,
                "case {} next edge",
                case.name
            );
            let action_name = match edge.state_action {
                SpeedglitchStateAction::None => "none",
                SpeedglitchStateAction::Toggle => "toggle",
                SpeedglitchStateAction::Clear => "clear",
            };
            assert_eq!(action_name, case.state_action, "case {} action", case.name);
            assert_eq!(next_is_speed, case.next_is_speed, "case {} state", case.name);
            assert_eq!(execute_cycle, case.execute_cycle, "case {} execute", case.name);

            let mut trace = TraceRecorder::default();
            let observed_plan = if execute_cycle {
                let plan = build_speedglitch_cycle_plan(case.fps, case.strength_x, case.strength_y);
                execute_speedglitch_cycle(
                    &mut trace,
                    LaneId::CONTROLLER,
                    plan.delays,
                    || plan.strength_x,
                    || plan.strength_y,
                );
                json!({
                    "strength_x": plan.strength_x,
                    "strength_y": plan.strength_y,
                    "first_ms": plan.delays.first_ms,
                    "second_ms": plan.delays.second_ms,
                })
            } else {
                Value::Null
            };
            assert_eq!(observed_plan, case.plan, "case {} plan", case.name);
            assert_eq!(trace_events(&trace), case.events, "case {} events", case.name);
        }
    }
}
