//! Native macOS CoreGraphics input injection.
//!
//! This checkpoint intentionally covers output only. Physical key/button state and active-layout text
//! translation still belong to the later event-tap/Text Input Services port.

use crate::{
    InputError, InputInjector, MACOS_MOUSE_LEFT, MACOS_MOUSE_MIDDLE, MACOS_MOUSE_RIGHT,
    MACOS_MOUSE_X1, MACOS_MOUSE_X2, macos_key_code,
};
use objc2_application_services::AXIsProcessTrusted;
use objc2_core_foundation::CGPoint;
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventTapLocation, CGEventType, CGMouseButton, CGScrollEventUnit,
};
use smu_core::KeyCode;
use std::sync::atomic::{AtomicBool, Ordering};

const INJECTED_EVENT_TAG: i64 = 0x534D_554D_4143_4F53;

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
}
