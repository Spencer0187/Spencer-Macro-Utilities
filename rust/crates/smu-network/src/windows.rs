// WinDivert state mutations deliberately retain mutex guards through policy updates and capture
// synchronization. Changing their scope solely for this heuristic can alter synchronization ordering.
#![allow(clippy::significant_drop_tightening)]

use crate::{
    LagSwitchConfig, LagSwitchControl, LagSwitchInitializationFailure, LagSwitchState,
    LagSwitchTargetMode, WindowsDynamicTargetState, WindowsPacketAction,
    build_windows_windivert_filter, windows_delay::WindowsDelayedSender, windows_packet_decision,
    windows_payload_range, windows_udp_ports_for_rows,
};
use smu_process::ProcessDiscovery;
use smu_roblox::RobloxLogSource;
use std::{
    env,
    ffi::{CString, c_void},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INSUFFICIENT_BUFFER, ERROR_INVALID_PARAMETER, FreeLibrary,
        GetLastError, HANDLE, HMODULE, INVALID_HANDLE_VALUE, NO_ERROR,
    },
    NetworkManagement::IpHelper::{GetExtendedUdpTable, MIB_UDPROW_OWNER_PID, UDP_TABLE_OWNER_PID},
    Security::{AllocateAndInitializeSid, CheckTokenMembership, FreeSid, SECURITY_NT_AUTHORITY},
    System::{
        LibraryLoader::{GetProcAddress, LoadLibraryW},
        SystemServices::{DOMAIN_ALIAS_RID_ADMINS, SECURITY_BUILTIN_DOMAIN_RID},
    },
};

const WINDIVERT_LAYER_NETWORK: i32 = 0;
const WINDIVERT_FLAG_SNIFF: u64 = 0x0001;
const WINDIVERT_SHUTDOWN_BOTH: i32 = 0x3;
const ADDRESS_FAMILY_INET: u32 = 2;
const ADDRESS_FAMILY_INET6: u32 = 23;
const DYNAMIC_TARGET_REFRESH: Duration = Duration::from_secs(1);

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Udp6RowOwnerPid {
    local_addr: [u8; 16],
    local_scope_id: u32,
    local_port: u32,
    owning_pid: u32,
}

pub const WINDIVERT_MTU_MAX: usize = 40 + 0xFFFF;
pub const WINDIVERT_DLL_NAME: &str = "SMCWinDivert.dll";
// The vendor DLL requests WinDivert64.sys when installing its kernel service.
pub const WINDIVERT_DRIVER_NAME: &str = "WinDivert64.sys";

#[must_use]
pub fn is_running_as_admin() -> bool {
    let authority = SECURITY_NT_AUTHORITY;
    let mut admin_sid = std::ptr::null_mut();
    // SAFETY: the authority and output pointer are valid for the duration of the call, and the
    // subauthority count matches the two populated RID arguments.
    let allocated = unsafe {
        AllocateAndInitializeSid(
            &raw const authority,
            2,
            SECURITY_BUILTIN_DOMAIN_RID.unsigned_abs(),
            DOMAIN_ALIAS_RID_ADMINS.unsigned_abs(),
            0,
            0,
            0,
            0,
            0,
            0,
            &raw mut admin_sid,
        )
    };
    if allocated == 0 {
        return false;
    }

    let mut is_member = 0;
    // SAFETY: `admin_sid` was allocated successfully above; a null token handle asks Windows to
    // inspect the effective token for the current thread/process, matching the C++ implementation.
    let checked =
        unsafe { CheckTokenMembership(std::ptr::null_mut(), admin_sid, &raw mut is_member) };
    // SAFETY: `admin_sid` is owned by this function after a successful allocation.
    unsafe {
        let _ = FreeSid(admin_sid);
    }
    checked != 0 && is_member != 0
}

fn query_udp_table_buffer(address_family: u32) -> Option<(Vec<u32>, usize)> {
    let mut byte_len = 0_u32;
    // SAFETY: the first call intentionally passes a null table pointer to query the required size.
    let first = unsafe {
        GetExtendedUdpTable(
            std::ptr::null_mut(),
            &raw mut byte_len,
            0,
            address_family,
            UDP_TABLE_OWNER_PID,
            0,
        )
    };
    if first != ERROR_INSUFFICIENT_BUFFER || byte_len < 4 {
        return None;
    }
    let bytes = usize::try_from(byte_len).ok()?;
    let words = bytes.checked_add(3)?.checked_div(4)?;
    let mut buffer = vec![0_u32; words];
    // SAFETY: `buffer` is writable and at least `byte_len` bytes long. DWORD storage provides the
    // alignment required by both IPv4 and the compatibility IPv6 owner-table layouts.
    let second = unsafe {
        GetExtendedUdpTable(
            buffer.as_mut_ptr().cast(),
            &raw mut byte_len,
            0,
            address_family,
            UDP_TABLE_OWNER_PID,
            0,
        )
    };
    (second == NO_ERROR).then_some((buffer, usize::try_from(byte_len).ok()?))
}

fn udp4_owner_rows() -> Vec<(u32, u32)> {
    let Some((buffer, byte_len)) = query_udp_table_buffer(ADDRESS_FAMILY_INET) else {
        return Vec::new();
    };
    let count = usize::try_from(buffer[0]).unwrap_or(0);
    let header = std::mem::size_of::<u32>();
    let row_size = std::mem::size_of::<MIB_UDPROW_OWNER_PID>();
    let Some(required) = count.checked_mul(row_size).and_then(|size| size.checked_add(header))
    else {
        return Vec::new();
    };
    if required > byte_len {
        return Vec::new();
    }
    let base = buffer.as_ptr().cast::<u8>();
    (0..count)
        .map(|index| {
            // SAFETY: the table size was validated above and DWORD storage is suitably aligned.
            let row = unsafe {
                std::ptr::read_unaligned(
                    base.add(header + index * row_size).cast::<MIB_UDPROW_OWNER_PID>(),
                )
            };
            (row.dwOwningPid, row.dwLocalPort)
        })
        .collect()
}

fn udp6_owner_rows() -> Vec<(u32, u32)> {
    let Some((buffer, byte_len)) = query_udp_table_buffer(ADDRESS_FAMILY_INET6) else {
        return Vec::new();
    };
    let count = usize::try_from(buffer[0]).unwrap_or(0);
    let header = std::mem::size_of::<u32>();
    let row_size = std::mem::size_of::<Udp6RowOwnerPid>();
    let Some(required) = count.checked_mul(row_size).and_then(|size| size.checked_add(header))
    else {
        return Vec::new();
    };
    if required > byte_len {
        return Vec::new();
    }
    let base = buffer.as_ptr().cast::<u8>();
    (0..count)
        .map(|index| {
            // SAFETY: the table size was validated above and DWORD storage is suitably aligned.
            let row = unsafe {
                std::ptr::read_unaligned(
                    base.add(header + index * row_size).cast::<Udp6RowOwnerPid>(),
                )
            };
            (row.owning_pid, row.local_port)
        })
        .collect()
}

fn target_process_ids(
    discovery: &dyn ProcessDiscovery,
    selector: &str,
    take_all: bool,
) -> Vec<u32> {
    if selector.is_empty() {
        return Vec::new();
    }
    if take_all {
        return discovery.find_all_processes(selector).unwrap_or_default();
    }
    discovery.find_main_process(selector).ok().flatten().into_iter().collect()
}

fn dynamic_process_targets(
    discovery: &dyn ProcessDiscovery,
    selector: &str,
    take_all: bool,
) -> (bool, Vec<i32>) {
    let pids = target_process_ids(discovery, selector, take_all);
    if pids.is_empty() {
        return (false, Vec::new());
    }
    let mut rows = udp4_owner_rows();
    rows.extend(udp6_owner_rows());
    (true, windows_udp_ports_for_rows(&pids, &rows))
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WinDivertAddress {
    pub timestamp: i64,
    flags: u32,
    reserved2: u32,
    data: [u8; 64],
}

impl Default for WinDivertAddress {
    fn default() -> Self {
        Self { timestamp: 0, flags: 0, reserved2: 0, data: [0; 64] }
    }
}

impl WinDivertAddress {
    #[must_use]
    pub const fn outbound(&self) -> bool {
        self.flags & (1 << 17) != 0
    }

    #[cfg(test)]
    const fn with_flags(flags: u32) -> Self {
        Self { timestamp: 0, flags, reserved2: 0, data: [0; 64] }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WinDivertPaths {
    pub dll: PathBuf,
    pub driver: PathBuf,
}

impl WinDivertPaths {
    #[must_use]
    pub fn beside_executable(executable: &Path) -> Self {
        let directory = executable.parent().unwrap_or_else(|| Path::new("."));
        Self {
            dll: directory.join(WINDIVERT_DLL_NAME),
            driver: directory.join(WINDIVERT_DRIVER_NAME),
        }
    }

    /// # Errors
    /// Returns the current-executable lookup failure if the running image cannot be located.
    pub fn current() -> Result<Self, WinDivertLoadError> {
        let executable = env::current_exe().map_err(WinDivertLoadError::CurrentExecutable)?;
        Ok(Self::beside_executable(&executable))
    }

    /// # Errors
    /// Returns the missing DLL or driver path if the runtime layout is incomplete.
    pub fn validate(&self) -> Result<(), WinDivertLoadError> {
        if !self.dll.is_file() {
            return Err(WinDivertLoadError::MissingAsset(self.dll.clone()));
        }
        if !self.driver.is_file() {
            return Err(WinDivertLoadError::MissingAsset(self.driver.clone()));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum WinDivertLoadError {
    CurrentExecutable(std::io::Error),
    MissingAsset(PathBuf),
    LoadLibrary { path: PathBuf, win32_error: u32 },
    MissingExport(&'static str),
    ProbeOpen { win32_error: u32 },
}

impl std::fmt::Display for WinDivertLoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CurrentExecutable(error) => {
                write!(formatter, "Could not locate the running executable: {error}")
            }
            Self::MissingAsset(path) => {
                write!(formatter, "Required WinDivert asset is missing: {}", path.display())
            }
            Self::LoadLibrary { path, win32_error } => {
                write!(formatter, "Failed to load {} (Win32 error {win32_error}).", path.display())
            }
            Self::MissingExport(name) => {
                write!(formatter, "SMCWinDivert.dll is missing required export {name}.")
            }
            Self::ProbeOpen { win32_error } => {
                write!(
                    formatter,
                    "WinDivertOpen capability probe failed (Win32 error {win32_error})."
                )
            }
        }
    }
}

impl WinDivertLoadError {
    #[must_use]
    const fn requires_administrator(&self) -> bool {
        matches!(
            self,
            Self::LoadLibrary { win32_error, .. } | Self::ProbeOpen { win32_error }
                if *win32_error == ERROR_ACCESS_DENIED
        )
    }
}

impl std::error::Error for WinDivertLoadError {}

type OpenFn = unsafe extern "system" fn(*const u8, i32, i16, u64) -> HANDLE;
type RecvFn =
    unsafe extern "system" fn(HANDLE, *mut c_void, u32, *mut u32, *mut WinDivertAddress) -> i32;
type SendFn =
    unsafe extern "system" fn(HANDLE, *const c_void, u32, *mut u32, *const WinDivertAddress) -> i32;
type ParsePacketFn = unsafe extern "system" fn(
    *const c_void,
    u32,
    *mut *mut c_void,
    *mut *mut c_void,
    *mut u8,
    *mut *mut c_void,
    *mut *mut c_void,
    *mut *mut c_void,
    *mut *mut c_void,
    *mut *mut c_void,
    *mut u32,
    *mut *mut c_void,
    *mut u32,
) -> i32;
type ShutdownFn = unsafe extern "system" fn(HANDLE, i32) -> i32;
type CloseFn = unsafe extern "system" fn(HANDLE) -> i32;

pub struct WinDivertApi {
    module: usize,
    open: OpenFn,
    recv: RecvFn,
    send: SendFn,
    parse_packet: ParsePacketFn,
    shutdown: ShutdownFn,
    close: CloseFn,
}

impl std::fmt::Debug for WinDivertApi {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("WinDivertApi").finish_non_exhaustive()
    }
}

impl WinDivertApi {
    /// # Errors
    /// Returns an error when required assets, DLL loading, or `WinDivert` exports are missing.
    pub fn load(paths: &WinDivertPaths) -> Result<Self, WinDivertLoadError> {
        paths.validate()?;
        let wide =
            paths.dll.as_os_str().encode_wide().chain(std::iter::once(0)).collect::<Vec<_>>();
        // SAFETY: `wide` is NUL-terminated and remains valid for the duration of LoadLibraryW.
        let module = unsafe { LoadLibraryW(wide.as_ptr()) };
        if module.is_null() {
            // SAFETY: GetLastError has no preconditions.
            let win32_error = unsafe { GetLastError() };
            return Err(WinDivertLoadError::LoadLibrary { path: paths.dll.clone(), win32_error });
        }

        let result = unsafe { Self::from_module(module) };
        if result.is_err() {
            // SAFETY: `module` is owned by this function until a successful Self is constructed.
            unsafe {
                let _ = FreeLibrary(module);
            }
        }
        result
    }

    /// # Safety
    ///
    /// `module` must be a live `SMCWinDivert.dll` module handle owned by the caller.
    unsafe fn from_module(module: HMODULE) -> Result<Self, WinDivertLoadError> {
        let open = unsafe { resolve_open(module)? };
        let recv = unsafe { resolve_recv(module)? };
        let send = unsafe { resolve_send(module)? };
        let parse_packet = unsafe { resolve_parse_packet(module)? };
        let shutdown = unsafe { resolve_shutdown(module)? };
        let close = unsafe { resolve_close(module)? };
        Ok(Self { module: module as usize, open, recv, send, parse_packet, shutdown, close })
    }

    /// # Errors
    /// Returns the Win32 probe error if the driver cannot open a disabled capture filter.
    pub fn probe(&self) -> Result<(), WinDivertLoadError> {
        // SAFETY: the function pointer comes from the loaded WinDivert module and the filter is a
        // static NUL-terminated C string.
        let handle = unsafe {
            (self.open)(c"false".as_ptr().cast(), WINDIVERT_LAYER_NETWORK, 0, WINDIVERT_FLAG_SNIFF)
        };
        let Some(handle) = OwnedWinDivertHandle::new(self, handle) else {
            // SAFETY: GetLastError has no preconditions and WinDivertOpen sets it on failure.
            let win32_error = unsafe { GetLastError() };
            return Err(WinDivertLoadError::ProbeOpen { win32_error });
        };
        drop(handle);
        Ok(())
    }

    #[must_use]
    pub fn open_filter(&self, filter: &str) -> Option<OwnedWinDivertHandle<'_>> {
        let filter = CString::new(filter).ok()?;
        // SAFETY: the pointer is valid NUL-terminated storage for the duration of the call.
        let handle = unsafe { (self.open)(filter.as_ptr().cast(), WINDIVERT_LAYER_NETWORK, 0, 0) };
        OwnedWinDivertHandle::new(self, handle)
    }

    #[must_use]
    pub fn open_filter_shared(self: &Arc<Self>, filter: &str) -> Option<SharedWinDivertHandle> {
        let filter = CString::new(filter).ok()?;
        // SAFETY: the pointer is valid NUL-terminated storage for the duration of the call.
        let handle = unsafe { (self.open)(filter.as_ptr().cast(), WINDIVERT_LAYER_NETWORK, 0, 0) };
        SharedWinDivertHandle::new(Arc::clone(self), handle)
    }

    fn recv(
        &self,
        handle: HANDLE,
        packet: &mut [u8],
        address: &mut WinDivertAddress,
    ) -> Option<usize> {
        let capacity = u32::try_from(packet.len()).ok()?;
        let mut received = 0_u32;
        // SAFETY: packet/address point to writable caller-owned storage and all lengths match it.
        let ok = unsafe {
            (self.recv)(handle, packet.as_mut_ptr().cast(), capacity, &raw mut received, address)
        };
        (ok != 0).then(|| usize::try_from(received).unwrap_or(0))
    }

    fn send(&self, handle: HANDLE, packet: &[u8], address: &WinDivertAddress) -> bool {
        let Ok(length) = u32::try_from(packet.len()) else {
            return false;
        };
        // SAFETY: packet/address are immutable caller-owned storage valid for the duration of the call.
        unsafe {
            (self.send)(handle, packet.as_ptr().cast(), length, std::ptr::null_mut(), address) != 0
        }
    }

    #[must_use]
    pub fn parsed_payload<'packet>(&self, packet: &'packet [u8]) -> Option<&'packet [u8]> {
        let packet_len = u32::try_from(packet.len()).ok()?;
        let mut payload = std::ptr::null_mut::<c_void>();
        let mut payload_len = 0_u32;
        // SAFETY: the packet buffer stays alive for the full call; every omitted WinDivert parser
        // output is documented optional, and payload/payload_len are valid writable out parameters.
        let ok = unsafe {
            (self.parse_packet)(
                packet.as_ptr().cast(),
                packet_len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &raw mut payload,
                &raw mut payload_len,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if ok == 0 || payload.is_null() {
            return None;
        }
        let packet_start = packet.as_ptr() as usize;
        let payload_start = payload as usize;
        let offset = payload_start.checked_sub(packet_start)?;
        let range =
            windows_payload_range(packet.len(), offset, usize::try_from(payload_len).ok()?)?;
        Some(&packet[range])
    }

    fn shutdown_handle(&self, handle: HANDLE) {
        // SAFETY: handle belongs to this WinDivert module. Shutdown is best-effort during teardown.
        unsafe {
            let _ = (self.shutdown)(handle, WINDIVERT_SHUTDOWN_BOTH);
        }
    }

    fn close_handle(&self, handle: HANDLE) {
        // SAFETY: handle belongs to this WinDivert module and is closed exactly once by its owner.
        unsafe {
            let _ = (self.close)(handle);
        }
    }
}

impl Drop for WinDivertApi {
    fn drop(&mut self) {
        // SAFETY: `module` stores the pointer-sized HMODULE value returned by LoadLibraryW. Shared
        // handle owners keep an Arc<WinDivertApi>, so the module cannot unload while they are alive.
        unsafe {
            let _ = FreeLibrary(self.module as HMODULE);
        }
    }
}

pub struct OwnedWinDivertHandle<'a> {
    api: &'a WinDivertApi,
    raw: HANDLE,
}

impl std::fmt::Debug for OwnedWinDivertHandle<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("OwnedWinDivertHandle").finish_non_exhaustive()
    }
}

impl<'a> OwnedWinDivertHandle<'a> {
    fn new(api: &'a WinDivertApi, raw: HANDLE) -> Option<Self> {
        (!raw.is_null() && raw != INVALID_HANDLE_VALUE).then_some(Self { api, raw })
    }

    #[must_use]
    pub const fn raw(&self) -> HANDLE {
        self.raw
    }

    pub fn shutdown(&self) {
        self.api.shutdown_handle(self.raw);
    }
}

impl Drop for OwnedWinDivertHandle<'_> {
    fn drop(&mut self) {
        self.api.shutdown_handle(self.raw);
        self.api.close_handle(self.raw);
    }
}

pub struct SharedWinDivertHandle {
    api: Arc<WinDivertApi>,
    raw: usize,
}

#[derive(Debug, Default)]
pub struct WinDivertHandleSlot {
    current: Mutex<Option<Arc<SharedWinDivertHandle>>>,
}

impl std::fmt::Debug for SharedWinDivertHandle {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("SharedWinDivertHandle").finish_non_exhaustive()
    }
}

impl SharedWinDivertHandle {
    fn new(api: Arc<WinDivertApi>, raw: HANDLE) -> Option<Self> {
        (!raw.is_null() && raw != INVALID_HANDLE_VALUE).then_some(Self { api, raw: raw as usize })
    }

    #[must_use]
    pub const fn raw(&self) -> HANDLE {
        self.raw as HANDLE
    }

    pub fn shutdown(&self) {
        self.api.shutdown_handle(self.raw());
    }

    #[must_use]
    pub fn send(&self, packet: &[u8], address: &WinDivertAddress) -> bool {
        self.api.send(self.raw(), packet, address)
    }
}

impl WinDivertHandleSlot {
    pub fn install(&self, handle: Arc<SharedWinDivertHandle>) {
        let previous = {
            let mut current = lock_mutex(&self.current);
            current.replace(handle)
        };
        if let Some(previous) = previous {
            previous.shutdown();
        }
    }

    #[must_use]
    pub fn current(&self) -> Option<Arc<SharedWinDivertHandle>> {
        lock_mutex(&self.current).clone()
    }

    pub fn shutdown_current(&self) {
        let current = lock_mutex(&self.current).take();
        if let Some(current) = current {
            current.shutdown();
        }
    }

    pub fn clear_if_current(&self, expected: &Arc<SharedWinDivertHandle>) {
        let removed = {
            let mut current = lock_mutex(&self.current);
            if current.as_ref().is_some_and(|handle| Arc::ptr_eq(handle, expected)) {
                current.take()
            } else {
                None
            }
        };
        drop(removed);
    }

    fn send(&self, packet: &[u8], address: &WinDivertAddress) -> bool {
        let current = lock_mutex(&self.current);
        current.as_ref().is_some_and(|handle| handle.send(packet, address))
    }
}

impl Drop for SharedWinDivertHandle {
    fn drop(&mut self) {
        self.api.shutdown_handle(self.raw());
        self.api.close_handle(self.raw());
    }
}

#[derive(Debug)]
struct DelayedWinDivertPacket {
    data: Vec<u8>,
    address: WinDivertAddress,
}

#[derive(Debug)]
pub struct WinDivertDelayedPacketSender {
    sender: WindowsDelayedSender<DelayedWinDivertPacket>,
}

impl WinDivertDelayedPacketSender {
    pub fn start(handle_slot: Arc<WinDivertHandleSlot>) -> Self {
        let sender = WindowsDelayedSender::start(move |packet: DelayedWinDivertPacket| {
            let _ = handle_slot.send(&packet.data, &packet.address);
        });
        Self { sender }
    }

    pub(crate) fn enqueue(&self, packet: &[u8], address: WinDivertAddress, delay_ms: u64) -> bool {
        self.sender.enqueue(
            DelayedWinDivertPacket { data: packet.to_vec(), address },
            Duration::from_millis(delay_ms),
        )
    }

    pub(crate) fn stop(&self) {
        self.sender.stop();
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct WinDivertCaptureSnapshot {
    pub config: LagSwitchConfig,
    pub roblox_dynamic_ips: Vec<String>,
    pub process_udp_ports: Vec<i32>,
    pub target_process_present: bool,
    pub is_rcc: bool,
}

impl WinDivertCaptureSnapshot {
    fn filter(&self) -> String {
        build_windows_windivert_filter(
            &self.config,
            &self.roblox_dynamic_ips,
            &self.process_udp_ports,
            self.target_process_present,
        )
    }
}

struct WinDivertPacketWorkerShared {
    running: AtomicBool,
    capture: Mutex<WinDivertCaptureSnapshot>,
    handle_slot: Arc<WinDivertHandleSlot>,
    invalid_dynamic_ips_reset: AtomicBool,
}

pub(crate) struct WinDivertPacketWorker {
    shared: Arc<WinDivertPacketWorkerShared>,
    delayed_sender: Arc<WinDivertDelayedPacketSender>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for WinDivertPacketWorker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WinDivertPacketWorker")
            .field("running", &self.shared.running.load(Ordering::Acquire))
            .field("capture", &*lock_mutex(&self.shared.capture))
            .finish_non_exhaustive()
    }
}

impl WinDivertPacketWorker {
    pub(crate) fn start(api: Arc<WinDivertApi>, initial: WinDivertCaptureSnapshot) -> Self {
        let handle_slot = Arc::new(WinDivertHandleSlot::default());
        let delayed_sender =
            Arc::new(WinDivertDelayedPacketSender::start(Arc::clone(&handle_slot)));
        let shared = Arc::new(WinDivertPacketWorkerShared {
            running: AtomicBool::new(true),
            capture: Mutex::new(initial),
            handle_slot,
            invalid_dynamic_ips_reset: AtomicBool::new(false),
        });
        let worker_api = api;
        let worker_shared = Arc::clone(&shared);
        let worker_sender = Arc::clone(&delayed_sender);
        let worker = std::thread::spawn(move || {
            packet_worker_loop(&worker_api, &worker_shared, &worker_sender);
        });
        Self { shared, delayed_sender, worker: Mutex::new(Some(worker)) }
    }

    pub(crate) fn update_capture(&self, capture: WinDivertCaptureSnapshot) {
        let restart = {
            let mut current = lock_mutex(&self.shared.capture);
            let restart = current.filter() != capture.filter();
            *current = capture;
            restart
        };
        if restart {
            self.shared.handle_slot.shutdown_current();
        }
    }

    fn invalid_dynamic_ips_reset_pending(&self) -> bool {
        self.shared.invalid_dynamic_ips_reset.load(Ordering::Acquire)
    }

    fn take_invalid_dynamic_ips_reset(&self) -> bool {
        self.shared.invalid_dynamic_ips_reset.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn stop(&self) {
        self.shared.running.store(false, Ordering::Release);
        self.shared.handle_slot.shutdown_current();
        let worker = lock_mutex(&self.worker).take();
        if let Some(worker) = worker {
            let _ = worker.join();
        }
        self.delayed_sender.stop();
    }
}

impl Drop for WinDivertPacketWorker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Debug, Default)]
struct WindowsBackendState {
    policy: LagSwitchState,
    driver_enabled: bool,
    last_error: String,
    last_error_requires_administrator: bool,
    packet_worker: Option<Arc<WinDivertPacketWorker>>,
    dynamic_targets: WindowsDynamicTargetState,
    process_selector: String,
    take_all_process_ids: bool,
}

struct WindowsBackendInner {
    state: Mutex<WindowsBackendState>,
    process_discovery: Arc<dyn ProcessDiscovery>,
    roblox_log: Option<Arc<dyn RobloxLogSource>>,
}

impl std::fmt::Debug for WindowsBackendInner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WindowsBackendInner")
            .field("state", &*lock_mutex(&self.state))
            .field("roblox_log", &self.roblox_log.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
struct DynamicRefreshState {
    stop: bool,
}

#[derive(Debug, Default)]
struct DynamicRefreshControl {
    state: Mutex<DynamicRefreshState>,
    wake: Condvar,
}

pub struct WindowsLagSwitchBackend {
    inner: Arc<WindowsBackendInner>,
    refresh: Arc<DynamicRefreshControl>,
    refresh_worker: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for WindowsLagSwitchBackend {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WindowsLagSwitchBackend")
            .field("inner", &self.inner)
            .field("refresh_worker", &lock_mutex(&self.refresh_worker).is_some())
            .finish_non_exhaustive()
    }
}

impl WindowsLagSwitchBackend {
    #[must_use]
    pub fn new(
        process_discovery: Arc<dyn ProcessDiscovery>,
        roblox_log: Option<Arc<dyn RobloxLogSource>>,
    ) -> Self {
        Self {
            inner: Arc::new(WindowsBackendInner {
                state: Mutex::new(WindowsBackendState::default()),
                process_discovery,
                roblox_log,
            }),
            refresh: Arc::new(DynamicRefreshControl::default()),
            refresh_worker: Mutex::new(None),
        }
    }

    pub fn init(&self) -> bool {
        if lock_mutex(&self.inner.state).driver_enabled {
            return true;
        }
        let paths = match WinDivertPaths::current() {
            Ok(paths) => paths,
            Err(error) => {
                let mut state = lock_mutex(&self.inner.state);
                state.last_error_requires_administrator = false;
                error.to_string().clone_into(&mut state.last_error);
                return false;
            }
        };
        let api = match WinDivertApi::load(&paths) {
            Ok(api) => Arc::new(api),
            Err(error) => {
                let requires_administrator = error.requires_administrator();
                let mut state = lock_mutex(&self.inner.state);
                state.last_error_requires_administrator = requires_administrator;
                if requires_administrator {
                    state.last_error.clear();
                } else {
                    error.to_string().clone_into(&mut state.last_error);
                }
                return false;
            }
        };
        if let Err(error) = api.probe() {
            let requires_administrator = error.requires_administrator();
            let mut state = lock_mutex(&self.inner.state);
            state.last_error_requires_administrator = requires_administrator;
            if requires_administrator {
                state.last_error.clear();
            } else {
                error.to_string().clone_into(&mut state.last_error);
            }
            return false;
        }
        let mut dynamic_targets = lock_mutex(&self.inner.state).dynamic_targets.clone();
        refresh_dynamic_targets_values(self.inner.as_ref(), &mut dynamic_targets);
        let initial = {
            let state = lock_mutex(&self.inner.state);
            capture_snapshot(&state, &dynamic_targets)
        };
        let packet_worker = Arc::new(WinDivertPacketWorker::start(api, initial));
        {
            let mut state = lock_mutex(&self.inner.state);
            if state.driver_enabled {
                drop(state);
                packet_worker.stop();
                return true;
            }
            state.dynamic_targets = dynamic_targets;
            state.packet_worker = Some(packet_worker);
            state.driver_enabled = true;
            state.last_error.clear();
            state.last_error_requires_administrator = false;
            update_capture_locked(&state);
        }
        self.start_refresh_worker();
        true
    }

    pub fn shutdown(&self) {
        self.stop_refresh_worker();
        let packet_worker = {
            let mut state = lock_mutex(&self.inner.state);
            state.driver_enabled = false;
            state.policy.clear_blocking();
            state.dynamic_targets.update_process_ports(false, []);
            state.packet_worker.take()
        };
        if let Some(packet_worker) = packet_worker {
            packet_worker.stop();
        }
    }

    pub fn set_process_target(&self, selector: String, take_all_process_ids: bool) {
        let changed = {
            let mut state = lock_mutex(&self.inner.state);
            if state.process_selector == selector
                && state.take_all_process_ids == take_all_process_ids
            {
                false
            } else {
                state.process_selector = selector;
                state.take_all_process_ids = take_all_process_ids;
                true
            }
        };
        if changed {
            self.refresh_dynamic_targets_once();
            self.refresh.wake.notify_all();
        }
    }

    pub fn set_base_config(&self, config: LagSwitchConfig) {
        {
            let mut state = lock_mutex(&self.inner.state);
            state.policy.set_base_config(config);
            update_capture_locked(&state);
        }
        self.refresh.wake.notify_all();
    }

    pub fn set_base_blocking(&self, active: bool) {
        let mut state = lock_mutex(&self.inner.state);
        state.policy.set_base_blocking(active);
        update_capture_locked(&state);
    }

    #[must_use]
    pub fn driver_enabled(&self) -> bool {
        lock_mutex(&self.inner.state).driver_enabled
    }

    pub fn refresh_dynamic_targets_once(&self) {
        refresh_dynamic_targets(self.inner.as_ref());
    }

    fn start_refresh_worker(&self) {
        let mut worker = lock_mutex(&self.refresh_worker);
        if worker.is_some() {
            return;
        }
        lock_mutex(&self.refresh.state).stop = false;
        let inner = Arc::clone(&self.inner);
        let refresh = Arc::clone(&self.refresh);
        *worker = Some(std::thread::spawn(move || {
            let mut refresh_state = lock_mutex(&refresh.state);
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
                refresh_dynamic_targets(inner.as_ref());
                refresh_state = lock_mutex(&refresh.state);
            }
        }));
    }

    fn stop_refresh_worker(&self) {
        {
            let mut state = lock_mutex(&self.refresh.state);
            state.stop = true;
        }
        self.refresh.wake.notify_all();
        let worker = { lock_mutex(&self.refresh_worker).take() };
        if let Some(worker) = worker {
            let _ = worker.join();
        }
    }
}

impl Drop for WindowsLagSwitchBackend {
    fn drop(&mut self) {
        self.stop_refresh_worker();
    }
}

fn target_uses_roblox(config: &LagSwitchConfig) -> bool {
    config.target_mode == LagSwitchTargetMode::Roblox
        || (config.target_mode == LagSwitchTargetMode::Custom && config.include_roblox_dynamic_ips)
}

fn capture_snapshot(
    state: &WindowsBackendState,
    dynamic_targets: &WindowsDynamicTargetState,
) -> WinDivertCaptureSnapshot {
    let mut roblox_dynamic_ips = dynamic_targets.roblox_dynamic_ips.clone();
    if state.packet_worker.as_ref().is_some_and(|worker| worker.invalid_dynamic_ips_reset_pending())
    {
        roblox_dynamic_ips.clear();
    }
    WinDivertCaptureSnapshot {
        config: state.policy.effective_config(),
        roblox_dynamic_ips,
        process_udp_ports: dynamic_targets.process_udp_ports.clone(),
        target_process_present: dynamic_targets.target_process_present,
        is_rcc: dynamic_targets.is_rcc,
    }
}

fn update_capture_locked(state: &WindowsBackendState) {
    if let Some(worker) = state.packet_worker.as_ref() {
        worker.update_capture(capture_snapshot(state, &state.dynamic_targets));
    }
}

fn refresh_dynamic_targets_values(
    inner: &WindowsBackendInner,
    dynamic_targets: &mut WindowsDynamicTargetState,
) {
    let (config, selector, take_all) = {
        let state = lock_mutex(&inner.state);
        (
            state.policy.effective_config(),
            state.process_selector.clone(),
            state.take_all_process_ids,
        )
    };
    if !target_uses_roblox(&config) {
        return;
    }
    if config.target_mode == LagSwitchTargetMode::Roblox {
        let (present, ports) =
            dynamic_process_targets(inner.process_discovery.as_ref(), &selector, take_all);
        dynamic_targets.update_process_ports(present, ports);
    }
    if let Some(source) = inner.roblox_log.as_ref() {
        let snapshot = source.read_log(true);
        let _ = dynamic_targets.apply_log_snapshot(&snapshot);
    }
}

fn refresh_dynamic_targets(inner: &WindowsBackendInner) {
    let mut next = {
        let mut state = lock_mutex(&inner.state);
        if !state.driver_enabled || !target_uses_roblox(&state.policy.effective_config()) {
            return;
        }
        let reset_dynamic_ips = state
            .packet_worker
            .as_ref()
            .is_some_and(|worker| worker.take_invalid_dynamic_ips_reset());
        if reset_dynamic_ips {
            state.dynamic_targets.roblox_dynamic_ips.clear();
            update_capture_locked(&state);
            return;
        }
        state.dynamic_targets.clone()
    };
    refresh_dynamic_targets_values(inner, &mut next);
    let mut state = lock_mutex(&inner.state);
    if !state.driver_enabled || !target_uses_roblox(&state.policy.effective_config()) {
        return;
    }
    if state.dynamic_targets != next {
        state.dynamic_targets = next;
        update_capture_locked(&state);
    }
}

impl LagSwitchControl for WindowsLagSwitchBackend {
    fn initialize(&self) -> bool {
        self.init()
    }

    fn is_initialized(&self) -> bool {
        lock_mutex(&self.inner.state).driver_enabled
    }

    fn initialization_failure(&self) -> LagSwitchInitializationFailure {
        let state = lock_mutex(&self.inner.state);
        if state.last_error_requires_administrator {
            return LagSwitchInitializationFailure::RequiresAdministrator;
        }
        if state.last_error.is_empty() {
            LagSwitchInitializationFailure::Error(
                "Lag switch backend could not be initialized.".to_owned(),
            )
        } else {
            LagSwitchInitializationFailure::Error(state.last_error.clone())
        }
    }

    fn is_available(&self) -> bool {
        true
    }

    fn effective_config(&self) -> LagSwitchConfig {
        lock_mutex(&self.inner.state).policy.effective_config()
    }

    fn is_blocking_active(&self) -> bool {
        self.effective_config().currently_blocking
    }

    fn is_base_blocking_active(&self) -> bool {
        lock_mutex(&self.inner.state).policy.base_blocking()
    }

    fn set_base_blocking_active(&self, active: bool) {
        Self::set_base_blocking(self, active);
    }

    fn set_base_config(&self, config: LagSwitchConfig) {
        Self::set_base_config(self, config);
    }

    fn unsupported_reason(&self) -> String {
        let state = lock_mutex(&self.inner.state);
        if !state.last_error.is_empty() {
            return state.last_error.clone();
        }
        if state.driver_enabled {
            "WinDivert driver is running.".to_owned()
        } else {
            "WinDivert backend is available.".to_owned()
        }
    }

    fn set_script_blocking_active(&self, owner_token: usize, active: bool) {
        let mut state = lock_mutex(&self.inner.state);
        state.policy.set_script_blocking(owner_token, active);
        update_capture_locked(&state);
    }

    fn set_script_config_override(&self, owner_token: usize, config: LagSwitchConfig) {
        {
            let mut state = lock_mutex(&self.inner.state);
            state.policy.set_script_config_override(owner_token, config);
            update_capture_locked(&state);
        }
        self.refresh.wake.notify_all();
    }

    fn clear_script_config_override(&self, owner_token: usize) {
        {
            let mut state = lock_mutex(&self.inner.state);
            state.policy.clear_script_config_override(owner_token);
            update_capture_locked(&state);
        }
        self.refresh.wake.notify_all();
    }

    fn clear_script_state(&self, owner_token: usize) {
        {
            let mut state = lock_mutex(&self.inner.state);
            state.policy.clear_script_state(owner_token);
            update_capture_locked(&state);
        }
        self.refresh.wake.notify_all();
    }
}

fn packet_worker_loop(
    api: &Arc<WinDivertApi>,
    shared: &Arc<WinDivertPacketWorkerShared>,
    delayed_sender: &Arc<WinDivertDelayedPacketSender>,
) {
    while shared.running.load(Ordering::Acquire) {
        let filter = lock_mutex(&shared.capture).filter();
        let Some(handle) = api.open_filter_shared(&filter).map(Arc::new) else {
            // SAFETY: GetLastError has no preconditions and WinDivertOpen sets it on failure.
            let win32_error = unsafe { GetLastError() };
            if win32_error == ERROR_INVALID_PARAMETER {
                let mut capture = lock_mutex(&shared.capture);
                if !capture.roblox_dynamic_ips.is_empty() {
                    capture.roblox_dynamic_ips.clear();
                    shared.invalid_dynamic_ips_reset.store(true, Ordering::Release);
                }
            }
            std::thread::sleep(Duration::from_millis(500));
            continue;
        };
        let installed = {
            let current = lock_mutex(&shared.capture);
            if current.filter() == filter {
                shared.handle_slot.install(Arc::clone(&handle));
                true
            } else {
                false
            }
        };
        if !installed {
            handle.shutdown();
            continue;
        }
        run_capture_handle(api.as_ref(), shared.as_ref(), delayed_sender.as_ref(), &handle);
        shared.handle_slot.clear_if_current(&handle);
    }
    shared.handle_slot.shutdown_current();
}

fn run_capture_handle(
    api: &WinDivertApi,
    shared: &WinDivertPacketWorkerShared,
    delayed_sender: &WinDivertDelayedPacketSender,
    handle: &Arc<SharedWinDivertHandle>,
) {
    let mut packet = vec![0_u8; WINDIVERT_MTU_MAX];
    let mut last_safety_pulse = Instant::now();
    let mut safety_pulse_active = false;
    while shared.running.load(Ordering::Acquire) {
        let mut address = WinDivertAddress::default();
        let Some(packet_len) = api.recv(handle.raw(), &mut packet, &mut address) else {
            break;
        };
        let Some(packet) = packet.get(..packet_len) else {
            continue;
        };
        let capture = lock_mutex(&shared.capture).clone();
        let payload = api.parsed_payload(packet);
        let elapsed_ms = u64::try_from(last_safety_pulse.elapsed().as_millis()).unwrap_or(u64::MAX);
        let decision = windows_packet_decision(
            &capture.config,
            address.outbound(),
            capture.is_rcc,
            elapsed_ms,
            safety_pulse_active,
            payload,
        );
        safety_pulse_active = decision.safety_pulse_active;
        if decision.reset_safety_clock {
            last_safety_pulse = Instant::now();
        }
        match decision.action {
            WindowsPacketAction::Drop => {}
            WindowsPacketAction::Send => {
                let _ = handle.send(packet, &address);
            }
            WindowsPacketAction::DelayMs(delay_ms) => {
                let _ = delayed_sender.enqueue(packet, address, delay_ms);
            }
        }
    }
}

fn lock_mutex<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

unsafe fn export(
    module: HMODULE,
    name: &'static [u8],
    display: &'static str,
) -> Result<unsafe extern "system" fn() -> isize, WinDivertLoadError> {
    // SAFETY: `module` is live and each name is a static NUL-terminated byte string.
    unsafe { GetProcAddress(module, name.as_ptr()) }
        .ok_or(WinDivertLoadError::MissingExport(display))
}

unsafe fn resolve_open(module: HMODULE) -> Result<OpenFn, WinDivertLoadError> {
    let function = unsafe { export(module, b"WinDivertOpen\0", "WinDivertOpen")? };
    // SAFETY: WinDivertOpen has the signature declared in the bundled windivert.h.
    Ok(unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, OpenFn>(function) })
}

unsafe fn resolve_recv(module: HMODULE) -> Result<RecvFn, WinDivertLoadError> {
    let function = unsafe { export(module, b"WinDivertRecv\0", "WinDivertRecv")? };
    // SAFETY: WinDivertRecv has the signature declared in the bundled windivert.h.
    Ok(unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, RecvFn>(function) })
}

unsafe fn resolve_send(module: HMODULE) -> Result<SendFn, WinDivertLoadError> {
    let function = unsafe { export(module, b"WinDivertSend\0", "WinDivertSend")? };
    // SAFETY: WinDivertSend has the signature declared in the bundled windivert.h.
    Ok(unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, SendFn>(function) })
}

unsafe fn resolve_parse_packet(module: HMODULE) -> Result<ParsePacketFn, WinDivertLoadError> {
    let function =
        unsafe { export(module, b"WinDivertHelperParsePacket\0", "WinDivertHelperParsePacket")? };
    // SAFETY: WinDivertHelperParsePacket has the signature declared in the bundled windivert.h.
    Ok(unsafe {
        std::mem::transmute::<unsafe extern "system" fn() -> isize, ParsePacketFn>(function)
    })
}

unsafe fn resolve_shutdown(module: HMODULE) -> Result<ShutdownFn, WinDivertLoadError> {
    let function = unsafe { export(module, b"WinDivertShutdown\0", "WinDivertShutdown")? };
    // SAFETY: WinDivertShutdown has the signature declared in the bundled windivert.h.
    Ok(unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, ShutdownFn>(function) })
}

unsafe fn resolve_close(module: HMODULE) -> Result<CloseFn, WinDivertLoadError> {
    let function = unsafe { export(module, b"WinDivertClose\0", "WinDivertClose")? };
    // SAFETY: WinDivertClose has the signature declared in the bundled windivert.h.
    Ok(unsafe { std::mem::transmute::<unsafe extern "system" fn() -> isize, CloseFn>(function) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windivert_address_layout_matches_bundled_header() {
        assert_eq!(std::mem::size_of::<WinDivertAddress>(), 80);
        assert_eq!(std::mem::align_of::<WinDivertAddress>(), 8);
        assert!(!WinDivertAddress::default().outbound());
        assert!(WinDivertAddress::with_flags(1 << 17).outbound());
        assert!(!WinDivertAddress::with_flags(1 << 16).outbound());
    }

    #[test]
    fn only_access_denied_load_failures_require_administrator() {
        let denied_load = WinDivertLoadError::LoadLibrary {
            path: PathBuf::from("SMCWinDivert.dll"),
            win32_error: ERROR_ACCESS_DENIED,
        };
        let denied_probe = WinDivertLoadError::ProbeOpen { win32_error: ERROR_ACCESS_DENIED };
        let broken_runtime = WinDivertLoadError::MissingAsset(PathBuf::from("SMCWinDivert.dll"));
        let invalid_probe = WinDivertLoadError::ProbeOpen { win32_error: 577 };

        assert!(denied_load.requires_administrator());
        assert!(denied_probe.requires_administrator());
        assert!(!broken_runtime.requires_administrator());
        assert!(!invalid_probe.requires_administrator());
    }

    #[test]
    fn windivert_assets_resolve_beside_executable() {
        let paths = WinDivertPaths::beside_executable(Path::new(
            r"C:\Apps\SMU\Spencer-Macro-Utilities.exe",
        ));
        assert_eq!(paths.dll, PathBuf::from(r"C:\Apps\SMU\SMCWinDivert.dll"));
        assert_eq!(paths.driver, PathBuf::from(r"C:\Apps\SMU\WinDivert64.sys"));
    }
}
