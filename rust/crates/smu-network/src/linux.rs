//! Linux `nethelper` transport and lag-switch backend.

use crate::{
    LagSwitchConfig, LagSwitchControl, LagSwitchInitializationFailure, LagSwitchState,
    LagSwitchTargetMode, build_linux_apply_command, is_valid_ipv4_address,
};
use smu_roblox::RobloxLogSource;
use std::fmt;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const HELPER_TIMEOUT: Duration = Duration::from_secs(2);
const HELPER_RESPONSE_LIMIT: u64 = 1024;
const DYNAMIC_TARGET_REFRESH: Duration = Duration::from_secs(1);

pub trait NetHelperClient: Send + Sync {
    fn send_command(&self, command: &str) -> Option<String>;
}

#[derive(Clone, Debug)]
pub struct UnixNetHelperClient {
    socket_path: PathBuf,
}

impl Default for UnixNetHelperClient {
    fn default() -> Self {
        // SAFETY: `getuid` has no preconditions and does not access caller-owned memory.
        let uid = unsafe { libc::getuid() };
        Self::new(format!("/tmp/smu-nethelper-{uid}.sock"))
    }
}

impl UnixNetHelperClient {
    #[must_use]
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self { socket_path: socket_path.into() }
    }

    fn root_peer(stream: &UnixStream) -> bool {
        let mut credentials = libc::ucred { pid: 0, uid: 0, gid: 0 };
        let Ok(mut length) = libc::socklen_t::try_from(std::mem::size_of::<libc::ucred>()) else {
            return false;
        };
        // SAFETY: the credential and length pointers remain valid for the syscall and the fd is
        // borrowed from a live Unix stream.
        let result = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                std::ptr::addr_of_mut!(credentials).cast(),
                std::ptr::addr_of_mut!(length),
            )
        };
        result == 0
            && usize::try_from(length).ok() == Some(std::mem::size_of::<libc::ucred>())
            && credentials.uid == 0
    }
}

impl NetHelperClient for UnixNetHelperClient {
    fn send_command(&self, command: &str) -> Option<String> {
        let mut stream = UnixStream::connect(&self.socket_path).ok()?;
        stream.set_read_timeout(Some(HELPER_TIMEOUT)).ok()?;
        stream.set_write_timeout(Some(HELPER_TIMEOUT)).ok()?;
        if !Self::root_peer(&stream) {
            return None;
        }

        stream.write_all(command.as_bytes()).ok()?;
        stream.write_all(b"\n").ok()?;
        stream.flush().ok()?;
        stream.shutdown(std::net::Shutdown::Write).ok()?;

        let mut response = String::new();
        BufReader::new(stream).take(HELPER_RESPONSE_LIMIT).read_line(&mut response).ok()?;
        if response.is_empty() {
            return None;
        }
        if let Some(newline) = response.find('\n') {
            response.truncate(newline);
        }
        (response.len() <= usize::try_from(HELPER_RESPONSE_LIMIT).unwrap_or(usize::MAX))
            .then_some(response)
    }
}

#[derive(Debug, Default)]
struct LinuxState {
    policy: LagSwitchState,
    available: bool,
    applied_blocking: bool,
    last_command: String,
    last_error: String,
}

struct Inner {
    state: Mutex<LinuxState>,
    helper: Arc<dyn NetHelperClient>,
    roblox_log: Option<Arc<dyn RobloxLogSource>>,
}

impl fmt::Debug for Inner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Inner")
            .field("state", &*lock(&self.state))
            .field("roblox_log", &self.roblox_log.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
struct RefreshState {
    stop: bool,
}

#[derive(Debug, Default)]
struct RefreshControl {
    state: Mutex<RefreshState>,
    wake: Condvar,
}

pub struct LinuxLagSwitchBackend {
    inner: Arc<Inner>,
    refresh: Arc<RefreshControl>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl fmt::Debug for LinuxLagSwitchBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LinuxLagSwitchBackend")
            .field("inner", &self.inner)
            .field("refresh_worker", &lock(&self.worker).is_some())
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl Default for LinuxLagSwitchBackend {
    fn default() -> Self {
        Self::new(None)
    }
}

#[allow(clippy::significant_drop_tightening)]
impl LinuxLagSwitchBackend {
    #[must_use]
    pub fn new(roblox_log: Option<Arc<dyn RobloxLogSource>>) -> Self {
        Self::with_client(roblox_log, Arc::new(UnixNetHelperClient::default()))
    }

    #[must_use]
    pub fn with_client(
        roblox_log: Option<Arc<dyn RobloxLogSource>>,
        helper: Arc<dyn NetHelperClient>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner { state: Mutex::new(LinuxState::default()), helper, roblox_log }),
            refresh: Arc::new(RefreshControl::default()),
            worker: Mutex::new(None),
        }
    }

    pub fn init(&self) -> bool {
        {
            let mut state = lock(&self.inner.state);
            if state.available {
                return true;
            }
            if self.inner.helper.send_command("ping").as_deref() != Some("PONG") {
                state.available = false;
                "Linux lagswitch helper is not running. Enable lagswitch to start it with elevated permissions."
                    .clone_into(&mut state.last_error);
                return false;
            }
            state.available = true;
            state.last_error.clear();
        }
        self.start_refresh_worker();
        true
    }

    pub fn shutdown(&self) {
        self.stop_refresh_worker();
        let mut state = lock(&self.inner.state);
        let _ = self.inner.helper.send_command("shutdown");
        state.applied_blocking = false;
        state.available = false;
        state.last_command.clear();
    }

    pub fn set_base_config(&self, config: LagSwitchConfig) {
        let mut state = lock(&self.inner.state);
        state.policy.set_base_config(config);
        apply_locked(&self.inner, &mut state);
    }

    #[must_use]
    pub fn base_config(&self) -> LagSwitchConfig {
        lock(&self.inner.state).policy.base_config().clone()
    }

    pub fn set_base_blocking(&self, active: bool) {
        let mut state = lock(&self.inner.state);
        state.policy.set_base_blocking(active);
        apply_locked(&self.inner, &mut state);
    }

    pub fn restart_capture(&self) {
        let mut state = lock(&self.inner.state);
        state.last_command.clear();
        apply_locked(&self.inner, &mut state);
    }

    /// Re-evaluate dynamic Roblox targets immediately; the production worker also does this once
    /// per second while an applicable block is active.
    pub fn refresh_dynamic_targets_once(&self) {
        let mut state = lock(&self.inner.state);
        if needs_dynamic_target_refresh(&state) {
            apply_locked(&self.inner, &mut state);
        }
    }

    fn start_refresh_worker(&self) {
        let mut worker = lock(&self.worker);
        if worker.is_some() {
            return;
        }
        lock(&self.refresh.state).stop = false;
        let inner = Arc::clone(&self.inner);
        let refresh = Arc::clone(&self.refresh);
        *worker = Some(std::thread::spawn(move || {
            let mut refresh_state = lock(&refresh.state);
            loop {
                let waited = refresh.wake.wait_timeout(refresh_state, DYNAMIC_TARGET_REFRESH);
                let (next_state, _) = match waited {
                    Ok(value) => value,
                    Err(poisoned) => poisoned.into_inner(),
                };
                refresh_state = next_state;
                if refresh_state.stop {
                    break;
                }
                drop(refresh_state);
                {
                    let mut state = lock(&inner.state);
                    if needs_dynamic_target_refresh(&state) {
                        apply_locked(&inner, &mut state);
                    }
                }
                refresh_state = lock(&refresh.state);
            }
        }));
    }

    fn stop_refresh_worker(&self) {
        {
            let mut state = lock(&self.refresh.state);
            state.stop = true;
        }
        self.refresh.wake.notify_all();
        let worker = lock(&self.worker).take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

impl Drop for LinuxLagSwitchBackend {
    fn drop(&mut self) {
        self.stop_refresh_worker();
    }
}

fn roblox_dynamic_ips(source: Option<&Arc<dyn RobloxLogSource>>) -> Vec<String> {
    let Some(source) = source else {
        return Vec::new();
    };
    let snapshot = source.read_log(true);
    let mut result = Vec::with_capacity(2);
    for address in [snapshot.server.udmux_address, snapshot.server.rcc_address] {
        if is_valid_ipv4_address(&address) && !result.contains(&address) {
            result.push(address);
        }
    }
    result
}

fn needs_dynamic_target_refresh(state: &LinuxState) -> bool {
    let config = state.policy.effective_config();
    state.available
        && config.currently_blocking
        && (config.target_mode == LagSwitchTargetMode::Roblox
            || (config.target_mode == LagSwitchTargetMode::Custom
                && config.include_roblox_dynamic_ips))
}

fn disable_with_error(inner: &Inner, state: &mut LinuxState, message: &str) {
    let reset = inner.helper.send_command("reset").as_deref() == Some("OK");
    state.applied_blocking = false;
    if reset {
        "reset".clone_into(&mut state.last_command);
        message.clone_into(&mut state.last_error);
    } else {
        state.available = false;
        state.last_command.clear();
        state.last_error = format!("{message} The helper could not confirm firewall cleanup.");
    }
}

fn apply_locked(inner: &Inner, state: &mut LinuxState) {
    if !state.available {
        return;
    }
    let config = state.policy.effective_config();
    let dynamic_ips = if config.currently_blocking
        && (config.target_mode == LagSwitchTargetMode::Roblox
            || (config.target_mode == LagSwitchTargetMode::Custom
                && config.include_roblox_dynamic_ips))
    {
        roblox_dynamic_ips(inner.roblox_log.as_ref())
    } else {
        Vec::new()
    };
    let command = match build_linux_apply_command(&config, &dynamic_ips) {
        Ok(command) => command,
        Err(error) => {
            disable_with_error(inner, state, &error.to_string());
            return;
        }
    };

    if command == state.last_command {
        if command == "reset" {
            state.applied_blocking = false;
            state.last_error.clear();
        }
        return;
    }

    let Some(response) = inner.helper.send_command(&command) else {
        state.available = false;
        state.applied_blocking = false;
        state.last_command.clear();
        "Lost contact with the Linux lagswitch helper.".clone_into(&mut state.last_error);
        return;
    };
    if response != "OK" {
        state.applied_blocking = false;
        state.last_command.clear();
        state.last_error = response.strip_prefix("ERR ").map_or_else(
            || "Linux lagswitch helper returned an invalid response.".to_owned(),
            str::to_owned,
        );
        return;
    }

    state.applied_blocking = command.starts_with("block ") || command.starts_with("lag ");
    state.last_command = command;
    state.last_error.clear();
}

#[allow(clippy::significant_drop_tightening)]
impl LagSwitchControl for LinuxLagSwitchBackend {
    fn initialize(&self) -> bool {
        self.init()
    }

    fn is_initialized(&self) -> bool {
        lock(&self.inner.state).available
    }

    fn initialization_failure(&self) -> LagSwitchInitializationFailure {
        if lock(&self.inner.state).available {
            LagSwitchInitializationFailure::Error(self.unsupported_reason())
        } else {
            LagSwitchInitializationFailure::RequiresHelper
        }
    }

    fn is_available(&self) -> bool {
        lock(&self.inner.state).available
    }

    fn effective_config(&self) -> LagSwitchConfig {
        lock(&self.inner.state).policy.effective_config()
    }

    fn is_blocking_active(&self) -> bool {
        lock(&self.inner.state).applied_blocking
    }

    fn is_base_blocking_active(&self) -> bool {
        lock(&self.inner.state).policy.base_blocking()
    }

    fn set_base_blocking_active(&self, active: bool) {
        Self::set_base_blocking(self, active);
    }

    fn set_base_config(&self, config: LagSwitchConfig) {
        Self::set_base_config(self, config);
    }

    fn unsupported_reason(&self) -> String {
        let state = lock(&self.inner.state);
        if !state.last_error.is_empty() {
            return state.last_error.clone();
        }
        if state.available {
            "Linux lagswitch helper is running.".to_owned()
        } else {
            "Linux lagswitch helper is unavailable.".to_owned()
        }
    }

    fn set_script_blocking_active(&self, owner_token: usize, active: bool) {
        let mut state = lock(&self.inner.state);
        state.policy.set_script_blocking(owner_token, active);
        apply_locked(&self.inner, &mut state);
    }

    fn set_script_config_override(&self, owner_token: usize, config: LagSwitchConfig) {
        let mut state = lock(&self.inner.state);
        state.policy.set_script_config_override(owner_token, config);
        apply_locked(&self.inner, &mut state);
    }

    fn clear_script_config_override(&self, owner_token: usize) {
        let mut state = lock(&self.inner.state);
        state.policy.clear_script_config_override(owner_token);
        apply_locked(&self.inner, &mut state);
    }

    fn clear_script_state(&self, owner_token: usize) {
        let mut state = lock(&self.inner.state);
        state.policy.clear_script_state(owner_token);
        apply_locked(&self.inner, &mut state);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use smu_roblox::{RobloxLogServerInfo, RobloxLogSnapshot};
    use std::collections::VecDeque;

    #[derive(Debug, Default)]
    struct FakeHelper {
        commands: Mutex<Vec<String>>,
        responses: Mutex<VecDeque<Option<String>>>,
    }

    impl FakeHelper {
        fn with_responses(responses: &[Option<&str>]) -> Self {
            Self {
                commands: Mutex::new(Vec::new()),
                responses: Mutex::new(
                    responses.iter().map(|value| value.map(str::to_owned)).collect(),
                ),
            }
        }

        fn commands(&self) -> Vec<String> {
            lock(&self.commands).clone()
        }
    }

    impl NetHelperClient for FakeHelper {
        fn send_command(&self, command: &str) -> Option<String> {
            lock(&self.commands).push(command.to_owned());
            lock(&self.responses).pop_front().unwrap_or_else(|| Some("OK".to_owned()))
        }
    }

    #[derive(Debug)]
    struct FakeLog(Mutex<RobloxLogSnapshot>);

    impl FakeLog {
        fn set_addresses(&self, udmux: &str, rcc: &str) {
            let mut snapshot = lock(&self.0);
            snapshot.server.udmux_address = udmux.to_owned();
            snapshot.server.rcc_address = rcc.to_owned();
        }
    }

    impl RobloxLogSource for FakeLog {
        fn read_log(&self, _include_existing: bool) -> RobloxLogSnapshot {
            lock(&self.0).clone()
        }
    }

    fn log(udmux: &str, rcc: &str) -> Arc<FakeLog> {
        Arc::new(FakeLog(Mutex::new(RobloxLogSnapshot {
            server: RobloxLogServerInfo {
                udmux_address: udmux.to_owned(),
                rcc_address: rcc.to_owned(),
                ..RobloxLogServerInfo::default()
            },
            ..RobloxLogSnapshot::default()
        })))
    }

    #[test]
    fn init_ping_sets_cpp_availability_messages() {
        let helper = Arc::new(FakeHelper::with_responses(&[Some("PONG")]));
        let backend = LinuxLagSwitchBackend::with_client(None, helper.clone());
        assert!(backend.init());
        assert!(backend.is_available());
        assert_eq!(backend.unsupported_reason(), "Linux lagswitch helper is running.");
        assert_eq!(helper.commands(), ["ping"]);
        backend.shutdown();
    }

    #[test]
    fn failed_init_uses_cpp_permission_prompt_message() {
        let helper = Arc::new(FakeHelper::with_responses(&[None]));
        let backend = LinuxLagSwitchBackend::with_client(None, helper);
        assert!(!backend.init());
        assert_eq!(
            backend.unsupported_reason(),
            "Linux lagswitch helper is not running. Enable lagswitch to start it with elevated permissions."
        );
    }

    #[test]
    fn base_block_applies_once_and_deduplicates_identical_command() {
        let helper = Arc::new(FakeHelper::with_responses(&[Some("PONG"), Some("OK")]));
        let backend = LinuxLagSwitchBackend::with_client(None, helper.clone());
        assert!(backend.init());
        backend.set_base_blocking(true);
        backend.set_base_blocking(true);
        assert!(backend.is_blocking_active());
        assert_eq!(
            helper.commands(),
            ["ping", "block in=1 out=1 udp=1 tcp=0 mode=roblox ips=- ports=-"]
        );
        backend.shutdown();
    }

    #[test]
    fn dynamic_roblox_addresses_preserve_udmux_then_rcc_source_order_before_sorting() {
        let helper = Arc::new(FakeHelper::with_responses(&[Some("PONG"), Some("OK"), Some("OK")]));
        let source = log("128.116.2.3", "10.0.0.5");
        let backend = LinuxLagSwitchBackend::with_client(Some(source.clone()), helper.clone());
        assert!(backend.init());
        backend.set_base_blocking(true);
        source.set_addresses("128.116.9.9", "10.0.0.7");
        backend.refresh_dynamic_targets_once();
        assert_eq!(
            helper.commands(),
            [
                "ping",
                "block in=1 out=1 udp=1 tcp=0 mode=roblox ips=10.0.0.5,128.116.2.3 ports=-",
                "block in=1 out=1 udp=1 tcp=0 mode=roblox ips=10.0.0.7,128.116.9.9 ports=-",
            ]
        );
        backend.shutdown();
    }

    #[test]
    fn helper_error_response_preserves_payload_without_err_prefix() {
        let helper = Arc::new(FakeHelper::with_responses(&[
            Some("PONG"),
            Some("ERR firewall rejected rule"),
        ]));
        let backend = LinuxLagSwitchBackend::with_client(None, helper);
        assert!(backend.init());
        backend.set_base_blocking(true);
        assert!(!backend.is_blocking_active());
        assert_eq!(backend.unsupported_reason(), "firewall rejected rule");
        backend.shutdown();
    }

    #[test]
    fn lost_helper_marks_backend_unavailable() {
        let helper = Arc::new(FakeHelper::with_responses(&[Some("PONG"), None]));
        let backend = LinuxLagSwitchBackend::with_client(None, helper);
        assert!(backend.init());
        backend.set_base_blocking(true);
        assert!(!backend.is_available());
        assert!(!backend.is_blocking_active());
        assert_eq!(backend.unsupported_reason(), "Lost contact with the Linux lagswitch helper.");
        backend.shutdown();
    }

    #[test]
    fn invalid_active_config_resets_and_keeps_helper_available() {
        let helper = Arc::new(FakeHelper::with_responses(&[Some("PONG"), Some("OK")]));
        let backend = LinuxLagSwitchBackend::with_client(None, helper.clone());
        assert!(backend.init());
        backend.set_base_config(LagSwitchConfig {
            inbound_hard_block: false,
            outbound_hard_block: true,
            use_udp: false,
            use_tcp: false,
            ..LagSwitchConfig::default()
        });
        backend.set_base_blocking(true);
        assert!(backend.is_available());
        assert!(!backend.is_blocking_active());
        assert_eq!(backend.unsupported_reason(), "Linux hard blocking requires UDP, TCP, or both.");
        assert_eq!(helper.commands(), ["ping", "reset", "reset"]);
        backend.shutdown();
    }

    #[test]
    fn script_owner_state_applies_and_clear_restores_base_policy() {
        let helper = Arc::new(FakeHelper::with_responses(&[Some("PONG"), Some("OK"), Some("OK")]));
        let backend = LinuxLagSwitchBackend::with_client(None, helper.clone());
        assert!(backend.init());
        backend.set_script_config_override(
            7,
            LagSwitchConfig { target_mode: LagSwitchTargetMode::All, ..LagSwitchConfig::default() },
        );
        backend.set_script_blocking_active(7, true);
        assert!(backend.is_blocking_active());
        backend.clear_script_state(7);
        assert!(!backend.is_blocking_active());
        assert_eq!(
            helper.commands(),
            ["ping", "reset", "block in=1 out=1 udp=1 tcp=0 mode=all ips=- ports=-", "reset",]
        );
        backend.shutdown();
    }

    #[test]
    fn shutdown_stops_refresh_and_sends_helper_shutdown() {
        let helper = Arc::new(FakeHelper::with_responses(&[Some("PONG"), Some("OK")]));
        let backend = LinuxLagSwitchBackend::with_client(None, helper.clone());
        assert!(backend.init());
        backend.shutdown();
        assert_eq!(helper.commands(), ["ping", "shutdown"]);
        assert!(!backend.is_available());
    }

    #[test]
    fn endpoint_extraction_ignores_invalid_and_duplicate_addresses() {
        let source = log("001.002.003.004", "001.002.003.004");
        assert_eq!(
            roblox_dynamic_ips(Some(&(source as Arc<dyn RobloxLogSource>))),
            ["001.002.003.004"]
        );
        let invalid = log("not-ip", "10.0.0.1");
        assert_eq!(roblox_dynamic_ips(Some(&(invalid as Arc<dyn RobloxLogSource>))), ["10.0.0.1"]);
    }
}
