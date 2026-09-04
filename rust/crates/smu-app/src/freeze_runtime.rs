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
        let worker = std::thread::spawn(move || {
            controller_loop(
                worker_running.as_ref(),
                worker_settings.as_ref(),
                worker_suspended.as_ref(),
                input.as_ref(),
                discovery.as_ref(),
                foreground.as_ref(),
                control.as_ref(),
            );
        });
        Self { running, settings, suspended, worker: Mutex::new(Some(worker)) }
    }

    pub fn update_settings(&self, settings: FreezeControllerSettings) {
        *lock(&self.settings) = settings;
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        let worker = lock(&self.worker).take();
        if let Some(worker) = worker {
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
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }

        process_step(&current, running, input, discovery, foreground, control, &mut state);
        suspended_flag.store(state.suspend.freeze_suspended, Ordering::Release);
        std::thread::sleep(CONTROLLER_POLL_INTERVAL);
    }

    if state.suspend.freeze_suspended {
        let current = lock(settings).clone();
        set_target_suspended(&current, discovery, control, &mut state, false);
        suspended_flag.store(state.suspend.freeze_suspended, Ordering::Release);
    }
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
    let foreground_allowed =
        state.targets.foreground_allowed(settings.disable_outside_target, foreground);
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
