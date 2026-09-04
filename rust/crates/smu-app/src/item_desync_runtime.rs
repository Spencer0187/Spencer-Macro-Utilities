//! Live Linux Item Desync controller.
//!
//! The C++ runtime emits two immediate down/up pulses for the selected inventory slot every 2 ms
//! while the hotkey remains physically pressed. This controller keeps that per-poll ordering on its
//! own long-lived lane so the pulse train cannot monopolize other macro controllers.

use smu_core::{Hotkey, KeyCode};
use smu_input::InputDevice;
use smu_process::{ProcessDiscovery, ProcessForeground};
use smu_runtime::macros::inventory_slot_key;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);
pub const DEFAULT_ITEM_DESYNC_HOTKEY: Hotkey = Hotkey::new(0x74);

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemDesyncControllerSettings {
    pub macro_enabled: bool,
    pub section_enabled: bool,
    pub hotkey: Hotkey,
    pub slot: i32,
    pub disable_outside_target: bool,
    pub process_selector: String,
}

impl Default for ItemDesyncControllerSettings {
    fn default() -> Self {
        Self {
            macro_enabled: true,
            section_enabled: false,
            hotkey: DEFAULT_ITEM_DESYNC_HOTKEY,
            slot: 5,
            disable_outside_target: true,
            process_selector: "sober".to_owned(),
        }
    }
}

pub struct ItemDesyncController {
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<ItemDesyncControllerSettings>>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for ItemDesyncController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ItemDesyncController")
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

impl ItemDesyncController {
    #[must_use]
    pub fn start(
        input: Arc<dyn InputDevice>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        initial_settings: ItemDesyncControllerSettings,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let settings = Arc::new(Mutex::new(initial_settings));
        let worker_running = Arc::clone(&running);
        let worker_settings = Arc::clone(&settings);
        let worker = std::thread::spawn(move || {
            controller_loop(
                worker_running.as_ref(),
                worker_settings.as_ref(),
                input.as_ref(),
                discovery.as_ref(),
                foreground.as_ref(),
            );
        });
        Self { running, settings, worker: Mutex::new(Some(worker)) }
    }

    pub fn update_settings(&self, settings: ItemDesyncControllerSettings) {
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

impl Drop for ItemDesyncController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn controller_loop(
    running: &AtomicBool,
    settings: &Mutex<ItemDesyncControllerSettings>,
    input: &dyn InputDevice,
    discovery: &dyn ProcessDiscovery,
    foreground: &dyn ProcessForeground,
) {
    let mut targets = crate::target_runtime::TargetProcessCache::default();
    while running.load(Ordering::Acquire) {
        let current = lock(settings).clone();
        targets.refresh_if_due(&current.process_selector, false, discovery);
        if !current.macro_enabled {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }

        let foreground_allowed =
            targets.foreground_allowed(current.disable_outside_target, foreground);
        process_step(&current, foreground_allowed, input);
        std::thread::sleep(CONTROLLER_POLL_INTERVAL);
    }
}

fn process_step(
    settings: &ItemDesyncControllerSettings,
    foreground_allowed: bool,
    input: &dyn InputDevice,
) -> bool {
    let pressed = foreground_allowed
        && settings.section_enabled
        && crate::runtime_input::hotkey_pressed(input, settings.hotkey);
    if !pressed {
        return false;
    }

    let key = KeyCode::new(inventory_slot_key(settings.slot));
    input.hold_key(key, false);
    input.release_key(key, false);
    input.hold_key(key, false);
    input.release_key(key, false);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use smu_input::{InputInjector, KeyState};
    use std::sync::atomic::AtomicBool;

    #[derive(Debug, Default)]
    struct FakeInput {
        trigger_pressed: AtomicBool,
        events: Mutex<Vec<(KeyCode, bool)>>,
    }

    impl KeyState for FakeInput {
        fn is_pressed(&self, key: KeyCode) -> bool {
            key == DEFAULT_ITEM_DESYNC_HOTKEY.key() && self.trigger_pressed.load(Ordering::Acquire)
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
    fn defaults_match_cpp_item_desync_settings() {
        let settings = ItemDesyncControllerSettings::default();
        assert!(settings.macro_enabled);
        assert!(!settings.section_enabled);
        assert_eq!(settings.hotkey, DEFAULT_ITEM_DESYNC_HOTKEY);
        assert_eq!(settings.slot, 5);
        assert!(settings.disable_outside_target);
        assert_eq!(settings.process_selector, "sober");
    }

    #[test]
    fn active_poll_emits_exact_double_slot_pulse() {
        let input = FakeInput::default();
        input.trigger_pressed.store(true, Ordering::Release);
        let settings = ItemDesyncControllerSettings {
            section_enabled: true,
            ..ItemDesyncControllerSettings::default()
        };
        assert!(process_step(&settings, true, &input));
        let slot = KeyCode::new(inventory_slot_key(5));
        assert_eq!(
            *lock(&input.events),
            [(slot, true), (slot, false), (slot, true), (slot, false)]
        );
    }

    #[test]
    fn foreground_or_section_gate_prevents_pulse() {
        let input = FakeInput::default();
        input.trigger_pressed.store(true, Ordering::Release);
        assert!(!process_step(&ItemDesyncControllerSettings::default(), true, &input));
        let enabled = ItemDesyncControllerSettings {
            section_enabled: true,
            ..ItemDesyncControllerSettings::default()
        };
        assert!(!process_step(&enabled, false, &input));
        assert!(lock(&input.events).is_empty());
    }
}
