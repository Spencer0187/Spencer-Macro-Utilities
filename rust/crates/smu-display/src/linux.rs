//! Linux display/capture backend.
//!
//! X11/XWayland geometry and pixel reads are native here. Native Wayland screen capture remains a
//! separate portal/PipeWire capability; until that service is composed, the backend preserves the
//! legacy unavailable paths instead of pretending global compositor access exists.

use crate::{CursorPosition, DisplayCapture, DisplayError, PixelColor, ScreenBounds};
use std::env;
use x11rb::connection::Connection;
use x11rb::protocol::xinerama::ConnectionExt as XineramaConnectionExt;
use x11rb::protocol::xproto::{
    ConnectionExt as XprotoConnectionExt, ImageFormat, ImageOrder, Screen,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LinuxDisplayServer {
    X11,
    Wayland,
    Unknown,
}

#[must_use]
fn detect_display_server_values(
    session_type: Option<&str>,
    wayland_display: bool,
    x11_display: bool,
) -> LinuxDisplayServer {
    let session_type = session_type.unwrap_or_default().to_ascii_lowercase();
    if session_type == "wayland" || wayland_display {
        LinuxDisplayServer::Wayland
    } else if session_type == "x11" || x11_display {
        LinuxDisplayServer::X11
    } else {
        LinuxDisplayServer::Unknown
    }
}

fn detect_display_server() -> LinuxDisplayServer {
    detect_display_server_values(
        env::var("XDG_SESSION_TYPE").ok().as_deref(),
        env::var_os("WAYLAND_DISPLAY").is_some(),
        env::var_os("DISPLAY").is_some(),
    )
}

fn appears_wayland() -> bool {
    env::var("XDG_SESSION_TYPE").is_ok_and(|value| value == "wayland")
}

fn x11_cursor_and_screen() -> Option<(CursorPosition, ScreenBounds)> {
    let (connection, screen_index) = x11rb::connect(None).ok()?;
    let screen = connection.setup().roots.get(screen_index)?;
    let pointer = connection.query_pointer(screen.root).ok()?.reply().ok()?;
    if !pointer.same_screen {
        return None;
    }
    let width = i32::from(screen.width_in_pixels);
    let height = i32::from(screen.height_in_pixels);
    if width <= 0 || height <= 0 {
        return None;
    }
    Some((
        CursorPosition { x: i32::from(pointer.root_x), y: i32::from(pointer.root_y) },
        ScreenBounds { x: 0, y: 0, width, height },
    ))
}

fn x11_cursor_position() -> Option<CursorPosition> {
    x11_cursor_and_screen().map(|(cursor, _)| cursor)
}

fn nearest_monitor(cursor: CursorPosition, monitors: &[ScreenBounds]) -> Option<ScreenBounds> {
    let mut fallback = None;
    let mut best_distance = i64::MAX;
    for monitor in monitors.iter().copied().filter(|monitor| monitor.is_valid()) {
        let right = monitor.x.saturating_add(monitor.width);
        let bottom = monitor.y.saturating_add(monitor.height);
        if cursor.x >= monitor.x && cursor.x < right && cursor.y >= monitor.y && cursor.y < bottom {
            return Some(monitor);
        }

        let center_x = i64::from(monitor.x) + i64::from(monitor.width / 2);
        let center_y = i64::from(monitor.y) + i64::from(monitor.height / 2);
        let dx = i64::from(cursor.x) - center_x;
        let dy = i64::from(cursor.y) - center_y;
        let distance = dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy));
        if fallback.is_none() || distance < best_distance {
            fallback = Some(monitor);
            best_distance = distance;
        }
    }
    fallback
}

fn x11_active_monitor_bounds() -> Option<ScreenBounds> {
    let (connection, screen_index) = x11rb::connect(None).ok()?;
    let screen = connection.setup().roots.get(screen_index)?;
    let pointer = connection.query_pointer(screen.root).ok()?.reply().ok()?;
    if !pointer.same_screen {
        return None;
    }
    let cursor = CursorPosition { x: i32::from(pointer.root_x), y: i32::from(pointer.root_y) };

    if connection
        .xinerama_is_active()
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .is_some_and(|reply| reply.state != 0)
        && let Some(reply) =
            connection.xinerama_query_screens().ok().and_then(|cookie| cookie.reply().ok())
    {
        let monitors = reply
            .screen_info
            .into_iter()
            .map(|monitor| ScreenBounds {
                x: i32::from(monitor.x_org),
                y: i32::from(monitor.y_org),
                width: i32::from(monitor.width),
                height: i32::from(monitor.height),
            })
            .collect::<Vec<_>>();
        if let Some(bounds) = nearest_monitor(cursor, &monitors) {
            return Some(bounds);
        }
    }

    let width = i32::from(screen.width_in_pixels);
    let height = i32::from(screen.height_in_pixels);
    ScreenBounds { x: 0, y: 0, width, height }.is_valid().then_some(ScreenBounds {
        x: 0,
        y: 0,
        width,
        height,
    })
}

fn component_from_mask(pixel: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }
    let shift = mask.trailing_zeros();
    let max_value = mask >> shift;
    let value = (pixel & mask) >> shift;
    if max_value == 0 {
        return 0;
    }
    let scaled = (u64::from(value) * 255 + u64::from(max_value) / 2) / u64::from(max_value);
    u8::try_from(scaled).unwrap_or(u8::MAX)
}

fn decode_pixel(bytes: &[u8], bits_per_pixel: u8, order: ImageOrder) -> Option<u32> {
    let byte_count = usize::from(bits_per_pixel).div_ceil(8);
    if byte_count == 0 || byte_count > 4 || bytes.len() < byte_count {
        return None;
    }
    let bytes = &bytes[..byte_count];
    let pixel = if order == ImageOrder::LSB_FIRST {
        bytes
            .iter()
            .enumerate()
            .fold(0_u32, |value, (index, byte)| value | (u32::from(*byte) << (index * 8)))
    } else {
        bytes.iter().fold(0_u32, |value, byte| (value << 8) | u32::from(*byte))
    };
    Some(pixel)
}

fn visual_masks(screen: &Screen, visual_id: u32) -> Option<(u32, u32, u32)> {
    screen.allowed_depths.iter().find_map(|depth| {
        depth.visuals.iter().find_map(|visual| {
            (visual.visual_id == visual_id).then_some((
                visual.red_mask,
                visual.green_mask,
                visual.blue_mask,
            ))
        })
    })
}

fn x11_sample_pixel(x: i32, y: i32) -> Result<PixelColor, DisplayError> {
    let x = i16::try_from(x)
        .map_err(|_| DisplayError::new("XGetImage failed while sampling the screen pixel"))?;
    let y = i16::try_from(y)
        .map_err(|_| DisplayError::new("XGetImage failed while sampling the screen pixel"))?;
    let (connection, screen_index) = x11rb::connect(None)
        .map_err(|_| DisplayError::new(linux_screen_read_unavailable_reason()))?;
    let screen = connection
        .setup()
        .roots
        .get(screen_index)
        .ok_or_else(|| DisplayError::new(linux_screen_read_unavailable_reason()))?;
    let cookie = connection
        .get_image(ImageFormat::Z_PIXMAP, screen.root, x, y, 1, 1, u32::MAX)
        .map_err(|_| DisplayError::new("XGetImage failed while sampling the screen pixel"))?;
    let reply = cookie
        .reply()
        .map_err(|_| DisplayError::new("XGetImage failed while sampling the screen pixel"))?;
    let format = connection
        .setup()
        .pixmap_formats
        .iter()
        .find(|format| format.depth == reply.depth)
        .ok_or_else(|| DisplayError::new("XGetImage failed while sampling the screen pixel"))?;
    let pixel =
        decode_pixel(&reply.data, format.bits_per_pixel, connection.setup().image_byte_order)
            .ok_or_else(|| DisplayError::new("XGetImage failed while sampling the screen pixel"))?;
    let visual_id = if reply.visual == x11rb::NONE { screen.root_visual } else { reply.visual };
    let (red_mask, green_mask, blue_mask) = visual_masks(screen, visual_id)
        .ok_or_else(|| DisplayError::new("XGetImage failed while sampling the screen pixel"))?;
    Ok(PixelColor {
        r: component_from_mask(pixel, red_mask),
        g: component_from_mask(pixel, green_mask),
        b: component_from_mask(pixel, blue_mask),
    })
}

fn linux_absolute_pointer_unavailable_reason() -> String {
    if detect_display_server() == LinuxDisplayServer::Wayland {
        return "absolute screen coordinates are unavailable on native Wayland, and this build does not include the desktop portal support required for screen reads or pointer control."
            .to_owned();
    }
    let display = env::var_os("DISPLAY").is_some_and(|value| !value.is_empty());
    if !display {
        if appears_wayland() {
            return "absolute screen coordinates are unavailable. moveMouseAbs and getPixelColor on Linux currently require X11/XWayland cursor-position access, but this appears to be a native Wayland session with no X11 DISPLAY. Native Wayland usually blocks global cursor position and arbitrary screen reads; use relative moveMouse() or run under X11/XWayland."
                .to_owned();
        }
        return "absolute screen coordinates are unavailable. moveMouseAbs and getPixelColor on Linux currently require an X11 DISPLAY so SMU can query the global cursor position and active monitor bounds."
            .to_owned();
    }
    if x11rb::connect(None).is_err() {
        if appears_wayland() {
            return "absolute screen coordinates are unavailable. moveMouseAbs and getPixelColor on Linux currently require X11/XWayland cursor-position access; this Wayland session exposes DISPLAY but XOpenDisplay failed. Native Wayland usually blocks global cursor position and arbitrary screen reads."
                .to_owned();
        }
        return "absolute screen coordinates are unavailable. moveMouseAbs and getPixelColor on Linux require X11 cursor-position access, but XOpenDisplay failed for the current DISPLAY."
            .to_owned();
    }
    if appears_wayland() {
        "absolute screen coordinates are unavailable. moveMouseAbs and getPixelColor on Linux currently rely on X11/XWayland cursor-position queries; this Wayland session did not provide usable global cursor/monitor data."
            .to_owned()
    } else {
        "absolute screen coordinates are unavailable. moveMouseAbs and getPixelColor on Linux require X11 cursor-position and active-monitor queries, but the current X11 query failed."
            .to_owned()
    }
}

fn linux_screen_read_unavailable_reason() -> String {
    if detect_display_server() == LinuxDisplayServer::Wayland {
        return "screen pixel color sampling is unavailable. This SMU build was compiled without PipeWire ScreenCast support."
            .to_owned();
    }
    let display = env::var_os("DISPLAY").is_some_and(|value| !value.is_empty());
    if !display {
        if appears_wayland() {
            return "screen pixel color sampling is unavailable. getPixelColor on Linux currently requires X11/XWayland screen-read access, but this appears to be a native Wayland session with no X11 DISPLAY. Native Wayland usually blocks arbitrary global screen reads."
                .to_owned();
        }
        return "screen pixel color sampling is unavailable. getPixelColor on Linux currently requires an X11 DISPLAY so SMU can read pixels from the root window."
            .to_owned();
    }
    if x11rb::connect(None).is_err() {
        if appears_wayland() {
            return "screen pixel color sampling is unavailable. getPixelColor on Linux currently requires X11/XWayland screen-read access; this Wayland session exposes DISPLAY but XOpenDisplay failed. Native Wayland usually blocks arbitrary global screen reads."
                .to_owned();
        }
        return "screen pixel color sampling is unavailable. getPixelColor on Linux requires X11 root-window access, but XOpenDisplay failed for the current DISPLAY."
            .to_owned();
    }
    "screen pixel color sampling failed while reading from the X11 root window.".to_owned()
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxDisplayCapture;

impl DisplayCapture for LinuxDisplayCapture {
    fn active_monitor_bounds(&self) -> Option<ScreenBounds> {
        if detect_display_server() == LinuxDisplayServer::Wayland {
            return None;
        }
        x11_active_monitor_bounds()
    }

    fn cursor_position(&self) -> Option<CursorPosition> {
        if detect_display_server() == LinuxDisplayServer::Wayland {
            return None;
        }
        x11_cursor_position()
    }

    fn pointer_unavailable_reason(&self) -> String {
        linux_absolute_pointer_unavailable_reason()
    }

    fn sample_pixel(&self, x: i32, y: i32) -> Result<PixelColor, DisplayError> {
        if detect_display_server() == LinuxDisplayServer::Wayland {
            return Err(DisplayError::new(linux_screen_read_unavailable_reason()));
        }
        x11_sample_pixel(x, y)
    }

    fn unavailable_reason(&self) -> String {
        linux_screen_read_unavailable_reason()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_server_detection_preserves_wayland_precedence() {
        assert_eq!(
            detect_display_server_values(Some("X11"), true, true),
            LinuxDisplayServer::Wayland
        );
        assert_eq!(
            detect_display_server_values(Some("x11"), false, false),
            LinuxDisplayServer::X11
        );
        assert_eq!(detect_display_server_values(None, false, true), LinuxDisplayServer::X11);
        assert_eq!(detect_display_server_values(None, false, false), LinuxDisplayServer::Unknown);
    }

    #[test]
    fn monitor_selection_prefers_containing_monitor() {
        let monitors = [
            ScreenBounds { x: -1920, y: 0, width: 1920, height: 1080 },
            ScreenBounds { x: 0, y: 0, width: 2560, height: 1440 },
        ];
        assert_eq!(
            nearest_monitor(CursorPosition { x: -100, y: 500 }, &monitors),
            Some(monitors[0])
        );
        assert_eq!(
            nearest_monitor(CursorPosition { x: 2000, y: 1000 }, &monitors),
            Some(monitors[1])
        );
    }

    #[test]
    fn monitor_selection_uses_nearest_center_fallback() {
        let monitors = [
            ScreenBounds { x: 0, y: 0, width: 100, height: 100 },
            ScreenBounds { x: 500, y: 0, width: 100, height: 100 },
        ];
        assert_eq!(
            nearest_monitor(CursorPosition { x: 400, y: 500 }, &monitors),
            Some(monitors[1])
        );
    }

    #[test]
    fn invalid_monitors_are_ignored() {
        let monitors = [
            ScreenBounds { x: 0, y: 0, width: 0, height: 100 },
            ScreenBounds { x: 100, y: 100, width: 50, height: 50 },
        ];
        assert_eq!(
            nearest_monitor(CursorPosition { x: 110, y: 110 }, &monitors),
            Some(monitors[1])
        );
    }

    #[test]
    fn component_mask_scaling_matches_cpp_formula() {
        assert_eq!(component_from_mask(0x00AB_CDEF, 0x00FF_0000), 0xAB);
        assert_eq!(component_from_mask(0x00AB_CDEF, 0x0000_FF00), 0xCD);
        assert_eq!(component_from_mask(0x00AB_CDEF, 0x0000_00FF), 0xEF);
        assert_eq!(component_from_mask(0b1_1111, 0b1_1111), 255);
        assert_eq!(component_from_mask(0, 0), 0);
    }

    #[test]
    fn pixel_decode_respects_x11_image_byte_order() {
        assert_eq!(
            decode_pixel(&[0xEF, 0xCD, 0xAB, 0x00], 32, ImageOrder::LSB_FIRST),
            Some(0x00AB_CDEF)
        );
        assert_eq!(
            decode_pixel(&[0x00, 0xAB, 0xCD, 0xEF], 32, ImageOrder::MSB_FIRST),
            Some(0x00AB_CDEF)
        );
        assert_eq!(decode_pixel(&[0xEF, 0xCD, 0xAB], 24, ImageOrder::LSB_FIRST), Some(0x00AB_CDEF));
        assert_eq!(decode_pixel(&[0xFF], 32, ImageOrder::LSB_FIRST), None);
    }
}
