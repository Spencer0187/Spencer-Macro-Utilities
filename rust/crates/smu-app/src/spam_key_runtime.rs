//! Live Linux multi-instance `SpamKey` controller.
//!
//! Runtime slots are stable by index so C++'s copy-left/pop-back removal semantics cannot leave a
//! worker with a dangling reference. A slot owns one long-lived worker lane; the controller only
//! changes its active flag. Each cycle snapshots delay and key-down before the first wait, then
//! deliberately re-reads the output key for key-up after that wait.

use smu_core::Hotkey;
use smu_input::InputDevice;
use smu_process::{ProcessDiscovery, ProcessForeground};
use smu_runtime::macros::{
    SpamKeyThreadAction, apply_spam_key_thread_action, evaluate_spam_key_controller,
};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);
const IDLE_WORKER_POLL_INTERVAL: Duration = Duration::from_millis(2);

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpamKeyInstanceSettings {
    pub section_enabled: bool,
    pub trigger: Hotkey,
    pub output: Hotkey,
    pub disable_outside_target: bool,
    pub hold_mode: bool,
    pub real_delay_ms: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpamKeyControllerSettings {
    pub macro_enabled: bool,
    pub process_selector: String,
    pub instances: Vec<SpamKeyInstanceSettings>,
}

struct RuntimeSlot {
    settings: Arc<Mutex<SpamKeyInstanceSettings>>,
    active: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl RuntimeSlot {
    fn new(
        settings: SpamKeyInstanceSettings,
        running: Arc<AtomicBool>,
        input: Arc<dyn InputDevice>,
    ) -> Self {
        let settings = Arc::new(Mutex::new(settings));
        let active = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));
        let worker_settings = Arc::clone(&settings);
        let worker_active = Arc::clone(&active);
        let worker_alive = Arc::clone(&alive);
        let worker = std::thread::spawn(move || {
            worker_loop(
                running.as_ref(),
                worker_alive.as_ref(),
                worker_active.as_ref(),
                worker_settings.as_ref(),
                input.as_ref(),
            );
        });
        Self { settings, active, alive, worker: Some(worker) }
    }

    fn stop(&mut self) {
        self.active.store(false, Ordering::Release);
        self.alive.store(false, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl Drop for RuntimeSlot {
    fn drop(&mut self) {
        self.stop();
    }
}

pub struct SpamKeyController {
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<SpamKeyControllerSettings>>,
    controller: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for SpamKeyController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SpamKeyController")
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

impl SpamKeyController {
    #[must_use]
    pub fn start(
        input: Arc<dyn InputDevice>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        initial_settings: SpamKeyControllerSettings,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let settings = Arc::new(Mutex::new(initial_settings));
        let worker_running = Arc::clone(&running);
        let worker_settings = Arc::clone(&settings);
        let controller = std::thread::spawn(move || {
            controller_loop(&worker_running, &worker_settings, &input, &discovery, &foreground);
        });
        Self { running, settings, controller: Mutex::new(Some(controller)) }
    }

    pub fn update_settings(&self, settings: SpamKeyControllerSettings) {
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

impl Drop for SpamKeyController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn controller_loop(
    running: &Arc<AtomicBool>,
    settings: &Arc<Mutex<SpamKeyControllerSettings>>,
    input: &Arc<dyn InputDevice>,
    discovery: &Arc<dyn ProcessDiscovery>,
    foreground: &Arc<dyn ProcessForeground>,
) {
    let mut targets = crate::target_runtime::TargetProcessCache::default();
    let mut was_pressed = Vec::<bool>::new();
    let mut slots = Vec::<RuntimeSlot>::new();
    while running.load(Ordering::Acquire) {
        let current = lock(settings).clone();
        targets.refresh_if_due(&current.process_selector, false, discovery.as_ref());
        resize_slots(&mut slots, &current.instances, running, input);
        if !current.macro_enabled {
            for slot in &slots {
                slot.active.store(false, Ordering::Release);
            }
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        if was_pressed.len() != current.instances.len() {
            was_pressed = vec![false; current.instances.len()];
        }

        for (index, instance) in current.instances.iter().enumerate() {
            *lock(&slots[index].settings) = instance.clone();
            let allowed = instance.section_enabled
                && targets.foreground_allowed(instance.disable_outside_target, foreground.as_ref());
            let pressed =
                allowed && crate::runtime_input::hotkey_pressed(input.as_ref(), instance.trigger);
            let decision = evaluate_spam_key_controller(
                instance.section_enabled,
                allowed,
                pressed,
                was_pressed[index],
                instance.hold_mode,
            );
            was_pressed[index] = decision.next_was_pressed;
            apply_thread_action(decision.thread_action, slots[index].active.as_ref());
        }
        std::thread::sleep(CONTROLLER_POLL_INTERVAL);
    }
    for slot in &mut slots {
        slot.stop();
    }
}

fn resize_slots(
    slots: &mut Vec<RuntimeSlot>,
    instances: &[SpamKeyInstanceSettings],
    running: &Arc<AtomicBool>,
    input: &Arc<dyn InputDevice>,
) {
    while slots.len() > instances.len() {
        if let Some(mut removed) = slots.pop() {
            removed.stop();
        }
    }
    while slots.len() < instances.len() {
        let index = slots.len();
        slots.push(RuntimeSlot::new(
            instances[index].clone(),
            Arc::clone(running),
            Arc::clone(input),
        ));
    }
}

fn apply_thread_action(action: SpamKeyThreadAction, active: &AtomicBool) {
    let current = active.load(Ordering::Acquire);
    let outcome = apply_spam_key_thread_action(action, current);
    active.store(outcome.next_thread_active, Ordering::Release);
}

fn worker_loop(
    running: &AtomicBool,
    alive: &AtomicBool,
    active: &AtomicBool,
    settings: &Mutex<SpamKeyInstanceSettings>,
    input: &dyn InputDevice,
) {
    while running.load(Ordering::Acquire) && alive.load(Ordering::Acquire) {
        let current = lock(settings).clone();
        if active.load(Ordering::Acquire) && current.section_enabled {
            execute_iteration(settings, input, std::thread::sleep);
        } else {
            std::thread::sleep(IDLE_WORKER_POLL_INTERVAL);
        }
    }
    active.store(false, Ordering::Release);
}

fn execute_iteration(
    settings: &Mutex<SpamKeyInstanceSettings>,
    input: &dyn InputDevice,
    mut sleep: impl FnMut(Duration),
) {
    let (delay_ms, key_down) = {
        let current = lock(settings);
        (current.real_delay_ms.max(1), current.output)
    };
    input.hold_key_chord(key_down);
    sleep(Duration::from_millis(u64::try_from(delay_ms).unwrap_or_default()));
    let key_up = lock(settings).output;
    input.release_key_chord(key_up);
    sleep(Duration::from_millis(u64::try_from(delay_ms).unwrap_or_default()));
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

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
        fn move_mouse(&self, _dx: i32, _dy: i32) {}
        fn mouse_wheel(&self, delta: i32) {
            lock(&self.events).push(format!("wheel:{delta}"));
        }
    }

    fn settings() -> SpamKeyInstanceSettings {
        SpamKeyInstanceSettings {
            section_enabled: true,
            trigger: Hotkey::new(0xDB),
            output: Hotkey::new(KeyCode::LBUTTON.raw()),
            disable_outside_target: false,
            hold_mode: false,
            real_delay_ms: 10,
        }
    }

    #[test]
    fn iteration_rereads_release_key_after_first_wait() {
        let settings = Mutex::new(settings());
        let input = FakeInput::default();
        let mut waits = Vec::new();
        execute_iteration(&settings, &input, |duration| {
            waits.push(duration);
            if waits.len() == 1 {
                lock(&settings).output = Hotkey::new(0x44);
            }
        });
        assert_eq!(*lock(&input.events), ["down:1", "up:68"]);
        assert_eq!(waits, [Duration::from_millis(10), Duration::from_millis(10)]);
    }

    #[test]
    fn copy_left_removal_keeps_surviving_slot_identity() {
        let running = Arc::new(AtomicBool::new(true));
        let input: Arc<dyn InputDevice> = Arc::new(FakeInput::default());
        let mut slots = vec![
            RuntimeSlot::new(settings(), Arc::clone(&running), Arc::clone(&input)),
            RuntimeSlot::new(settings(), Arc::clone(&running), Arc::clone(&input)),
            RuntimeSlot::new(settings(), Arc::clone(&running), Arc::clone(&input)),
        ];
        let surviving_slot = Arc::as_ptr(&slots[1].settings);
        let mut shifted = settings();
        shifted.output = Hotkey::new(0x44);
        let instances = vec![settings(), shifted.clone()];

        resize_slots(&mut slots, &instances, &running, &input);
        assert_eq!(slots.len(), 2);
        assert_eq!(Arc::as_ptr(&slots[1].settings), surviving_slot);
        *lock(&slots[1].settings) = shifted.clone();
        assert_eq!(*lock(&slots[1].settings), shifted);

        running.store(false, Ordering::Release);
        for slot in &mut slots {
            slot.stop();
        }
    }

    #[test]
    fn controller_actions_match_hold_and_toggle_state_transitions() {
        let active = AtomicBool::new(false);
        apply_thread_action(SpamKeyThreadAction::HoldEnsureActive, &active);
        assert!(active.load(Ordering::Acquire));
        apply_thread_action(SpamKeyThreadAction::Toggle, &active);
        assert!(!active.load(Ordering::Acquire));
        apply_thread_action(SpamKeyThreadAction::Toggle, &active);
        assert!(active.load(Ordering::Acquire));
        apply_thread_action(SpamKeyThreadAction::Clear, &active);
        assert!(!active.load(Ordering::Acquire));
    }
}
