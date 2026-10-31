//! Live Windows Anti-AFK controller.
//!
//! Timer decisions stay on a dedicated 2 ms controller lane while the characterized foreground/
//! click/key/restore sequence runs on one owned worker, matching the C++ runtime topology.

use smu_core::Hotkey;
use smu_display::DisplayCapture;
use smu_input::InputDevice;
use smu_process::windows::{find_newest_process_window, foreground_window};
use smu_process::{ProcessDiscovery, ProcessForeground};
use smu_runtime::anti_afk::evaluate_anti_afk_timer_step;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);
const KEY_PRESS_MS: Duration = Duration::from_millis(20);
const CENTER_CLICK_MS: Duration = Duration::from_millis(50);
const CENTER_RESTORE_MS: Duration = Duration::from_millis(100);
const BACKGROUND_FOCUS_SETTLE_MS: Duration = Duration::from_millis(500);
const BACKGROUND_KEY_SETTLE_MS: Duration = Duration::from_millis(200);
const ORIGINAL_TOPMOST_SETTLE_MS: Duration = Duration::from_millis(50);
const ORIGINAL_FOREGROUND_SETTLE_MS: Duration = Duration::from_millis(1000);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AntiAfkControllerSettings {
    pub enabled: bool,
    pub process_selector: String,
    pub take_all_process_ids: bool,
    pub afk_key: Hotkey,
    pub minutes: i32,
    pub double_press: bool,
}

impl Default for AntiAfkControllerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            process_selector: "RobloxPlayerBeta.exe".to_owned(),
            take_all_process_ids: false,
            afk_key: Hotkey::new(0xDC),
            minutes: 15,
            double_press: true,
        }
    }
}

struct ControllerState {
    armed: bool,
    last_keyboard_serial_seen: u64,
    last_activity: Instant,
    targets: crate::target_runtime::TargetProcessCache,
    action_worker: Option<JoinHandle<()>>,
}

impl Default for ControllerState {
    fn default() -> Self {
        Self {
            armed: false,
            last_keyboard_serial_seen: 0,
            last_activity: Instant::now(),
            targets: crate::target_runtime::TargetProcessCache::default(),
            action_worker: None,
        }
    }
}

pub struct AntiAfkController {
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<AntiAfkControllerSettings>>,
    controller: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for AntiAfkController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AntiAfkController")
            .field("running", &self.running.load(Ordering::Acquire))
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

impl AntiAfkController {
    #[must_use]
    pub fn start(
        input: Arc<dyn InputDevice>,
        display: Arc<dyn DisplayCapture>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        initial_settings: AntiAfkControllerSettings,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let settings = Arc::new(Mutex::new(initial_settings));
        let controller_running = Arc::clone(&running);
        let controller_settings = Arc::clone(&settings);
        let controller = std::thread::spawn(move || {
            controller_loop(
                controller_running.as_ref(),
                &controller_settings,
                &input,
                &display,
                &discovery,
                &foreground,
            );
        });
        Self { running, settings, controller: Mutex::new(Some(controller)) }
    }

    pub fn update_settings(&self, settings: AntiAfkControllerSettings) {
        *lock(&self.settings) = settings;
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        let controller = { lock(&self.controller).take() };
        if let Some(controller) = controller {
            let _ = controller.join();
        }
    }
}

impl Drop for AntiAfkController {
    fn drop(&mut self) {
        self.stop();
    }
}

const fn reset_timer_state(state: &mut ControllerState) {
    state.armed = false;
    state.last_keyboard_serial_seen = 0;
}

fn join_completed_action(state: &mut ControllerState, now: Instant) -> bool {
    let Some(worker) = state.action_worker.as_ref() else {
        return false;
    };
    if !worker.is_finished() {
        return true;
    }
    if let Some(worker) = state.action_worker.take() {
        let _ = worker.join();
    }
    state.last_activity = now;
    false
}

fn controller_loop(
    running: &AtomicBool,
    settings: &Arc<Mutex<AntiAfkControllerSettings>>,
    input: &Arc<dyn InputDevice>,
    display: &Arc<dyn DisplayCapture>,
    discovery: &Arc<dyn ProcessDiscovery>,
    foreground: &Arc<dyn ProcessForeground>,
) {
    let mut state = ControllerState::default();
    while running.load(Ordering::Acquire) {
        let now = Instant::now();
        if join_completed_action(&mut state, now) {
            std::thread::sleep(CONTROLLER_POLL_INTERVAL);
            continue;
        }

        let current = lock(settings).clone();
        state.targets.refresh_if_due(
            &current.process_selector,
            current.take_all_process_ids,
            discovery.as_ref(),
        );
        let pids = state.targets.selected_pids().to_vec();
        if !current.enabled || pids.is_empty() {
            reset_timer_state(&mut state);
            std::thread::sleep(CONTROLLER_POLL_INTERVAL);
            continue;
        }

        let target_foreground =
            pids.iter().copied().any(|pid| foreground.is_foreground_process(pid));
        let keyboard_serial = smu_input::windows::real_keyboard_activity_serial();
        let timer = evaluate_anti_afk_timer_step(
            state.armed,
            state.last_keyboard_serial_seen,
            keyboard_serial,
            target_foreground,
            current.minutes,
            now.saturating_duration_since(state.last_activity),
        );
        state.armed = timer.armed;
        state.last_keyboard_serial_seen = timer.last_keyboard_serial_seen;
        if timer.reset_last_activity_to_now {
            state.last_activity = now;
        }
        if !timer.due {
            std::thread::sleep(CONTROLLER_POLL_INTERVAL);
            continue;
        }
        if !current.afk_key.is_bound() {
            state.last_activity = now;
            std::thread::sleep(CONTROLLER_POLL_INTERVAL);
            continue;
        }

        if target_foreground {
            state.last_keyboard_serial_seen = keyboard_serial;
        }
        let worker_input = Arc::clone(input);
        let worker_display = Arc::clone(display);
        let worker_settings = Arc::clone(settings);
        let afk_key = current.afk_key;
        state.action_worker = Some(std::thread::spawn(move || {
            run_anti_afk_action(
                worker_input.as_ref(),
                worker_display.as_ref(),
                &worker_settings,
                &pids,
                afk_key,
                target_foreground,
            );
        }));
        std::thread::sleep(CONTROLLER_POLL_INTERVAL);
    }

    if let Some(worker) = state.action_worker.take() {
        let _ = worker.join();
    }
}

fn press_anti_afk_key(
    input: &dyn InputDevice,
    settings: &Mutex<AntiAfkControllerSettings>,
    afk_key: Hotkey,
) {
    let double_press = lock(settings).double_press;
    input.hold_key_chord(afk_key);
    std::thread::sleep(KEY_PRESS_MS);
    input.release_key_chord(afk_key);
    if double_press {
        std::thread::sleep(KEY_PRESS_MS);
        input.hold_key_chord(afk_key);
        std::thread::sleep(KEY_PRESS_MS);
        input.release_key_chord(afk_key);
    }
}

fn tap_window_center(
    display: &dyn DisplayCapture,
    window: smu_process::windows::WindowsWindowHandle,
) -> bool {
    let Some(rect) = window.rect() else {
        return false;
    };
    let original_mouse = display.cursor_position();
    let center_x = rect.left + (rect.right - rect.left) / 2;
    let center_y = rect.top + (rect.bottom - rect.top) / 2;
    let _ = smu_input::windows::set_cursor_position_direct(center_x, center_y);
    smu_input::windows::set_left_mouse_button_direct(true);
    std::thread::sleep(CENTER_CLICK_MS);
    smu_input::windows::set_left_mouse_button_direct(false);
    if let Some(original_mouse) = original_mouse {
        std::thread::sleep(CENTER_RESTORE_MS);
        let _ = smu_input::windows::set_cursor_position_direct(original_mouse.x, original_mouse.y);
    }
    true
}

fn run_anti_afk_action(
    input: &dyn InputDevice,
    display: &dyn DisplayCapture,
    settings: &Mutex<AntiAfkControllerSettings>,
    pids: &[u32],
    afk_key: Hotkey,
    target_foreground: bool,
) {
    if target_foreground {
        press_anti_afk_key(input, settings, afk_key);
        return;
    }

    let Some(window) = find_newest_process_window(pids) else {
        return;
    };
    let original_foreground = foreground_window();
    let original_mouse = display.cursor_position();

    window.show_restore();
    window.set_topmost(true);
    window.set_topmost(false);
    let _ = tap_window_center(display, window);
    std::thread::sleep(BACKGROUND_FOCUS_SETTLE_MS);
    press_anti_afk_key(input, settings, afk_key);
    std::thread::sleep(BACKGROUND_KEY_SETTLE_MS);

    if let Some(original_foreground) = original_foreground {
        original_foreground.set_topmost(true);
        original_foreground.set_topmost(false);
        std::thread::sleep(ORIGINAL_TOPMOST_SETTLE_MS);
        original_foreground.set_foreground();
        std::thread::sleep(ORIGINAL_FOREGROUND_SETTLE_MS);
    }
    if let Some(original_mouse) = original_mouse {
        let _ = smu_input::windows::set_cursor_position_direct(original_mouse.x, original_mouse.y);
    }
}
