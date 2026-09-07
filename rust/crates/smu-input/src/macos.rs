//! Native macOS CoreGraphics input injection, physical input state, and active-layout text entry.

use crate::{
    InputError, InputInjector, KeyState, MACOS_MOUSE_LEFT, MACOS_MOUSE_MIDDLE, MACOS_MOUSE_RIGHT,
    MACOS_MOUSE_X1, MACOS_MOUSE_X2, macos_key_code, macos_windows_key_code,
};
use objc2_application_services::AXIsProcessTrusted;
use objc2_core_foundation::{
    CFData, CFMachPort, CFRetained, CFRunLoop, CFType, CGPoint, kCFRunLoopCommonModes,
    kCFRunLoopDefaultMode,
};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType, CGMouseButton, CGScrollEventUnit,
};
use smu_core::KeyCode;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const INJECTED_EVENT_TAG: i64 = 0x534D_554D_4143_4F53;
const MINIMUM_UC_KEYBOARD_LAYOUT_BYTES: usize = 40;
const SHIFT_KEY: u32 = 1 << 9;
const OPTION_KEY: u32 = 1 << 11;
const UC_KEY_ACTION_DOWN: u16 = 0;
const UC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK: u32 = 1;
const MAC_KEY_SHIFT: u16 = 0x38;
const MAC_KEY_OPTION: u16 = 0x3A;

#[repr(C)]
struct UcKeyboardLayout {
    _opaque: [u8; 0],
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    static kTISPropertyUnicodeKeyLayoutData: *const c_void;

    fn TISCopyCurrentKeyboardLayoutInputSource() -> *mut c_void;
    fn TISGetInputSourceProperty(
        input_source: *const c_void,
        property_key: *const c_void,
    ) -> *const c_void;
    fn UCKeyTranslate(
        key_layout: *const UcKeyboardLayout,
        virtual_key_code: u16,
        key_action: u16,
        modifier_key_state: u32,
        keyboard_type: u32,
        key_translate_options: u32,
        dead_key_state: *mut u32,
        max_string_length: usize,
        actual_string_length: *mut usize,
        unicode_string: *mut u16,
    ) -> i32;
    fn LMGetKbdType() -> u8;
}

#[derive(Clone, Copy, Debug)]
struct MacCharacterKey {
    key_code: u16,
    shift: bool,
    option: bool,
}

#[derive(Clone, Copy, Debug)]
struct MouseButtonDescription {
    down_type: CGEventType,
    up_type: CGEventType,
    button: CGMouseButton,
    button_number: i64,
}

fn mouse_button_description(mac_key: u16) -> Option<MouseButtonDescription> {
    match mac_key {
        MACOS_MOUSE_LEFT => Some(MouseButtonDescription {
            down_type: CGEventType::LeftMouseDown,
            up_type: CGEventType::LeftMouseUp,
            button: CGMouseButton::Left,
            button_number: 0,
        }),
        MACOS_MOUSE_RIGHT => Some(MouseButtonDescription {
            down_type: CGEventType::RightMouseDown,
            up_type: CGEventType::RightMouseUp,
            button: CGMouseButton::Right,
            button_number: 1,
        }),
        MACOS_MOUSE_MIDDLE => Some(MouseButtonDescription {
            down_type: CGEventType::OtherMouseDown,
            up_type: CGEventType::OtherMouseUp,
            button: CGMouseButton::Center,
            button_number: 2,
        }),
        MACOS_MOUSE_X1 => Some(MouseButtonDescription {
            down_type: CGEventType::OtherMouseDown,
            up_type: CGEventType::OtherMouseUp,
            button: CGMouseButton(3),
            button_number: 3,
        }),
        MACOS_MOUSE_X2 => Some(MouseButtonDescription {
            down_type: CGEventType::OtherMouseDown,
            up_type: CGEventType::OtherMouseUp,
            button: CGMouseButton(4),
            button_number: 4,
        }),
        _ => None,
    }
}

fn accessibility_trusted() -> bool {
    // SAFETY: AXIsProcessTrusted takes no arguments and only queries the current process trust state.
    unsafe { AXIsProcessTrusted() }
}

fn current_cursor_point() -> Option<CGPoint> {
    let event = CGEvent::new(None)?;
    Some(CGEvent::location(Some(&event)))
}

fn post_tagged_event(event: &CGEvent) {
    CGEvent::set_integer_value_field(
        Some(event),
        CGEventField::EventSourceUserData,
        INJECTED_EVENT_TAG,
    );
    CGEvent::post(CGEventTapLocation::HIDEventTap, Some(event));
}

fn post_raw_keyboard_event(key_code: u16, down: bool) -> bool {
    let Some(event) = CGEvent::new_keyboard_event(None, key_code, down) else {
        return false;
    };
    post_tagged_event(&event);
    true
}

fn resolve_character_key(character: u8) -> Option<MacCharacterKey> {
    let input_source = NonNull::new(unsafe { TISCopyCurrentKeyboardLayoutInputSource() })?;
    // SAFETY: TISCopyCurrentKeyboardLayoutInputSource follows the CoreFoundation Copy rule and returns +1.
    let input_source = unsafe { CFRetained::<CFType>::from_raw(input_source.cast::<CFType>()) };
    // SAFETY: this is an immutable process-lifetime Carbon property-key constant.
    let property_key = unsafe { kTISPropertyUnicodeKeyLayoutData };
    if property_key.is_null() {
        return None;
    }
    // SAFETY: the retained input source remains alive for the returned property reference's use below.
    let layout_data = unsafe {
        TISGetInputSourceProperty(
            CFRetained::as_ptr(&input_source).as_ptr().cast::<c_void>(),
            property_key,
        )
    };
    let layout_data = NonNull::new(layout_data.cast_mut())?;
    // SAFETY: kTISPropertyUnicodeKeyLayoutData is documented to return a CFDataRef when present.
    let layout_data = unsafe { layout_data.cast::<CFData>().as_ref() };
    if layout_data.len() < MINIMUM_UC_KEYBOARD_LAYOUT_BYTES {
        return None;
    }
    // SAFETY: the CFData is immutable for the retained input source lifetime and non-empty after the size check.
    let layout_bytes = unsafe { layout_data.as_bytes_unchecked() };
    let layout = layout_bytes.as_ptr().cast::<UcKeyboardLayout>();
    let modifier_states = [0, SHIFT_KEY, OPTION_KEY, SHIFT_KEY | OPTION_KEY];
    let wanted = u16::from(character);
    // SAFETY: LMGetKbdType has no arguments and returns the current physical keyboard type identifier.
    let keyboard_type = u32::from(unsafe { LMGetKbdType() });

    for key_code in 0_u16..128 {
        for modifier_state in modifier_states {
            let mut dead_key_state = 0_u32;
            let mut output = [0_u16; 4];
            let mut output_length = 0_usize;
            // SAFETY: layout points into immutable CFData retained by input_source; all output pointers are valid
            // for their declared sizes, and the no-dead-keys option prevents persistent dead-key state.
            let result = unsafe {
                UCKeyTranslate(
                    layout,
                    key_code,
                    UC_KEY_ACTION_DOWN,
                    (modifier_state >> 8) & 0xFF,
                    keyboard_type,
                    UC_KEY_TRANSLATE_NO_DEAD_KEYS_MASK,
                    &mut dead_key_state,
                    output.len(),
                    &mut output_length,
                    output.as_mut_ptr(),
                )
            };
            if result == 0 && output_length == 1 && output[0] == wanted {
                return Some(MacCharacterKey {
                    key_code,
                    shift: modifier_state & SHIFT_KEY != 0,
                    option: modifier_state & OPTION_KEY != 0,
                });
            }
        }
    }
    None
}

const fn event_mask(event_type: CGEventType) -> u64 {
    1_u64 << event_type.0
}

fn mouse_event_key(event_type: CGEventType, event: &CGEvent) -> Option<KeyCode> {
    match event_type {
        CGEventType::LeftMouseDown | CGEventType::LeftMouseUp => Some(KeyCode::LBUTTON),
        CGEventType::RightMouseDown | CGEventType::RightMouseUp => Some(KeyCode::RBUTTON),
        CGEventType::OtherMouseDown | CGEventType::OtherMouseUp => {
            match CGEvent::integer_value_field(Some(event), CGEventField::MouseEventButtonNumber) {
                2 => Some(KeyCode::MBUTTON),
                3 => Some(KeyCode::XBUTTON1),
                4 => Some(KeyCode::XBUTTON2),
                _ => None,
            }
        }
        _ => None,
    }
}

fn modifier_down_for_flags_changed(key: KeyCode, flags: CGEventFlags) -> bool {
    match key {
        KeyCode::SHIFT | KeyCode::LSHIFT | KeyCode::RSHIFT => {
            flags.contains(CGEventFlags::MaskShift)
        }
        KeyCode::CONTROL | KeyCode::LCONTROL | KeyCode::RCONTROL => {
            flags.contains(CGEventFlags::MaskControl)
        }
        KeyCode::MENU | KeyCode::LMENU | KeyCode::RMENU => {
            flags.contains(CGEventFlags::MaskAlternate)
        }
        KeyCode::LWIN | KeyCode::RWIN => flags.contains(CGEventFlags::MaskCommand),
        KeyCode::CAPITAL => flags.contains(CGEventFlags::MaskAlphaShift),
        _ => false,
    }
}

#[derive(Debug)]
struct MacosInputStateShared {
    running: AtomicBool,
    read_initialized: AtomicBool,
    key_states: [AtomicBool; 258],
    pending_transient: AtomicU32,
}

impl Default for MacosInputStateShared {
    fn default() -> Self {
        Self {
            running: AtomicBool::new(false),
            read_initialized: AtomicBool::new(false),
            key_states: std::array::from_fn(|_| AtomicBool::new(false)),
            pending_transient: AtomicU32::new(KeyCode::NONE.raw()),
        }
    }
}

impl MacosInputStateShared {
    fn clear(&self) {
        for state in &self.key_states {
            state.store(false, Ordering::Relaxed);
        }
        self.pending_transient.store(KeyCode::NONE.raw(), Ordering::Release);
    }

    fn set_key_state(&self, key: KeyCode, down: bool) {
        let Ok(index) = usize::try_from(key.raw()) else {
            return;
        };
        if let Some(state) = self.key_states.get(index) {
            state.store(down, Ordering::Release);
        }
    }

    fn handle_event(&self, event_type: CGEventType, event: &CGEvent) {
        if CGEvent::integer_value_field(Some(event), CGEventField::EventSourceUserData)
            == INJECTED_EVENT_TAG
        {
            return;
        }

        match event_type {
            CGEventType::KeyDown | CGEventType::KeyUp => {
                let mac_key =
                    CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventKeycode);
                if let Ok(mac_key) = u16::try_from(mac_key)
                    && let Some(key) = macos_windows_key_code(mac_key)
                {
                    self.set_key_state(key, event_type == CGEventType::KeyDown);
                }
            }
            CGEventType::FlagsChanged => {
                let mac_key =
                    CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventKeycode);
                if let Ok(mac_key) = u16::try_from(mac_key)
                    && let Some(key) = macos_windows_key_code(mac_key)
                {
                    self.set_key_state(
                        key,
                        modifier_down_for_flags_changed(key, CGEvent::flags(Some(event))),
                    );
                }
            }
            CGEventType::LeftMouseDown
            | CGEventType::RightMouseDown
            | CGEventType::OtherMouseDown
            | CGEventType::LeftMouseUp
            | CGEventType::RightMouseUp
            | CGEventType::OtherMouseUp => {
                if let Some(key) = mouse_event_key(event_type, event) {
                    let down = matches!(
                        event_type,
                        CGEventType::LeftMouseDown
                            | CGEventType::RightMouseDown
                            | CGEventType::OtherMouseDown
                    );
                    self.set_key_state(key, down);
                }
            }
            CGEventType::ScrollWheel => {
                let delta = CGEvent::integer_value_field(
                    Some(event),
                    CGEventField::ScrollWheelEventDeltaAxis1,
                );
                if delta != 0 {
                    let key =
                        if delta > 0 { KeyCode::MOUSE_WHEEL_UP } else { KeyCode::MOUSE_WHEEL_DOWN };
                    self.pending_transient.store(key.raw(), Ordering::Release);
                }
            }
            _ => {}
        }
    }
}

struct EventTapContext {
    shared: Arc<MacosInputStateShared>,
    tap: *const CFMachPort,
}

unsafe extern "C-unwind" fn event_tap_callback(
    _proxy: CGEventTapProxy,
    event_type: CGEventType,
    event: NonNull<CGEvent>,
    user_info: *mut c_void,
) -> *mut CGEvent {
    let event_pointer = event.as_ptr();
    if user_info.is_null() {
        return event_pointer;
    }
    // SAFETY: user_info points to the EventTapContext box kept alive by event_tap_run_loop.
    let context = unsafe { &*(user_info.cast::<EventTapContext>()) };
    if matches!(event_type, CGEventType::TapDisabledByTimeout | CGEventType::TapDisabledByUserInput)
    {
        if !context.tap.is_null() {
            // SAFETY: context.tap references the retained event tap for the lifetime of this callback.
            CGEvent::tap_enable(unsafe { &*context.tap }, true);
        }
        return event_pointer;
    }
    // SAFETY: CoreGraphics guarantees the callback event pointer is valid for the callback duration.
    context.shared.handle_event(event_type, unsafe { event.as_ref() });
    event_pointer
}

fn event_tap_run_loop(shared: Arc<MacosInputStateShared>, started: mpsc::SyncSender<bool>) {
    let mask = event_mask(CGEventType::KeyDown)
        | event_mask(CGEventType::KeyUp)
        | event_mask(CGEventType::FlagsChanged)
        | event_mask(CGEventType::LeftMouseDown)
        | event_mask(CGEventType::LeftMouseUp)
        | event_mask(CGEventType::RightMouseDown)
        | event_mask(CGEventType::RightMouseUp)
        | event_mask(CGEventType::OtherMouseDown)
        | event_mask(CGEventType::OtherMouseUp)
        | event_mask(CGEventType::ScrollWheel);

    let mut context =
        Box::new(EventTapContext { shared: Arc::clone(&shared), tap: std::ptr::null() });
    // SAFETY: callback and user_info remain valid until the retained tap is invalidated and dropped below.
    let Some(tap) = (unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            mask,
            Some(event_tap_callback),
            std::ptr::from_mut(context.as_mut()).cast::<c_void>(),
        )
    }) else {
        let _ = started.send(false);
        shared.running.store(false, Ordering::Release);
        return;
    };
    context.tap = std::ptr::from_ref(&*tap);

    let Some(source) = CFMachPort::new_run_loop_source(None, Some(&tap), 0) else {
        let _ = started.send(false);
        shared.running.store(false, Ordering::Release);
        return;
    };
    let Some(run_loop) = CFRunLoop::current() else {
        let _ = started.send(false);
        shared.running.store(false, Ordering::Release);
        return;
    };
    // SAFETY: the CoreFoundation common/default mode globals are immutable process-lifetime objects.
    let (common_mode, default_mode) = unsafe { (kCFRunLoopCommonModes, kCFRunLoopDefaultMode) };
    run_loop.add_source(Some(&source), common_mode);
    CGEvent::tap_enable(&tap, true);
    shared.read_initialized.store(true, Ordering::Release);
    let _ = started.send(true);

    while shared.running.load(Ordering::Acquire) {
        let _ = CFRunLoop::run_in_mode(default_mode, 0.05, false);
    }

    shared.read_initialized.store(false, Ordering::Release);
    run_loop.remove_source(Some(&source), common_mode);
    tap.invalidate();
}

#[derive(Debug)]
pub struct MacosInputState {
    shared: Arc<MacosInputStateShared>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Default for MacosInputState {
    fn default() -> Self {
        Self { shared: Arc::new(MacosInputStateShared::default()), thread: Mutex::new(None) }
    }
}

impl MacosInputState {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Start the listen-only CoreGraphics event tap used for physical key/button state and wheel capture.
    ///
    /// # Errors
    ///
    /// Returns an error when Accessibility is unavailable, a reader is already being started, the reader
    /// thread cannot be created, or CoreGraphics cannot create the global event tap/run-loop source.
    pub fn init(&self) -> Result<(), InputError> {
        if self.shared.read_initialized.load(Ordering::Acquire) {
            return Ok(());
        }
        if !accessibility_trusted() {
            return Err(InputError::new(
                "macOS Accessibility permission is required before SMU can read global keyboard and mouse input.",
            ));
        }

        let mut thread_slot = self
            .thread
            .lock()
            .map_err(|_| InputError::new("macOS input reader thread state was poisoned."))?;
        if thread_slot.is_some() {
            return Err(InputError::new("macOS input reader is already starting."));
        }
        self.shared.clear();
        self.shared.running.store(true, Ordering::Release);
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let shared = Arc::clone(&self.shared);
        let thread = thread::Builder::new()
            .name("smu-macos-input-tap".to_owned())
            .spawn(move || event_tap_run_loop(shared, started_tx))
            .map_err(|error| {
                self.shared.running.store(false, Ordering::Release);
                InputError::new(format!("macOS input event-tap thread could not start: {error}"))
            })?;
        *thread_slot = Some(thread);
        drop(thread_slot);

        match started_rx.recv() {
            Ok(true) => Ok(()),
            Ok(false) | Err(_) => {
                self.shutdown();
                Err(InputError::new(
                    "macOS could not create the global input event tap. Check Accessibility permission, then restart SMU.",
                ))
            }
        }
    }

    pub fn shutdown(&self) {
        self.shared.running.store(false, Ordering::Release);
        if let Ok(mut thread_slot) = self.thread.lock()
            && let Some(thread) = thread_slot.take()
        {
            let _ = thread.join();
        }
        self.shared.read_initialized.store(false, Ordering::Release);
        self.shared.clear();
    }

    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.shared.read_initialized.load(Ordering::Acquire)
    }
}

impl Drop for MacosInputState {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl KeyState for MacosInputState {
    fn is_pressed(&self, key: KeyCode) -> bool {
        if !self.is_initialized() {
            return false;
        }
        let Ok(index) = usize::try_from(key.raw()) else {
            return false;
        };
        self.shared.key_states.get(index).is_some_and(|state| state.load(Ordering::Acquire))
    }

    fn current_pressed_key(&self) -> Option<KeyCode> {
        if !self.is_initialized() {
            return None;
        }
        self.shared.key_states.iter().enumerate().skip(1).find_map(|(raw, state)| {
            state
                .load(Ordering::Acquire)
                .then(|| u32::try_from(raw).ok().map(KeyCode::new))
                .flatten()
        })
    }

    fn consume_next_transient_input(&self) -> Option<KeyCode> {
        let raw = self.shared.pending_transient.swap(KeyCode::NONE.raw(), Ordering::AcqRel);
        (raw != KeyCode::NONE.raw()).then_some(KeyCode::new(raw))
    }
}

#[derive(Debug, Default)]
pub struct MacosInputInjector {
    initialized: AtomicBool,
    direct_cursor_movement: AtomicBool,
}

impl MacosInputInjector {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Check that macOS Accessibility permission is currently available for global event injection.
    ///
    /// # Errors
    ///
    /// Returns the same permission failure that the legacy CoreGraphics backend reported at startup.
    pub fn init(&self) -> Result<(), InputError> {
        if !accessibility_trusted() {
            self.initialized.store(false, Ordering::Release);
            return Err(InputError::new(
                "macOS Accessibility permission is required before SMU can inject keyboard and mouse input.",
            ));
        }
        self.initialized.store(true, Ordering::Release);
        Ok(())
    }

    fn output_allowed(&self) -> bool {
        self.initialized.load(Ordering::Acquire) && accessibility_trusted()
    }

    /// Match the legacy `macos_cursor_movement` preference. `false` preserves game-safe relative deltas;
    /// `true` visibly repositions the cursor by the requested delta.
    pub fn set_direct_cursor_movement(&self, enabled: bool) {
        self.direct_cursor_movement.store(enabled, Ordering::Release);
    }

    #[must_use]
    pub fn direct_cursor_movement(&self) -> bool {
        self.direct_cursor_movement.load(Ordering::Acquire)
    }

    fn post_unicode_character(&self, character: u8, delay_ms: i32) -> bool {
        if !self.output_allowed() {
            return false;
        }
        let unicode_character = u16::from(character);
        let Some(key_down) = CGEvent::new_keyboard_event(None, 0, true) else {
            return false;
        };
        // SAFETY: unicode_character points to one valid UTF-16 code unit for the duration of the call.
        unsafe {
            CGEvent::keyboard_set_unicode_string(Some(&key_down), 1, &unicode_character);
        }
        post_tagged_event(&key_down);

        thread::sleep(Duration::from_millis(u64::try_from(delay_ms.max(0)).unwrap_or(0)));

        let Some(key_up) = CGEvent::new_keyboard_event(None, 0, false) else {
            return false;
        };
        // SAFETY: unicode_character points to one valid UTF-16 code unit for the duration of the call.
        unsafe {
            CGEvent::keyboard_set_unicode_string(Some(&key_up), 1, &unicode_character);
        }
        post_tagged_event(&key_up);
        true
    }

    fn press_character_with_active_layout(&self, character: u8, delay_ms: i32) -> bool {
        if !self.output_allowed() {
            return false;
        }
        let Some(resolved) = resolve_character_key(character) else {
            return self.post_unicode_character(character, delay_ms);
        };

        let option_held = resolved.option && post_raw_keyboard_event(MAC_KEY_OPTION, true);
        let shift_held = resolved.shift
            && (!resolved.option || option_held)
            && post_raw_keyboard_event(MAC_KEY_SHIFT, true);
        let modifiers_ready = (!resolved.option || option_held) && (!resolved.shift || shift_held);
        let key_down = modifiers_ready && post_raw_keyboard_event(resolved.key_code, true);
        if !key_down {
            if shift_held {
                let _ = post_raw_keyboard_event(MAC_KEY_SHIFT, false);
            }
            if option_held {
                let _ = post_raw_keyboard_event(MAC_KEY_OPTION, false);
            }
            return self.post_unicode_character(character, delay_ms);
        }

        thread::sleep(Duration::from_millis(u64::try_from(delay_ms.max(0)).unwrap_or(0)));
        let _ = post_raw_keyboard_event(resolved.key_code, false);
        if shift_held {
            let _ = post_raw_keyboard_event(MAC_KEY_SHIFT, false);
        }
        if option_held {
            let _ = post_raw_keyboard_event(MAC_KEY_OPTION, false);
        }
        true
    }

    fn post_keyboard(&self, key: KeyCode, down: bool) -> bool {
        if !self.output_allowed() {
            return false;
        }
        let Some(mac_key) = macos_key_code(key) else {
            return false;
        };
        if mouse_button_description(mac_key).is_some() {
            return false;
        }
        let Some(event) = CGEvent::new_keyboard_event(None, mac_key, down) else {
            return false;
        };
        post_tagged_event(&event);
        true
    }

    fn post_mouse_button(&self, key: KeyCode, down: bool) -> bool {
        if !self.output_allowed() {
            return false;
        }
        let Some(description) = macos_key_code(key).and_then(mouse_button_description) else {
            return false;
        };
        let Some(cursor) = current_cursor_point() else {
            return false;
        };
        let event_type = if down { description.down_type } else { description.up_type };
        let Some(event) = CGEvent::new_mouse_event(None, event_type, cursor, description.button)
        else {
            return false;
        };
        if description.down_type == CGEventType::OtherMouseDown {
            CGEvent::set_integer_value_field(
                Some(&event),
                CGEventField::MouseEventButtonNumber,
                description.button_number,
            );
        }
        post_tagged_event(&event);
        true
    }

    fn post_mouse_move(&self, target: CGPoint, delta: Option<(i32, i32)>) -> bool {
        if !self.output_allowed() {
            return false;
        }
        let Some(event) =
            CGEvent::new_mouse_event(None, CGEventType::MouseMoved, target, CGMouseButton::Left)
        else {
            return false;
        };
        if let Some((dx, dy)) = delta {
            CGEvent::set_integer_value_field(
                Some(&event),
                CGEventField::MouseEventDeltaX,
                i64::from(dx),
            );
            CGEvent::set_integer_value_field(
                Some(&event),
                CGEventField::MouseEventDeltaY,
                i64::from(dy),
            );
        }
        post_tagged_event(&event);
        true
    }
}

impl InputInjector for MacosInputInjector {
    fn hold_key(&self, key: KeyCode, _extended: bool) {
        if self.post_mouse_button(key, true) {
            return;
        }
        let _ = self.post_keyboard(key, true);
    }

    fn release_key(&self, key: KeyCode, _extended: bool) {
        if self.post_mouse_button(key, false) {
            return;
        }
        let _ = self.post_keyboard(key, false);
    }

    fn move_mouse(&self, dx: i32, dy: i32) {
        self.move_mouse_raw(dx, dy);
    }

    fn move_mouse_raw(&self, dx: i32, dy: i32) {
        let Some(cursor) = current_cursor_point() else {
            return;
        };
        if self.direct_cursor_movement() {
            let _ = self.post_mouse_move(
                CGPoint::new(cursor.x + f64::from(dx), cursor.y + f64::from(dy)),
                None,
            );
        } else {
            let _ = self.post_mouse_move(cursor, Some((dx, dy)));
        }
    }

    fn move_mouse_absolute(&self, x: i32, y: i32) -> Result<(), InputError> {
        if !self.output_allowed() {
            return Err(InputError::new(
                "macOS Accessibility permission is required to move the global cursor.",
            ));
        }
        if self.post_mouse_move(CGPoint::new(f64::from(x), f64::from(y)), None) {
            Ok(())
        } else {
            Err(InputError::new("macOS failed to create a global absolute mouse movement event."))
        }
    }

    fn prefers_direct_absolute_positioning(&self) -> bool {
        true
    }

    fn absolute_pointer_unavailable_reason(&self) -> String {
        if accessibility_trusted() {
            "macOS could not create a global absolute mouse movement event.".to_owned()
        } else {
            "macOS Accessibility permission is required for absolute cursor movement.".to_owned()
        }
    }

    fn mouse_wheel(&self, delta: i32) {
        if delta == 0 || !self.output_allowed() {
            return;
        }
        let Some(event) =
            CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Line, 1, delta, 0, 0)
        else {
            return;
        };
        post_tagged_event(&event);
    }

    fn press_character(&self, character: u8, delay_ms: i32) -> bool {
        self.press_character_with_active_layout(character, delay_ms)
    }
}
