//! Live imported-script hotkey controller.
//!
//! The manager owns script workers and cancellation. This controller only owns C++ hotkey edge,
//! global enable, target-foreground eligibility, and the per-record latch vector.

use smu_input::KeyState;
use smu_process::{ProcessDiscovery, ProcessForeground};
use smu_scripting::{ScriptHotkeySnapshot, ScriptManager};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const CONTROLLER_POLL_INTERVAL: Duration = Duration::from_millis(2);
const ACTIVE_FOREGROUND_RECHECK_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptRuntimeSettings {
    pub macro_enabled: bool,
    pub process_selector: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScriptHotkeyAction {
    None,
    Execute,
    ForceStop,
}

pub struct ScriptRuntimeController {
    input: Arc<dyn KeyState>,
    running: Arc<AtomicBool>,
    settings: Arc<Mutex<ScriptRuntimeSettings>>,
    controller: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for ScriptRuntimeController {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ScriptRuntimeController")
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

impl ScriptRuntimeController {
    #[must_use]
    pub fn start(
        input: Arc<dyn KeyState>,
        discovery: Arc<dyn ProcessDiscovery>,
        foreground: Arc<dyn ProcessForeground>,
        manager: Arc<ScriptManager>,
        initial_settings: ScriptRuntimeSettings,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let settings = Arc::new(Mutex::new(initial_settings));
        let controller = {
            let running = Arc::clone(&running);
            let settings = Arc::clone(&settings);
            let worker_input = Arc::clone(&input);
            std::thread::spawn(move || {
                controller_loop(
                    running.as_ref(),
                    settings.as_ref(),
                    worker_input.as_ref(),
                    discovery.as_ref(),
                    foreground.as_ref(),
                    manager.as_ref(),
                );
            })
        };
        Self { input, running, settings, controller: Mutex::new(Some(controller)) }
    }

    pub fn update_settings(&self, settings: ScriptRuntimeSettings) {
        let mut current = lock(&self.settings);
        if *current == settings {
            return;
        }
        *current = settings;
        drop(current);
        self.input.wake_input_waiters();
        if let Some(controller) = lock(&self.controller).as_ref() {
            controller.thread().unpark();
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
        self.input.wake_input_waiters();
        let controller = lock(&self.controller).take();
        if let Some(controller) = controller {
            controller.thread().unpark();
            let _ = controller.join();
        }
    }
}

impl Drop for ScriptRuntimeController {
    fn drop(&mut self) {
        self.stop();
    }
}

fn controller_loop(
    running: &AtomicBool,
    settings: &Mutex<ScriptRuntimeSettings>,
    input: &dyn KeyState,
    discovery: &dyn ProcessDiscovery,
    foreground: &dyn ProcessForeground,
    manager: &ScriptManager,
) {
    let mut targets = crate::target_runtime::TargetProcessCache::default();
    let mut was_pressed = Vec::<bool>::new();
    while running.load(Ordering::Acquire) {
        // Sample before testing physical key state; notifications cannot be lost while dispatching.
        let input_sequence = input.input_change_sequence();
        let current = lock(settings).clone();
        targets.refresh_if_due(&current.process_selector, false, discovery);
        if !current.macro_enabled {
            std::thread::park_timeout(Duration::from_secs(1));
            continue;
        }

        let scripts = manager.hotkey_snapshots();
        if was_pressed.len() != scripts.len() {
            was_pressed = vec![false; scripts.len()];
        }
        for (index, script) in scripts.iter().enumerate() {
            let pressed = script.loaded
                && !script.missing
                && script.hotkey.is_bound()
                && (script.enabled || script.running)
                && crate::runtime_input::hotkey_pressed(input, script.hotkey);
            // Foreground eligibility only affects pressed-key edges. Avoid X11/namespace
            // lookups while no script hotkey is pressed, without changing the 2 ms hotkey
            // sampling interval or the immediate eligibility check on a new key press.
            let foreground_allowed = !pressed
                || !script.disable_outside_roblox
                || targets.foreground_allowed(true, foreground);
            let action =
                decide_script_action(script, was_pressed[index], pressed, foreground_allowed);
            was_pressed[index] = next_latch(script, pressed, foreground_allowed);
            match action {
                ScriptHotkeyAction::None => {}
                ScriptHotkeyAction::Execute => {
                    let _ = manager.spawn_execute_script(index);
                }
                ScriptHotkeyAction::ForceStop => {
                    let _ = manager.force_stop_script(index);
                }
            }
        }
        if scripts.iter().any(|script| script.loaded && (script.enabled || script.running)) {
            if let Some(sequence) = input_sequence {
                // Physical press/release wakes immediately. The existing 25 ms foreground
                // cadence also bounds latency for script state changes made through the UI.
                input.wait_for_input_change(sequence, ACTIVE_FOREGROUND_RECHECK_INTERVAL);
            } else {
                // Preserve the original 2 ms loop on backends without an input notifier.
                std::thread::sleep(CONTROLLER_POLL_INTERVAL);
            }
        } else {
            // Newly imported scripts are discovered within 100 ms; empty script lists don't spin.
            std::thread::park_timeout(Duration::from_millis(100));
        }
    }
}

#[must_use]
const fn decide_script_action(
    script: &ScriptHotkeySnapshot,
    was_pressed: bool,
    pressed: bool,
    foreground_allowed: bool,
) -> ScriptHotkeyAction {
    if !script.loaded || script.missing || !script.hotkey.is_bound() {
        return ScriptHotkeyAction::None;
    }
    if !script.enabled && !script.running {
        return ScriptHotkeyAction::None;
    }
    let edge = pressed && !was_pressed;
    if script.running {
        return if edge { ScriptHotkeyAction::ForceStop } else { ScriptHotkeyAction::None };
    }
    if !foreground_allowed || !edge {
        ScriptHotkeyAction::None
    } else {
        ScriptHotkeyAction::Execute
    }
}

#[must_use]
const fn next_latch(
    script: &ScriptHotkeySnapshot,
    pressed: bool,
    foreground_allowed: bool,
) -> bool {
    if !script.loaded || script.missing || !script.hotkey.is_bound() {
        return false;
    }
    if !script.enabled && !script.running {
        return false;
    }
    if script.running {
        return pressed;
    }
    if script.disable_outside_roblox && !foreground_allowed {
        return false;
    }
    pressed
}

#[cfg(test)]
mod tests {
    use super::*;
    use smu_core::Hotkey;

    fn snapshot() -> ScriptHotkeySnapshot {
        ScriptHotkeySnapshot {
            hotkey: Hotkey::new(0x41),
            enabled: true,
            disable_outside_roblox: true,
            loaded: true,
            missing: false,
            running: false,
        }
    }

    #[test]
    fn running_edge_force_stops_before_foreground_gate() {
        let mut script = snapshot();
        script.running = true;
        assert_eq!(
            decide_script_action(&script, false, true, false),
            ScriptHotkeyAction::ForceStop
        );
        assert!(next_latch(&script, true, false));
    }

    #[test]
    fn foreground_rejection_resets_latch_for_future_held_key_edge() {
        let script = snapshot();
        assert_eq!(decide_script_action(&script, false, true, false), ScriptHotkeyAction::None);
        assert!(!next_latch(&script, true, false));
        assert_eq!(decide_script_action(&script, false, true, true), ScriptHotkeyAction::Execute);
    }

    #[test]
    fn disabled_idle_and_invalid_records_reset_latches() {
        let mut script = snapshot();
        script.enabled = false;
        assert!(!next_latch(&script, true, true));
        script.enabled = true;
        script.loaded = false;
        assert!(!next_latch(&script, true, true));
        script.loaded = true;
        script.hotkey = Hotkey::new(smu_scripting::SCRIPT_UNBOUND_HOTKEY);
        assert!(!next_latch(&script, true, true));
    }
}
