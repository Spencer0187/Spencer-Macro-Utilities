//! Multi-lane compatibility tracing for macro execution.
//!
//! Native SMU macros currently mix controller-thread sleeps with independent workers. A single
//! virtual clock would serialize those lanes and hide observable overlap, so each lane owns an
//! independent monotonic timestamp while all events share one deterministic insertion sequence.

use crate::{MonotonicMicros, WaitKind};
use std::collections::BTreeMap;
use std::time::Duration;

/// Execution topology used by the current C++ runtime.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum LaneKind {
    Controller,
    GenericWorker,
    DedicatedWorker,
    Script,
}

/// Stable lane identity inside one compatibility trace.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct LaneId {
    pub kind: LaneKind,
    pub ordinal: u32,
}

impl LaneId {
    pub const CONTROLLER: Self = Self { kind: LaneKind::Controller, ordinal: 0 };

    #[must_use]
    pub const fn new(kind: LaneKind, ordinal: u32) -> Self {
        Self { kind, ordinal }
    }
}

/// Which current input helper family emitted a key transition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputPath {
    /// `HoldKey` / `ReleaseKey` style direct virtual-key injection.
    Direct,
    /// `HoldKeyBinded` / `ReleaseKeyBinded` compatibility path.
    Bound,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyTransition {
    Down,
    Up,
}

/// Network configuration fields observable at the lag backend boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
#[allow(clippy::struct_excessive_bools)]
pub struct LagConfigTrace {
    pub enabled: bool,
    pub inbound_hard_block: bool,
    pub outbound_hard_block: bool,
    pub fake_lag_enabled: bool,
    pub inbound_fake_lag: bool,
    pub outbound_fake_lag: bool,
    pub fake_lag_delay_ms: i32,
    pub target_roblox_only: bool,
    pub use_tcp: bool,
    pub use_udp: bool,
    pub prevent_disconnect: bool,
    pub auto_unblock: bool,
    pub max_duration_seconds: f32,
    pub unblock_duration_ms: i32,
}

/// One compatibility-relevant side effect or scheduling operation.
#[derive(Clone, Debug, PartialEq)]
pub enum TraceAction {
    Wait { duration: Duration, kind: WaitKind },
    SpawnWorker { child: LaneId },
    Key { key: u32, transition: KeyTransition, path: InputPath },
    MouseMove { dx: i32, dy: i32 },
    TargetSuspendAttempt { suspended: bool, success: bool },
    HotkeyRecheck { pressed: bool },
    WallWalkTailDelayRead { delay_us: i32 },
    ProcessSuspended { pids: Vec<u32>, suspended: bool },
    LagBackendInit { success: bool },
    LagConfig(LagConfigTrace),
    LagBlocking { active: bool },
    RunningCheck { running: bool },
}

/// Timestamped event. `sequence` preserves call order when lanes have the same virtual timestamp.
#[derive(Clone, Debug, PartialEq)]
pub struct TraceEvent {
    pub sequence: u64,
    pub at: MonotonicMicros,
    pub lane: LaneId,
    pub action: TraceAction,
}

/// Deterministic multi-lane event recorder.
#[derive(Debug)]
pub struct TraceRecorder {
    lanes: BTreeMap<LaneId, MonotonicMicros>,
    events: Vec<TraceEvent>,
    next_sequence: u64,
    next_generic_worker: u32,
    next_dedicated_worker: u32,
    next_script: u32,
}

impl Default for TraceRecorder {
    fn default() -> Self {
        Self::at(MonotonicMicros::default())
    }
}

impl TraceRecorder {
    #[must_use]
    pub fn at(start: MonotonicMicros) -> Self {
        Self {
            lanes: BTreeMap::from([(LaneId::CONTROLLER, start)]),
            events: Vec::new(),
            next_sequence: 0,
            next_generic_worker: 0,
            next_dedicated_worker: 0,
            next_script: 0,
        }
    }

    #[must_use]
    pub fn events(&self) -> &[TraceEvent] {
        &self.events
    }

    #[must_use]
    pub fn lane_now(&self, lane: LaneId) -> Option<MonotonicMicros> {
        self.lanes.get(&lane).copied()
    }

    pub fn wait(&mut self, lane: LaneId, duration: Duration, kind: WaitKind) {
        let Some(started_at) = self.lane_now(lane) else {
            return;
        };
        self.push_at(lane, started_at, TraceAction::Wait { duration, kind });
        self.lanes.insert(lane, started_at.saturating_add(duration));
    }

    #[must_use]
    pub fn spawn_generic_worker(&mut self, parent: LaneId) -> Option<LaneId> {
        let ordinal = self.next_generic_worker;
        self.next_generic_worker = self.next_generic_worker.saturating_add(1);
        self.spawn_lane(parent, LaneId::new(LaneKind::GenericWorker, ordinal))
    }

    #[must_use]
    pub fn spawn_dedicated_worker(&mut self, parent: LaneId) -> Option<LaneId> {
        let ordinal = self.next_dedicated_worker;
        self.next_dedicated_worker = self.next_dedicated_worker.saturating_add(1);
        self.spawn_lane(parent, LaneId::new(LaneKind::DedicatedWorker, ordinal))
    }

    #[must_use]
    pub fn spawn_script(&mut self, parent: LaneId) -> Option<LaneId> {
        let ordinal = self.next_script;
        self.next_script = self.next_script.saturating_add(1);
        self.spawn_lane(parent, LaneId::new(LaneKind::Script, ordinal))
    }

    pub fn key(&mut self, lane: LaneId, key: u32, transition: KeyTransition, path: InputPath) {
        self.push_now(lane, TraceAction::Key { key, transition, path });
    }

    pub fn mouse_move(&mut self, lane: LaneId, dx: i32, dy: i32) {
        self.push_now(lane, TraceAction::MouseMove { dx, dy });
    }

    pub fn target_suspend_attempt(&mut self, lane: LaneId, suspended: bool, success: bool) {
        self.push_now(lane, TraceAction::TargetSuspendAttempt { suspended, success });
    }

    pub fn hotkey_recheck(&mut self, lane: LaneId, pressed: bool) {
        self.push_now(lane, TraceAction::HotkeyRecheck { pressed });
    }

    pub fn wallwalk_tail_delay_read(&mut self, lane: LaneId, delay_us: i32) {
        self.push_now(lane, TraceAction::WallWalkTailDelayRead { delay_us });
    }

    pub fn process_suspended(&mut self, lane: LaneId, pids: Vec<u32>, suspended: bool) {
        self.push_now(lane, TraceAction::ProcessSuspended { pids, suspended });
    }

    pub fn lag_backend_init(&mut self, lane: LaneId, success: bool) {
        self.push_now(lane, TraceAction::LagBackendInit { success });
    }

    pub fn lag_config(&mut self, lane: LaneId, config: LagConfigTrace) {
        self.push_now(lane, TraceAction::LagConfig(config));
    }

    pub fn lag_blocking(&mut self, lane: LaneId, active: bool) {
        self.push_now(lane, TraceAction::LagBlocking { active });
    }

    pub fn running_check(&mut self, lane: LaneId, running: bool) {
        self.push_now(lane, TraceAction::RunningCheck { running });
    }

    fn spawn_lane(&mut self, parent: LaneId, child: LaneId) -> Option<LaneId> {
        let started_at = self.lane_now(parent)?;
        self.lanes.insert(child, started_at);
        self.push_at(parent, started_at, TraceAction::SpawnWorker { child });
        Some(child)
    }

    fn push_now(&mut self, lane: LaneId, action: TraceAction) {
        let Some(at) = self.lane_now(lane) else {
            return;
        };
        self.push_at(lane, at, action);
    }

    fn push_at(&mut self, lane: LaneId, at: MonotonicMicros, action: TraceAction) {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.events.push(TraceEvent { sequence, at, lane, action });
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{InputPath, KeyTransition, LaneId, LaneKind, TraceAction, TraceRecorder};
    use crate::{MonotonicMicros, WaitKind};
    use std::time::Duration;

    #[test]
    fn worker_sleep_does_not_advance_controller_clock() {
        let mut trace = TraceRecorder::at(MonotonicMicros::from_micros(100));
        let worker =
            trace.spawn_generic_worker(LaneId::CONTROLLER).expect("controller lane exists");
        trace.wait(worker, Duration::from_millis(50), WaitKind::Coarse);
        trace.wait(LaneId::CONTROLLER, Duration::from_millis(2), WaitKind::Poll);

        assert_eq!(trace.lane_now(worker).map(MonotonicMicros::as_micros), Some(50_100));
        assert_eq!(trace.lane_now(LaneId::CONTROLLER).map(MonotonicMicros::as_micros), Some(2_100));
    }

    #[test]
    fn spawned_workers_get_stable_ordinals_and_parent_timestamp() {
        let mut trace = TraceRecorder::at(MonotonicMicros::from_micros(42));
        let first = trace.spawn_generic_worker(LaneId::CONTROLLER).expect("controller lane exists");
        let second =
            trace.spawn_generic_worker(LaneId::CONTROLLER).expect("controller lane exists");

        assert_eq!(first, LaneId::new(LaneKind::GenericWorker, 0));
        assert_eq!(second, LaneId::new(LaneKind::GenericWorker, 1));
        assert_eq!(trace.events()[0].at.as_micros(), 42);
        assert_eq!(trace.events()[1].at.as_micros(), 42);
    }

    #[test]
    fn equal_timestamp_actions_preserve_call_sequence() {
        let mut trace = TraceRecorder::default();
        trace.key(LaneId::CONTROLLER, 65, KeyTransition::Down, InputPath::Direct);
        trace.key(LaneId::CONTROLLER, 65, KeyTransition::Up, InputPath::Direct);

        assert_eq!(trace.events()[0].sequence, 0);
        assert_eq!(trace.events()[1].sequence, 1);
        assert!(matches!(
            trace.events()[0].action,
            TraceAction::Key { transition: KeyTransition::Down, .. }
        ));
        assert!(matches!(
            trace.events()[1].action,
            TraceAction::Key { transition: KeyTransition::Up, .. }
        ));
    }
}
