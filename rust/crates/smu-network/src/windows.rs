use crate::{
    LagSwitchConfig, WindowsPacketAction, build_windows_windivert_filter,
    windows_delay::WindowsDelayedSender, windows_packet_decision, windows_payload_range,
};
use std::{
    env,
    ffi::{CString, c_void},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{FreeLibrary, GetLastError, HANDLE, HMODULE, INVALID_HANDLE_VALUE},
    System::LibraryLoader::{GetProcAddress, LoadLibraryW},
};

const WINDIVERT_LAYER_NETWORK: i32 = 0;
const WINDIVERT_FLAG_SNIFF: u64 = 0x0001;
const WINDIVERT_SHUTDOWN_BOTH: i32 = 0x3;

pub const WINDIVERT_MTU_MAX: usize = 40 + 0xFFFF;
pub const WINDIVERT_DLL_NAME: &str = "SMCWinDivert.dll";
pub const WINDIVERT_DRIVER_NAME: &str = "SMCWinDivert64.sys";

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

    pub fn current() -> Result<Self, WinDivertLoadError> {
        let executable = env::current_exe().map_err(WinDivertLoadError::CurrentExecutable)?;
        Ok(Self::beside_executable(&executable))
    }

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

    pub fn probe(&self) -> Result<(), WinDivertLoadError> {
        // SAFETY: the function pointer comes from the loaded WinDivert module and the filter is a
        // static NUL-terminated C string.
        let handle = unsafe {
            (self.open)(b"false\0".as_ptr(), WINDIVERT_LAYER_NETWORK, 0, WINDIVERT_FLAG_SNIFF)
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

    pub fn recv(
        &self,
        handle: HANDLE,
        packet: &mut [u8],
        address: &mut WinDivertAddress,
    ) -> Option<usize> {
        let capacity = u32::try_from(packet.len()).ok()?;
        let mut received = 0_u32;
        // SAFETY: packet/address point to writable caller-owned storage and all lengths match it.
        let ok = unsafe {
            (self.recv)(handle, packet.as_mut_ptr().cast(), capacity, &mut received, address)
        };
        (ok != 0).then(|| usize::try_from(received).unwrap_or(0))
    }

    pub fn send(&self, handle: HANDLE, packet: &[u8], address: &WinDivertAddress) -> bool {
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
                &mut payload,
                &mut payload_len,
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
    pub fn raw(&self) -> HANDLE {
        self.raw as HANDLE
    }

    pub fn shutdown(&self) {
        self.api.shutdown_handle(self.raw());
    }

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

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WinDivertCaptureSnapshot {
    pub config: LagSwitchConfig,
    pub roblox_dynamic_ips: Vec<String>,
    pub process_udp_ports: Vec<i32>,
    pub target_process_present: bool,
    pub is_rcc: bool,
}

impl Default for WinDivertCaptureSnapshot {
    fn default() -> Self {
        Self {
            config: LagSwitchConfig::default(),
            roblox_dynamic_ips: Vec::new(),
            process_udp_ports: Vec::new(),
            target_process_present: false,
            is_rcc: false,
        }
    }
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
        });
        let worker_api = Arc::clone(&api);
        let worker_shared = Arc::clone(&shared);
        let worker_sender = Arc::clone(&delayed_sender);
        let worker = std::thread::spawn(move || {
            packet_worker_loop(worker_api, worker_shared, worker_sender);
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

fn packet_worker_loop(
    api: Arc<WinDivertApi>,
    shared: Arc<WinDivertPacketWorkerShared>,
    delayed_sender: Arc<WinDivertDelayedPacketSender>,
) {
    while shared.running.load(Ordering::Acquire) {
        let filter = lock_mutex(&shared.capture).filter();
        let Some(handle) = api.open_filter_shared(&filter).map(Arc::new) else {
            std::thread::sleep(Duration::from_millis(500));
            continue;
        };
        let installed = {
            let current = lock_mutex(&shared.capture);
            if current.filter() != filter {
                false
            } else {
                shared.handle_slot.install(Arc::clone(&handle));
                true
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
    fn windivert_assets_resolve_beside_executable() {
        let paths = WinDivertPaths::beside_executable(Path::new(
            r"C:\Apps\SMU\Spencer-Macro-Utilities.exe",
        ));
        assert_eq!(paths.dll, PathBuf::from(r"C:\Apps\SMU\SMCWinDivert.dll"));
        assert_eq!(paths.driver, PathBuf::from(r"C:\Apps\SMU\SMCWinDivert64.sys"));
    }
}
