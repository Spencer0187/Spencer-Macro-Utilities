//! Live Linux multi-instance `PressKey` controller.
//!
//! Per-instance rising-edge latches reset together when the instance count changes. Each edge
//! snapshots trigger/output/delays before spawning an overlapping one-shot worker. The worker keeps
//! the C++ same-key release quirk and checks runtime cancellation only after the optional bonus delay.

use smu_core::Hotkey;
use smu_input::InputDevice;
use smu_process::{ProcessDiscovery, ProcessForeground};
use smu_runtime::macros::{PressKeyWorkerPlan, build_presskey_worker_plan};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PressKeyInstanceSettings {
    pub section_enabled: bool,
    pub trigger: Hotkey,
    pub output: Hotkey,
    pub disable_outside_target: bool,
    pub press_delay_ms: i32,
    pub bonus_delay_ms: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PressKeyControllerSettings {
    pub macro_enabled: bool,
    pub process_selector: String,
    pub instances: Vec<PressKeyInstanceSettings>,
}

pub struct PressKeyController {
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<PressKeyControllerSettings>>,
    controller: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for PressKeyController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PressKeyController")
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

impl PressKeyController {
    #[must_use]
    pub fn start(
        input: Arc<dyn InputDevice>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        initial_settings: PressKeyControllerSettings,
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

    pub fn update_settings(&self, settings: PressKeyControllerSettings) {
        let mut current = lock(&self.settings);
        if *current == settings {
            return;
        }
        *current = settings;
        drop(current);
        if let Some(controller) = lock(&self.controller).as_ref() {
            controller.thread().unpark();
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

impl Drop for PressKeyController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn controller_loop(
    running: &Arc<AtomicBool>,
    settings: &Arc<Mutex<PressKeyControllerSettings>>,
    input: &Arc<dyn InputDevice>,
    discovery: &Arc<dyn ProcessDiscovery>,
    foreground: &Arc<dyn ProcessForeground>,
) {
    let mut targets = crate::target_runtime::TargetProcessCache::default();
    let mut was_pressed = Vec::<bool>::new();
    let mut workers = Vec::<JoinHandle<()>>::new();
    while running.load(Ordering::Acquire) {
        prune_finished_workers(&mut workers);
        let current = lock(settings).clone();
        targets.refresh_if_due(&current.process_selector, false, discovery.as_ref());
        if !current.macro_enabled {
            std::thread::park_timeout(Duration::from_secs(1));
            continue;
        }
        if was_pressed.len() != current.instances.len() {
            was_pressed = vec![false; current.instances.len()];
        }
        for (index, instance) in current.instances.iter().enumerate() {
            if !instance.section_enabled {
                was_pressed[index] = false;
                continue;
            }
            let allowed =
                targets.foreground_allowed(instance.disable_outside_target, foreground.as_ref());
            let pressed =
                allowed && crate::runtime_input::hotkey_pressed(input.as_ref(), instance.trigger);
            let rising = pressed && !was_pressed[index];
            was_pressed[index] = pressed;
            if !rising {
                continue;
            }
            let plan = build_presskey_worker_plan(
                instance.trigger.raw(),
                instance.output.raw(),
                instance.bonus_delay_ms,
                instance.press_delay_ms,
            );
            let input = Arc::clone(input);
            let running = Arc::clone(running);
            workers.push(std::thread::spawn(move || {
                execute_worker(plan, input.as_ref(), running.as_ref(), std::thread::sleep);
            }));
        }
        if current.macro_enabled
            && current.instances.iter().any(|instance| instance.section_enabled)
        {
            std::thread::sleep(CONTROLLER_POLL_INTERVAL);
        } else {
            // Park inactive instances; update_settings wakes this thread synchronously.
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
    plan: PressKeyWorkerPlan,
    input: &dyn InputDevice,
    running: &AtomicBool,
    mut sleep: impl FnMut(Duration),
) {
    if plan.release_trigger_first {
        input.release_key_chord(Hotkey::new(plan.trigger));
    }
    if plan.bonus_delay_ms > 0 {
        sleep(Duration::from_millis(u64::try_from(plan.bonus_delay_ms).unwrap_or_default()));
    }
    if !running.load(Ordering::Acquire) {
        return;
    }
    input.hold_key_chord(Hotkey::new(plan.output));
    sleep(Duration::from_millis(u64::try_from(plan.press_delay_ms).unwrap_or_default()));
    input.release_key_chord(Hotkey::new(plan.output));
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
        fn move_mouse(&self, _dx: i32, _dy: i32) {}
        fn mouse_wheel(&self, delta: i32) {
            lock(&self.events).push(format!("wheel:{delta}"));
        }
    }

    #[test]
    fn worker_matches_characterized_same_key_release_and_timing() {
        let input = FakeInput::default();
        let running = AtomicBool::new(true);
        let plan = build_presskey_worker_plan(0x44, 0x44, 7, 16);
        let mut waits = Vec::new();
        execute_worker(plan, &input, &running, |duration| waits.push(duration));
        assert_eq!(*lock(&input.events), ["up:68", "down:68", "up:68"]);
        assert_eq!(waits, [Duration::from_millis(7), Duration::from_millis(16)]);
    }

    #[test]
    fn cancelled_after_bonus_does_not_press_output() {
        let input = FakeInput::default();
        let running = AtomicBool::new(true);
        let plan = build_presskey_worker_plan(KeyCode::Z.raw(), 0x44, 7, 16);
        execute_worker(plan, &input, &running, |_| running.store(false, Ordering::Release));
        assert!(lock(&input.events).is_empty());
    }
}
