//! Live Linux Item Clip controller and timing worker.

use smu_core::{Hotkey, KeyCode};
use smu_input::InputDevice;
use smu_process::{ProcessDiscovery, ProcessForeground};
use smu_runtime::macros::{
    apply_toggle_loop_state_action, build_itemclip_cycle_plan, evaluate_toggle_loop_edge,
};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);
const IDLE_WORKER_POLL_INTERVAL: Duration = Duration::from_millis(2);
pub const DEFAULT_ITEM_CLIP_HOTKEY: Hotkey = Hotkey::new(0x72);

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemClipControllerSettings {
    pub macro_enabled: bool,
    pub section_enabled: bool,
    pub hotkey: Hotkey,
    pub hold_mode: bool,
    pub slot: i32,
    pub delay_ms: i32,
    pub disable_outside_target: bool,
    pub process_selector: String,
}

impl Default for ItemClipControllerSettings {
    fn default() -> Self {
        Self {
            macro_enabled: true,
            section_enabled: false,
            hotkey: DEFAULT_ITEM_CLIP_HOTKEY,
            hold_mode: false,
            slot: 7,
            delay_ms: 34,
            disable_outside_target: true,
            process_selector: "sober".to_owned(),
        }
    }
}

pub struct ItemClipController {
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<ItemClipControllerSettings>>,
    worker_allowed: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    controller: Mutex<Option<JoinHandle<()>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for ItemClipController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ItemClipController")
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

impl ItemClipController {
    #[must_use]
    pub fn start(
        input: Arc<dyn InputDevice>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        initial_settings: ItemClipControllerSettings,
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

    pub fn update_settings(&self, settings: ItemClipControllerSettings) {
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

impl Drop for ItemClipController {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn controller_loop(
    running: &AtomicBool,
    settings: &Mutex<ItemClipControllerSettings>,
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
    settings: &Mutex<ItemClipControllerSettings>,
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
            execute_cycle(current.delay_ms, current.slot, input, std::thread::sleep);
        } else if !current.macro_enabled || !current.section_enabled {
            // Park disabled worker lanes rather than waking 500 times per second.
            std::thread::park_timeout(Duration::from_secs(1));
        } else {
            std::thread::sleep(IDLE_WORKER_POLL_INTERVAL);
        }
    }
}

fn execute_cycle(
    delay_ms: i32,
    slot: i32,
    input: &dyn InputDevice,
    mut sleep: impl FnMut(Duration),
) {
    let plan = build_itemclip_cycle_plan(delay_ms, slot);
    let key = KeyCode::new(plan.slot_key);
    input.hold_key(key, false);
    sleep(Duration::from_millis(u64::try_from(plan.half_delay_ms).unwrap_or_default()));
    input.release_key(key, false);
    sleep(Duration::from_millis(u64::try_from(plan.half_delay_ms).unwrap_or_default()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use smu_input::{InputInjector, KeyState};

    #[derive(Debug, Default)]
    struct FakeInput {
        events: Mutex<Vec<(KeyCode, bool)>>,
    }

    impl KeyState for FakeInput {
        fn is_pressed(&self, _key: KeyCode) -> bool {
            false
        }
    }

    impl InputInjector for FakeInput {
        fn hold_key(&self, key: KeyCode, _extended: bool) {
            lock(&self.events).push((key, true));
        }
        fn release_key(&self, key: KeyCode, _extended: bool) {
            lock(&self.events).push((key, false));
        }
        fn move_mouse(&self, _dx: i32, _dy: i32) {}
        fn mouse_wheel(&self, _delta: i32) {}
    }

    #[test]
    fn defaults_match_cpp_item_clip_runtime() {
        let settings = ItemClipControllerSettings::default();
        assert!(settings.macro_enabled);
        assert!(!settings.section_enabled);
        assert_eq!(settings.hotkey, DEFAULT_ITEM_CLIP_HOTKEY);
        assert!(!settings.hold_mode);
        assert_eq!(settings.slot, 7);
        assert_eq!(settings.delay_ms, 34);
        assert!(settings.disable_outside_target);
    }

    #[test]
    fn cycle_snapshots_delay_and_slot_and_completes_both_halves() {
        let input = FakeInput::default();
        let mut waits = Vec::new();
        execute_cycle(34, 7, &input, |duration| waits.push(duration));
        let key = KeyCode::new(smu_runtime::macros::inventory_slot_key(7));
        assert_eq!(*lock(&input.events), [(key, true), (key, false)]);
        assert_eq!(waits, [Duration::from_millis(17), Duration::from_millis(17)]);
    }
}
