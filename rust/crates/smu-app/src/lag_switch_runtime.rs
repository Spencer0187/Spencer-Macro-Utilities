//! Live Linux Lag Switch controller vertical slice.
//!
//! The controller retains the legacy 2 ms polling cadence and owns hotkey edge/config-cache state.
//! It performs no blocking packet work; the native network service owns packet interception and its
//! dynamic-target refresh worker.

use smu_core::Hotkey;
use smu_input::KeyState;
use smu_network::{LagSwitchConfig, LagSwitchControl};
use smu_runtime::lag_switch::{LagSwitchStepContext, LagSwitchStepInput, execute_lag_switch_step};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);
pub const DEFAULT_LAG_SWITCH_HOTKEY: Hotkey = Hotkey::new(0xBB);

#[derive(Clone, Debug, PartialEq)]
pub struct LagSwitchControllerSettings {
    pub macro_enabled: bool,
    pub section_enabled: bool,
    pub hotkey: Hotkey,
    pub toggle_mode: bool,
    pub config: LagSwitchConfig,
}

impl Default for LagSwitchControllerSettings {
    fn default() -> Self {
        Self {
            macro_enabled: true,
            section_enabled: false,
            hotkey: DEFAULT_LAG_SWITCH_HOTKEY,
            toggle_mode: true,
            config: LagSwitchConfig { prevent_disconnect: false, ..LagSwitchConfig::default() },
        }
    }
}

#[derive(Debug)]
struct ControllerState {
    was_pressed: bool,
    last_config: Option<LagSwitchConfig>,
    start_time: Instant,
}

impl Default for ControllerState {
    fn default() -> Self {
        Self { was_pressed: false, last_config: None, start_time: Instant::now() }
    }
}

struct StepContext<'a> {
    backend: &'a dyn LagSwitchControl,
    current_config: &'a LagSwitchConfig,
    last_config: &'a Option<LagSwitchConfig>,
    helper_prompt_requested: &'a AtomicBool,
    start_time: &'a mut Instant,
    applied_config: bool,
}

impl LagSwitchStepContext for StepContext<'_> {
    fn initialize_backend(&mut self) -> bool {
        let initialized = self.backend.initialize();
        if !initialized {
            self.helper_prompt_requested.store(true, Ordering::Release);
        }
        initialized
    }

    fn config_changed(&self) -> bool {
        self.last_config.as_ref() != Some(self.current_config)
    }

    fn apply_config(&mut self) {
        self.backend.set_base_config(self.current_config.clone());
        self.applied_config = true;
    }

    fn is_base_blocking(&self) -> bool {
        self.backend.is_base_blocking_active()
    }

    fn set_blocking(&mut self, active: bool) {
        self.backend.set_base_blocking_active(active);
    }

    fn reset_start(&mut self) {
        *self.start_time = Instant::now();
    }

    fn elapsed_seconds(&mut self) -> f32 {
        self.start_time.elapsed().as_secs_f32()
    }
}

pub struct LagSwitchController {
    running: Arc<AtomicBool>,
    backend_enabled: Arc<AtomicBool>,
    settings: Arc<Mutex<LagSwitchControllerSettings>>,
    helper_prompt_requested: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for LagSwitchController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LagSwitchController")
            .field("running", &self.running.load(Ordering::Acquire))
            .field("backend_enabled", &self.backend_enabled.load(Ordering::Acquire))
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

impl LagSwitchController {
    #[must_use]
    pub fn start(
        input: Arc<dyn KeyState>,
        backend: Arc<dyn LagSwitchControl>,
        backend_enabled: Arc<AtomicBool>,
        initial_settings: LagSwitchControllerSettings,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let settings = Arc::new(Mutex::new(initial_settings));
        let helper_prompt_requested = Arc::new(AtomicBool::new(false));
        let worker_running = Arc::clone(&running);
        let worker_enabled = Arc::clone(&backend_enabled);
        let worker_settings = Arc::clone(&settings);
        let worker_prompt = Arc::clone(&helper_prompt_requested);
        let worker = std::thread::spawn(move || {
            controller_loop(
                worker_running.as_ref(),
                worker_settings.as_ref(),
                input.as_ref(),
                backend.as_ref(),
                worker_enabled.as_ref(),
                worker_prompt.as_ref(),
            );
        });

        Self {
            running,
            backend_enabled,
            settings,
            helper_prompt_requested,
            worker: Mutex::new(Some(worker)),
        }
    }

    pub fn update_settings(&self, settings: LagSwitchControllerSettings) {
        *lock(&self.settings) = settings;
    }

    pub fn take_helper_prompt_request(&self) -> bool {
        self.helper_prompt_requested.swap(false, Ordering::AcqRel)
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        let worker = lock(&self.worker).take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

impl Drop for LagSwitchController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn controller_loop(
    running: &AtomicBool,
    settings: &Mutex<LagSwitchControllerSettings>,
    input: &dyn KeyState,
    backend: &dyn LagSwitchControl,
    backend_enabled: &AtomicBool,
    helper_prompt_requested: &AtomicBool,
) {
    let mut state = ControllerState::default();
    while running.load(Ordering::Acquire) {
        let current = lock(settings).clone();
        if current.macro_enabled {
            process_step(
                &current,
                input,
                backend,
                backend_enabled,
                helper_prompt_requested,
                &mut state,
            );
        }
        std::thread::sleep(CONTROLLER_POLL_INTERVAL);
    }
}

fn process_step(
    settings: &LagSwitchControllerSettings,
    input: &dyn KeyState,
    backend: &dyn LagSwitchControl,
    backend_enabled: &AtomicBool,
    helper_prompt_requested: &AtomicBool,
    state: &mut ControllerState,
) {
    let pressed = crate::runtime_input::hotkey_pressed(input, settings.hotkey);
    let mut current_config = settings.config.clone();
    current_config.enabled = backend_enabled.load(Ordering::Acquire);
    current_config.currently_blocking = false;

    let had_last_config = state.last_config.is_some();
    let last_config = state.last_config.clone();
    let (result, applied_config) = {
        let mut context = StepContext {
            backend,
            current_config: &current_config,
            last_config: &last_config,
            helper_prompt_requested,
            start_time: &mut state.start_time,
            applied_config: false,
        };
        let result = execute_lag_switch_step(
            LagSwitchStepInput {
                section_enabled: settings.section_enabled,
                backend_present: true,
                foreground_allowed: true,
                pressed,
                was_pressed: state.was_pressed,
                toggle_mode: settings.toggle_mode,
                backend_enabled: backend_enabled.load(Ordering::Acquire),
                has_last_config: had_last_config,
                auto_unblock: current_config.auto_unblock,
                max_duration_seconds: current_config.max_duration_seconds,
            },
            &mut context,
        );
        (result, context.applied_config)
    };

    state.was_pressed = result.next_was_pressed;
    if !result.next_has_last_config {
        state.last_config = None;
    } else if applied_config {
        state.last_config = Some(current_config);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use smu_core::KeyCode;
    use std::collections::BTreeSet;
    use std::sync::atomic::AtomicUsize;

    #[derive(Debug, Default)]
    struct FakeInput {
        keys: Mutex<BTreeSet<KeyCode>>,
        transient: Mutex<Option<KeyCode>>,
    }

    impl FakeInput {
        fn set(&self, key: KeyCode, pressed: bool) {
            let mut keys = lock(&self.keys);
            if pressed {
                keys.insert(key);
            } else {
                keys.remove(&key);
            }
        }
    }

    impl KeyState for FakeInput {
        fn is_pressed(&self, key: KeyCode) -> bool {
            lock(&self.keys).contains(&key)
        }

        fn consume_next_transient_input(&self) -> Option<KeyCode> {
            lock(&self.transient).take()
        }
    }

    #[derive(Debug, Default)]
    struct FakeBackend {
        available: AtomicBool,
        base_blocking: AtomicBool,
        init_success: AtomicBool,
        init_calls: AtomicUsize,
        configs: Mutex<Vec<LagSwitchConfig>>,
    }

    impl LagSwitchControl for FakeBackend {
        fn initialize(&self) -> bool {
            self.init_calls.fetch_add(1, Ordering::AcqRel);
            let success = self.init_success.load(Ordering::Acquire);
            if success {
                self.available.store(true, Ordering::Release);
            }
            success
        }

        fn is_available(&self) -> bool {
            self.available.load(Ordering::Acquire)
        }

        fn effective_config(&self) -> LagSwitchConfig {
            let mut config = lock(&self.configs).last().cloned().unwrap_or_default();
            config.currently_blocking = self.base_blocking.load(Ordering::Acquire);
            config
        }

        fn is_blocking_active(&self) -> bool {
            self.base_blocking.load(Ordering::Acquire)
        }

        fn is_base_blocking_active(&self) -> bool {
            self.base_blocking.load(Ordering::Acquire)
        }

        fn set_base_blocking_active(&self, active: bool) {
            self.base_blocking.store(active, Ordering::Release);
        }

        fn set_base_config(&self, config: LagSwitchConfig) {
            lock(&self.configs).push(config);
        }

        fn unsupported_reason(&self) -> String {
            String::new()
        }

        fn set_script_blocking_active(&self, _owner_token: usize, _active: bool) {}

        fn set_script_config_override(&self, _owner_token: usize, _config: LagSwitchConfig) {}

        fn clear_script_config_override(&self, _owner_token: usize) {}
    }

    #[test]
    fn default_controller_settings_match_linux_cpp_lag_switch_defaults() {
        let settings = LagSwitchControllerSettings::default();
        assert!(settings.macro_enabled);
        assert!(!settings.section_enabled);
        assert_eq!(settings.hotkey, Hotkey::new(0xBB));
        assert!(settings.toggle_mode);
        assert!(settings.config.inbound_hard_block);
        assert!(settings.config.outbound_hard_block);
        assert!(settings.config.target_roblox_only);
        assert!(settings.config.use_udp);
        assert!(!settings.config.use_tcp);
        assert!(!settings.config.prevent_disconnect);
        assert!(!settings.config.auto_unblock);
        assert!((settings.config.max_duration_seconds - 9.0).abs() < f32::EPSILON);
        assert_eq!(settings.config.unblock_duration_ms, 50);
    }

    #[test]
    fn toggle_edge_initializes_backend_publishes_config_and_blocks() {
        let input = FakeInput::default();
        input.set(KeyCode::new(0xBB), true);
        let backend = FakeBackend::default();
        backend.init_success.store(true, Ordering::Release);
        let backend_enabled = AtomicBool::new(false);
        let prompt = AtomicBool::new(false);
        let mut state = ControllerState::default();
        let settings = LagSwitchControllerSettings {
            section_enabled: true,
            ..LagSwitchControllerSettings::default()
        };

        process_step(&settings, &input, &backend, &backend_enabled, &prompt, &mut state);
        assert!(backend.available.load(Ordering::Acquire));
        assert!(!backend_enabled.load(Ordering::Acquire));
        assert!(backend.is_base_blocking_active());
        assert_eq!(backend.init_calls.load(Ordering::Acquire), 1);
        assert_eq!(lock(&backend.configs).len(), 1);
        assert!(!prompt.load(Ordering::Acquire));
        assert!(state.was_pressed);
    }

    #[test]
    fn failed_init_requests_helper_and_holds_edge_until_release() {
        let input = FakeInput::default();
        input.set(KeyCode::new(0xBB), true);
        let backend = FakeBackend::default();
        let backend_enabled = AtomicBool::new(false);
        let prompt = AtomicBool::new(false);
        let mut state = ControllerState::default();
        let settings = LagSwitchControllerSettings {
            section_enabled: true,
            ..LagSwitchControllerSettings::default()
        };

        process_step(&settings, &input, &backend, &backend_enabled, &prompt, &mut state);
        assert!(prompt.load(Ordering::Acquire));
        assert!(state.was_pressed);
        backend.available.store(true, Ordering::Release);
        backend.init_success.store(true, Ordering::Release);
        process_step(&settings, &input, &backend, &backend_enabled, &prompt, &mut state);
        assert!(!backend.is_base_blocking_active());
        input.set(KeyCode::new(0xBB), false);
        process_step(&settings, &input, &backend, &backend_enabled, &prompt, &mut state);
        input.set(KeyCode::new(0xBB), true);
        process_step(&settings, &input, &backend, &backend_enabled, &prompt, &mut state);
        assert!(backend.is_base_blocking_active());
    }

    #[test]
    fn wheel_hotkey_consumes_transient_input_once() {
        let input = FakeInput::default();
        *lock(&input.transient) = Some(KeyCode::MOUSE_WHEEL_UP);
        assert!(crate::runtime_input::hotkey_pressed(
            &input,
            Hotkey::new(KeyCode::MOUSE_WHEEL_UP.raw())
        ));
        assert!(!crate::runtime_input::hotkey_pressed(
            &input,
            Hotkey::new(KeyCode::MOUSE_WHEEL_UP.raw())
        ));
    }
}
