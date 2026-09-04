//! Live Linux Floor Bounce rising-edge controller and one-shot workers.
//!
//! Workers snapshot only the target PID vector. The fixed Space/suspend prefix runs first; HHJ,
//! delays, and shift-key values are read later at the same points as the characterized C++ worker.

use smu_core::{Hotkey, KeyCode};
use smu_input::InputDevice;
use smu_process::{ProcessControl, ProcessDiscovery, ProcessForeground};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);
pub const DEFAULT_FLOOR_BOUNCE_HOTKEY: Hotkey = Hotkey::new(0x73);

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FloorBounceControllerSettings {
    pub macro_enabled: bool,
    pub section_enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub process_selector: String,
    pub take_all_process_ids: bool,
    pub hhj_enabled: bool,
    pub delay1_ms: i32,
    pub shift_key: u32,
    pub delay2_ms: i32,
    pub delay3_ms: i32,
}

impl Default for FloorBounceControllerSettings {
    fn default() -> Self {
        Self {
            macro_enabled: true,
            section_enabled: false,
            hotkey: DEFAULT_FLOOR_BOUNCE_HOTKEY,
            disable_outside_target: true,
            process_selector: "sober".to_owned(),
            take_all_process_ids: false,
            hhj_enabled: false,
            delay1_ms: 5,
            shift_key: 0x10,
            delay2_ms: 8,
            delay3_ms: 100,
        }
    }
}

pub struct FloorBounceController {
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<FloorBounceControllerSettings>>,
    shared_hhj_active: Arc<AtomicBool>,
    controller: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for FloorBounceController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FloorBounceController")
            .field("running", &self.running.load(Ordering::Acquire))
            .field("shared_hhj_active", &self.shared_hhj_active.load(Ordering::Acquire))
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

impl FloorBounceController {
    #[must_use]
    pub fn start(
        input: Arc<dyn InputDevice>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        control: Arc<dyn ProcessControl>,
        shared_hhj_active: Arc<AtomicBool>,
        initial_settings: FloorBounceControllerSettings,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let settings = Arc::new(Mutex::new(initial_settings));
        let worker_running = Arc::clone(&running);
        let worker_settings = Arc::clone(&settings);
        let worker_hhj_active = Arc::clone(&shared_hhj_active);
        let controller = std::thread::spawn(move || {
            controller_loop(
                worker_running.as_ref(),
                &worker_settings,
                &input,
                &discovery,
                &foreground,
                &control,
                &worker_hhj_active,
            );
        });
        Self { running, settings, shared_hhj_active, controller: Mutex::new(Some(controller)) }
    }

    pub fn update_settings(&self, settings: FloorBounceControllerSettings) {
        *lock(&self.settings) = settings;
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        let controller = lock(&self.controller).take();
        if let Some(controller) = controller {
            let _ = controller.join();
        }
    }
}

impl Drop for FloorBounceController {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn controller_loop(
    running: &AtomicBool,
    settings: &Arc<Mutex<FloorBounceControllerSettings>>,
    input: &Arc<dyn InputDevice>,
    discovery: &Arc<dyn ProcessDiscovery>,
    foreground: &Arc<dyn ProcessForeground>,
    control: &Arc<dyn ProcessControl>,
    shared_hhj_active: &Arc<AtomicBool>,
) {
    let mut targets = crate::target_runtime::TargetProcessCache::default();
    let mut was_pressed = false;
    let mut workers = Vec::<JoinHandle<()>>::new();
    while running.load(Ordering::Acquire) {
        prune_finished_workers(&mut workers);
        let current = lock(settings).clone();
        targets.refresh_if_due(
            &current.process_selector,
            current.take_all_process_ids,
            discovery.as_ref(),
        );
        if !current.macro_enabled {
            std::thread::sleep(Duration::from_millis(5));
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
            let pids = targets.selected_pids().to_vec();
            let settings = Arc::clone(settings);
            let input = Arc::clone(input);
            let control = Arc::clone(control);
            let shared_hhj_active = Arc::clone(shared_hhj_active);
            workers.push(std::thread::spawn(move || {
                execute_worker(
                    settings.as_ref(),
                    input.as_ref(),
                    control.as_ref(),
                    shared_hhj_active.as_ref(),
                    &pids,
                    std::thread::sleep,
                );
            }));
        }
        std::thread::sleep(CONTROLLER_POLL_INTERVAL);
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
    settings: &Mutex<FloorBounceControllerSettings>,
    input: &dyn InputDevice,
    control: &dyn ProcessControl,
    shared_hhj_active: &AtomicBool,
    pids: &[u32],
    mut sleep: impl FnMut(Duration),
) {
    input.hold_key(KeyCode::SPACE, false);
    sleep(Duration::from_millis(521));
    set_pids_suspended(control, pids, true);
    sleep(Duration::from_millis(72));
    set_pids_suspended(control, pids, false);
    sleep(Duration::from_millis(72));
    set_pids_suspended(control, pids, true);
    sleep(Duration::from_millis(72));
    set_pids_suspended(control, pids, false);
    input.release_key(KeyCode::SPACE, false);

    if !lock(settings).hhj_enabled {
        return;
    }

    let delay1_ms = lock(settings).delay1_ms;
    sleep_ms(delay1_ms, &mut sleep);
    let shift_key_action = lock(settings).shift_key;
    input.hold_key_chord(Hotkey::new(shift_key_action));
    let delay2_ms = lock(settings).delay2_ms;
    sleep_ms(delay2_ms, &mut sleep);
    shared_hhj_active.store(true, Ordering::Release);
    let delay3_ms = lock(settings).delay3_ms;
    sleep_ms(delay3_ms, &mut sleep);
    shared_hhj_active.store(false, Ordering::Release);
    sleep(Duration::from_millis(100));
    let shift_key_release = lock(settings).shift_key;
    input.release_key_chord(Hotkey::new(shift_key_release));
}

fn set_pids_suspended(control: &dyn ProcessControl, pids: &[u32], suspended: bool) {
    for &pid in pids {
        if suspended {
            let _ = control.suspend(pid);
        } else {
            let _ = control.resume(pid);
        }
    }
}

fn sleep_ms(value: i32, sleep: &mut impl FnMut(Duration)) {
    sleep(Duration::from_millis(u64::try_from(value.max(0)).unwrap_or_default()));
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
        fn move_mouse(&self, _dx: i32, _dy: i32) {}
        fn mouse_wheel(&self, delta: i32) {
            lock(&self.events).push(format!("wheel:{delta}"));
        }
    }

    #[derive(Debug, Default)]
    struct FakeControl {
        events: Mutex<Vec<String>>,
    }

    impl ProcessControl for FakeControl {
        fn suspend(&self, pid: u32) -> bool {
            lock(&self.events).push(format!("suspend:{pid}"));
            true
        }
        fn resume(&self, pid: u32) -> bool {
            lock(&self.events).push(format!("resume:{pid}"));
            true
        }
    }

    #[test]
    fn defaults_match_cpp_floor_bounce_runtime() {
        let settings = FloorBounceControllerSettings::default();
        assert!(!settings.section_enabled);
        assert_eq!(settings.hotkey, DEFAULT_FLOOR_BOUNCE_HOTKEY);
        assert!(settings.disable_outside_target);
        assert!(!settings.hhj_enabled);
        assert_eq!((settings.delay1_ms, settings.delay2_ms, settings.delay3_ms), (5, 8, 100));
        assert_eq!(settings.shift_key, 0x10);
    }

    #[test]
    fn fixed_prefix_is_737ms_and_hhj_flag_is_read_after_it() {
        let settings = Mutex::new(FloorBounceControllerSettings {
            hhj_enabled: true,
            ..FloorBounceControllerSettings::default()
        });
        let input = FakeInput::default();
        let control = FakeControl::default();
        let active = AtomicBool::new(false);
        let mut elapsed = Duration::ZERO;
        execute_worker(&settings, &input, &control, &active, &[10, 20], |duration| {
            elapsed += duration;
            if elapsed == Duration::from_millis(737) {
                lock(&settings).hhj_enabled = false;
            }
        });
        assert_eq!(elapsed, Duration::from_millis(737));
        assert_eq!(*lock(&input.events), ["down:32", "up:32"]);
        assert_eq!(
            *lock(&control.events),
            [
                "suspend:10",
                "suspend:20",
                "resume:10",
                "resume:20",
                "suspend:10",
                "suspend:20",
                "resume:10",
                "resume:20"
            ]
        );
        assert!(!active.load(Ordering::Acquire));
    }

    #[test]
    fn hhj_path_rereads_action_and_release_shift_keys() {
        let settings = Mutex::new(FloorBounceControllerSettings {
            hhj_enabled: true,
            delay1_ms: 5,
            shift_key: 0x41,
            delay2_ms: 8,
            delay3_ms: 100,
            ..FloorBounceControllerSettings::default()
        });
        let input = FakeInput::default();
        let control = FakeControl::default();
        let active = AtomicBool::new(false);
        let mut elapsed = Duration::ZERO;
        execute_worker(&settings, &input, &control, &active, &[7], |duration| {
            elapsed += duration;
            if elapsed == Duration::from_millis(742) {
                lock(&settings).shift_key = 0x42;
            }
            if elapsed == Duration::from_millis(950) {
                lock(&settings).shift_key = 0x43;
            }
            if elapsed == Duration::from_millis(850) {
                assert!(active.load(Ordering::Acquire));
            }
        });
        assert_eq!(elapsed, Duration::from_millis(950));
        assert_eq!(*lock(&input.events), ["down:32", "up:32", "down:66", "up:67"]);
        assert!(!active.load(Ordering::Acquire));
    }
}
