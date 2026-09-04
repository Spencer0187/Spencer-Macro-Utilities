//! Live Linux HHJ controller, one-shot workers, and owned motion worker.
//!
//! Trigger workers snapshot only target PIDs. Settings remain live through the shared mutex at the
//! characterized read points. The motion lane uses the shared `is_hhj` atomic so Floor Bounce can
//! activate the same helicopter motion.

use smu_core::Hotkey;
use smu_input::InputDevice;
use smu_process::{ProcessControl, ProcessDiscovery, ProcessForeground};
use smu_runtime::macros::frame_delays_for_fps;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);
const MOTION_IDLE_POLL_INTERVAL: Duration = Duration::from_millis(2);
pub const DEFAULT_HHJ_HOTKEY: Hotkey = Hotkey::new(0x05);

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HhjControllerSettings {
    pub macro_enabled: bool,
    pub section_enabled: bool,
    pub hotkey: Hotkey,
    pub disable_outside_target: bool,
    pub process_selector: String,
    pub take_all_process_ids: bool,
    pub auto_toggle: bool,
    pub auto_key1: Hotkey,
    pub auto_key1_time_ms: i32,
    pub auto_key2: Hotkey,
    pub auto_key2_time_ms: i32,
    pub freeze_delay_apply: bool,
    pub fast_hhj: bool,
    pub freeze_delay_override_ms: i32,
    pub delay1_ms: i32,
    pub global_zoom: bool,
    pub shift_key: u32,
    pub zoom_reverse: bool,
    pub delay2_ms: i32,
    pub length_ms: i32,
    pub delay3_ms: i32,
    pub fps: u32,
    pub strength_x: i32,
    pub strength_y: i32,
}

impl Default for HhjControllerSettings {
    fn default() -> Self {
        Self {
            macro_enabled: true,
            section_enabled: false,
            hotkey: DEFAULT_HHJ_HOTKEY,
            disable_outside_target: false,
            process_selector: "sober".to_owned(),
            take_all_process_ids: false,
            auto_toggle: false,
            auto_key1: Hotkey::new(0x20),
            auto_key1_time_ms: 550,
            auto_key2: Hotkey::new(0x57),
            auto_key2_time_ms: 68,
            freeze_delay_apply: false,
            fast_hhj: false,
            freeze_delay_override_ms: 500,
            delay1_ms: 9,
            global_zoom: false,
            shift_key: 0x10,
            zoom_reverse: false,
            delay2_ms: 17,
            length_ms: 243,
            delay3_ms: 16,
            fps: 120,
            strength_x: 959,
            strength_y: -959,
        }
    }
}

pub struct HhjController {
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<HhjControllerSettings>>,
    shared_active: Arc<AtomicBool>,
    controller: Mutex<Option<JoinHandle<()>>>,
    motion_worker: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for HhjController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HhjController")
            .field("running", &self.running.load(Ordering::Acquire))
            .field("shared_active", &self.shared_active.load(Ordering::Acquire))
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

impl HhjController {
    #[must_use]
    pub fn start(
        input: Arc<dyn InputDevice>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        control: Arc<dyn ProcessControl>,
        shared_active: Arc<AtomicBool>,
        initial_settings: HhjControllerSettings,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let settings = Arc::new(Mutex::new(initial_settings));

        let controller = {
            let running = Arc::clone(&running);
            let settings = Arc::clone(&settings);
            let input = Arc::clone(&input);
            let shared_active = Arc::clone(&shared_active);
            std::thread::spawn(move || {
                controller_loop(
                    running.as_ref(),
                    &settings,
                    &input,
                    &discovery,
                    &foreground,
                    &control,
                    &shared_active,
                );
            })
        };

        let motion_worker = {
            let running = Arc::clone(&running);
            let settings = Arc::clone(&settings);
            let shared_active = Arc::clone(&shared_active);
            std::thread::spawn(move || {
                motion_loop(
                    running.as_ref(),
                    settings.as_ref(),
                    shared_active.as_ref(),
                    input.as_ref(),
                );
            })
        };

        Self {
            running,
            settings,
            shared_active,
            controller: Mutex::new(Some(controller)),
            motion_worker: Mutex::new(Some(motion_worker)),
        }
    }

    pub fn update_settings(&self, settings: HhjControllerSettings) {
        *lock(&self.settings) = settings;
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        let controller = lock(&self.controller).take();
        if let Some(controller) = controller {
            let _ = controller.join();
        }
        let worker = lock(&self.motion_worker).take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

impl Drop for HhjController {
    fn drop(&mut self) {
        self.stop();
    }
}

#[allow(clippy::too_many_arguments)]
fn controller_loop(
    running: &AtomicBool,
    settings: &Arc<Mutex<HhjControllerSettings>>,
    input: &Arc<dyn InputDevice>,
    discovery: &Arc<dyn ProcessDiscovery>,
    foreground: &Arc<dyn ProcessForeground>,
    control: &Arc<dyn ProcessControl>,
    shared_active: &Arc<AtomicBool>,
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
            let shared_active = Arc::clone(shared_active);
            workers.push(std::thread::spawn(move || {
                execute_worker(
                    settings.as_ref(),
                    input.as_ref(),
                    control.as_ref(),
                    shared_active.as_ref(),
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
    settings: &Mutex<HhjControllerSettings>,
    input: &dyn InputDevice,
    control: &dyn ProcessControl,
    shared_active: &AtomicBool,
    pids: &[u32],
    mut sleep: impl FnMut(Duration),
) {
    if lock(settings).auto_toggle {
        let key1 = lock(settings).auto_key1;
        input.hold_key_chord(key1);
        let auto_key1_time_ms = lock(settings).auto_key1_time_ms;
        sleep_ms(auto_key1_time_ms, &mut sleep);
        let key2 = lock(settings).auto_key2;
        input.hold_key_chord(key2);
        let auto_key2_time_ms = lock(settings).auto_key2_time_ms;
        sleep_ms(auto_key2_time_ms, &mut sleep);
    }

    set_pids_suspended(control, pids, true);
    if lock(settings).freeze_delay_apply {
        let check = lock(settings).freeze_delay_override_ms;
        if check > 0 {
            let freeze_delay_override_ms = lock(settings).freeze_delay_override_ms;
            sleep_ms(freeze_delay_override_ms, &mut sleep);
        }
    } else {
        if !lock(settings).fast_hhj {
            sleep(Duration::from_millis(300));
        }
        sleep(Duration::from_millis(200));
    }

    if lock(settings).auto_toggle {
        input.release_key_chord(lock(settings).auto_key1);
        input.release_key_chord(lock(settings).auto_key2);
    }
    set_pids_suspended(control, pids, false);

    let delay1_ms = lock(settings).delay1_ms;
    sleep_ms(delay1_ms, &mut sleep);
    if lock(settings).global_zoom {
        let wheel = if lock(settings).zoom_reverse {
            smu_core::KeyCode::MOUSE_WHEEL_DOWN.raw()
        } else {
            smu_core::KeyCode::MOUSE_WHEEL_UP.raw()
        };
        input.hold_key_chord(Hotkey::new(wheel));
    } else {
        input.hold_key_chord(Hotkey::new(lock(settings).shift_key));
    }

    let delay2_ms = lock(settings).delay2_ms;
    sleep_ms(delay2_ms, &mut sleep);
    if lock(settings).length_ms > 0 {
        shared_active.store(true, Ordering::Release);
    }

    let delay3_ms = lock(settings).delay3_ms;
    sleep_ms(delay3_ms, &mut sleep);
    if !lock(settings).global_zoom {
        input.release_key_chord(Hotkey::new(lock(settings).shift_key));
    }

    if lock(settings).length_ms > 0 {
        let length_ms = lock(settings).length_ms;
        sleep_ms(length_ms, &mut sleep);
        shared_active.store(false, Ordering::Release);
    }
}

fn motion_loop(
    running: &AtomicBool,
    settings: &Mutex<HhjControllerSettings>,
    shared_active: &AtomicBool,
    input: &dyn InputDevice,
) {
    while running.load(Ordering::Acquire) {
        let current = lock(settings).clone();
        if current.macro_enabled && current.section_enabled && shared_active.load(Ordering::Acquire)
        {
            execute_motion_cycle(settings, input, std::thread::sleep);
        } else {
            std::thread::sleep(MOTION_IDLE_POLL_INTERVAL);
        }
    }
}

fn execute_motion_cycle(
    settings: &Mutex<HhjControllerSettings>,
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
    use smu_core::KeyCode;
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
    fn defaults_match_cpp_hhj_runtime() {
        let settings = HhjControllerSettings::default();
        assert_eq!(settings.hotkey, DEFAULT_HHJ_HOTKEY);
        assert!(!settings.section_enabled);
        assert!(!settings.disable_outside_target);
        assert!(!settings.auto_toggle);
        assert!(!settings.fast_hhj);
        assert_eq!(settings.length_ms, 243);
        assert_eq!((settings.delay1_ms, settings.delay2_ms, settings.delay3_ms), (9, 17, 16));
    }

    #[test]
    fn motion_cycle_rereads_y_after_first_wait() {
        let settings = Mutex::new(HhjControllerSettings {
            fps: 200,
            strength_x: 10,
            strength_y: -20,
            ..HhjControllerSettings::default()
        });
        let input = FakeInput::default();
        let mut waits = 0;
        execute_motion_cycle(&settings, &input, |_| {
            waits += 1;
            if waits == 1 {
                lock(&settings).strength_y = -30;
            }
        });
        assert_eq!(*lock(&input.events), ["move:10:0", "move:-30:0"]);
        assert_eq!(waits, 2);
    }

    #[test]
    fn length_can_activate_then_change_to_zero_and_leave_shared_state_active() {
        let settings = Mutex::new(HhjControllerSettings {
            freeze_delay_apply: true,
            freeze_delay_override_ms: 0,
            delay1_ms: 0,
            delay2_ms: 0,
            length_ms: 10,
            delay3_ms: 0,
            ..HhjControllerSettings::default()
        });
        let input = FakeInput::default();
        let control = FakeControl::default();
        let active = AtomicBool::new(false);
        let mut waits = 0;
        execute_worker(&settings, &input, &control, &active, &[7], |_| {
            waits += 1;
            if waits == 3 {
                lock(&settings).length_ms = 0;
            }
        });
        assert!(active.load(Ordering::Acquire));
        assert_eq!(*lock(&control.events), ["suspend:7", "resume:7"]);
    }
}
