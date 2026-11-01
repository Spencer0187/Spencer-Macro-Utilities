//! Live Linux Wall-Walk controller and motion worker.

use smu_core::Hotkey;
use smu_input::InputDevice;
use smu_process::{ProcessDiscovery, ProcessForeground};
use smu_runtime::macros::{
    apply_toggle_loop_state_action, evaluate_toggle_loop_edge, wallwalk_frame_delay_ms,
};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);
const IDLE_WORKER_POLL_INTERVAL: Duration = Duration::from_millis(2);
pub const DEFAULT_WALL_WALK_HOTKEY: Hotkey = Hotkey::new(0x70);

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WallWalkControllerSettings {
    pub macro_enabled: bool,
    pub section_enabled: bool,
    pub hotkey: Hotkey,
    pub hold_mode: bool,
    pub fps: u32,
    pub left_flick: bool,
    pub strength_x: i32,
    pub strength_y: i32,
    pub tail_delay_us: i32,
    pub disable_outside_target: bool,
    pub process_selector: String,
}

impl Default for WallWalkControllerSettings {
    fn default() -> Self {
        Self {
            macro_enabled: true,
            section_enabled: false,
            hotkey: DEFAULT_WALL_WALK_HOTKEY,
            hold_mode: false,
            fps: 120,
            left_flick: false,
            strength_x: 94,
            strength_y: -94,
            tail_delay_us: 72_720,
            disable_outside_target: true,
            process_selector: "sober".to_owned(),
        }
    }
}

pub struct WallWalkController {
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<WallWalkControllerSettings>>,
    worker_allowed: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    controller: Mutex<Option<JoinHandle<()>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for WallWalkController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WallWalkController")
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

impl WallWalkController {
    #[must_use]
    pub fn start(
        input: Arc<dyn InputDevice>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        initial_settings: WallWalkControllerSettings,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let settings = Arc::new(Mutex::new(initial_settings));
        let worker_allowed = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicBool::new(false));

        let controller = {
            let running = Arc::clone(&running);
            let settings = Arc::clone(&settings);
            let allowed = Arc::clone(&worker_allowed);
            let active = Arc::clone(&active);
            let input = Arc::clone(&input);
            std::thread::spawn(move || {
                controller_loop(
                    running.as_ref(),
                    settings.as_ref(),
                    allowed.as_ref(),
                    active.as_ref(),
                    input.as_ref(),
                    discovery.as_ref(),
                    foreground.as_ref(),
                );
            })
        };
        let worker = {
            let running = Arc::clone(&running);
            let settings = Arc::clone(&settings);
            let allowed = Arc::clone(&worker_allowed);
            let active = Arc::clone(&active);
            std::thread::spawn(move || {
                worker_loop(
                    running.as_ref(),
                    settings.as_ref(),
                    allowed.as_ref(),
                    active.as_ref(),
                    input.as_ref(),
                );
            })
        };

        Self {
            running,
            settings,
            worker_allowed,
            active,
            controller: Mutex::new(Some(controller)),
            worker: Mutex::new(Some(worker)),
        }
    }

    pub fn update_settings(&self, settings: WallWalkControllerSettings) {
        let mut current = lock(&self.settings);
        if *current == settings {
            return;
        }
        *current = settings;
        drop(current);
        // Wake an inactive controller immediately when the user enables or changes its module.
        if let Some(handle) = lock(&self.controller).as_ref() {
            handle.thread().unpark();
        }
        if let Some(handle) = lock(&self.worker).as_ref() {
            handle.thread().unpark();
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        self.worker_allowed.store(false, Ordering::Release);
        let controller = lock(&self.controller).take();
        if let Some(controller) = controller {
            controller.thread().unpark();
            let _ = controller.join();
        }
        let worker = lock(&self.worker).take();
        if let Some(worker) = worker {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

impl Drop for WallWalkController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn cycle_prefix(settings: &Mutex<WallWalkControllerSettings>) -> (i32, i32, i32) {
    let fps = lock(settings).fps;
    let delay = wallwalk_frame_delay_ms(fps);
    let first = {
        let current = lock(settings);
        if current.left_flick { current.strength_x.wrapping_neg() } else { current.strength_x }
    };
    let second = {
        let current = lock(settings);
        if current.left_flick { current.strength_y.wrapping_neg() } else { current.strength_y }
    };
    (first, second, delay)
}

fn execute_cycle(
    settings: &Mutex<WallWalkControllerSettings>,
    input: &dyn InputDevice,
    mut sleep_frame: impl FnMut(Duration),
    mut sleep_tail: impl FnMut(Duration),
) {
    let (first, second, delay_ms) = cycle_prefix(settings);
    input.move_mouse(first, 0);
    sleep_frame(Duration::from_millis(u64::try_from(delay_ms).unwrap_or_default()));
    input.move_mouse(second, 0);
    let tail_delay_us = lock(settings).tail_delay_us.max(0);
    sleep_tail(Duration::from_micros(u64::try_from(tail_delay_us).unwrap_or_default()));
}

#[allow(clippy::too_many_arguments)]
fn controller_loop(
    running: &AtomicBool,
    settings: &Mutex<WallWalkControllerSettings>,
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
            std::thread::park_timeout(Duration::from_secs(1));
            continue;
        }
        let foreground_allowed =
            targets.foreground_allowed(current.disable_outside_target, foreground);
        let section_enabled = foreground_allowed && current.section_enabled;
        worker_allowed.store(section_enabled, Ordering::Release);
        let hotkey_pressed =
            section_enabled && crate::runtime_input::hotkey_pressed(input, current.hotkey);
        let toggle_switch = !hotkey_pressed && current.hold_mode;
        let edge = evaluate_toggle_loop_edge(
            foreground_allowed,
            section_enabled,
            hotkey_pressed,
            toggle_switch,
            was_pressed,
        );
        let next_active =
            apply_toggle_loop_state_action(edge.state_action, active.load(Ordering::Acquire));
        active.store(next_active, Ordering::Release);
        was_pressed = edge.next_was_pressed;
        if current.macro_enabled && current.section_enabled {
            std::thread::sleep(CONTROLLER_POLL_INTERVAL);
        } else {
            // No hotkeys can fire while disabled. Avoid a 2 ms polling loop entirely.
            // update_settings unparks this thread immediately upon any settings change.
            std::thread::park_timeout(Duration::from_secs(1));
        }
    }
    worker_allowed.store(false, Ordering::Release);
}

fn worker_loop(
    running: &AtomicBool,
    settings: &Mutex<WallWalkControllerSettings>,
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
            execute_cycle(settings, input, std::thread::sleep, std::thread::sleep);
        } else if !current.macro_enabled || !current.section_enabled {
            std::thread::park_timeout(Duration::from_secs(1));
        } else {
            std::thread::sleep(IDLE_WORKER_POLL_INTERVAL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smu_core::KeyCode;
    use smu_input::{InputInjector, KeyState};

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
    fn defaults_match_cpp_pre_render_wall_walk_runtime() {
        let settings = WallWalkControllerSettings::default();
        assert!(!settings.section_enabled);
        assert_eq!(settings.hotkey, DEFAULT_WALL_WALK_HOTKEY);
        assert_eq!(settings.fps, 120);
        assert!(!settings.left_flick);
        assert_eq!(settings.strength_x, 94);
        assert_eq!(settings.strength_y, -94);
        assert_eq!(settings.tail_delay_us, 72_720);
        assert!(!settings.hold_mode);
    }

    #[test]
    fn tail_delay_is_read_after_frame_wait_and_second_move() {
        let settings = Mutex::new(WallWalkControllerSettings {
            fps: 200,
            strength_x: 10,
            strength_y: -20,
            tail_delay_us: 30,
            ..WallWalkControllerSettings::default()
        });
        let input = FakeInput::default();
        let mut ms_waits = Vec::new();
        let mut us_waits = Vec::new();
        execute_cycle(
            &settings,
            &input,
            |duration| {
                ms_waits.push(duration);
                lock(&settings).tail_delay_us = 40;
            },
            |duration| us_waits.push(duration),
        );
        assert_eq!(*lock(&input.moves), [(10, 0), (-20, 0)]);
        assert_eq!(ms_waits, [Duration::from_millis(6)]);
        assert_eq!(us_waits, [Duration::from_micros(40)]);
    }
}
