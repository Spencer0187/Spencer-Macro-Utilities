//! Native Linux evdev/uinput input backend.
//!
//! Physical state is read passively from `/dev/input/event*`; injection goes through one uinput
//! device named `SMU Native Linux Input`, matching the C++ backend and intentionally excluding that
//! device from physical-state discovery.

use crate::{InputError, InputInjector, KeyState};
use evdev::uinput::VirtualDevice;
use evdev::{
    AttributeSet, BusType, Device, EventType, InputEvent, InputId, KeyCode as EvKeyCode,
    RelativeAxisCode,
};
use smu_core::KeyCode;
use smu_display::linux::WaylandPortalCapture;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt as _, Keysym};

const DEVICE_NAME: &str = "SMU Native Linux Input";
const KEY_STATE_COUNT: usize = 258;
const EVDEV_KEY_MAX_EXCLUSIVE: u16 = 0x2ff;
const INTERACTIVE_DETECTION_TIMEOUT: Duration = Duration::from_secs(12);
const READER_POLL_INTERVAL: Duration = Duration::from_millis(50);
const INTERACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(25);

const X11_SHIFT_MASK: u8 = 1 << 0;
const X11_MOD1_MASK: u8 = 1 << 3;
const X11_MOD5_MASK: u8 = 1 << 7;
const X11_CANDIDATE_STATES: [u8; 6] = [
    0,
    X11_SHIFT_MASK,
    X11_MOD1_MASK,
    X11_MOD5_MASK,
    X11_SHIFT_MASK | X11_MOD1_MASK,
    X11_SHIFT_MASK | X11_MOD5_MASK,
];
const XK_MODE_SWITCH: Keysym = 0xFF7E;
const XK_ISO_LEVEL3_SHIFT: Keysym = 0xFE03;

#[derive(Clone, Debug, Eq, PartialEq)]
struct X11CharacterKey {
    evdev_key: u16,
    modifier_evdev_keys: Vec<u16>,
}

#[derive(Debug)]
struct SharedInputState {
    running: AtomicBool,
    key_states: [AtomicBool; KEY_STATE_COUNT],
    pending_binding_input: AtomicU32,
}

impl Default for SharedInputState {
    fn default() -> Self {
        Self {
            running: AtomicBool::new(false),
            key_states: std::array::from_fn(|_| AtomicBool::new(false)),
            pending_binding_input: AtomicU32::new(KeyCode::NONE.raw()),
        }
    }
}

pub struct EvdevUinputInputDevice {
    shared: Arc<SharedInputState>,
    initialized: AtomicBool,
    virtual_device: Mutex<Option<VirtualDevice>>,
    device_paths: Mutex<Vec<PathBuf>>,
    reader_threads: Mutex<Vec<JoinHandle<()>>>,
    wayland_portal: Option<Arc<WaylandPortalCapture>>,
}

impl std::fmt::Debug for EvdevUinputInputDevice {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EvdevUinputInputDevice")
            .field("initialized", &self.initialized.load(Ordering::Acquire))
            .field("device_paths", &*lock(&self.device_paths))
            .field("wayland_portal", &self.wayland_portal.is_some())
            .finish_non_exhaustive()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl Default for EvdevUinputInputDevice {
    fn default() -> Self {
        Self {
            shared: Arc::new(SharedInputState::default()),
            initialized: AtomicBool::new(false),
            virtual_device: Mutex::new(None),
            device_paths: Mutex::new(Vec::new()),
            reader_threads: Mutex::new(Vec::new()),
            wayland_portal: None,
        }
    }
}

impl EvdevUinputInputDevice {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_wayland_portal(mut self, portal: Arc<WaylandPortalCapture>) -> Self {
        self.wayland_portal = Some(portal);
        self
    }

    /// Create the uinput device, discover readable physical keyboard/mouse event devices, and start
    /// passive state-reader workers.
    ///
    /// # Errors
    ///
    /// Returns the platform permission/discovery failure that prevents native Linux input.
    pub fn init(&self) -> Result<(), InputError> {
        if self.initialized.load(Ordering::Acquire) {
            return Ok(());
        }
        if !Path::new("/dev/input").exists() {
            return Err(InputError::new(format!(
                "Linux input backend cannot access /dev/input: {}",
                std::io::Error::last_os_error()
            )));
        }

        let virtual_device = create_virtual_device().map_err(|error| {
            InputError::new(format!("Linux input backend failed to open /dev/uinput: {error}"))
        })?;
        *lock(&self.virtual_device) = Some(virtual_device);
        std::thread::sleep(Duration::from_millis(150));

        let paths = match detect_input_devices() {
            Ok(paths) => paths,
            Err(error) => {
                lock(&self.virtual_device).take();
                return Err(error);
            }
        };

        self.shared.running.store(true, Ordering::Release);
        let mut threads = lock(&self.reader_threads);
        for path in &paths {
            let shared = Arc::clone(&self.shared);
            let path = path.clone();
            threads.push(std::thread::spawn(move || reader_thread(&shared, &path)));
        }
        drop(threads);
        *lock(&self.device_paths) = paths;
        self.initialized.store(true, Ordering::Release);
        Ok(())
    }

    pub fn shutdown(&self) {
        self.shared.running.store(false, Ordering::Release);
        let threads = std::mem::take(&mut *lock(&self.reader_threads));
        for thread in threads {
            let _ = thread.join();
        }
        lock(&self.device_paths).clear();
        lock(&self.virtual_device).take();
        for state in &self.shared.key_states {
            state.store(false, Ordering::Relaxed);
        }
        self.shared.pending_binding_input.store(KeyCode::NONE.raw(), Ordering::Relaxed);
        self.initialized.store(false, Ordering::Release);
    }

    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn device_paths(&self) -> Vec<PathBuf> {
        lock(&self.device_paths).clone()
    }

    #[must_use]
    pub fn current_pressed_key(&self) -> Option<KeyCode> {
        self.shared
            .key_states
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, state)| state.load(Ordering::Acquire))
            .and_then(|(raw, _)| u32::try_from(raw).ok())
            .map(KeyCode::new)
    }

    pub fn consume_next_transient_input(&self) -> Option<KeyCode> {
        let raw = self.shared.pending_binding_input.swap(KeyCode::NONE.raw(), Ordering::AcqRel);
        (raw != KeyCode::NONE.raw()).then_some(KeyCode::new(raw))
    }

    #[allow(clippy::significant_drop_tightening)] // uinput emit requires exclusive device access.
    fn emit(&self, events: &[InputEvent]) {
        let mut device = lock(&self.virtual_device);
        let Some(device) = device.as_mut() else {
            return;
        };
        let _ = device.emit(events);
    }
}

impl Drop for EvdevUinputInputDevice {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl KeyState for EvdevUinputInputDevice {
    fn is_pressed(&self, key: KeyCode) -> bool {
        usize::try_from(key.raw())
            .ok()
            .and_then(|index| self.shared.key_states.get(index))
            .is_some_and(|state| state.load(Ordering::Acquire))
    }

    fn current_pressed_key(&self) -> Option<KeyCode> {
        Self::current_pressed_key(self)
    }

    fn consume_next_transient_input(&self) -> Option<KeyCode> {
        Self::consume_next_transient_input(self)
    }
}

impl InputInjector for EvdevUinputInputDevice {
    fn hold_key(&self, key: KeyCode, _extended: bool) {
        let Some(evdev) = vk_to_evdev(key) else {
            return;
        };
        self.emit(&[InputEvent::new(EventType::KEY.0, evdev.0, 1)]);
    }

    fn release_key(&self, key: KeyCode, _extended: bool) {
        let Some(evdev) = vk_to_evdev(key) else {
            return;
        };
        self.emit(&[InputEvent::new(EventType::KEY.0, evdev.0, 0)]);
    }

    fn press_character(&self, character: u8, delay_ms: i32) -> bool {
        let Some(resolved) = resolve_character_with_x11(character) else {
            return false;
        };
        let mut down = resolved
            .modifier_evdev_keys
            .iter()
            .copied()
            .map(|key| InputEvent::new(EventType::KEY.0, key, 1))
            .collect::<Vec<_>>();
        down.push(InputEvent::new(EventType::KEY.0, resolved.evdev_key, 1));
        self.emit(&down);
        std::thread::sleep(Duration::from_millis(
            u64::try_from(delay_ms.max(0)).unwrap_or_default(),
        ));

        let mut up = Vec::with_capacity(resolved.modifier_evdev_keys.len() + 1);
        up.push(InputEvent::new(EventType::KEY.0, resolved.evdev_key, 0));
        up.extend(
            resolved
                .modifier_evdev_keys
                .iter()
                .rev()
                .copied()
                .map(|key| InputEvent::new(EventType::KEY.0, key, 0)),
        );
        self.emit(&up);
        true
    }

    fn move_mouse(&self, dx: i32, dy: i32) {
        self.emit(&[
            InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_X.0, dx),
            InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_Y.0, dy),
        ]);
    }

    fn move_mouse_raw(&self, dx: i32, dy: i32) {
        self.move_mouse(dx, dy);
    }

    fn move_mouse_absolute(&self, x: i32, y: i32) -> Result<(), InputError> {
        if is_native_wayland_session() {
            let Some(portal) = &self.wayland_portal else {
                return Err(InputError::new(self.absolute_pointer_unavailable_reason()));
            };
            return portal
                .move_pointer_absolute(x, y)
                .map_err(|error| InputError::new(error.to_string()));
        }
        move_mouse_absolute_x11(x, y)
    }

    fn prefers_direct_absolute_positioning(&self) -> bool {
        is_native_wayland_session()
            && self
                .wayland_portal
                .as_ref()
                .is_some_and(|portal| portal.has_remote_desktop_control())
    }

    fn absolute_pointer_unavailable_reason(&self) -> String {
        linux_absolute_pointer_unavailable_reason(self.wayland_portal.as_deref())
    }

    fn mouse_wheel(&self, delta: i32) {
        let value = if delta >= 0 { 1 } else { -1 };
        self.emit(&[InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_WHEEL.0, value)]);
    }
}

fn resolve_character_with_x11(character: u8) -> Option<X11CharacterKey> {
    let (connection, _) = x11rb::connect(None).ok()?;
    let setup = connection.setup();
    let minimum_keycode = setup.min_keycode;
    let maximum_keycode = setup.max_keycode;
    let count = maximum_keycode.checked_sub(minimum_keycode)?.checked_add(1)?;
    let keyboard = connection.get_keyboard_mapping(minimum_keycode, count).ok()?.reply().ok()?;
    let modifiers = connection.get_modifier_mapping().ok()?.reply().ok()?;
    resolve_character_from_x11_maps(
        character,
        minimum_keycode,
        maximum_keycode,
        keyboard.keysyms_per_keycode,
        &keyboard.keysyms,
        &modifiers.keycodes,
    )
}

fn resolve_character_from_x11_maps(
    character: u8,
    minimum_keycode: u8,
    maximum_keycode: u8,
    keysyms_per_keycode: u8,
    keysyms: &[Keysym],
    modifier_keycodes: &[u8],
) -> Option<X11CharacterKey> {
    if keysyms_per_keycode == 0 || !modifier_keycodes.len().is_multiple_of(8) {
        return None;
    }
    let mod1_level3 = modifier_has_level3(
        3,
        modifier_keycodes,
        minimum_keycode,
        maximum_keycode,
        keysyms_per_keycode,
        keysyms,
    );
    let mod5_level3 = modifier_has_level3(
        7,
        modifier_keycodes,
        minimum_keycode,
        maximum_keycode,
        keysyms_per_keycode,
        keysyms,
    );

    for keycode in minimum_keycode..=maximum_keycode {
        let symbols = keysyms_for_keycode(
            keycode,
            minimum_keycode,
            maximum_keycode,
            keysyms_per_keycode,
            keysyms,
        );
        for state in X11_CANDIDATE_STATES {
            let symbol = keysym_for_state(symbols, state, mod1_level3, mod5_level3);
            if keysym_to_byte(symbol) != Some(character) || keycode < 8 {
                continue;
            }
            let evdev_key = u16::from(keycode) - 8;
            if evdev_key == 0 || evdev_key >= EVDEV_KEY_MAX_EXCLUSIVE {
                return None;
            }
            let modifier_evdev_keys = modifier_evdev_keys_for_state(state, modifier_keycodes)?;
            return Some(X11CharacterKey { evdev_key, modifier_evdev_keys });
        }
    }
    None
}

fn keysyms_for_keycode(
    keycode: u8,
    minimum_keycode: u8,
    maximum_keycode: u8,
    keysyms_per_keycode: u8,
    keysyms: &[Keysym],
) -> &[Keysym] {
    if keycode < minimum_keycode || keycode > maximum_keycode || keysyms_per_keycode == 0 {
        return &[];
    }
    let width = usize::from(keysyms_per_keycode);
    let start = usize::from(keycode - minimum_keycode).saturating_mul(width);
    let end = start.saturating_add(width);
    keysyms.get(start..end).unwrap_or(&[])
}

fn modifier_has_level3(
    modifier_index: usize,
    modifier_keycodes: &[u8],
    minimum_keycode: u8,
    maximum_keycode: u8,
    keysyms_per_keycode: u8,
    keysyms: &[Keysym],
) -> bool {
    let per_modifier = modifier_keycodes.len() / 8;
    let start = modifier_index.saturating_mul(per_modifier);
    let end = start.saturating_add(per_modifier);
    modifier_keycodes.get(start..end).is_some_and(|keys| {
        keys.iter().copied().filter(|keycode| *keycode != 0).any(|keycode| {
            keysyms_for_keycode(
                keycode,
                minimum_keycode,
                maximum_keycode,
                keysyms_per_keycode,
                keysyms,
            )
            .iter()
            .any(|symbol| matches!(*symbol, XK_MODE_SWITCH | XK_ISO_LEVEL3_SHIFT))
        })
    })
}

fn keysym_for_state(symbols: &[Keysym], state: u8, mod1_level3: bool, mod5_level3: bool) -> Keysym {
    let shift = state & X11_SHIFT_MASK != 0;
    let level3 =
        (state & X11_MOD1_MASK != 0 && mod1_level3) || (state & X11_MOD5_MASK != 0 && mod5_level3);
    let mut pair = if level3 { 2 } else { 0 };
    if pair >= symbols.len()
        || (symbols.get(pair).copied().unwrap_or_default() == 0
            && symbols.get(pair + 1).copied().unwrap_or_default() == 0)
    {
        pair = 0;
    }
    let base = symbols.get(pair).copied().unwrap_or_default();
    let shifted = symbols.get(pair + 1).copied().unwrap_or_default();
    if shift {
        if shifted != 0 { shifted } else { ascii_upper_keysym(base) }
    } else if shifted == 0 {
        ascii_lower_keysym(base)
    } else {
        base
    }
}

const fn ascii_lower_keysym(symbol: Keysym) -> Keysym {
    if symbol >= b'A' as u32 && symbol <= b'Z' as u32 { symbol + 32 } else { symbol }
}

const fn ascii_upper_keysym(symbol: Keysym) -> Keysym {
    if symbol >= b'a' as u32 && symbol <= b'z' as u32 { symbol - 32 } else { symbol }
}

fn keysym_to_byte(symbol: Keysym) -> Option<u8> {
    u8::try_from(symbol).ok()
}

fn modifier_evdev_keys_for_state(state: u8, modifier_keycodes: &[u8]) -> Option<Vec<u16>> {
    let per_modifier = modifier_keycodes.len() / 8;
    let mut result = Vec::new();
    for modifier_index in 0..8 {
        if state & (1 << modifier_index) == 0 {
            continue;
        }
        let start = modifier_index * per_modifier;
        let end = start + per_modifier;
        let modifier_keycode =
            modifier_keycodes.get(start..end)?.iter().copied().find(|keycode| *keycode >= 8)?;
        let evdev_key = u16::from(modifier_keycode) - 8;
        if evdev_key == 0 || evdev_key >= EVDEV_KEY_MAX_EXCLUSIVE {
            return None;
        }
        result.push(evdev_key);
    }
    Some(result)
}

fn create_virtual_device() -> std::io::Result<VirtualDevice> {
    let mut keys = AttributeSet::<EvKeyCode>::new();
    for code in 0..EVDEV_KEY_MAX_EXCLUSIVE {
        keys.insert(EvKeyCode::new(code));
    }
    let axes = AttributeSet::from_iter([
        RelativeAxisCode::REL_X,
        RelativeAxisCode::REL_Y,
        RelativeAxisCode::REL_WHEEL,
    ]);
    VirtualDevice::builder()?
        .name(DEVICE_NAME)
        .input_id(InputId::new(BusType::BUS_USB, 0x534d, 0x4301, 1))
        .with_keys(&keys)?
        .with_relative_axes(&axes)?
        .build()
}

fn looks_like_keyboard(device: &Device) -> bool {
    device.name() != Some(DEVICE_NAME)
        && device.supported_keys().is_some_and(|keys| {
            [EvKeyCode::KEY_A, EvKeyCode::KEY_SPACE, EvKeyCode::KEY_ENTER, EvKeyCode::KEY_ESC]
                .into_iter()
                .any(|key| keys.contains(key))
        })
}

fn looks_like_mouse(device: &Device) -> bool {
    device.name() != Some(DEVICE_NAME)
        && device.supported_relative_axes().is_some_and(|axes| {
            [RelativeAxisCode::REL_X, RelativeAxisCode::REL_Y]
                .into_iter()
                .any(|axis| axes.contains(axis))
        })
        && device.supported_keys().is_some_and(|keys| {
            [EvKeyCode::BTN_LEFT, EvKeyCode::BTN_RIGHT, EvKeyCode::BTN_MIDDLE]
                .into_iter()
                .any(|key| keys.contains(key))
        })
}

fn detect_input_devices() -> Result<Vec<PathBuf>, InputError> {
    let mut devices = evdev::enumerate().collect::<Vec<_>>();
    devices.sort_by(|left, right| left.0.cmp(&right.0));
    let mut paths = BTreeSet::new();
    let mut has_keyboard = false;
    let mut has_mouse = false;
    for (path, device) in &devices {
        let keyboard = looks_like_keyboard(device);
        let mouse = looks_like_mouse(device);
        if keyboard || mouse {
            paths.insert(path.clone());
        }
        has_keyboard |= keyboard;
        has_mouse |= mouse;
    }

    if !has_keyboard || !has_mouse {
        append_interactive_detection(&mut paths, &mut devices, &mut has_keyboard, &mut has_mouse)?;
    }
    if paths.is_empty() {
        return Err(InputError::new(
            "Linux input backend did not find readable /dev/input/event* keyboard or mouse devices.",
        ));
    }
    Ok(paths.into_iter().collect())
}

fn append_interactive_detection(
    paths: &mut BTreeSet<PathBuf>,
    devices: &mut [(PathBuf, Device)],
    has_keyboard: &mut bool,
    has_mouse: &mut bool,
) -> Result<(), InputError> {
    for (_, device) in devices.iter() {
        let _ = device.set_nonblocking(true);
    }
    let deadline = Instant::now() + INTERACTIVE_DETECTION_TIMEOUT;
    while (!*has_keyboard || !*has_mouse) && Instant::now() < deadline {
        for (path, device) in devices.iter_mut() {
            if device.name() == Some(DEVICE_NAME) {
                continue;
            }
            match device.fetch_events() {
                Ok(events) => {
                    for event in events {
                        let (keyboard, mouse) = classify_interactive_event(&event);
                        if keyboard && !*has_keyboard {
                            paths.insert(path.clone());
                            *has_keyboard = true;
                        }
                        if mouse && !*has_mouse {
                            paths.insert(path.clone());
                            *has_mouse = true;
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(_) => {}
            }
        }
        if !*has_keyboard || !*has_mouse {
            std::thread::sleep(INTERACTIVE_POLL_INTERVAL);
        }
    }
    if !*has_keyboard || !*has_mouse {
        return Err(InputError::new(
            "Linux input backend could not detect both keyboard and mouse event devices.",
        ));
    }
    Ok(())
}

fn classify_interactive_event(event: &InputEvent) -> (bool, bool) {
    let keyboard = event.event_type() == EventType::KEY
        && event.value() == 1
        && event.code() < EvKeyCode::BTN_LEFT.0;
    let mouse_relative = event.event_type() == EventType::RELATIVE
        && matches!(
            event.code(),
            code if code == RelativeAxisCode::REL_X.0
                || code == RelativeAxisCode::REL_Y.0
                || code == RelativeAxisCode::REL_WHEEL.0
        );
    let mouse_key = event.event_type() == EventType::KEY
        && (EvKeyCode::BTN_LEFT.0..=EvKeyCode::BTN_TASK.0).contains(&event.code());
    (keyboard, mouse_relative || mouse_key)
}

fn reader_thread(shared: &SharedInputState, path: &Path) {
    let Ok(mut device) = Device::open(path) else {
        return;
    };
    if device.set_nonblocking(true).is_err() {
        return;
    }
    while shared.running.load(Ordering::Acquire) {
        match device.fetch_events() {
            Ok(events) => {
                for event in events {
                    if event.event_type() == EventType::KEY {
                        set_key_state_from_evdev(shared, event.code(), event.value() != 0);
                    } else if event.event_type() == EventType::RELATIVE
                        && event.code() == RelativeAxisCode::REL_WHEEL.0
                        && event.value() != 0
                    {
                        shared.pending_binding_input.store(
                            if event.value() > 0 {
                                KeyCode::MOUSE_WHEEL_UP.raw()
                            } else {
                                KeyCode::MOUSE_WHEEL_DOWN.raw()
                            },
                            Ordering::Release,
                        );
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(READER_POLL_INTERVAL);
            }
            Err(_) => break,
        }
    }
}

fn set_key_state_from_evdev(shared: &SharedInputState, evdev_code: u16, pressed: bool) {
    let key = evdev_to_vk(evdev_code);
    if let Ok(index) = usize::try_from(key.raw())
        && let Some(state) = shared.key_states.get(index)
    {
        state.store(pressed, Ordering::Release);
    }
}

#[must_use]
#[allow(clippy::too_many_lines, clippy::match_same_arms)]
pub const fn vk_to_evdev(key: KeyCode) -> Option<EvKeyCode> {
    let raw = key.raw();
    let code = match raw {
        0xA0 | 0x10 => 42,
        0xA1 => 54,
        0xA2 | 0x11 => 29,
        0xA3 => 97,
        0xA4 | 0x12 => 56,
        0xA5 => 100,
        0x01 => 0x110,
        0x02 => 0x111,
        0x04 => 0x112,
        0x05 => 0x113,
        0x06 => 0x114,
        0x1B => 1,
        0x31 => 2,
        0x32 => 3,
        0x33 => 4,
        0x34 => 5,
        0x35 => 6,
        0x36 => 7,
        0x37 => 8,
        0x38 => 9,
        0x39 => 10,
        0x30 => 11,
        0xBD => 12,
        0xBB => 13,
        0x08 => 14,
        0x09 => 15,
        0x51 => 16,
        0x57 => 17,
        0x45 => 18,
        0x52 => 19,
        0x54 => 20,
        0x59 => 21,
        0x55 => 22,
        0x49 => 23,
        0x4F => 24,
        0x50 => 25,
        0xDB => 26,
        0xDD => 27,
        0x0D => 28,
        0x41 => 30,
        0x53 => 31,
        0x44 => 32,
        0x46 => 33,
        0x47 => 34,
        0x48 => 35,
        0x4A => 36,
        0x4B => 37,
        0x4C => 38,
        0xBA => 39,
        0xDE => 40,
        0xC0 => 41,
        0xDC => 43,
        0x5A => 44,
        0x58 => 45,
        0x43 => 46,
        0x56 => 47,
        0x42 => 48,
        0x4E => 49,
        0x4D => 50,
        0xBC => 51,
        0xBE => 52,
        0xBF => 53,
        0x6A => 55,
        0x20 => 57,
        0x14 => 58,
        0x70 => 59,
        0x71 => 60,
        0x72 => 61,
        0x73 => 62,
        0x74 => 63,
        0x75 => 64,
        0x76 => 65,
        0x77 => 66,
        0x78 => 67,
        0x79 => 68,
        0x90 => 69,
        0x91 => 70,
        0x67 => 71,
        0x68 => 72,
        0x69 => 73,
        0x6D => 74,
        0x64 => 75,
        0x65 => 76,
        0x66 => 77,
        0x6B => 78,
        0x61 => 79,
        0x62 => 80,
        0x63 => 81,
        0x60 => 82,
        0x6E => 83,
        0x7A => 87,
        0x7B => 88,
        0x6F => 98,
        0x2C => 99,
        0x24 => 102,
        0x26 => 103,
        0x21 => 104,
        0x25 => 105,
        0x27 => 106,
        0x23 => 107,
        0x28 => 108,
        0x22 => 109,
        0x2D => 110,
        0x2E => 111,
        0x13 => 119,
        0x5B => 125,
        0x5C => 126,
        0x5D => 127,
        0x7C => 183,
        0x7D => 184,
        0x7E => 185,
        0x7F => 186,
        0x80 => 187,
        0x81 => 188,
        0x82 => 189,
        0x83 => 190,
        0x84 => 191,
        0x85 => 192,
        0x86 => 193,
        0x87 => 194,
        _ => return None,
    };
    Some(EvKeyCode::new(code))
}

#[must_use]
#[allow(clippy::too_many_lines)]
pub const fn evdev_to_vk(evdev_code: u16) -> KeyCode {
    let raw = match evdev_code {
        0x110 => 0x01,
        0x111 => 0x02,
        0x112 => 0x04,
        0x113 => 0x05,
        0x114 => 0x06,
        1 => 0x1B,
        2 => 0x31,
        3 => 0x32,
        4 => 0x33,
        5 => 0x34,
        6 => 0x35,
        7 => 0x36,
        8 => 0x37,
        9 => 0x38,
        10 => 0x39,
        11 => 0x30,
        12 => 0xBD,
        13 => 0xBB,
        14 => 0x08,
        15 => 0x09,
        16 => 0x51,
        17 => 0x57,
        18 => 0x45,
        19 => 0x52,
        20 => 0x54,
        21 => 0x59,
        22 => 0x55,
        23 => 0x49,
        24 => 0x4F,
        25 => 0x50,
        26 => 0xDB,
        27 => 0xDD,
        28 | 96 => 0x0D,
        29 | 97 => 0x11,
        30 => 0x41,
        31 => 0x53,
        32 => 0x44,
        33 => 0x46,
        34 => 0x47,
        35 => 0x48,
        36 => 0x4A,
        37 => 0x4B,
        38 => 0x4C,
        39 => 0xBA,
        40 => 0xDE,
        41 => 0xC0,
        42 | 54 => 0x10,
        43 => 0xDC,
        44 => 0x5A,
        45 => 0x58,
        46 => 0x43,
        47 => 0x56,
        48 => 0x42,
        49 => 0x4E,
        50 => 0x4D,
        51 => 0xBC,
        52 => 0xBE,
        53 => 0xBF,
        55 => 0x6A,
        56 | 100 => 0x12,
        57 => 0x20,
        58 => 0x14,
        59 => 0x70,
        60 => 0x71,
        61 => 0x72,
        62 => 0x73,
        63 => 0x74,
        64 => 0x75,
        65 => 0x76,
        66 => 0x77,
        67 => 0x78,
        68 => 0x79,
        69 => 0x90,
        70 => 0x91,
        71 => 0x67,
        72 => 0x68,
        73 => 0x69,
        74 => 0x6D,
        75 => 0x64,
        76 => 0x65,
        77 => 0x66,
        78 => 0x6B,
        79 => 0x61,
        80 => 0x62,
        81 => 0x63,
        82 => 0x60,
        83 => 0x6E,
        87 => 0x7A,
        88 => 0x7B,
        98 => 0x6F,
        99 => 0x2C,
        102 => 0x24,
        103 => 0x26,
        104 => 0x21,
        105 => 0x25,
        106 => 0x27,
        107 => 0x23,
        108 => 0x28,
        109 => 0x22,
        110 => 0x2D,
        111 => 0x2E,
        119 => 0x13,
        125 => 0x5B,
        126 => 0x5C,
        127 => 0x5D,
        183 => 0x7C,
        184 => 0x7D,
        185 => 0x7E,
        186 => 0x7F,
        187 => 0x80,
        188 => 0x81,
        189 => 0x82,
        190 => 0x83,
        191 => 0x84,
        192 => 0x85,
        193 => 0x86,
        194 => 0x87,
        _ => 0x0F,
    };
    KeyCode::new(raw)
}

fn is_native_wayland_session() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|value| value.eq_ignore_ascii_case("wayland"))
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn move_mouse_absolute_x11(x: i32, y: i32) -> Result<(), InputError> {
    let (connection, screen_index) = x11rb::connect(None)
        .map_err(|_| InputError::new(linux_absolute_pointer_unavailable_reason(None)))?;
    let screen = connection
        .setup()
        .roots
        .get(screen_index)
        .ok_or_else(|| InputError::new(linux_absolute_pointer_unavailable_reason(None)))?;
    let width = i32::from(screen.width_in_pixels);
    let height = i32::from(screen.height_in_pixels);
    if width <= 0 || height <= 0 {
        return Err(InputError::new(
            "absolute screen coordinates are unavailable. X11 reported invalid screen bounds.",
        ));
    }
    let x = x.clamp(0, width - 1);
    let y = y.clamp(0, height - 1);
    let x = i16::try_from(x).unwrap_or(i16::MAX);
    let y = i16::try_from(y).unwrap_or(i16::MAX);
    let cookie = connection
        .warp_pointer(x11rb::NONE, screen.root, 0, 0, 0, 0, x, y)
        .map_err(|_| InputError::new("absolute X11 cursor movement failed"))?;
    cookie.check().map_err(|_| InputError::new("absolute X11 cursor movement failed"))?;
    connection.flush().map_err(|_| InputError::new("absolute X11 cursor movement failed"))?;
    Ok(())
}

fn linux_absolute_pointer_unavailable_reason(portal: Option<&WaylandPortalCapture>) -> String {
    if is_native_wayland_session() {
        if let Some(portal) = portal {
            if portal.has_remote_desktop_control() {
                return "Wayland RemoteDesktop lets moveMouseAbs target the selected monitor, but does not reveal the current global cursor position. Relative operations that need that position, including moveMouse in absolute mode and moveDegrees in absolute mode, remain unavailable."
                    .to_owned();
            }
            if !portal.is_active() {
                return "absolute screen coordinates are unavailable on native Wayland. moveMouseAbs cannot use a global cursor position; to use getPixelColor or getPixelRect, open Settings and choose Enable Wayland Screen Capture."
                    .to_owned();
            }
            return "absolute mouse positioning is unavailable on native Wayland. The active ScreenCast monitor will become available to pixel APIs after PipeWire delivers its first frame."
                .to_owned();
        }
        return "absolute screen coordinates are unavailable on native Wayland. moveMouseAbs requires a composed Wayland RemoteDesktop portal service."
            .to_owned();
    }
    let display = std::env::var_os("DISPLAY").is_some_and(|value| !value.is_empty());
    if !display {
        return "absolute screen coordinates are unavailable. moveMouseAbs and getPixelColor on Linux currently require an X11 DISPLAY so SMU can query the global cursor position and active monitor bounds."
            .to_owned();
    }
    if x11rb::connect(None).is_err() {
        return "absolute screen coordinates are unavailable. moveMouseAbs and getPixelColor on Linux require X11 cursor-position access, but XOpenDisplay failed for the current DISPLAY."
            .to_owned();
    }
    "absolute screen coordinates are unavailable. moveMouseAbs and getPixelColor on Linux require X11 cursor-position and active-monitor queries, but the current X11 query failed."
        .to_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use serde_json::Value;

    const KEYMAP_FIXTURE: &str = include_str!("../../../../compat/input/linux_keymap.json");

    #[test]
    fn full_linux_keymap_matches_cpp_generated_fixture() {
        let fixture: Value = serde_json::from_str(KEYMAP_FIXTURE)
            .expect("generated C++ keymap fixture should be valid JSON");
        let win_to_evdev =
            fixture["win_to_evdev"].as_array().expect("win_to_evdev should be an array");
        for case in win_to_evdev {
            let vk = u32::try_from(
                case["vk"].as_u64().expect("fixture vk should be an unsigned integer"),
            )
            .expect("fixture vk should fit u32");
            let expected = case["evdev"]
                .as_u64()
                .map(|value| u16::try_from(value).expect("fixture evdev should fit u16"));
            assert_eq!(
                vk_to_evdev(KeyCode::new(vk)).map(|key| key.0),
                expected,
                "VK mapping mismatch for 0x{vk:X}"
            );
        }

        let evdev_to_win =
            fixture["evdev_to_win"].as_array().expect("evdev_to_win should be an array");
        for case in evdev_to_win {
            let evdev = u16::try_from(
                case["evdev"].as_u64().expect("fixture evdev should be an unsigned integer"),
            )
            .expect("fixture evdev should fit u16");
            let expected = u32::try_from(
                case["vk"].as_u64().expect("fixture vk should be an unsigned integer"),
            )
            .expect("fixture vk should fit u32");
            assert_eq!(evdev_to_vk(evdev).raw(), expected, "evdev mapping mismatch for {evdev}");
        }
    }

    #[test]
    fn x11_character_map_resolves_plain_and_shifted_ascii() {
        let symbols = [u32::from(b'1'), u32::from(b'!'), 0, 0];
        let mut modifiers = [0_u8; 8];
        modifiers[0] = 50;
        assert_eq!(
            resolve_character_from_x11_maps(b'1', 10, 10, 4, &symbols, &modifiers),
            Some(X11CharacterKey { evdev_key: 2, modifier_evdev_keys: vec![] })
        );
        assert_eq!(
            resolve_character_from_x11_maps(b'!', 10, 10, 4, &symbols, &modifiers),
            Some(X11CharacterKey { evdev_key: 2, modifier_evdev_keys: vec![42] })
        );
    }

    #[test]
    fn x11_character_map_uses_mod5_iso_level3_shift() {
        let minimum = 8_u8;
        let maximum = 92_u8;
        let width = 4_usize;
        let mut symbols = vec![0_u32; usize::from(maximum - minimum + 1) * width];
        let q = usize::from(24 - minimum) * width;
        symbols[q..q + width].copy_from_slice(&[
            u32::from(b'q'),
            u32::from(b'Q'),
            u32::from(b'@'),
            u32::from(b'#'),
        ]);
        let level3 = usize::from(92 - minimum) * width;
        symbols[level3] = XK_ISO_LEVEL3_SHIFT;
        let mut modifiers = [0_u8; 8];
        modifiers[0] = 50;
        modifiers[7] = 92;

        assert_eq!(
            resolve_character_from_x11_maps(b'@', minimum, maximum, 4, &symbols, &modifiers,),
            Some(X11CharacterKey { evdev_key: 16, modifier_evdev_keys: vec![84] })
        );
        assert_eq!(
            resolve_character_from_x11_maps(b'#', minimum, maximum, 4, &symbols, &modifiers,),
            Some(X11CharacterKey { evdev_key: 16, modifier_evdev_keys: vec![42, 84] })
        );
    }

    #[test]
    fn key_state_trait_forwards_pressed_and_transient_binding_inputs() {
        let device = EvdevUinputInputDevice::new();
        set_key_state_from_evdev(&device.shared, EvKeyCode::KEY_A.0, true);
        device.shared.pending_binding_input.store(KeyCode::MOUSE_WHEEL_UP.raw(), Ordering::Release);
        let state: &dyn KeyState = &device;
        assert_eq!(state.current_pressed_key(), Some(KeyCode::A));
        assert_eq!(state.consume_next_transient_input(), Some(KeyCode::MOUSE_WHEEL_UP));
        assert_eq!(state.consume_next_transient_input(), None);
    }

    #[test]
    fn representative_vk_to_evdev_mapping_matches_cpp() {
        assert_eq!(vk_to_evdev(KeyCode::SPACE), Some(EvKeyCode::KEY_SPACE));
        assert_eq!(vk_to_evdev(KeyCode::MBUTTON), Some(EvKeyCode::BTN_MIDDLE));
        assert_eq!(vk_to_evdev(KeyCode::new(0x58)), Some(EvKeyCode::KEY_X));
        assert_eq!(vk_to_evdev(KeyCode::new(0x74)), Some(EvKeyCode::KEY_F5));
        assert_eq!(vk_to_evdev(KeyCode::LSHIFT), Some(EvKeyCode::KEY_LEFTSHIFT));
        assert_eq!(vk_to_evdev(KeyCode::RCONTROL), Some(EvKeyCode::KEY_RIGHTCTRL));
        assert_eq!(vk_to_evdev(KeyCode::MOUSE_WHEEL_UP), None);
        assert_eq!(vk_to_evdev(KeyCode::new(0x0F)), None);
    }

    #[test]
    fn evdev_modifier_sides_collapse_to_cpp_generic_state_keys() {
        assert_eq!(evdev_to_vk(EvKeyCode::KEY_LEFTSHIFT.0), KeyCode::SHIFT);
        assert_eq!(evdev_to_vk(EvKeyCode::KEY_RIGHTSHIFT.0), KeyCode::SHIFT);
        assert_eq!(evdev_to_vk(EvKeyCode::KEY_LEFTCTRL.0), KeyCode::CONTROL);
        assert_eq!(evdev_to_vk(EvKeyCode::KEY_RIGHTALT.0), KeyCode::MENU);
    }

    #[test]
    fn mouse_button_mapping_matches_cpp() {
        assert_eq!(evdev_to_vk(EvKeyCode::BTN_LEFT.0), KeyCode::LBUTTON);
        assert_eq!(evdev_to_vk(EvKeyCode::BTN_RIGHT.0), KeyCode::RBUTTON);
        assert_eq!(evdev_to_vk(EvKeyCode::BTN_MIDDLE.0), KeyCode::MBUTTON);
        assert_eq!(evdev_to_vk(EvKeyCode::BTN_SIDE.0), KeyCode::XBUTTON1);
        assert_eq!(evdev_to_vk(EvKeyCode::BTN_EXTRA.0), KeyCode::XBUTTON2);
    }

    #[test]
    fn unknown_evdev_key_uses_cpp_unassigned_vk() {
        assert_eq!(evdev_to_vk(0x2fe), KeyCode::new(0x0F));
    }

    #[test]
    fn interactive_event_classification_matches_cpp() {
        assert_eq!(
            classify_interactive_event(&InputEvent::new(EventType::KEY.0, EvKeyCode::KEY_A.0, 1)),
            (true, false)
        );
        assert_eq!(
            classify_interactive_event(&InputEvent::new(
                EventType::RELATIVE.0,
                RelativeAxisCode::REL_WHEEL.0,
                -1,
            )),
            (false, true)
        );
        assert_eq!(
            classify_interactive_event(&InputEvent::new(
                EventType::KEY.0,
                EvKeyCode::BTN_LEFT.0,
                1,
            )),
            (false, true)
        );
        assert_eq!(
            classify_interactive_event(&InputEvent::new(EventType::KEY.0, EvKeyCode::KEY_A.0, 0)),
            (false, false)
        );
    }

    #[test]
    fn physical_key_state_and_wheel_transient_use_separate_paths() {
        let shared = SharedInputState::default();
        set_key_state_from_evdev(&shared, EvKeyCode::KEY_A.0, true);
        assert!(
            shared.key_states[usize::try_from(KeyCode::A.raw()).unwrap_or_default()]
                .load(Ordering::Acquire)
        );
        assert_eq!(shared.pending_binding_input.load(Ordering::Acquire), KeyCode::NONE.raw());
    }
}
