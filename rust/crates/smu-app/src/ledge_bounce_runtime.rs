//! Live Linux Ledge Bounce rising-edge controller and one-shot workers.
//!
//! Workers may overlap. Sensitivity, Cam-Fix, and side are read at worker start. Realign is re-read
//! after the fixed 146 ms prefix, while auto-hold is re-read at 146 ms without realign or 216 ms with
//! realign, matching the characterized C++ callback read points.

use smu_core::{Hotkey, KeyCode};
use smu_input::InputDevice;
use smu_process::{ProcessDiscovery, ProcessForeground};
use smu_runtime::macros::ledge_bounce_turn90;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);
pub const DEFAULT_LEDGE_BOUNCE_HOTKEY: Hotkey = Hotkey::new(0x43);

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LedgeBounceControllerSettings {
    pub macro_enabled: bool,
    pub section_enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub sensitivity: String,
    pub cam_fix: bool,
    pub side_left: bool,
    pub realign: bool,
    pub auto_hold: bool,
    pub process_selector: String,
}

impl Default for LedgeBounceControllerSettings {
    fn default() -> Self {
        Self {
            macro_enabled: true,
            section_enabled: false,
            hotkey: DEFAULT_LEDGE_BOUNCE_HOTKEY,
            disable_outside_target: true,
            sensitivity: "0.5".to_owned(),
            cam_fix: false,
            side_left: false,
            realign: true,
            auto_hold: true,
            process_selector: "sober".to_owned(),
        }
    }
}

pub struct LedgeBounceController {
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<LedgeBounceControllerSettings>>,
    controller: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for LedgeBounceController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LedgeBounceController")
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

impl LedgeBounceController {
    #[must_use]
    pub fn start(
        input: Arc<dyn InputDevice>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        initial_settings: LedgeBounceControllerSettings,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let settings = Arc::new(Mutex::new(initial_settings));
        let worker_running = Arc::clone(&running);
        let worker_settings = Arc::clone(&settings);
        let controller = std::thread::spawn(move || {
            controller_loop(
                worker_running.as_ref(),
                &worker_settings,
                &input,
                &discovery,
                &foreground,
            );
        });
        Self { running, settings, controller: Mutex::new(Some(controller)) }
    }

    pub fn update_settings(&self, settings: LedgeBounceControllerSettings) {
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
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        let controller = lock(&self.controller).take();
        if let Some(controller) = controller {
            controller.thread().unpark();
            let _ = controller.join();
        }
    }
}

impl Drop for LedgeBounceController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn controller_loop(
    running: &AtomicBool,
    settings: &Arc<Mutex<LedgeBounceControllerSettings>>,
    input: &Arc<dyn InputDevice>,
    discovery: &Arc<dyn ProcessDiscovery>,
    foreground: &Arc<dyn ProcessForeground>,
) {
    let mut targets = crate::target_runtime::TargetProcessCache::default();
    let mut was_pressed = false;
    let mut workers = Vec::<JoinHandle<()>>::new();
    while running.load(Ordering::Acquire) {
        prune_finished_workers(&mut workers);
        let current = lock(settings).clone();
        targets.refresh_if_due(&current.process_selector, false, discovery.as_ref());
        if !current.macro_enabled {
            std::thread::park_timeout(Duration::from_secs(1));
            continue;
        }
        let foreground_allowed =
            targets.foreground_allowed(current.disable_outside_target, foreground.as_ref());
        let pressed = foreground_allowed
            && current.section_enabled
            && crate::runtime_input::hotkey_pressed(input.as_ref(), current.hotkey);
        let edge = pressed && !was_pressed;
        was_pressed = pressed;
        if edge {
            let worker_settings = Arc::clone(settings);
            let worker_input = Arc::clone(input);
            workers.push(std::thread::spawn(move || {
                execute_worker(worker_settings.as_ref(), worker_input.as_ref(), std::thread::sleep);
            }));
        }
        if current.macro_enabled && current.section_enabled {
            std::thread::sleep(CONTROLLER_POLL_INTERVAL);
        } else {
            // No hotkeys can fire while disabled. Avoid a 2 ms polling loop entirely.
            // update_settings unparks this thread immediately upon any settings change.
            std::thread::park_timeout(Duration::from_secs(1));
        }
    }
    for worker in workers {
        let _ = worker.join();
    }
}

fn prune_finished_workers(workers: &mut Vec<JoinHandle<()>>) {
    let mut index = 0;
    while index < workers.len() {
        if workers[index].is_finished() {
            let worker = workers.remove(index);
            let _ = worker.join();
        } else {
            index += 1;
        }
    }
}

fn execute_worker(
    settings: &Mutex<LedgeBounceControllerSettings>,
    input: &dyn InputDevice,
    mut sleep: impl FnMut(Duration),
) {
    let (sensitivity, cam_fix, side_left) = {
        let current = lock(settings);
        (current.sensitivity.clone(), current.cam_fix, current.side_left)
    };
    let mut turn90 = ledge_bounce_turn90(&sensitivity, cam_fix);
    let side_key = if side_left {
        turn90 = turn90.wrapping_neg();
        KeyCode::A
    } else {
        KeyCode::new(0x44)
    };

    input.move_mouse(turn90.wrapping_neg(), 0);
    sleep(Duration::from_millis(90));
    input.hold_key(KeyCode::new(0x53), false);
    sleep(Duration::from_millis(40));
    input.release_key(KeyCode::new(0x53), false);
    input.move_mouse(turn90, 0);
    input.hold_key(side_key, false);
    sleep(Duration::from_millis(16));

    let realign = lock(settings).realign;
    input.release_key(side_key, false);
    if realign {
        input.hold_key(KeyCode::new(0x57), false);
        input.move_mouse(turn90, 0);
        sleep(Duration::from_millis(70));
        input.release_key(KeyCode::new(0x57), false);
        input.move_mouse(turn90.wrapping_neg(), 0);
        if lock(settings).auto_hold {
            input.hold_key(side_key, false);
        }
    } else {
        if lock(settings).auto_hold {
            input.hold_key(KeyCode::new(0x57), false);
        }
        input.move_mouse(turn90, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use smu_input::{InputInjector, KeyState};

    #[derive(Debug, Default)]
    struct FakeInput {
        events: Mutex<Vec<String>>,
    }

    impl KeyState for FakeInput {
        fn is_pressed(&self, _key: KeyCode) -> bool {
            false
        }
    }

    impl InputInjector for FakeInput {
        fn hold_key(&self, key: KeyCode, _extended: bool) {
            lock(&self.events).push(format!("down:{}", key.raw()));
        }
        fn release_key(&self, key: KeyCode, _extended: bool) {
            lock(&self.events).push(format!("up:{}", key.raw()));
        }
        fn move_mouse(&self, dx: i32, dy: i32) {
            lock(&self.events).push(format!("move:{dx}:{dy}"));
        }
        fn mouse_wheel(&self, _delta: i32) {}
    }

    #[test]
    fn defaults_match_cpp_ledge_bounce_runtime() {
        let settings = LedgeBounceControllerSettings::default();
        assert!(!settings.section_enabled);
        assert_eq!(settings.hotkey, DEFAULT_LEDGE_BOUNCE_HOTKEY);
        assert!(settings.disable_outside_target);
        assert_eq!(settings.sensitivity, "0.5");
        assert!(!settings.cam_fix);
        assert!(!settings.side_left);
        assert!(settings.realign);
        assert!(settings.auto_hold);
    }

    #[test]
    fn worker_rereads_realign_and_auto_hold_at_legacy_points() {
        let settings = Mutex::new(LedgeBounceControllerSettings::default());
        let input = FakeInput::default();
        let mut elapsed = Duration::ZERO;
        execute_worker(&settings, &input, |duration| {
            elapsed += duration;
            if elapsed == Duration::from_millis(146) {
                let mut current = lock(&settings);
                current.realign = false;
                current.auto_hold = false;
            }
        });
        assert_eq!(elapsed, Duration::from_millis(146));
        assert_eq!(
            *lock(&input.events),
            ["move:-360:0", "down:83", "up:83", "move:360:0", "down:68", "up:68", "move:360:0"]
        );
    }

    #[test]
    fn realign_path_reaches_216ms_and_uses_late_auto_hold_value() {
        let settings = Mutex::new(LedgeBounceControllerSettings::default());
        let input = FakeInput::default();
        let mut elapsed = Duration::ZERO;
        execute_worker(&settings, &input, |duration| {
            elapsed += duration;
            if elapsed == Duration::from_millis(216) {
                lock(&settings).auto_hold = false;
            }
        });
        assert_eq!(elapsed, Duration::from_millis(216));
        assert_eq!(lock(&input.events).last().map(String::as_str), Some("move:-360:0"));
    }
}
