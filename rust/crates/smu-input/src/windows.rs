use crate::{
    InputError, InputInjector, KeyState, windows_is_extended_virtual_key,
    windows_is_mouse_button_virtual_key, windows_modifier_scan_code,
    windows_normalize_absolute_coordinate, windows_scale_relative_delta, windows_virtual_key,
    windows_wheel_direction,
};
use smu_core::KeyCode;
use std::{
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicI32, AtomicIsize, AtomicPtr, AtomicU32, AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, GetKeyboardLayout, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE,
            KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE,
            KEYEVENTF_UNICODE, MAPVK_VK_TO_VSC_EX, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
            MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE,
            MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL,
            MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT, MapVirtualKeyExW, SendInput,
            VK_CONTROL, VK_MENU, VK_SHIFT, VkKeyScanExW,
        },
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GA_ROOT, GetAncestor, GetForegroundWindow,
            GetMessageW, GetSystemMetrics, GetWindowThreadProcessId, HC_ACTION, KBDLLHOOKSTRUCT,
            LLKHF_INJECTED, LLKHF_LOWER_IL_INJECTED, LLMHF_INJECTED, LLMHF_LOWER_IL_INJECTED, MSG,
            MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SM_CXVIRTUALSCREEN,
            SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SetCursorPos,
            SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL,
            WM_KEYDOWN, WM_KEYUP, WM_MOUSEWHEEL, WM_QUIT, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_USER,
            XBUTTON1, XBUTTON2,
        },
    },
};

const INJECTED_INPUT_TAG: usize = 0x534D_4301;
const GUI_INPUT_BUDGET: usize = 50;
const GUI_INPUT_BUDGET_SLICE: Duration = Duration::from_millis(16);
static DISPLAY_SCALE_PERCENT: AtomicI32 = AtomicI32::new(100);
static GUI_WINDOW_HANDLE: AtomicIsize = AtomicIsize::new(0);
static INPUT_BUDGET_STATE: OnceLock<Mutex<InputBudgetState>> = OnceLock::new();
static WHEEL_HOOK_LIFECYCLE: OnceLock<Mutex<WheelHookLifecycle>> = OnceLock::new();
static WHEEL_HOOK_RUNNING: AtomicBool = AtomicBool::new(false);
static WHEEL_HOOK_READY: AtomicBool = AtomicBool::new(false);
static WHEEL_HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static WHEEL_HOOK_HANDLE: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
static KEYBOARD_HOOK_HANDLE: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
static KEYBOARD_HOOK_READY: AtomicBool = AtomicBool::new(false);
static REAL_KEYBOARD_ACTIVITY_SERIAL: AtomicU64 = AtomicU64::new(0);
static PENDING_BINDING_INPUT: AtomicU32 = AtomicU32::new(KeyCode::NONE.raw());

#[derive(Default)]
struct WheelHookLifecycle {
    users: usize,
    thread: Option<JoinHandle<()>>,
}

struct InputBudgetState {
    reset_time: Instant,
    remaining: usize,
}

pub fn set_display_scale_percent(scale: i32) {
    DISPLAY_SCALE_PERCENT.store(scale.clamp(10, 500), Ordering::Release);
}

pub fn set_cursor_position_direct(x: i32, y: i32) -> bool {
    // SAFETY: SetCursorPos takes value coordinates and retains no Rust memory.
    unsafe { SetCursorPos(x, y) != 0 }
}

pub fn set_gui_window_handle(handle: *mut core::ffi::c_void) {
    GUI_WINDOW_HANDLE.store(handle as isize, Ordering::Release);
}

fn gui_window_is_foreground() -> bool {
    let raw = GUI_WINDOW_HANDLE.load(Ordering::Acquire);
    if raw == 0 {
        return false;
    }
    let gui = raw as *mut core::ffi::c_void;
    // SAFETY: both calls read window-manager state only. `gui` is the HWND SDL provided for the live
    // application window and is never dereferenced by Rust.
    let foreground = unsafe { GetForegroundWindow() };
    if foreground.is_null() {
        return false;
    }
    foreground == gui || unsafe { GetAncestor(foreground, GA_ROOT) } == gui
}

fn acquire_input_budget(input_count: usize) {
    if !gui_window_is_foreground() {
        return;
    }
    let state = INPUT_BUDGET_STATE.get_or_init(|| {
        Mutex::new(InputBudgetState { reset_time: Instant::now(), remaining: GUI_INPUT_BUDGET })
    });
    loop {
        let sleep_for = {
            let mut state = match state.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let now = Instant::now();
            let elapsed = now.saturating_duration_since(state.reset_time);
            if elapsed >= GUI_INPUT_BUDGET_SLICE {
                state.reset_time = now;
                state.remaining = GUI_INPUT_BUDGET;
            }
            if input_count <= state.remaining {
                state.remaining -= input_count;
                return;
            }
            GUI_INPUT_BUDGET_SLICE.saturating_sub(elapsed).max(Duration::from_millis(1))
        };
        std::thread::sleep(sleep_for);
    }
}

fn scale_relative_delta(delta: i32) -> i32 {
    windows_scale_relative_delta(delta, DISPLAY_SCALE_PERCENT.load(Ordering::Acquire))
}

fn wheel_hook_lifecycle() -> &'static Mutex<WheelHookLifecycle> {
    WHEEL_HOOK_LIFECYCLE.get_or_init(|| Mutex::new(WheelHookLifecycle::default()))
}

fn lock_lifecycle() -> std::sync::MutexGuard<'static, WheelHookLifecycle> {
    match wheel_hook_lifecycle().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn keyboard_event_is_injected(event: &KBDLLHOOKSTRUCT) -> bool {
    event.flags & (LLKHF_INJECTED | LLKHF_LOWER_IL_INJECTED) != 0
        || event.dwExtraInfo == INJECTED_INPUT_TAG
}

fn mouse_event_is_injected(event: &MSLLHOOKSTRUCT) -> bool {
    event.flags & (LLMHF_INJECTED | LLMHF_LOWER_IL_INJECTED) != 0
        || event.dwExtraInfo == INJECTED_INPUT_TAG
}

#[must_use]
pub fn real_keyboard_activity_serial() -> u64 {
    REAL_KEYBOARD_ACTIVITY_SERIAL.load(Ordering::Acquire)
}

#[must_use]
pub fn keyboard_activity_hook_ready() -> bool {
    KEYBOARD_HOOK_READY.load(Ordering::Acquire)
}

fn hook_code_is_action(code: i32) -> bool {
    u32::try_from(code) == Ok(HC_ACTION)
}

fn keyboard_hook_message(wparam: WPARAM) -> bool {
    matches!(u32::try_from(wparam), Ok(WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP))
}

unsafe extern "system" fn keyboard_activity_hook(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if hook_code_is_action(code) && keyboard_hook_message(wparam) && lparam != 0 {
        // SAFETY: WH_KEYBOARD_LL supplies a KBDLLHOOKSTRUCT pointer for HC_ACTION keyboard messages.
        let event = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if !keyboard_event_is_injected(event) {
            REAL_KEYBOARD_ACTIVITY_SERIAL.fetch_add(1, Ordering::Release);
        }
    }
    let hook = KEYBOARD_HOOK_HANDLE.load(Ordering::Acquire);
    // SAFETY: forwarding the hook event is required by the Win32 low-level hook contract.
    unsafe { CallNextHookEx(hook, code, wparam, lparam) }
}

unsafe extern "system" fn binding_mouse_wheel_hook(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if hook_code_is_action(code) && u32::try_from(wparam) == Ok(WM_MOUSEWHEEL) && lparam != 0 {
        // SAFETY: WH_MOUSE_LL supplies an MSLLHOOKSTRUCT pointer for HC_ACTION mouse messages.
        let event = unsafe { &*(lparam as *const MSLLHOOKSTRUCT) };
        if !mouse_event_is_injected(event) {
            let delta = ((event.mouseData >> 16) as u16) as i16;
            if let Some(key) = windows_wheel_direction(delta) {
                PENDING_BINDING_INPUT.store(key.raw(), Ordering::Release);
            }
        }
    }
    let hook = WHEEL_HOOK_HANDLE.load(Ordering::Acquire);
    // SAFETY: forwarding the hook event is required by the Win32 low-level hook contract.
    unsafe { CallNextHookEx(hook, code, wparam, lparam) }
}

fn wheel_hook_thread() {
    // SAFETY: these APIs operate on the current thread's Win32 message queue and retain no Rust refs.
    unsafe {
        WHEEL_HOOK_THREAD_ID.store(GetCurrentThreadId(), Ordering::Release);
        let mut message = MSG::default();
        let _ = PeekMessageW(&mut message, std::ptr::null_mut(), WM_USER, WM_USER, PM_NOREMOVE);
        let module = GetModuleHandleW(std::ptr::null());
        let keyboard_hook =
            SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_activity_hook), module, 0);
        KEYBOARD_HOOK_HANDLE.store(keyboard_hook, Ordering::Release);
        KEYBOARD_HOOK_READY.store(!keyboard_hook.is_null(), Ordering::Release);
        let hook = SetWindowsHookExW(WH_MOUSE_LL, Some(binding_mouse_wheel_hook), module, 0);
        WHEEL_HOOK_HANDLE.store(hook, Ordering::Release);
        WHEEL_HOOK_READY.store(true, Ordering::Release);

        while WHEEL_HOOK_RUNNING.load(Ordering::Acquire) {
            let result = GetMessageW(&mut message, std::ptr::null_mut(), 0, 0);
            if result <= 0 {
                break;
            }
            let _ = TranslateMessage(&message);
            let _ = DispatchMessageW(&message);
        }

        if !hook.is_null() {
            let _ = UnhookWindowsHookEx(hook);
        }
        if !keyboard_hook.is_null() {
            let _ = UnhookWindowsHookEx(keyboard_hook);
        }
    }
    WHEEL_HOOK_HANDLE.store(std::ptr::null_mut(), Ordering::Release);
    KEYBOARD_HOOK_HANDLE.store(std::ptr::null_mut(), Ordering::Release);
    KEYBOARD_HOOK_READY.store(false, Ordering::Release);
    WHEEL_HOOK_THREAD_ID.store(0, Ordering::Release);
    WHEEL_HOOK_READY.store(false, Ordering::Release);
}

fn acquire_wheel_hook() {
    let mut lifecycle = lock_lifecycle();
    if lifecycle.users == 0 {
        PENDING_BINDING_INPUT.store(KeyCode::NONE.raw(), Ordering::Release);
        WHEEL_HOOK_READY.store(false, Ordering::Release);
        KEYBOARD_HOOK_READY.store(false, Ordering::Release);
        WHEEL_HOOK_THREAD_ID.store(0, Ordering::Release);
        WHEEL_HOOK_RUNNING.store(true, Ordering::Release);
        lifecycle.thread = Some(std::thread::spawn(wheel_hook_thread));
        for _ in 0..250 {
            if WHEEL_HOOK_READY.load(Ordering::Acquire) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    lifecycle.users = lifecycle.users.saturating_add(1);
}

fn release_wheel_hook() {
    let mut lifecycle = lock_lifecycle();
    if lifecycle.users == 0 {
        return;
    }
    lifecycle.users -= 1;
    if lifecycle.users != 0 {
        return;
    }

    WHEEL_HOOK_RUNNING.store(false, Ordering::Release);
    let thread_id = WHEEL_HOOK_THREAD_ID.load(Ordering::Acquire);
    if thread_id != 0 {
        // SAFETY: thread_id belongs to the hook thread and WM_QUIT carries no pointer payload.
        unsafe {
            let _ = PostThreadMessageW(thread_id, WM_QUIT, 0, 0);
        }
    }
    if let Some(thread) = lifecycle.thread.take() {
        let _ = thread.join();
    }
    PENDING_BINDING_INPUT.store(KeyCode::NONE.raw(), Ordering::Release);
}

#[derive(Debug)]
pub struct WindowsKeyState {
    _hook_lease: (),
}

impl WindowsKeyState {
    #[must_use]
    pub fn new() -> Self {
        acquire_wheel_hook();
        Self { _hook_lease: () }
    }
}

impl Default for WindowsKeyState {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WindowsKeyState {
    fn drop(&mut self) {
        release_wheel_hook();
    }
}

fn foreground_keyboard_layout() -> *mut core::ffi::c_void {
    // SAFETY: these Win32 calls retain no caller-owned pointers. A null foreground window simply
    // falls back to this thread's active keyboard layout, matching the C++ backend.
    unsafe {
        let foreground = GetForegroundWindow();
        if !foreground.is_null() {
            let thread = GetWindowThreadProcessId(foreground, std::ptr::null_mut());
            if thread != 0 {
                let layout = GetKeyboardLayout(thread);
                if !layout.is_null() {
                    return layout;
                }
            }
        }
        GetKeyboardLayout(0)
    }
}

fn scan_code_for_virtual_key(vk: u16) -> u16 {
    if let Some(scan_code) = windows_modifier_scan_code(vk) {
        return scan_code;
    }
    // SAFETY: the keyboard-layout handle comes from Win32 and the function has no pointer
    // arguments. The low byte matches C++ `ScanCodeForVirtualKey`.
    unsafe {
        (MapVirtualKeyExW(u32::from(vk), MAPVK_VK_TO_VSC_EX, foreground_keyboard_layout()) & 0xFF)
            as u16
    }
}

fn keyboard_input(scan_code: u16, extended: bool, key_up: bool) -> INPUT {
    let mut flags = KEYEVENTF_SCANCODE;
    if key_up {
        flags |= KEYEVENTF_KEYUP;
    }
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: scan_code,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECTED_INPUT_TAG,
            },
        },
    }
}

fn mouse_input(flags: u32, mouse_data: u32, dx: i32, dy: i32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: mouse_data,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECTED_INPUT_TAG,
            },
        },
    }
}

pub fn set_left_mouse_button_direct(down: bool) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: if down { MOUSEEVENTF_LEFTDOWN } else { MOUSEEVENTF_LEFTUP },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    // SAFETY: `input` points to one initialized Win32 INPUT record and remains valid for the call.
    unsafe {
        let _ = SendInput(
            1,
            &raw const input,
            i32::try_from(std::mem::size_of::<INPUT>()).unwrap_or_default(),
        );
    }
}

fn dispatch_inputs(inputs: &[INPUT]) {
    if inputs.is_empty() {
        return;
    }
    acquire_input_budget(inputs.len());
    let Ok(count) = u32::try_from(inputs.len()) else {
        return;
    };
    // SAFETY: `inputs` is a contiguous initialized slice that remains valid for the duration of SendInput.
    unsafe {
        let _ = SendInput(
            count,
            inputs.as_ptr(),
            i32::try_from(std::mem::size_of::<INPUT>()).unwrap_or_default(),
        );
    }
}

fn dispatch(input: &INPUT) {
    dispatch_inputs(std::slice::from_ref(input));
}

fn unicode_keyboard_input(code_unit: u16, key_up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: 0,
                wScan: code_unit,
                dwFlags: KEYEVENTF_UNICODE | if key_up { KEYEVENTF_KEYUP } else { 0 },
                time: 0,
                dwExtraInfo: INJECTED_INPUT_TAG,
            },
        },
    }
}

fn type_unicode_text_native(text: &str, delay_ms: i32) -> bool {
    let delay = Duration::from_millis(u64::try_from(delay_ms.max(0)).unwrap_or_default());
    for code_unit in text.encode_utf16() {
        dispatch(&unicode_keyboard_input(code_unit, false));
        dispatch(&unicode_keyboard_input(code_unit, true));
        std::thread::sleep(delay);
    }
    true
}

fn type_unicode_byte_native(character: u8, delay_ms: i32) -> bool {
    if character.is_ascii() {
        let text = [character];
        let Ok(text) = std::str::from_utf8(&text) else {
            return false;
        };
        return type_unicode_text_native(text, delay_ms);
    }
    false
}

fn layout_scan_input(
    virtual_key: u16,
    layout: *mut core::ffi::c_void,
    key_up: bool,
) -> Option<INPUT> {
    // SAFETY: `layout` is a keyboard-layout handle returned by GetKeyboardLayout and MapVirtualKeyExW retains
    // no caller-owned pointers.
    let mapped = unsafe { MapVirtualKeyExW(u32::from(virtual_key), MAPVK_VK_TO_VSC_EX, layout) };
    if mapped == 0 {
        return None;
    }
    Some(keyboard_input((mapped & 0xFF) as u16, (mapped & 0xFF00) == 0xE000, key_up))
}

fn press_character_native(character: u8, delay_ms: i32) -> bool {
    let layout = foreground_keyboard_layout();
    // SAFETY: `layout` is a keyboard-layout handle returned by GetKeyboardLayout.
    let encoded = unsafe { VkKeyScanExW(u16::from(character), layout) };
    if encoded == -1 {
        return type_unicode_byte_native(character, delay_ms);
    }

    let encoded = encoded as u16;
    let virtual_key = encoded & 0x00FF;
    let modifiers = ((encoded >> 8) & 0x00FF) as u8;
    if virtual_key == 0 || (modifiers & 0xF8) != 0 {
        return type_unicode_byte_native(character, delay_ms);
    }

    let mut key_downs = Vec::with_capacity(4);
    for (mask, modifier) in [(0x04, VK_MENU), (0x02, VK_CONTROL), (0x01, VK_SHIFT)] {
        if modifiers & mask != 0 {
            let Some(input) = layout_scan_input(modifier, layout, false) else {
                return type_unicode_byte_native(character, delay_ms);
            };
            key_downs.push(input);
        }
    }
    let Some(key_down) = layout_scan_input(virtual_key, layout, false) else {
        return type_unicode_byte_native(character, delay_ms);
    };
    key_downs.push(key_down);
    dispatch_inputs(&key_downs);

    std::thread::sleep(Duration::from_millis(u64::try_from(delay_ms.max(0)).unwrap_or_default()));

    let mut key_ups = Vec::with_capacity(key_downs.len());
    if let Some(input) = layout_scan_input(virtual_key, layout, true) {
        key_ups.push(input);
    }
    for (mask, modifier) in [(0x01, VK_SHIFT), (0x02, VK_CONTROL), (0x04, VK_MENU)] {
        if modifiers & mask != 0 {
            if let Some(input) = layout_scan_input(modifier, layout, true) {
                key_ups.push(input);
            }
        }
    }
    dispatch_inputs(&key_ups);
    true
}

fn mouse_button_input(vk: u16, down: bool) -> INPUT {
    let (flags, data) = match vk {
        0x01 => (if down { MOUSEEVENTF_LEFTDOWN } else { MOUSEEVENTF_LEFTUP }, 0),
        0x02 => (if down { MOUSEEVENTF_RIGHTDOWN } else { MOUSEEVENTF_RIGHTUP }, 0),
        0x04 => (if down { MOUSEEVENTF_MIDDLEDOWN } else { MOUSEEVENTF_MIDDLEUP }, 0),
        0x05 => (if down { MOUSEEVENTF_XDOWN } else { MOUSEEVENTF_XUP }, u32::from(XBUTTON1)),
        0x06 => (if down { MOUSEEVENTF_XDOWN } else { MOUSEEVENTF_XUP }, u32::from(XBUTTON2)),
        // C++ treats the full VK_LBUTTON..VK_XBUTTON2 range as mouse input, including VK_CANCEL.
        _ => (0, 0),
    };
    mouse_input(flags, data, 0, 0)
}

fn inject_virtual_key(key: KeyCode, extended: bool, down: bool) {
    let vk = key.raw() as u16;
    if windows_is_mouse_button_virtual_key(vk) {
        dispatch(&mouse_button_input(vk, down));
        return;
    }
    let scan_code = scan_code_for_virtual_key(vk);
    if scan_code == 0 {
        return;
    }
    dispatch(&keyboard_input(scan_code, extended || windows_is_extended_virtual_key(vk), !down));
}

impl KeyState for WindowsKeyState {
    fn is_pressed(&self, key: KeyCode) -> bool {
        let Some(vkey) = windows_virtual_key(key) else {
            return false;
        };
        // SAFETY: GetAsyncKeyState accepts a Win32 virtual-key code and has no pointer arguments.
        (unsafe { GetAsyncKeyState(vkey) } as u16 & 0x8000) != 0
    }

    fn current_pressed_key(&self) -> Option<KeyCode> {
        (1_u32..KeyCode::MOUSE_WHEEL_UP.raw())
            .find(|&raw| self.is_pressed(KeyCode::new(raw)))
            .map(KeyCode::new)
    }

    fn consume_next_transient_input(&self) -> Option<KeyCode> {
        let raw = PENDING_BINDING_INPUT.swap(KeyCode::NONE.raw(), Ordering::AcqRel);
        (raw != KeyCode::NONE.raw()).then(|| KeyCode::new(raw))
    }
}

impl InputInjector for WindowsKeyState {
    fn hold_key(&self, key: KeyCode, extended: bool) {
        inject_virtual_key(key, extended, true);
    }

    fn release_key(&self, key: KeyCode, extended: bool) {
        inject_virtual_key(key, extended, false);
    }

    fn move_mouse(&self, dx: i32, dy: i32) {
        self.move_mouse_raw(scale_relative_delta(dx), scale_relative_delta(dy));
    }

    fn move_mouse_raw(&self, dx: i32, dy: i32) {
        dispatch(&mouse_input(MOUSEEVENTF_MOVE, 0, dx, dy));
    }

    fn move_mouse_absolute(&self, x: i32, y: i32) -> Result<(), InputError> {
        // SAFETY: GetSystemMetrics reads process-global desktop metrics and retains no pointers.
        let (left, top, width, height) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        let Some(normalized_x) = windows_normalize_absolute_coordinate(x, left, width) else {
            return Err(InputError::new(
                "virtual desktop bounds are not available in the current Windows session",
            ));
        };
        let Some(normalized_y) = windows_normalize_absolute_coordinate(y, top, height) else {
            return Err(InputError::new(
                "virtual desktop bounds are not available in the current Windows session",
            ));
        };
        dispatch(&mouse_input(
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
            0,
            normalized_x,
            normalized_y,
        ));
        Ok(())
    }

    fn prefers_direct_absolute_positioning(&self) -> bool {
        true
    }

    fn mouse_wheel(&self, delta: i32) {
        dispatch(&mouse_input(MOUSEEVENTF_WHEEL, delta as u32, 0, 0));
    }

    fn press_character(&self, character: u8, delay_ms: i32) -> bool {
        press_character_native(character, delay_ms)
    }

    fn type_unicode_text(&self, text: &str, delay_ms: i32) -> bool {
        type_unicode_text_native(text, delay_ms)
    }
}
