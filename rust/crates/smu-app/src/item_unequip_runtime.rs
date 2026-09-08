//! Live Linux Item Unequip COM Offset rising-edge controller.
//!
//! Only custom/emote text are captured before spawn, matching C++. The worker keeps chat/paste,
//! Enter, dropdown, slot, and keep-equipped reads live. A single active guard drops trigger edges
//! while one Item Unequip worker is already running.

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
pub const DEFAULT_ITEM_UNEQUIP_HOTKEY: Hotkey = Hotkey::new(0x77);

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemUnequipControllerSettings {
    pub macro_enabled: bool,
    pub section_enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub process_selector: String,
    pub custom_text: String,
    pub emote_text: String,
    pub selected_dropdown: i32,
    pub speed_slot: i32,
    pub unequip_toggle: bool,
    pub chat_override: bool,
    pub chat_key: u32,
    pub enter_key: u32,
    pub paste_delay_ms: i32,
    pub use_old_paste: bool,
}

impl Default for ItemUnequipControllerSettings {
    fn default() -> Self {
        Self {
            macro_enabled: true,
            section_enabled: false,
            hotkey: DEFAULT_ITEM_UNEQUIP_HOTKEY,
            disable_outside_target: true,
            process_selector: "sober".to_owned(),
            custom_text: String::new(),
            emote_text: "/e dance2".to_owned(),
            selected_dropdown: 0,
            speed_slot: 3,
            unequip_toggle: false,
            chat_override: true,
            chat_key: 0xBF,
            enter_key: 0x0D,
            paste_delay_ms: 1,
            use_old_paste: true,
        }
    }
}

pub struct ItemUnequipController {
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<ItemUnequipControllerSettings>>,
    active: Arc<AtomicBool>,
    controller: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for ItemUnequipController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ItemUnequipController")
            .field("running", &self.running.load(Ordering::Acquire))
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

impl ItemUnequipController {
    #[must_use]
    pub fn start(
        input: Arc<dyn InputDevice>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        initial_settings: ItemUnequipControllerSettings,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let settings = Arc::new(Mutex::new(initial_settings));
        let active = Arc::new(AtomicBool::new(false));
        let controller = {
            let running = Arc::clone(&running);
            let settings = Arc::clone(&settings);
            let active = Arc::clone(&active);
            std::thread::spawn(move || {
                controller_loop(
                    running.as_ref(),
                    &settings,
                    &active,
                    &input,
                    &discovery,
                    &foreground,
                );
            })
        };
        Self { running, settings, active, controller: Mutex::new(Some(controller)) }
    }

    pub fn update_settings(&self, settings: ItemUnequipControllerSettings) {
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

impl Drop for ItemUnequipController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn controller_loop(
    running: &AtomicBool,
    settings: &Arc<Mutex<ItemUnequipControllerSettings>>,
    active: &Arc<AtomicBool>,
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
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        let allowed =
            targets.foreground_allowed(current.disable_outside_target, foreground.as_ref());
        let pressed = allowed
            && current.section_enabled
            && crate::runtime_input::hotkey_pressed(input.as_ref(), current.hotkey);
        let edge = pressed && !was_pressed;
        was_pressed = pressed;
        if edge && active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_ok()
        {
            let custom_text = current.custom_text;
            let emote_text = current.emote_text;
            let settings = Arc::clone(settings);
            let input = Arc::clone(input);
            let worker_active = Arc::clone(active);
            workers.push(spawn_worker(settings, input, worker_active, custom_text, emote_text));
        }
        std::thread::sleep(CONTROLLER_POLL_INTERVAL);
    }
    for worker in workers {
        let _ = worker.join();
    }
    active.store(false, Ordering::Release);
}

fn spawn_worker(
    settings: Arc<Mutex<ItemUnequipControllerSettings>>,
    input: Arc<dyn InputDevice>,
    active: Arc<AtomicBool>,
    custom_text: String,
    emote_text: String,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        execute_worker(
            settings.as_ref(),
            input.as_ref(),
            &custom_text,
            &emote_text,
            std::thread::sleep,
        );
        active.store(false, Ordering::Release);
    })
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
    settings: &Mutex<ItemUnequipControllerSettings>,
    input: &dyn InputDevice,
    custom_text: &str,
    emote_text: &str,
    mut sleep: impl FnMut(Duration),
) {
    crate::macro_text::open_chat(
        input,
        || lock(settings).chat_override,
        || Hotkey::new(lock(settings).chat_key),
        || Hotkey::new(lock(settings).chat_key),
        &mut sleep,
    );
    sleep(Duration::from_millis(25));

    let custom = !custom_text.is_empty();
    let text = if custom { custom_text } else { emote_text };
    let paste_delay = lock(settings).paste_delay_ms.max(0);
    let use_old_paste = lock(settings).use_old_paste;
    let _ = crate::macro_text::paste_text(input, text, paste_delay, use_old_paste, &mut sleep);
    sleep(Duration::from_millis(100));
    input.hold_key_chord(Hotkey::new(lock(settings).enter_key));
    sleep(Duration::from_millis(35));
    input.release_key_chord(Hotkey::new(lock(settings).enter_key));

    if custom {
        return;
    }

    if lock(settings).selected_dropdown == 2 {
        sleep(Duration::from_millis(16));
    } else {
        sleep(Duration::from_millis(65));
    }
    if lock(settings).selected_dropdown == 0 {
        sleep(Duration::from_millis(815));
    }
    if lock(settings).selected_dropdown == 1 {
        sleep(Duration::from_millis(175));
    }

    let slot = KeyCode::new(inventory_slot_key(lock(settings).speed_slot));
    input.hold_key(slot, false);
    sleep(Duration::from_millis(4));
    input.release_key(slot, false);
    sleep(Duration::from_millis(4));
    if !lock(settings).unequip_toggle {
        input.hold_key(slot, false);
    }
    input.release_key(slot, false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use smu_input::{InputInjector, KeyState};

    #[derive(Debug, Default)]
    struct FakeInput {
        unicode_success: bool,
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
        fn type_unicode_text(&self, text: &str, delay_ms: i32) -> bool {
            lock(&self.events).push(format!("unicode:{text}:{delay_ms}"));
            self.unicode_success
        }
    }

    #[test]
    fn defaults_match_cpp_item_unequip_runtime() {
        let settings = ItemUnequipControllerSettings::default();
        assert_eq!(settings.hotkey, DEFAULT_ITEM_UNEQUIP_HOTKEY);
        assert!(!settings.section_enabled);
        assert!(settings.disable_outside_target);
        assert_eq!(settings.emote_text, "/e dance2");
        assert_eq!(settings.selected_dropdown, 0);
        assert_eq!(settings.speed_slot, 3);
        assert!(!settings.unequip_toggle);
        assert!(settings.chat_override);
        assert_eq!(settings.enter_key, 0x0D);
        assert_eq!(settings.paste_delay_ms, 1);
        assert!(settings.use_old_paste);
    }

    #[test]
    fn worker_reads_old_unicode_toggle_live_at_paste_site() {
        let settings = Mutex::new(ItemUnequipControllerSettings {
            paste_delay_ms: 7,
            use_old_paste: false,
            ..ItemUnequipControllerSettings::default()
        });
        let input = FakeInput { unicode_success: true, ..FakeInput::default() };
        let mut elapsed = Duration::ZERO;
        execute_worker(&settings, &input, "hé", "/e dance2", |duration| {
            elapsed += duration;
            if elapsed == Duration::from_millis(75) {
                lock(&settings).use_old_paste = true;
            }
        });
        assert!(lock(&input.events).contains(&"unicode:hé:7".to_owned()));
    }

    #[test]
    fn custom_text_path_short_circuits_before_dropdown_and_slot_actions() {
        let settings = Mutex::new(ItemUnequipControllerSettings {
            paste_delay_ms: 0,
            ..ItemUnequipControllerSettings::default()
        });
        let input = FakeInput::default();
        let mut elapsed = Duration::ZERO;
        execute_worker(&settings, &input, "x", "/e dance2", |duration| elapsed += duration);
        assert_eq!(elapsed, Duration::from_millis(210));
        let events = lock(&input.events).clone();
        assert!(events.contains(&"down:191".to_owned()));
        assert!(events.contains(&"down:88".to_owned()));
        assert!(!events.contains(&"down:51".to_owned()));
    }
}
