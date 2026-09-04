//! Live Linux Speedglitch controller and motion worker.
//!
//! The 2 ms controller owns edge/toggle/foreground state. A separate long-lived worker owns camera
//! motion. Each cycle snapshots FPS once, reads X immediately before the first move, then deliberately
//! re-reads Y only after the first frame wait, matching the characterized C++ worker.

use smu_core::Hotkey;
use smu_input::InputDevice;
use smu_process::{ProcessDiscovery, ProcessForeground};
use smu_runtime::macros::{
    apply_speedglitch_state_action, evaluate_speedglitch_edge, frame_delays_for_fps,
};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);
const IDLE_WORKER_POLL_INTERVAL: Duration = Duration::from_millis(2);
pub const DEFAULT_SPEEDGLITCH_HOTKEY: Hotkey = Hotkey::new(0x58);

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpeedglitchControllerSettings {
    pub macro_enabled: bool,
    pub section_enabled: bool,
    pub hotkey: Hotkey,
    pub hold_mode: bool,
    pub fps: u32,
    pub strength_x: i32,
    pub strength_y: i32,
    pub disable_outside_target: bool,
    pub process_selector: String,
}

impl Default for SpeedglitchControllerSettings {
    fn default() -> Self {
        Self {
            macro_enabled: true,
            section_enabled: false,
            hotkey: DEFAULT_SPEEDGLITCH_HOTKEY,
            hold_mode: false,
            fps: 120,
            strength_x: 959,
            strength_y: -959,
            disable_outside_target: true,
            process_selector: "sober".to_owned(),
        }
    }
}

pub struct SpeedglitchController {
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<SpeedglitchControllerSettings>>,
    worker_allowed: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    controller: Mutex<Option<JoinHandle<()>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for SpeedglitchController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpeedglitchController")
            .field("running", &self.running.load(Ordering::Acquire))
            .field("worker_allowed", &self.worker_allowed.load(Ordering::Acquire))
            .field("active", &self.active.load(Ordering::Acquire))
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

impl SpeedglitchController {
    #[must_use]
    pub fn start(
        input: Arc<dyn InputDevice>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        initial_settings: SpeedglitchControllerSettings,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let settings = Arc::new(Mutex::new(initial_settings));
        let worker_allowed = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(false));

        let controller_running = Arc::clone(&running);
        let controller_settings = Arc::clone(&settings);
        let controller_allowed = Arc::clone(&worker_allowed);
        let controller_active = Arc::clone(&active);
        let controller_input = Arc::clone(&input);
        let controller = std::thread::spawn(move || {
            controller_loop(
                controller_running.as_ref(),
                controller_settings.as_ref(),
                controller_allowed.as_ref(),
                controller_active.as_ref(),
                controller_input.as_ref(),
                discovery.as_ref(),
                foreground.as_ref(),
            );
        });

        let worker_running = Arc::clone(&running);
        let worker_settings = Arc::clone(&settings);
        let worker_allowed_clone = Arc::clone(&worker_allowed);
        let worker_active = Arc::clone(&active);
        let worker = std::thread::spawn(move || {
            worker_loop(
                worker_running.as_ref(),
                worker_settings.as_ref(),
                worker_allowed_clone.as_ref(),
                worker_active.as_ref(),
                input.as_ref(),
            );
        });

        Self {
            running,
            settings,
            worker_allowed,
            active,
            controller: Mutex::new(Some(controller)),
            worker: Mutex::new(Some(worker)),
        }
    }

    pub fn update_settings(&self, settings: SpeedglitchControllerSettings) {
        *lock(&self.settings) = settings;
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        self.worker_allowed.store(false, Ordering::Release);
        let controller = lock(&self.controller).take();
        if let Some(controller) = controller {
            let _ = controller.join();
        }
        let worker = lock(&self.worker).take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

impl Drop for SpeedglitchController {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn controller_loop(
    running: &AtomicBool,
    settings: &Mutex<SpeedglitchControllerSettings>,
    worker_allowed: &AtomicBool,
    active: &AtomicBool,
    input: &dyn InputDevice,
    discovery: &dyn ProcessDiscovery,
    foreground: &dyn ProcessForeground,
) {
    let mut targets = crate::target_runtime::TargetProcessCache::default();
    let mut was_pressed = false;
    while running.load(Ordering::Acquire) {
        let current = lock(settings).clone();
        targets.refresh_if_due(&current.process_selector, false, discovery);
        if !current.macro_enabled {
            worker_allowed.store(false, Ordering::Release);
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }

        let foreground_allowed =
            targets.foreground_allowed(current.disable_outside_target, foreground);
        let section_enabled = foreground_allowed && current.section_enabled;
        worker_allowed.store(section_enabled, Ordering::Release);
        let hotkey_pressed =
            section_enabled && crate::runtime_input::hotkey_pressed(input, current.hotkey);
        let toggle_switch = !hotkey_pressed && current.hold_mode;
        let edge = evaluate_speedglitch_edge(
            foreground_allowed,
            section_enabled,
            hotkey_pressed,
            toggle_switch,
            was_pressed,
        );
        let previous_active = active.load(Ordering::Acquire);
        let next_active = apply_speedglitch_state_action(edge.state_action, previous_active);
        active.store(next_active, Ordering::Release);
        was_pressed = edge.next_was_pressed;
        std::thread::sleep(CONTROLLER_POLL_INTERVAL);
    }
    worker_allowed.store(false, Ordering::Release);
}

fn worker_loop(
    running: &AtomicBool,
    settings: &Mutex<SpeedglitchControllerSettings>,
    worker_allowed: &AtomicBool,
    active: &AtomicBool,
    input: &dyn InputDevice,
) {
    while running.load(Ordering::Acquire) {
        let current = lock(settings).clone();
        if current.macro_enabled
            && worker_allowed.load(Ordering::Acquire)
            && current.section_enabled
            && active.load(Ordering::Acquire)
        {
            execute_cycle(settings, input, std::thread::sleep);
        } else {
            std::thread::sleep(IDLE_WORKER_POLL_INTERVAL);
        }
    }
}

fn execute_cycle(
    settings: &Mutex<SpeedglitchControllerSettings>,
    input: &dyn InputDevice,
    mut sleep: impl FnMut(Duration),
) {
    let fps = lock(settings).fps;
    let delays = frame_delays_for_fps(fps);
    let first = lock(settings).strength_x;
    input.move_mouse(first, 0);
    sleep(Duration::from_millis(u64::try_from(delays.first_ms).unwrap_or_default()));
    let second = lock(settings).strength_y;
    input.move_mouse(second, 0);
    sleep(Duration::from_millis(u64::try_from(delays.second_ms).unwrap_or_default()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use smu_core::KeyCode;
    use smu_input::{InputInjector, KeyState};
    use smu_runtime::macros::SpeedglitchStateAction;

    #[derive(Debug, Default)]
    struct FakeInput {
        moves: Mutex<Vec<(i32, i32)>>,
    }

    impl KeyState for FakeInput {
        fn is_pressed(&self, _key: KeyCode) -> bool {
            false
        }
    }

    impl InputInjector for FakeInput {
        fn hold_key(&self, _key: KeyCode, _extended: bool) {}
        fn release_key(&self, _key: KeyCode, _extended: bool) {}
        fn move_mouse(&self, dx: i32, dy: i32) {
            lock(&self.moves).push((dx, dy));
        }
        fn mouse_wheel(&self, _delta: i32) {}
    }

    #[test]
    fn defaults_match_cpp_pre_render_speedglitch_runtime() {
        let settings = SpeedglitchControllerSettings::default();
        assert!(settings.macro_enabled);
        assert!(!settings.section_enabled);
        assert_eq!(settings.hotkey, DEFAULT_SPEEDGLITCH_HOTKEY);
        assert!(!settings.hold_mode);
        assert_eq!(settings.fps, 120);
        assert_eq!(settings.strength_x, 959);
        assert_eq!(settings.strength_y, -959);
        assert!(settings.disable_outside_target);
    }

    #[test]
    fn worker_cycle_reads_second_strength_after_first_wait() {
        let settings = Mutex::new(SpeedglitchControllerSettings {
            fps: 200,
            strength_x: 10,
            strength_y: -20,
            ..SpeedglitchControllerSettings::default()
        });
        let input = FakeInput::default();
        let mut waits = 0_u32;
        execute_cycle(&settings, &input, |_| {
            waits += 1;
            if waits == 1 {
                lock(&settings).strength_y = -30;
            }
        });
        assert_eq!(*lock(&input.moves), [(10, 0), (-30, 0)]);
        assert_eq!(waits, 2);
    }

    #[test]
    fn hold_mode_clear_matches_characterized_state_action() {
        let edge = evaluate_speedglitch_edge(true, true, false, true, true);
        assert_eq!(edge.state_action, SpeedglitchStateAction::Clear);
        assert!(!apply_speedglitch_state_action(edge.state_action, true));
    }
}
