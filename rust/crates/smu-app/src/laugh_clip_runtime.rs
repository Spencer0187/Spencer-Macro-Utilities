//! Live Linux Laugh Clip rising-edge controller.
//!
//! The worker keeps every shared setting live at the historical C++ read sites. A single active
//! guard drops trigger edges while one Laugh Clip worker is already running.

use smu_core::{Hotkey, KeyCode};
use smu_input::InputDevice;
use smu_process::{ProcessDiscovery, ProcessForeground};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);
pub const DEFAULT_LAUGH_CLIP_HOTKEY: Hotkey = Hotkey::new(0x76);

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LaughClipControllerSettings {
    pub macro_enabled: bool,
    pub section_enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub process_selector: String,
    pub chat_override: bool,
    pub chat_key: u32,
    pub enter_key: u32,
    pub paste_delay_ms: i32,
    pub use_old_paste: bool,
    pub global_zoom: bool,
    pub shift_key: u32,
    pub zoom_reverse: bool,
    pub laugh_move_switch: bool,
}

impl Default for LaughClipControllerSettings {
    fn default() -> Self {
        Self {
            macro_enabled: true,
            section_enabled: false,
            hotkey: DEFAULT_LAUGH_CLIP_HOTKEY,
            disable_outside_target: true,
            process_selector: "sober".to_owned(),
            chat_override: true,
            chat_key: 0xBF,
            enter_key: 0x0D,
            paste_delay_ms: 1,
            use_old_paste: true,
            global_zoom: false,
            shift_key: 0x10,
            zoom_reverse: false,
            laugh_move_switch: false,
        }
    }
}

pub struct LaughClipController {
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<LaughClipControllerSettings>>,
    active: Arc<AtomicBool>,
    controller: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for LaughClipController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LaughClipController")
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

impl LaughClipController {
    #[must_use]
    pub fn start(
        input: Arc<dyn InputDevice>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        initial_settings: LaughClipControllerSettings,
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

    pub fn update_settings(&self, settings: LaughClipControllerSettings) {
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

impl Drop for LaughClipController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn controller_loop(
    running: &AtomicBool,
    settings: &Arc<Mutex<LaughClipControllerSettings>>,
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
            let settings = Arc::clone(settings);
            let input = Arc::clone(input);
            let worker_active = Arc::clone(active);
            workers.push(std::thread::spawn(move || {
                execute_worker(settings.as_ref(), input.as_ref(), std::thread::sleep);
                worker_active.store(false, Ordering::Release);
            }));
        }
        std::thread::sleep(CONTROLLER_POLL_INTERVAL);
    }
    for worker in workers {
        let _ = worker.join();
    }
    active.store(false, Ordering::Release);
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
    settings: &Mutex<LaughClipControllerSettings>,
    input: &dyn InputDevice,
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

    let paste_delay = lock(settings).paste_delay_ms.max(0);
    let use_old_paste = lock(settings).use_old_paste;
    let _ =
        crate::macro_text::paste_text(input, "/e laugh", paste_delay, use_old_paste, &mut sleep);
    sleep(Duration::from_millis(100));
    input.hold_key_chord(Hotkey::new(lock(settings).enter_key));
    sleep(Duration::from_millis(35));
    input.release_key_chord(Hotkey::new(lock(settings).enter_key));

    sleep(Duration::from_millis(248));
    input.hold_key(KeyCode::SPACE, false);
    if lock(settings).global_zoom {
        let wheel = if lock(settings).zoom_reverse {
            KeyCode::MOUSE_WHEEL_DOWN
        } else {
            KeyCode::MOUSE_WHEEL_UP
        };
        input.hold_key_chord(Hotkey::new(wheel.raw()));
    } else {
        input.hold_key_chord(Hotkey::new(lock(settings).shift_key));
    }

    if !lock(settings).laugh_move_switch {
        input.hold_key(KeyCode::new(0x53), false);
    }

    sleep(Duration::from_millis(25));
    input.release_key(KeyCode::SPACE, false);
    if !lock(settings).global_zoom {
        input.release_key_chord(Hotkey::new(lock(settings).shift_key));
    }
    sleep(Duration::from_millis(25));

    if !lock(settings).laugh_move_switch {
        input.release_key(KeyCode::new(0x53), false);
    }
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
    fn defaults_match_cpp_laugh_clip_runtime() {
        let settings = LaughClipControllerSettings::default();
        assert_eq!(settings.hotkey, DEFAULT_LAUGH_CLIP_HOTKEY);
        assert!(!settings.section_enabled);
        assert!(settings.disable_outside_target);
        assert!(settings.chat_override);
        assert_eq!(settings.chat_key, 0xBF);
        assert_eq!(settings.enter_key, 0x0D);
        assert_eq!(settings.paste_delay_ms, 1);
        assert!(settings.use_old_paste);
        assert!(!settings.global_zoom);
        assert_eq!(settings.shift_key, 0x10);
        assert!(!settings.zoom_reverse);
        assert!(!settings.laugh_move_switch);
    }

    #[test]
    fn worker_reads_old_unicode_toggle_live_at_paste_site() {
        let settings = Mutex::new(LaughClipControllerSettings {
            paste_delay_ms: 7,
            use_old_paste: false,
            ..LaughClipControllerSettings::default()
        });
        let input = FakeInput { unicode_success: true, ..FakeInput::default() };
        let mut elapsed = Duration::ZERO;
        execute_worker(&settings, &input, |duration| {
            elapsed += duration;
            if elapsed == Duration::from_millis(75) {
                lock(&settings).use_old_paste = true;
            }
        });
        assert!(lock(&input.events).contains(&"unicode:/e laugh:7".to_owned()));
    }

    #[test]
    fn worker_re_reads_zoom_shift_and_movement_release_state() {
        let settings = Mutex::new(LaughClipControllerSettings {
            paste_delay_ms: 0,
            shift_key: 0x5A,
            ..LaughClipControllerSettings::default()
        });
        let input = FakeInput::default();
        let mut elapsed = Duration::ZERO;
        execute_worker(&settings, &input, |duration| {
            elapsed += duration;
            if elapsed == Duration::from_millis(483) {
                let mut current = lock(&settings);
                current.global_zoom = true;
                current.laugh_move_switch = true;
            }
        });
        assert_eq!(elapsed, Duration::from_millis(508));
        let events = lock(&input.events).clone();
        assert!(events.contains(&"down:90".to_owned()));
        assert!(!events.contains(&"up:90".to_owned()));
        assert!(events.contains(&"down:83".to_owned()));
        assert!(!events.contains(&"up:83".to_owned()));
    }
}
