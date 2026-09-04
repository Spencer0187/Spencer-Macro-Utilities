//! Live Linux Bunnyhop controller and owned worker.
//!
//! The controller owns foreground eligibility, smart-chat lock, and physical trigger state. The
//! worker owns bound-key injection and timing. Linux physical key state comes only from real evdev
//! devices, so SMU's own uinput key-up cannot cancel a keyboard-triggered bunnyhop hold.

use smu_core::{Hotkey, KeyCode};
use smu_input::{HotkeyMatch, InputDevice, is_hotkey_pressed};
use smu_process::{ProcessDiscovery, ProcessForeground};
use smu_runtime::macros::{
    BunnyhopPhysicalSource, build_bunnyhop_physical_plan, bunnyhop_half_delay_ms,
    should_continue_bunnyhop_after_half, should_run_bunnyhop_worker,
};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);
const IDLE_WORKER_POLL_INTERVAL: Duration = Duration::from_millis(2);
pub const DEFAULT_BUNNYHOP_HOTKEY: Hotkey = Hotkey::new(0x20);

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BunnyhopControllerSettings {
    pub macro_enabled: bool,
    pub section_enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub delay_ms: i32,
    pub smart: bool,
    pub chat_key: u32,
    pub process_selector: String,
}

impl Default for BunnyhopControllerSettings {
    fn default() -> Self {
        Self {
            macro_enabled: true,
            section_enabled: false,
            hotkey: DEFAULT_BUNNYHOP_HOTKEY,
            disable_outside_target: true,
            delay_ms: 10,
            smart: true,
            chat_key: 0xBF,
            process_selector: "sober".to_owned(),
        }
    }
}

pub struct BunnyhopController {
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<BunnyhopControllerSettings>>,
    loop_active: Arc<AtomicBool>,
    worker_active: Arc<AtomicBool>,
    controller: Mutex<Option<JoinHandle<()>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for BunnyhopController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BunnyhopController")
            .field("running", &self.running.load(Ordering::Acquire))
            .field("loop_active", &self.loop_active.load(Ordering::Acquire))
            .field("worker_active", &self.worker_active.load(Ordering::Acquire))
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

impl BunnyhopController {
    #[must_use]
    pub fn start(
        input: Arc<dyn InputDevice>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        initial_settings: BunnyhopControllerSettings,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let settings = Arc::new(Mutex::new(initial_settings));
        let loop_active = Arc::new(AtomicBool::new(false));
        let worker_active = Arc::new(AtomicBool::new(false));

        let controller = {
            let running = Arc::clone(&running);
            let settings = Arc::clone(&settings);
            let loop_active = Arc::clone(&loop_active);
            let worker_active = Arc::clone(&worker_active);
            let input = Arc::clone(&input);
            std::thread::spawn(move || {
                controller_loop(
                    running.as_ref(),
                    settings.as_ref(),
                    loop_active.as_ref(),
                    worker_active.as_ref(),
                    input.as_ref(),
                    discovery.as_ref(),
                    foreground.as_ref(),
                );
            })
        };

        let worker = {
            let running = Arc::clone(&running);
            let settings = Arc::clone(&settings);
            let loop_active = Arc::clone(&loop_active);
            let worker_active = Arc::clone(&worker_active);
            std::thread::spawn(move || {
                owned_worker_loop(
                    running.as_ref(),
                    settings.as_ref(),
                    loop_active.as_ref(),
                    worker_active.as_ref(),
                    input.as_ref(),
                );
            })
        };

        Self {
            running,
            settings,
            loop_active,
            worker_active,
            controller: Mutex::new(Some(controller)),
            worker: Mutex::new(Some(worker)),
        }
    }

    pub fn update_settings(&self, settings: BunnyhopControllerSettings) {
        *lock(&self.settings) = settings;
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        self.loop_active.store(false, Ordering::Release);
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

impl Drop for BunnyhopController {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn controller_loop(
    running: &AtomicBool,
    settings: &Mutex<BunnyhopControllerSettings>,
    loop_active: &AtomicBool,
    worker_active: &AtomicBool,
    input: &dyn InputDevice,
    discovery: &dyn ProcessDiscovery,
    foreground: &dyn ProcessForeground,
) {
    let mut targets = crate::target_runtime::TargetProcessCache::default();
    let mut chat_locked = false;
    while running.load(Ordering::Acquire) {
        let current = lock(settings).clone();
        targets.refresh_if_due(&current.process_selector, false, discovery);
        if !current.macro_enabled {
            loop_active.store(false, Ordering::Release);
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }

        let foreground_allowed =
            targets.foreground_allowed(current.disable_outside_target, foreground);
        if !foreground_allowed || !current.section_enabled {
            loop_active.store(false, Ordering::Release);
            std::thread::sleep(CONTROLLER_POLL_INTERVAL);
            continue;
        }

        let chat_pressed =
            crate::runtime_input::hotkey_pressed(input, Hotkey::new(current.chat_key));
        let chat_exit_pressed =
            input.is_pressed(KeyCode::RETURN) || input.is_pressed(KeyCode::LBUTTON);
        chat_locked = update_chat_lock(chat_locked, chat_pressed, chat_exit_pressed);

        let held = bunnyhop_physically_held(input, current.hotkey);
        let should_hop = held && (!current.smart || !chat_locked);
        loop_active.store(should_hop, Ordering::Release);
        if should_hop {
            let _ =
                worker_active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire);
        }
        std::thread::sleep(CONTROLLER_POLL_INTERVAL);
    }
    loop_active.store(false, Ordering::Release);
}

#[must_use]
const fn update_chat_lock(current: bool, chat_pressed: bool, exit_pressed: bool) -> bool {
    (current || chat_pressed) && !exit_pressed
}

fn bunnyhop_physically_held(input: &dyn InputDevice, hotkey: Hotkey) -> bool {
    match build_bunnyhop_physical_plan(hotkey.raw()).source {
        BunnyhopPhysicalSource::None => false,
        BunnyhopPhysicalSource::MouseBackend | BunnyhopPhysicalSource::KeyboardLatch => {
            is_hotkey_pressed(input, hotkey, HotkeyMatch::Loose)
        }
    }
}

fn owned_worker_loop(
    running: &AtomicBool,
    settings: &Mutex<BunnyhopControllerSettings>,
    loop_active: &AtomicBool,
    worker_active: &AtomicBool,
    input: &dyn InputDevice,
) {
    while running.load(Ordering::Acquire) {
        if worker_active.load(Ordering::Acquire) {
            run_active_worker(running, settings, loop_active, input);
            loop_active.store(false, Ordering::Release);
            worker_active.store(false, Ordering::Release);
        } else {
            std::thread::sleep(IDLE_WORKER_POLL_INTERVAL);
        }
    }
    loop_active.store(false, Ordering::Release);
    worker_active.store(false, Ordering::Release);
}

fn run_active_worker(
    running: &AtomicBool,
    settings: &Mutex<BunnyhopControllerSettings>,
    loop_active: &AtomicBool,
    input: &dyn InputDevice,
) {
    loop {
        let current = lock(settings).clone();
        if !should_run_bunnyhop_worker(
            running.load(Ordering::Acquire),
            true,
            loop_active.load(Ordering::Acquire),
            current.macro_enabled,
            true,
            current.section_enabled,
        ) {
            break;
        }

        let output = current.hotkey;
        let half_delay_ms = bunnyhop_half_delay_ms(current.delay_ms);
        let completed = execute_iteration(output, half_delay_ms, input, std::thread::sleep, || {
            should_continue_bunnyhop_after_half(
                running.load(Ordering::Acquire),
                true,
                loop_active.load(Ordering::Acquire),
            )
        });
        if !completed {
            break;
        }
    }
}

fn execute_iteration(
    output: Hotkey,
    half_delay_ms: i32,
    input: &dyn InputDevice,
    mut sleep: impl FnMut(Duration),
    continue_after_half: impl FnOnce() -> bool,
) -> bool {
    let delay = half_delay_ms.max(1);
    input.hold_key_chord(output);
    sleep(Duration::from_millis(u64::try_from(delay).unwrap_or_default()));
    let should_continue = continue_after_half();
    input.release_key_chord(output);
    if !should_continue {
        return false;
    }
    sleep(Duration::from_millis(u64::try_from(delay).unwrap_or_default()));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use smu_input::{InputInjector, KeyState};
    use std::collections::BTreeSet;

    #[derive(Debug, Default)]
    struct FakeInput {
        pressed: BTreeSet<KeyCode>,
        events: Mutex<Vec<String>>,
    }

    impl KeyState for FakeInput {
        fn is_pressed(&self, key: KeyCode) -> bool {
            self.pressed.contains(&key)
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

    #[test]
    fn defaults_match_cpp_bunnyhop_runtime() {
        let settings = BunnyhopControllerSettings::default();
        assert!(settings.macro_enabled);
        assert!(!settings.section_enabled);
        assert_eq!(settings.hotkey, DEFAULT_BUNNYHOP_HOTKEY);
        assert!(settings.disable_outside_target);
        assert_eq!(settings.delay_ms, 10);
        assert!(settings.smart);
        assert_eq!(settings.chat_key, 0xBF);
    }

    #[test]
    fn smart_chat_lock_preserves_cpp_update_order() {
        assert!(update_chat_lock(false, true, false));
        assert!(update_chat_lock(true, false, false));
        assert!(!update_chat_lock(true, false, true));
        assert!(!update_chat_lock(false, true, true));
    }

    #[test]
    fn physical_hold_uses_real_key_state_for_keyboard_mouse_and_modifiers() {
        let mut input = FakeInput::default();
        input.pressed.insert(KeyCode::SPACE);
        assert!(bunnyhop_physically_held(&input, Hotkey::new(KeyCode::SPACE.raw())));
        assert!(!bunnyhop_physically_held(&input, Hotkey::new(KeyCode::MOUSE_WHEEL_UP.raw())));

        input.pressed.insert(KeyCode::LBUTTON);
        assert!(bunnyhop_physically_held(&input, Hotkey::new(KeyCode::LBUTTON.raw())));

        let shifted_space = Hotkey::new(0x1_0000 | KeyCode::SPACE.raw());
        assert!(!bunnyhop_physically_held(&input, shifted_space));
        input.pressed.insert(KeyCode::SHIFT);
        assert!(bunnyhop_physically_held(&input, shifted_space));
    }

    #[test]
    fn mid_cycle_cancellation_always_releases_snapshotted_output() {
        let input = FakeInput::default();
        let mut waits = Vec::new();
        let completed = execute_iteration(
            Hotkey::new(KeyCode::SPACE.raw()),
            5,
            &input,
            |duration| waits.push(duration),
            || false,
        );
        assert!(!completed);
        assert_eq!(*lock(&input.events), ["down:32", "up:32"]);
        assert_eq!(waits, [Duration::from_millis(5)]);
    }

    #[test]
    fn complete_cycle_has_two_equal_half_delays() {
        let input = FakeInput::default();
        let mut waits = Vec::new();
        assert!(execute_iteration(
            Hotkey::new(0x44),
            7,
            &input,
            |duration| waits.push(duration),
            || true,
        ));
        assert_eq!(*lock(&input.events), ["down:68", "up:68"]);
        assert_eq!(waits, [Duration::from_millis(7), Duration::from_millis(7)]);
    }
}
