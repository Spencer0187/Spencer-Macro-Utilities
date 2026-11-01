//! Live Linux Freeze controller.
//!
//! Freeze owns a dedicated long-lived controller lane. This is an intentional post-characterization
//! migration from the monolithic C++ controller: the macro's internal suspend/wait/resuspend order is
//! unchanged, but its auto-unfreeze override wait cannot stall Lag Switch or later macro controllers.

use smu_core::Hotkey;
use smu_input::KeyState;
use smu_process::{ProcessControl, ProcessDiscovery, ProcessForeground};
use smu_runtime::macros::{freeze_timeout_threshold_ms, should_toggle_freeze};
use smu_runtime::process_control::{TargetSuspendState, execute_target_suspend_request_untraced};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);
pub const DEFAULT_FREEZE_HOTKEY: Hotkey = Hotkey::new(0x04);

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq)]
pub struct FreezeControllerSettings {
    pub macro_enabled: bool,
    pub section_enabled: bool,
    pub hotkey: Hotkey,
    pub configured_toggle: bool,
    pub max_seconds: f32,
    pub override_ms: i32,
    pub take_all_process_ids: bool,
    pub disable_outside_target: bool,
    pub process_selector: String,
}

impl Default for FreezeControllerSettings {
    fn default() -> Self {
        Self {
            macro_enabled: true,
            section_enabled: true,
            hotkey: DEFAULT_FREEZE_HOTKEY,
            configured_toggle: false,
            max_seconds: 9.0,
            override_ms: 50,
            take_all_process_ids: false,
            disable_outside_target: true,
            process_selector: "sober".to_owned(),
        }
    }
}

#[derive(Debug)]
struct ControllerState {
    was_pressed: bool,
    suspend: TargetSuspendState,
    freeze_start: Instant,
    targets: crate::target_runtime::TargetProcessCache,
}

impl Default for ControllerState {
    fn default() -> Self {
        let now = Instant::now();
        Self {
            was_pressed: false,
            suspend: TargetSuspendState::default(),
            freeze_start: now,
            targets: crate::target_runtime::TargetProcessCache::default(),
        }
    }
}

pub struct FreezeController {
    input: Arc<dyn KeyState>,
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<FreezeControllerSettings>>,
    suspended: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for FreezeController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FreezeController")
            .field("running", &self.running.load(Ordering::Acquire))
            .field("suspended", &self.suspended.load(Ordering::Acquire))
            .field("settings", &*lock(&self.settings))
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl FreezeController {
    #[must_use]
    pub fn start(
        input: Arc<dyn KeyState>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        control: Arc<dyn ProcessControl>,
        initial_settings: FreezeControllerSettings,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let settings = Arc::new(Mutex::new(initial_settings));
        let suspended = Arc::new(AtomicBool::new(false));
        let worker_running = Arc::clone(&running);
        let worker_settings = Arc::clone(&settings);
        let worker_suspended = Arc::clone(&suspended);
        let worker_input = Arc::clone(&input);
        let worker = std::thread::spawn(move || {
            controller_loop(
                worker_running.as_ref(),
                worker_settings.as_ref(),
                worker_suspended.as_ref(),
                worker_input.as_ref(),
                discovery.as_ref(),
                foreground.as_ref(),
                control.as_ref(),
            );
        });
        Self { input, running, settings, suspended, worker: Mutex::new(Some(worker)) }
    }

    pub fn update_settings(&self, settings: FreezeControllerSettings) {
        let mut current = lock(&self.settings);
        if *current == settings {
            return;
        }
        *current = settings;
        drop(current);
        self.input.wake_input_waiters();
        // Wake an inactive controller immediately when the user enables or changes its module.
        if let Some(handle) = lock(&self.worker).as_ref() {
            handle.thread().unpark();
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        self.input.wake_input_waiters();
        let worker = lock(&self.worker).take();
        if let Some(worker) = worker {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

impl Drop for FreezeController {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn controller_loop(
    running: &AtomicBool,
    settings: &Mutex<FreezeControllerSettings>,
    suspended_flag: &AtomicBool,
    input: &dyn KeyState,
    discovery: &dyn ProcessDiscovery,
    foreground: &dyn ProcessForeground,
    control: &dyn ProcessControl,
) {
    let mut state = ControllerState::default();
    while running.load(Ordering::Acquire) {
        // Capture the serial before reading the key. If a physical event arrives during
        // process_step, the subsequent wait returns immediately rather than losing the edge.
        let input_sequence = input.input_change_sequence();
        let current = lock(settings).clone();
        state.targets.refresh_if_due(
            &current.process_selector,
            current.take_all_process_ids,
            discovery,
        );
        state.suspend.target_pids = state.targets.selected_pids().to_vec();

        if !current.macro_enabled {
            // C++ resumes Freeze here but deliberately preserves freezeWasPressed_, so re-enabling
            // while a toggle hotkey is still held does not synthesize a fresh rising edge.
            if state.suspend.freeze_suspended {
                set_target_suspended(&current, discovery, control, &mut state, false);
            }
            suspended_flag.store(state.suspend.freeze_suspended, Ordering::Release);
            std::thread::park_timeout(Duration::from_secs(1));
            continue;
        }

        process_step(&current, running, input, discovery, foreground, control, &mut state);
        suspended_flag.store(state.suspend.freeze_suspended, Ordering::Release);
        if current.macro_enabled && current.section_enabled {
            if let Some(sequence) = input_sequence {
                input.wait_for_input_change(sequence, freeze_event_wait_budget(&current, &state));
            } else {
                // Windows/macOS/mock backends preserve the original 2 ms timing.
                std::thread::sleep(CONTROLLER_POLL_INTERVAL);
            }
        } else {
            // No hotkeys can fire while disabled. Avoid a 2 ms polling loop entirely.
            // update_settings unparks this thread immediately upon any settings change.
            std::thread::park_timeout(Duration::from_secs(1));
        }
    }

    if state.suspend.freeze_suspended {
        let current = lock(settings).clone();
        set_target_suspended(&current, discovery, control, &mut state, false);
        suspended_flag.store(state.suspend.freeze_suspended, Ordering::Release);
    }
}

const FOREGROUND_RECHECK_INTERVAL: Duration = Duration::from_millis(25);

fn freeze_event_wait_budget(
    settings: &FreezeControllerSettings,
    state: &ControllerState,
) -> Duration {
    if !state.suspend.freeze_suspended {
        return FOREGROUND_RECHECK_INTERVAL;
    }
    // Don't postpone auto-unfreeze to the next 25 ms foreground tick: wake at its
    // precise millisecond threshold instead, matching the original timeout logic.
    let remaining = freeze_timeout_threshold_ms(settings.max_seconds)
        .saturating_sub(elapsed_ms(state.freeze_start))
        .max(1);
    Duration::from_millis(u64::try_from(remaining).unwrap_or(u64::MAX))
        .min(FOREGROUND_RECHECK_INTERVAL)
}

fn process_step(
    settings: &FreezeControllerSettings,
    running: &AtomicBool,
    input: &dyn KeyState,
    discovery: &dyn ProcessDiscovery,
    foreground: &dyn ProcessForeground,
    control: &dyn ProcessControl,
    state: &mut ControllerState,
) {
    if !settings.section_enabled {
        if state.suspend.freeze_suspended {
            set_target_suspended(settings, discovery, control, state, false);
        }
        state.was_pressed = false;
        return;
    }

    let pressed = crate::runtime_input::hotkey_pressed(input, settings.hotkey);
    // Foreground eligibility can only affect a newly pressed/held hotkey. Both toggle
    // and hold modes ignore foreground when the physical key is released, so avoid
    // querying X11 in that state without changing any suspend/resume decisions.
    let foreground_allowed =
        !pressed || state.targets.foreground_allowed(settings.disable_outside_target, foreground);
    let toggle = should_toggle_freeze(settings.configured_toggle, settings.hotkey.key().raw());

    if toggle {
        if pressed && !state.was_pressed && foreground_allowed {
            set_target_suspended(
                settings,
                discovery,
                control,
                state,
                !state.suspend.freeze_suspended,
            );
        }
    } else if pressed && foreground_allowed {
        if !state.suspend.freeze_suspended {
            set_target_suspended(settings, discovery, control, state, true);
        }
    } else if state.suspend.freeze_suspended {
        set_target_suspended(settings, discovery, control, state, false);
    }

    if state.suspend.freeze_suspended
        && elapsed_ms(state.freeze_start) >= freeze_timeout_threshold_ms(settings.max_seconds)
    {
        set_target_suspended(settings, discovery, control, state, false);
        std::thread::sleep(Duration::from_millis(
            u64::try_from(settings.override_ms.max(0)).unwrap_or_default(),
        ));
        if running.load(Ordering::Acquire) {
            let should_resuspend =
                toggle || crate::runtime_input::hotkey_pressed(input, settings.hotkey);
            if should_resuspend {
                set_target_suspended(settings, discovery, control, state, true);
            }
        }
    }

    state.was_pressed = pressed;
}

fn elapsed_ms(start: Instant) -> i64 {
    i64::try_from(start.elapsed().as_millis()).unwrap_or(i64::MAX)
}

fn set_target_suspended(
    settings: &FreezeControllerSettings,
    discovery: &dyn ProcessDiscovery,
    control: &dyn ProcessControl,
    state: &mut ControllerState,
    suspended: bool,
) {
    let outcome = execute_target_suspend_request_untraced(
        &mut state.suspend,
        suspended,
        || {
            state.targets.force_refresh(
                &settings.process_selector,
                settings.take_all_process_ids,
                discovery,
            );
            state.targets.selected_pids().to_vec()
        },
        |pid, requested| {
            if requested { control.suspend(pid) } else { control.resume(pid) }
        },
    );
    if outcome.reset_freeze_start {
        state.freeze_start = Instant::now();
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use smu_process::ProcessRecord;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    struct FakeInput {
        pressed: AtomicBool,
    }

    impl KeyState for FakeInput {
        fn is_pressed(&self, key: smu_core::KeyCode) -> bool {
            key == DEFAULT_FREEZE_HOTKEY.key() && self.pressed.load(Ordering::Acquire)
        }
    }

    #[derive(Debug, Default)]
    struct NotifyingInput {
        pressed: AtomicBool,
        serial: Mutex<u64>,
        changed: std::sync::Condvar,
    }

    impl NotifyingInput {
        fn set_pressed(&self, pressed: bool) {
            self.pressed.store(pressed, Ordering::Release);
            self.wake_input_waiters();
        }
    }

    impl KeyState for NotifyingInput {
        fn is_pressed(&self, key: smu_core::KeyCode) -> bool {
            key == DEFAULT_FREEZE_HOTKEY.key() && self.pressed.load(Ordering::Acquire)
        }

        fn input_change_sequence(&self) -> Option<u64> {
            Some(*lock(&self.serial))
        }

        fn wait_for_input_change(&self, since: u64, max_wait: Duration) {
            let guard = lock(&self.serial);
            let _ = self.changed.wait_timeout_while(guard, max_wait, |serial| *serial == since);
        }

        fn wake_input_waiters(&self) {
            let mut serial = lock(&self.serial);
            *serial += 1;
            drop(serial);
            self.changed.notify_all();
        }
    }

    #[derive(Debug)]
    struct FakeDiscovery(Vec<ProcessRecord>);

    impl ProcessDiscovery for FakeDiscovery {
        fn snapshot(&self) -> std::io::Result<Vec<ProcessRecord>> {
            Ok(self.0.clone())
        }
    }

    #[derive(Debug, Default)]
    struct FakeForeground(bool);

    impl ProcessForeground for FakeForeground {
        fn is_foreground_process(&self, _pid: u32) -> bool {
            self.0
        }
    }

    #[derive(Debug, Default)]
    struct FakeControl {
        events: Mutex<Vec<(u32, bool)>>,
    }

    impl ProcessControl for FakeControl {
        fn suspend(&self, pid: u32) -> bool {
            lock(&self.events).push((pid, true));
            true
        }

        fn resume(&self, pid: u32) -> bool {
            lock(&self.events).push((pid, false));
            true
        }
    }

    fn records() -> Vec<ProcessRecord> {
        vec![
            ProcessRecord {
                pid: 10,
                parent_pid: Some(1),
                command_name: Some("sober".to_owned()),
                executable_name: Some("sober".to_owned()),
            },
            ProcessRecord {
                pid: 20,
                parent_pid: Some(10),
                command_name: Some("sober".to_owned()),
                executable_name: Some("sober".to_owned()),
            },
        ]
    }

    #[test]
    fn event_driven_freeze_reacts_to_press_release_and_cleanly_stops() {
        let input = Arc::new(NotifyingInput::default());
        let control = Arc::new(FakeControl::default());
        let controller = FreezeController::start(
            input.clone(),
            Arc::new(FakeDiscovery(records())),
            Arc::new(FakeForeground(true)),
            control.clone(),
            FreezeControllerSettings {
                disable_outside_target: false,
                ..FreezeControllerSettings::default()
            },
        );
        let wait_for_event = |expected: (u32, bool)| {
            let deadline = Instant::now() + Duration::from_secs(1);
            while !lock(&control.events).contains(&expected) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(2));
            }
            assert!(
                lock(&control.events).contains(&expected),
                "missing expected event {expected:?}"
            );
        };
        input.set_pressed(true);
        wait_for_event((10, true));
        input.set_pressed(false);
        wait_for_event((10, false));
        controller.stop();
    }

    #[test]
    fn event_wait_does_not_delay_foreground_or_freeze_timeout() {
        let mut state = ControllerState::default();
        let settings = FreezeControllerSettings::default();
        assert_eq!(freeze_event_wait_budget(&settings, &state), Duration::from_millis(25));
        state.suspend.freeze_suspended = true;
        state.freeze_start = Instant::now();
        assert!(freeze_event_wait_budget(&settings, &state) <= Duration::from_millis(25));
        state.freeze_start =
            Instant::now().checked_sub(Duration::from_millis(8_999)).expect("valid test instant");
        assert!(freeze_event_wait_budget(&settings, &state) <= Duration::from_millis(2));
        state.freeze_start =
            Instant::now().checked_sub(Duration::from_millis(10_000)).expect("valid test instant");
        assert_eq!(freeze_event_wait_budget(&settings, &state), Duration::from_millis(1));
    }

    #[test]
    fn defaults_match_cpp_linux_freeze_settings() {
        let settings = FreezeControllerSettings::default();
        assert!(settings.macro_enabled);
        assert!(settings.section_enabled);
        assert_eq!(settings.hotkey, DEFAULT_FREEZE_HOTKEY);
        assert!(!settings.configured_toggle);
        assert!((settings.max_seconds - 9.0).abs() < f32::EPSILON);
        assert_eq!(settings.override_ms, 50);
        assert!(!settings.take_all_process_ids);
        assert!(settings.disable_outside_target);
        assert_eq!(settings.process_selector, "sober");
    }

    #[test]
    fn target_selection_matches_main_vs_all_cpp_setting() {
        let discovery = FakeDiscovery(records());
        let settings = FreezeControllerSettings::default();
        assert_eq!(
            crate::target_runtime::discover_targets(
                &settings.process_selector,
                settings.take_all_process_ids,
                &discovery,
            )
            .0,
            [10]
        );
        let all_settings = FreezeControllerSettings { take_all_process_ids: true, ..settings };
        assert_eq!(
            crate::target_runtime::discover_targets(
                &all_settings.process_selector,
                all_settings.take_all_process_ids,
                &discovery,
            )
            .0,
            [10, 20]
        );
    }

    #[test]
    fn hold_mode_suspends_and_releases_selected_process() {
        let input = FakeInput::default();
        input.pressed.store(true, Ordering::Release);
        let discovery = FakeDiscovery(records());
        let foreground = FakeForeground(true);
        let control = FakeControl::default();
        let running = AtomicBool::new(true);
        let mut state = ControllerState::default();
        let settings = FreezeControllerSettings {
            disable_outside_target: false,
            ..FreezeControllerSettings::default()
        };
        state.targets.force_refresh(
            &settings.process_selector,
            settings.take_all_process_ids,
            &discovery,
        );
        state.suspend.target_pids = state.targets.selected_pids().to_vec();
        process_step(&settings, &running, &input, &discovery, &foreground, &control, &mut state);
        assert!(state.suspend.freeze_suspended);
        input.pressed.store(false, Ordering::Release);
        process_step(&settings, &running, &input, &discovery, &foreground, &control, &mut state);
        assert!(!state.suspend.freeze_suspended);
        assert_eq!(*lock(&control.events), [(10, true), (10, false)]);
    }
}
