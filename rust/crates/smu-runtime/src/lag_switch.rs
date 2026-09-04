//! Lag Switch controller semantics frozen from the C++ runtime.
//!
//! The network transport owns packet interception and its dynamic-target worker. This module owns
//! only the non-blocking controller decisions: foreground eligibility, hold/toggle edge behavior,
//! backend initialization, config publication, start-time reset, and auto-unlag.

#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LagSwitchStepInput {
    pub section_enabled: bool,
    pub backend_present: bool,
    pub foreground_allowed: bool,
    pub pressed: bool,
    pub was_pressed: bool,
    pub toggle_mode: bool,
    pub backend_enabled: bool,
    pub has_last_config: bool,
    pub auto_unblock: bool,
    pub max_duration_seconds: f32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LagSwitchStepResult {
    pub next_was_pressed: bool,
    pub next_has_last_config: bool,
}

/// Side effects used by one Lag Switch controller poll.
pub trait LagSwitchStepContext {
    /// Initialize the native network backend when the controller first needs it.
    fn initialize_backend(&mut self) -> bool;

    /// Whether the UI/runtime config differs from the controller's cached last config.
    fn config_changed(&self) -> bool;

    /// Publish the new base config to the network service.
    fn apply_config(&mut self);

    /// Read the base (non-script) block state.
    fn is_base_blocking(&self) -> bool;

    /// Set the base block state.
    fn set_blocking(&mut self, active: bool);

    /// Reset the auto-unlag interval start to the current monotonic time.
    fn reset_start(&mut self);

    /// Elapsed seconds since the last activating transition.
    fn elapsed_seconds(&mut self) -> f32;
}

/// Execute one legacy Lag Switch controller poll without introducing waits or worker-side effects.
///
/// The order intentionally mirrors `MacroRuntime::processLagSwitchMacro`: section cleanup and
/// foreground loss happen before hotkey-driven initialization; initialization failure returns before
/// config publication; config publication precedes the hold/toggle state transition; auto-unlag is
/// evaluated last.
pub fn execute_lag_switch_step(
    mut input: LagSwitchStepInput,
    context: &mut impl LagSwitchStepContext,
) -> LagSwitchStepResult {
    if !input.section_enabled {
        if (input.was_pressed || input.has_last_config) && input.backend_present {
            context.set_blocking(false);
        }
        return LagSwitchStepResult { next_was_pressed: false, next_has_last_config: false };
    }

    if !input.backend_present {
        return LagSwitchStepResult {
            next_was_pressed: input.was_pressed,
            next_has_last_config: input.has_last_config,
        };
    }

    if !input.foreground_allowed {
        if input.was_pressed || context.is_base_blocking() {
            context.set_blocking(false);
        }
        return LagSwitchStepResult {
            next_was_pressed: false,
            next_has_last_config: input.has_last_config,
        };
    }

    let should_initialize = if input.toggle_mode {
        input.pressed && !input.was_pressed && !context.is_base_blocking()
    } else {
        input.pressed
    };
    if should_initialize && !input.backend_enabled && !context.initialize_backend() {
        return LagSwitchStepResult {
            next_was_pressed: input.pressed,
            next_has_last_config: input.has_last_config,
        };
    }

    if context.config_changed() {
        context.apply_config();
        input.has_last_config = true;
    }

    if input.toggle_mode {
        if input.pressed && !input.was_pressed {
            let next_active = !context.is_base_blocking();
            context.set_blocking(next_active);
            if next_active {
                context.reset_start();
            }
        }
    } else {
        if input.pressed && !context.is_base_blocking() {
            context.reset_start();
        }
        context.set_blocking(input.pressed);
    }

    input.was_pressed = input.pressed;
    if input.auto_unblock
        && context.is_base_blocking()
        && context.elapsed_seconds() >= input.max_duration_seconds
    {
        context.set_blocking(false);
    }

    LagSwitchStepResult {
        next_was_pressed: input.was_pressed,
        next_has_last_config: input.has_last_config,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{LagSwitchStepContext, LagSwitchStepInput, execute_lag_switch_step};
    use serde::Deserialize;
    use serde_json::{Value, json};

    const FIXTURE: &str = include_str!("../../../../compat/timing/runtime_cases.json");

    #[derive(Debug, Deserialize)]
    struct Fixture {
        lag_switch_cases: Vec<LagSwitchCase>,
    }

    #[allow(clippy::struct_excessive_bools)]
    #[derive(Debug, Deserialize)]
    struct LagSwitchCase {
        name: String,
        spec: LagSwitchSpec,
        next_was_pressed: bool,
        next_has_last_config: bool,
        final_base_blocking: bool,
        config_applied: bool,
        start_reset: bool,
        events: Vec<Value>,
    }

    #[allow(clippy::struct_excessive_bools)]
    #[derive(Clone, Copy, Debug, Deserialize)]
    struct LagSwitchSpec {
        section_enabled: bool,
        backend_present: bool,
        foreground_allowed: bool,
        pressed: bool,
        was_pressed: bool,
        toggle_mode: bool,
        backend_enabled: bool,
        has_last_config: bool,
        base_blocking: bool,
        init_success: bool,
        config_changed: bool,
        auto_unblock: bool,
        max_duration_seconds: f32,
        elapsed_seconds: f32,
    }

    #[allow(clippy::struct_excessive_bools)]
    #[derive(Debug)]
    struct FakeContext {
        base_blocking: bool,
        init_success: bool,
        config_changed: bool,
        elapsed_seconds: f32,
        config_applied: bool,
        start_reset: bool,
        events: Vec<Value>,
    }

    impl FakeContext {
        fn new(spec: LagSwitchSpec) -> Self {
            Self {
                base_blocking: spec.base_blocking,
                init_success: spec.init_success,
                config_changed: spec.config_changed,
                elapsed_seconds: spec.elapsed_seconds,
                config_applied: false,
                start_reset: false,
                events: Vec::new(),
            }
        }
    }

    impl LagSwitchStepContext for FakeContext {
        fn initialize_backend(&mut self) -> bool {
            self.events.push(json!({
                "action": "initialize_backend",
                "success": self.init_success,
            }));
            self.init_success
        }

        fn config_changed(&self) -> bool {
            self.config_changed
        }

        fn apply_config(&mut self) {
            self.config_applied = true;
            self.events.push(json!({"action": "set_config"}));
        }

        fn is_base_blocking(&self) -> bool {
            self.base_blocking
        }

        fn set_blocking(&mut self, active: bool) {
            self.base_blocking = active;
            self.events.push(json!({"action": "set_blocking", "active": active}));
        }

        fn reset_start(&mut self) {
            self.start_reset = true;
            self.events.push(json!({"action": "reset_start_time"}));
        }

        fn elapsed_seconds(&mut self) -> f32 {
            self.events.push(json!({
                "action": "elapsed_seconds",
                "value": f64::from(self.elapsed_seconds),
            }));
            self.elapsed_seconds
        }
    }

    #[test]
    fn cpp_lag_switch_controller_cases_match() {
        let fixture: Fixture =
            serde_json::from_str(FIXTURE).expect("C++ generated runtime fixture should parse");
        assert_eq!(fixture.lag_switch_cases.len(), 140);

        for case in fixture.lag_switch_cases {
            let spec = case.spec;
            let mut context = FakeContext::new(spec);
            let result = execute_lag_switch_step(
                LagSwitchStepInput {
                    section_enabled: spec.section_enabled,
                    backend_present: spec.backend_present,
                    foreground_allowed: spec.foreground_allowed,
                    pressed: spec.pressed,
                    was_pressed: spec.was_pressed,
                    toggle_mode: spec.toggle_mode,
                    backend_enabled: spec.backend_enabled,
                    has_last_config: spec.has_last_config,
                    auto_unblock: spec.auto_unblock,
                    max_duration_seconds: spec.max_duration_seconds,
                },
                &mut context,
            );

            let events = context.events.clone();

            assert_eq!(
                result.next_was_pressed, case.next_was_pressed,
                "case {} pressed state",
                case.name
            );
            assert_eq!(
                result.next_has_last_config, case.next_has_last_config,
                "case {} config cache",
                case.name
            );
            assert_eq!(
                context.base_blocking, case.final_base_blocking,
                "case {} base blocking",
                case.name
            );
            assert_eq!(
                context.config_applied, case.config_applied,
                "case {} config applied",
                case.name
            );
            assert_eq!(context.start_reset, case.start_reset, "case {} start reset", case.name);
            assert_eq!(events, case.events, "case {} event order", case.name);
        }
    }
}
