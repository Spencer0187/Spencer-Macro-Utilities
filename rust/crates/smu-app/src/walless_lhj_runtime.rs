//! Live Linux Walless LHJ controller and one-shot workers.
//!
//! The controller owns the F6 rising edge and snapshots target PIDs before spawning. Each worker
//! deliberately re-reads side/zoom/shift settings at the same delayed points as the C++ worker.

use smu_core::{Hotkey, KeyCode};
use smu_input::InputDevice;
use smu_process::{ProcessControl, ProcessDiscovery, ProcessForeground};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);
pub const DEFAULT_WALLESS_LHJ_HOTKEY: Hotkey = Hotkey::new(0x75);

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WallessLhjControllerSettings {
    pub macro_enabled: bool,
    pub section_enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub side_left: bool,
    pub global_zoom: bool,
    pub shift_key: u32,
    pub zoom_reverse: bool,
    pub take_all_process_ids: bool,
    pub process_selector: String,
}

impl Default for WallessLhjControllerSettings {
    fn default() -> Self {
        Self {
            macro_enabled: true,
            section_enabled: false,
            hotkey: DEFAULT_WALLESS_LHJ_HOTKEY,
            disable_outside_target: false,
            side_left: false,
            global_zoom: false,
            shift_key: 0x10,
            zoom_reverse: false,
            take_all_process_ids: false,
            process_selector: "sober".to_owned(),
        }
    }
}

pub struct WallessLhjController {
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<WallessLhjControllerSettings>>,
    controller: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for WallessLhjController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WallessLhjController")
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

impl WallessLhjController {
    #[must_use]
    pub fn start(
        input: Arc<dyn InputDevice>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        control: Arc<dyn ProcessControl>,
        initial_settings: WallessLhjControllerSettings,
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
                &control,
            );
        });
        Self { running, settings, controller: Mutex::new(Some(controller)) }
    }

    pub fn update_settings(&self, settings: WallessLhjControllerSettings) {
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

impl Drop for WallessLhjController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn controller_loop(
    running: &AtomicBool,
    settings: &Arc<Mutex<WallessLhjControllerSettings>>,
    input: &Arc<dyn InputDevice>,
    discovery: &Arc<dyn ProcessDiscovery>,
    foreground: &Arc<dyn ProcessForeground>,
    control: &Arc<dyn ProcessControl>,
) {
    let mut targets = crate::target_runtime::TargetProcessCache::default();
    let mut was_pressed = false;
    let mut workers: Vec<JoinHandle<()>> = Vec::new();
    while running.load(Ordering::Acquire) {
        prune_finished_workers(&mut workers);
        let current = lock(settings).clone();
        targets.refresh_if_due(
            &current.process_selector,
            current.take_all_process_ids,
            discovery.as_ref(),
        );
        if !current.macro_enabled {
            // The monolithic C++ controller skips this macro entirely while globally disabled;
            // preserve `was_pressed` rather than manufacturing a new edge on resume.
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        let foreground_allowed =
            targets.foreground_allowed(current.disable_outside_target, foreground.as_ref());
        let pressed = foreground_allowed
            && current.section_enabled
            && crate::runtime_input::hotkey_pressed(input.as_ref(), current.hotkey);
        let rising = pressed && !was_pressed;
        was_pressed = pressed;
        if rising {
            if targets.selected_pids().is_empty() {
                targets.force_refresh(
                    &current.process_selector,
                    current.take_all_process_ids,
                    discovery.as_ref(),
                );
            }
            let pids = targets.selected_pids().to_vec();
            let settings = Arc::clone(settings);
            let input = Arc::clone(input);
            let control = Arc::clone(control);
            workers.push(std::thread::spawn(move || {
                execute_worker(
                    &settings,
                    input.as_ref(),
                    control.as_ref(),
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
    settings: &Mutex<WallessLhjControllerSettings>,
    input: &dyn InputDevice,
    control: &dyn ProcessControl,
    pids: &[u32],
    mut sleep: impl FnMut(Duration),
) {
    let side_left = lock(settings).side_left;
    let side_key = if side_left { KeyCode::A } else { KeyCode::new(0x44) };
    input.hold_key(side_key, false);
    sleep(Duration::from_millis(15));
    input.hold_key(KeyCode::SPACE, false);
    sleep(Duration::from_millis(30));

    set_pids_suspended(control, pids, true);
    sleep(Duration::from_millis(50));
    input.release_key(side_key, false);
    sleep(Duration::from_millis(500));

    let global_zoom_action = lock(settings).global_zoom;
    if global_zoom_action {
        let reverse = lock(settings).zoom_reverse;
        let wheel = if reverse { KeyCode::MOUSE_WHEEL_DOWN } else { KeyCode::MOUSE_WHEEL_UP };
        input.hold_key_chord(Hotkey::new(wheel.raw()));
    } else {
        let shift_key = lock(settings).shift_key;
        input.hold_key_chord(Hotkey::new(shift_key));
    }

    set_pids_suspended(control, pids, false);
    sleep(Duration::from_millis(50));
    if !lock(settings).global_zoom {
        let shift_key = lock(settings).shift_key;
        input.release_key_chord(Hotkey::new(shift_key));
    }
    input.release_key(KeyCode::SPACE, false);
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
    fn defaults_match_cpp_walless_lhj_runtime() {
        let settings = WallessLhjControllerSettings::default();
        assert!(settings.macro_enabled);
        assert!(!settings.section_enabled);
        assert_eq!(settings.hotkey, DEFAULT_WALLESS_LHJ_HOTKEY);
        assert!(!settings.disable_outside_target);
        assert!(!settings.side_left);
        assert!(!settings.global_zoom);
        assert_eq!(settings.shift_key, 0x10);
        assert!(!settings.zoom_reverse);
        assert!(!settings.take_all_process_ids);
    }

    #[test]
    fn worker_preserves_645ms_sequence_and_delayed_live_reads() {
        let settings = Mutex::new(WallessLhjControllerSettings::default());
        let input = FakeInput::default();
        let control = FakeControl::default();
        let mut elapsed = Duration::ZERO;
        execute_worker(&settings, &input, &control, &[10, 20], |duration| {
            elapsed += duration;
            if elapsed == Duration::from_millis(595) {
                let mut current = lock(&settings);
                current.global_zoom = true;
                current.zoom_reverse = true;
            }
            if elapsed == Duration::from_millis(645) {
                lock(&settings).global_zoom = false;
            }
        });
        assert_eq!(elapsed, Duration::from_millis(645));
        assert_eq!(*lock(&control.events), ["suspend:10", "suspend:20", "resume:10", "resume:20"]);
        assert_eq!(
            *lock(&input.events),
            ["down:68", "down:32", "up:68", "wheel:-1", "up:16", "up:32"]
        );
    }
}
