//! Process-freeze Lua API adapter.

use crate::input_api::{ScriptExecutionMode, ScriptInputContext};
use crate::lua_runtime::LuaScriptVm;
use mlua::{Error as LuaError, Value};
use smu_process::{ProcessControl, ProcessDiscovery};
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone, Debug)]
pub struct ScriptProcessContext {
    selector: Arc<Mutex<String>>,
    take_all_process_ids: Arc<Mutex<bool>>,
    frozen_pids: Arc<Mutex<Vec<u32>>>,
}

impl Default for ScriptProcessContext {
    fn default() -> Self {
        Self {
            selector: Arc::new(Mutex::new(String::new())),
            take_all_process_ids: Arc::new(Mutex::new(false)),
            frozen_pids: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl ScriptProcessContext {
    fn selector_guard(&self) -> MutexGuard<'_, String> {
        match self.selector.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn take_all_guard(&self) -> MutexGuard<'_, bool> {
        match self.take_all_process_ids.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    fn frozen_guard(&self) -> MutexGuard<'_, Vec<u32>> {
        match self.frozen_pids.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    pub fn set_target_selector(&self, selector: impl Into<String>, take_all_process_ids: bool) {
        *self.selector_guard() = selector.into();
        *self.take_all_guard() = take_all_process_ids;
    }

    #[must_use]
    pub fn target_selector(&self) -> String {
        self.selector_guard().clone()
    }

    #[must_use]
    pub fn take_all_process_ids(&self) -> bool {
        *self.take_all_guard()
    }

    #[must_use]
    pub fn frozen_pids(&self) -> Vec<u32> {
        self.frozen_guard().clone()
    }
}

#[derive(Debug)]
pub struct ScriptProcessApiError(String);

impl ScriptProcessApiError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ScriptProcessApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ScriptProcessApiError {}

impl From<LuaError> for ScriptProcessApiError {
    fn from(error: LuaError) -> Self {
        Self::new(error.to_string())
    }
}

fn settings_callback_active(context: &ScriptInputContext) -> bool {
    context.mode() == ScriptExecutionMode::SettingsCallback
}

fn discover_targets(
    process_context: &ScriptProcessContext,
    discovery: Option<&Arc<dyn ProcessDiscovery>>,
) -> Vec<u32> {
    let Some(discovery) = discovery else {
        return Vec::new();
    };
    let selector = process_context.target_selector();
    if process_context.take_all_process_ids() {
        return discovery.find_all_processes(&selector).unwrap_or_default();
    }
    discovery.find_main_process(&selector).ok().flatten().into_iter().collect()
}

fn set_freeze(
    enabled: bool,
    process_context: &ScriptProcessContext,
    discovery: Option<&Arc<dyn ProcessDiscovery>>,
    control: Option<&Arc<dyn ProcessControl>>,
) {
    let Some(control) = control else {
        return;
    };

    if enabled {
        let pids = {
            let mut frozen = process_context.frozen_guard();
            if frozen.is_empty() {
                *frozen = discover_targets(process_context, discovery);
            }
            frozen.clone()
        };
        for pid in pids {
            let _ = control.suspend(pid);
        }
        return;
    }

    let pids = {
        let mut frozen = process_context.frozen_guard();
        std::mem::take(&mut *frozen)
    };
    for pid in pids {
        let _ = control.resume(pid);
    }
}

/// Register `freeze` and its two Roblox aliases against modular process services.
///
/// # Errors
///
/// Returns a Lua registration error if the functions cannot be installed.
pub fn install_process_api(
    vm: &LuaScriptVm,
    execution_context: ScriptInputContext,
    process_context: ScriptProcessContext,
    discovery: Option<Arc<dyn ProcessDiscovery>>,
    control: Option<Arc<dyn ProcessControl>>,
) -> Result<(), ScriptProcessApiError> {
    let resources = (execution_context, process_context, discovery, control);
    let execution_context = resources.0.clone();
    let process_context = resources.1.clone();
    let discovery = resources.2.clone();
    let control = resources.3.clone();

    let freeze = vm.lua.create_function(move |_, value: Value| {
        if settings_callback_active(&execution_context) {
            return Err(LuaError::RuntimeError(
                "freeze is not available inside script settings".to_owned(),
            ));
        }
        let enabled = !matches!(value, Value::Nil | Value::Boolean(false));
        if execution_context.mode() == ScriptExecutionMode::Cleanup && enabled {
            return Err(LuaError::RuntimeError(
                "freeze(true) is not available inside onCleanup".to_owned(),
            ));
        }
        set_freeze(enabled, &process_context, discovery.as_ref(), control.as_ref());
        Ok(())
    })?;

    vm.lua.globals().set("freeze", freeze.clone())?;
    vm.lua.globals().set("robloxFreeze", freeze.clone())?;
    vm.lua.globals().set("roblox_freeze", freeze)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use smu_process::ProcessRecord;
    use std::io;
    use std::sync::Mutex;

    #[derive(Debug)]
    struct FakeDiscovery {
        records: Vec<ProcessRecord>,
    }

    impl ProcessDiscovery for FakeDiscovery {
        fn snapshot(&self) -> io::Result<Vec<ProcessRecord>> {
            Ok(self.records.clone())
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Event {
        Suspend(u32),
        Resume(u32),
    }

    #[derive(Debug, Default)]
    struct FakeControl {
        events: Mutex<Vec<Event>>,
    }

    impl FakeControl {
        fn events(&self) -> Vec<Event> {
            self.events.lock().expect("process event lock").clone()
        }
    }

    impl ProcessControl for FakeControl {
        fn suspend(&self, pid: u32) -> bool {
            self.events.lock().expect("process event lock").push(Event::Suspend(pid));
            true
        }

        fn resume(&self, pid: u32) -> bool {
            self.events.lock().expect("process event lock").push(Event::Resume(pid));
            true
        }
    }

    fn record(pid: u32, parent_pid: Option<u32>) -> ProcessRecord {
        ProcessRecord {
            pid,
            parent_pid,
            command_name: Some("game".to_owned()),
            executable_name: None,
        }
    }

    fn vm_with_process() -> (LuaScriptVm, ScriptInputContext, ScriptProcessContext, Arc<FakeControl>)
    {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let execution = ScriptInputContext::default();
        let process = ScriptProcessContext::default();
        process.set_target_selector("game", false);
        let discovery =
            Arc::new(FakeDiscovery { records: vec![record(20, Some(1)), record(21, Some(20))] });
        let control = Arc::new(FakeControl::default());
        install_process_api(
            &vm,
            execution.clone(),
            process.clone(),
            Some(discovery),
            Some(control.clone()),
        )
        .expect("process API should install");
        (vm, execution, process, control)
    }

    #[test]
    fn main_process_is_snapshotted_only_on_first_enable() {
        let (vm, _, process, control) = vm_with_process();
        vm.execute_source("freeze(true); freeze(true)", "freeze.lua")
            .expect("freeze should execute");
        assert_eq!(process.frozen_pids(), [20]);
        assert_eq!(control.events(), [Event::Suspend(20), Event::Suspend(20)]);
    }

    #[test]
    fn disable_resumes_snapshotted_pids_and_clears_state() {
        let (vm, _, process, control) = vm_with_process();
        vm.execute_source("freeze(true); freeze(false)", "freeze.lua")
            .expect("freeze toggle should execute");
        assert!(process.frozen_pids().is_empty());
        assert_eq!(control.events(), [Event::Suspend(20), Event::Resume(20)]);
    }

    #[test]
    fn take_all_process_ids_uses_all_matching_pids() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        let execution = ScriptInputContext::default();
        let process = ScriptProcessContext::default();
        process.set_target_selector("game", true);
        let discovery =
            Arc::new(FakeDiscovery { records: vec![record(20, Some(1)), record(21, Some(20))] });
        let control = Arc::new(FakeControl::default());
        install_process_api(
            &vm,
            execution,
            process.clone(),
            Some(discovery),
            Some(control.clone()),
        )
        .expect("process API should install");
        vm.execute_source("freeze(true)", "all.lua").expect("freeze should execute");
        assert_eq!(process.frozen_pids(), [20, 21]);
        assert_eq!(control.events(), [Event::Suspend(20), Event::Suspend(21)]);
    }

    #[test]
    fn settings_and_cleanup_restrictions_match_cpp() {
        let (vm, execution, _, control) = vm_with_process();
        execution.set_mode(ScriptExecutionMode::SettingsCallback);
        assert!(
            vm.execute_source("freeze(true)", "settings.lua")
                .expect_err("settings callback should reject freeze")
                .message
                .contains("freeze is not available inside script settings")
        );
        execution.set_mode(ScriptExecutionMode::Cleanup);
        assert!(
            vm.execute_source("freeze(true)", "cleanup-enable.lua")
                .expect_err("cleanup must reject enabling freeze")
                .message
                .contains("freeze(true) is not available inside onCleanup")
        );
        vm.execute_source("freeze(false)", "cleanup-disable.lua")
            .expect("cleanup may release freeze");
        assert!(control.events().is_empty());
    }

    #[test]
    fn aliases_share_the_same_behavior() {
        let (vm, _, _, control) = vm_with_process();
        vm.execute_source("robloxFreeze(true); roblox_freeze(false)", "aliases.lua")
            .expect("aliases should execute");
        assert_eq!(control.events(), [Event::Suspend(20), Event::Resume(20)]);
    }

    #[test]
    fn missing_backends_are_silent_noops_like_cpp() {
        let vm = LuaScriptVm::new(None).expect("Lua VM should initialize");
        install_process_api(
            &vm,
            ScriptInputContext::default(),
            ScriptProcessContext::default(),
            None,
            None,
        )
        .expect("process API should install without backends");
        vm.execute_source("freeze(true); freeze(false)", "missing.lua")
            .expect("missing process backend should be a no-op");
    }
}
