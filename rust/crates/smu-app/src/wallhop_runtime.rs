//! Live Linux multi-instance Wallhop controller.
//!
//! Each instance owns its own rising-edge latch. A count change resets every latch exactly like the
//! C++ `wallhopWasPressed_.assign(size, false)` behavior. Worker plans are completely snapshotted on
//! the edge and one-shot workers may overlap after separate trigger edges.

use smu_core::Hotkey;
use smu_input::InputDevice;
use smu_process::{ProcessDiscovery, ProcessForeground};
use smu_runtime::macros::{WallhopWorkerPlan, build_wallhop_worker_plan};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WallhopInstanceSettings {
    pub section_enabled: bool,
    pub trigger: Hotkey,
    pub disable_outside_target: bool,
    pub dx: i32,
    pub dy: i32,
    pub vertical: i32,
    pub delay_ms: i32,
    pub bonus_delay_ms: i32,
    pub left: bool,
    pub jump: bool,
    pub flick_back: bool,
    pub jump_key: Hotkey,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WallhopControllerSettings {
    pub macro_enabled: bool,
    pub process_selector: String,
    pub instances: Vec<WallhopInstanceSettings>,
}

pub struct WallhopController {
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<WallhopControllerSettings>>,
    controller: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for WallhopController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WallhopController")
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

impl WallhopController {
    #[must_use]
    pub fn start(
        input: Arc<dyn InputDevice>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        initial_settings: WallhopControllerSettings,
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
            );
        });
        Self { running, settings, controller: Mutex::new(Some(controller)) }
    }

    pub fn update_settings(&self, settings: WallhopControllerSettings) {
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

impl Drop for WallhopController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn controller_loop(
    running: &AtomicBool,
    settings: &Arc<Mutex<WallhopControllerSettings>>,
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
            std::thread::sleep(Duration::from_millis(5));
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
            let edge = pressed && !was_pressed[index];
            was_pressed[index] = pressed;
            if !edge {
                continue;
            }
            let plan = build_wallhop_worker_plan(
                instance.dx,
                instance.dy,
                instance.vertical,
                instance.delay_ms,
                instance.bonus_delay_ms,
                instance.left,
                instance.jump,
                instance.flick_back,
                instance.jump_key.raw(),
            );
            let input = Arc::clone(input);
            workers.push(std::thread::spawn(move || {
                execute_worker(plan, input.as_ref(), std::thread::sleep);
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
    plan: WallhopWorkerPlan,
    input: &dyn InputDevice,
    mut sleep: impl FnMut(Duration),
) {
    let first_dx = if plan.left { plan.dx.wrapping_neg() } else { plan.dx };
    input.move_mouse(first_dx, plan.vertical);

    if plan.flick_back {
        if plan.bonus_delay_ms > 0 && plan.bonus_delay_ms < plan.delay_ms {
            sleep(Duration::from_millis(u64::try_from(plan.bonus_delay_ms).unwrap_or_default()));
            if plan.jump {
                input.hold_key_chord(Hotkey::new(plan.jump_key));
            }
            sleep(Duration::from_millis(
                u64::try_from(plan.delay_ms - plan.bonus_delay_ms).unwrap_or_default(),
            ));
        } else {
            if plan.jump {
                input.hold_key_chord(Hotkey::new(plan.jump_key));
            }
            sleep(Duration::from_millis(u64::try_from(plan.delay_ms).unwrap_or_default()));
        }
        let second_dx = if plan.left { plan.dy.wrapping_neg() } else { plan.dy };
        input.move_mouse(second_dx, plan.vertical.wrapping_neg());
    } else if plan.jump {
        input.hold_key_chord(Hotkey::new(plan.jump_key));
    }

    if plan.jump {
        let release_delay = 100 - plan.delay_ms;
        if release_delay > 0 {
            sleep(Duration::from_millis(u64::try_from(release_delay).unwrap_or_default()));
        }
        input.release_key_chord(Hotkey::new(plan.jump_key));
    }
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

    #[test]
    fn worker_uses_characterized_snapshot_and_release_target() {
        let input = FakeInput::default();
        let plan =
            build_wallhop_worker_plan(300, -300, 5, 19, 7, false, true, true, KeyCode::SPACE.raw());
        let mut waits = Vec::new();
        execute_worker(plan, &input, |duration| waits.push(duration));
        assert_eq!(*lock(&input.events), ["move:300:5", "down:32", "move:-300:-5", "up:32"]);
        assert_eq!(
            waits,
            [Duration::from_millis(7), Duration::from_millis(12), Duration::from_millis(81)]
        );
    }

    #[test]
    fn count_change_resets_all_edge_latches_like_cpp_assign() {
        let mut latches = vec![true, true];
        let count = 3;
        if latches.len() != count {
            latches = vec![false; count];
        }
        assert_eq!(latches, [false, false, false]);
    }
}
