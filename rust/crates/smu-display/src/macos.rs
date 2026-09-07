//! Native macOS display geometry, permission, cursor, and pixel-capture backend.

use crate::{
    CursorPosition, DisplayCapture, DisplayError, PixelColor, ScreenBounds,
    macos_display_index_for_point, macos_global_screen_bounds, macos_screen_bounds_from_edges,
};
use objc2_application_services::{
    AXIsProcessTrusted, AXIsProcessTrustedWithOptions, kAXTrustedCheckOptionPrompt,
};
use objc2_core_foundation::{
    CFBoolean, CFDictionary, CFRetained, CFString, CGPoint, CGRect, CGSize,
};
#[allow(deprecated)]
use objc2_core_graphics::CGDisplayCreateImage;
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGDirectDisplayID, CGDisplayBounds,
    CGDisplayCopyDisplayMode, CGDisplayMode, CGError, CGEvent, CGGetActiveDisplayList, CGImage,
    CGImageAlphaInfo, CGImageByteOrderInfo, CGPreflightScreenCaptureAccess,
    CGRequestScreenCaptureAccess,
};
use std::ffi::c_void;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

const SCREEN_CAPTURE_CACHE_DURATION: Duration = Duration::from_millis(16);
const SCREEN_RECORDING_PERMISSION_REASON: &str =
    "macOS Screen Recording permission is required before SMU can read screen pixels.";
const ACCESSIBILITY_PERMISSION_REASON: &str =
    "macOS Accessibility permission is required to move the global cursor.";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn active_displays() -> Vec<CGDirectDisplayID> {
    let mut count = 0_u32;
    // SAFETY: a zero-sized first query permits a null output buffer and writes only display_count.
    if unsafe { CGGetActiveDisplayList(0, std::ptr::null_mut(), &mut count) } != CGError::Success
        || count == 0
    {
        return Vec::new();
    }

    let Ok(capacity) = usize::try_from(count) else {
        return Vec::new();
    };
    let mut displays = vec![0_u32; capacity];
    // SAFETY: displays has room for count CGDirectDisplayID values and count is writable.
    if unsafe { CGGetActiveDisplayList(count, displays.as_mut_ptr(), &mut count) }
        != CGError::Success
    {
        return Vec::new();
    }
    let Ok(actual) = usize::try_from(count) else {
        return Vec::new();
    };
    displays.truncate(actual.min(displays.len()));
    displays
}

fn rect_edges(rect: CGRect) -> (f64, f64, f64, f64) {
    let min = rect.min();
    let max = rect.max();
    (min.x, min.y, max.x, max.y)
}

fn current_cursor_point() -> Option<CGPoint> {
    let event = CGEvent::new(None)?;
    Some(CGEvent::location(Some(&event)))
}

fn display_for_point(point: CGPoint) -> Option<(CGDirectDisplayID, CGRect)> {
    let displays = active_displays();
    if displays.is_empty() {
        return None;
    }
    let rects =
        displays.iter().map(|display| rect_edges(CGDisplayBounds(*display))).collect::<Vec<_>>();
    let index = macos_display_index_for_point(point.x, point.y, &rects)?;
    let display = *displays.get(index)?;
    Some((display, CGDisplayBounds(display)))
}

fn bounds_from_rect(rect: CGRect) -> Option<ScreenBounds> {
    let (min_x, min_y, max_x, max_y) = rect_edges(rect);
    macos_screen_bounds_from_edges(min_x, min_y, max_x, max_y)
}

fn active_monitor() -> Option<(CGDirectDisplayID, CGRect)> {
    display_for_point(current_cursor_point()?)
}

fn active_monitor_refresh_rate() -> Option<i32> {
    let (display, _) = active_monitor()?;
    let mode = CGDisplayCopyDisplayMode(display)?;
    let refresh = CGDisplayMode::refresh_rate(Some(&mode));
    if !refresh.is_finite() || refresh <= 0.0 || refresh > f64::from(i32::MAX) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some(refresh.round() as i32)
}

fn accessibility_trusted() -> bool {
    // SAFETY: AXIsProcessTrusted takes no arguments and only queries current-process trust state.
    unsafe { AXIsProcessTrusted() }
}

fn prompt_for_accessibility() {
    // SAFETY: kAXTrustedCheckOptionPrompt is a process-lifetime immutable CoreFoundation string.
    let prompt_key = unsafe { kAXTrustedCheckOptionPrompt };
    let options =
        CFDictionary::<CFString, CFBoolean>::from_slices(&[prompt_key], &[CFBoolean::new(true)]);
    // SAFETY: the dictionary contains the documented CFString -> CFBoolean accessibility option.
    let _ = unsafe { AXIsProcessTrustedWithOptions(Some(options.as_opaque())) };
}

#[derive(Default)]
struct ScreenPixelSampler {
    image: Option<CFRetained<CGImage>>,
    cached_display: Option<CGDirectDisplayID>,
    cached_bounds: Option<CGRect>,
    capture_time: Option<Instant>,
}

impl ScreenPixelSampler {
    fn reset(&mut self) {
        self.image = None;
        self.cached_display = None;
        self.cached_bounds = None;
        self.capture_time = None;
    }

    #[allow(deprecated)]
    fn capture_display_if_needed(
        &mut self,
        display: CGDirectDisplayID,
        bounds: CGRect,
    ) -> Result<(), DisplayError> {
        if self.image.is_some()
            && self.cached_display == Some(display)
            && self.cached_bounds == Some(bounds)
            && self
                .capture_time
                .is_some_and(|captured| captured.elapsed() < SCREEN_CAPTURE_CACHE_DURATION)
        {
            return Ok(());
        }

        self.reset();
        // Keep the synchronous CoreGraphics path for the same macOS 10.15 compatibility as C++.
        let image = CGDisplayCreateImage(display).ok_or_else(|| {
            DisplayError::new(
                "macOS could not capture the requested display. Check Screen Recording permission.",
            )
        })?;
        self.image = Some(image);
        self.cached_display = Some(display);
        self.cached_bounds = Some(bounds);
        self.capture_time = Some(Instant::now());
        Ok(())
    }

    fn sample(&mut self, x: i32, y: i32) -> Result<PixelColor, DisplayError> {
        if !CGPreflightScreenCaptureAccess() {
            return Err(DisplayError::new(SCREEN_RECORDING_PERMISSION_REASON));
        }

        let point = CGPoint::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
        let (display, display_bounds) = display_for_point(point).ok_or_else(|| {
            DisplayError::new("macOS could not resolve the display containing the requested pixel.")
        })?;
        let (min_x, min_y, max_x, max_y) = rect_edges(display_bounds);
        if point.x < min_x || point.x >= max_x || point.y < min_y || point.y >= max_y {
            return Err(DisplayError::new(
                "requested pixel is outside the active macOS display bounds.",
            ));
        }
        self.capture_display_if_needed(display, display_bounds)?;

        let image = self
            .image
            .as_deref()
            .ok_or_else(|| DisplayError::new("macOS screen pixel cache is not ready."))?;
        let image_width = CGImage::width(Some(image));
        let image_height = CGImage::height(Some(image));
        if image_width == 0
            || image_height == 0
            || display_bounds.size.width <= 0.0
            || display_bounds.size.height <= 0.0
        {
            return Err(DisplayError::new("macOS captured display dimensions were invalid."));
        }
        let image_width_f64 = f64::from(
            u32::try_from(image_width)
                .map_err(|_| DisplayError::new("macOS captured display width was too large."))?,
        );
        let image_height_f64 = f64::from(
            u32::try_from(image_height)
                .map_err(|_| DisplayError::new("macOS captured display height was too large."))?,
        );
        let local_x = point.x - min_x;
        let local_y = point.y - min_y;
        let scaled_x = (local_x * image_width_f64 / display_bounds.size.width).floor();
        let scaled_y = (local_y * image_height_f64 / display_bounds.size.height).floor();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pixel_x = (scaled_x as usize).min(image_width - 1);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pixel_y = (scaled_y as usize).min(image_height - 1);
        Self::sample_image_pixel(image, pixel_x, pixel_y)
    }

    fn sample_image_pixel(
        image: &CGImage,
        pixel_x: usize,
        pixel_y: usize,
    ) -> Result<PixelColor, DisplayError> {
        let pixel_x = f64::from(
            u32::try_from(pixel_x)
                .map_err(|_| DisplayError::new("macOS pixel x-coordinate was too large."))?,
        );
        let pixel_y = f64::from(
            u32::try_from(pixel_y)
                .map_err(|_| DisplayError::new("macOS pixel y-coordinate was too large."))?,
        );
        let pixel_rect = CGRect::new(CGPoint::new(pixel_x, pixel_y), CGSize::new(1.0, 1.0));
        let pixel_image =
            CGImage::with_image_in_rect(Some(image), pixel_rect).ok_or_else(|| {
                DisplayError::new("macOS could not isolate the requested display pixel.")
            })?;
        let color_space = CGColorSpace::new_device_rgb().ok_or_else(|| {
            DisplayError::new("macOS could not create a pixel sampling bitmap context.")
        })?;
        let mut rgba = [0_u8; 4];
        let bitmap_info =
            CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
        // SAFETY: rgba is writable for four bytes and remains alive until the context is dropped below.
        let context = unsafe {
            CGBitmapContextCreate(
                rgba.as_mut_ptr().cast::<c_void>(),
                1,
                1,
                8,
                rgba.len(),
                Some(&color_space),
                bitmap_info,
            )
        }
        .ok_or_else(|| {
            DisplayError::new("macOS could not create a pixel sampling bitmap context.")
        })?;
        CGContext::draw_image(
            Some(&context),
            CGRect::new(CGPoint::ZERO, CGSize::new(1.0, 1.0)),
            Some(&pixel_image),
        );
        drop(context);
        Ok(PixelColor { r: rgba[0], g: rgba[1], b: rgba[2] })
    }
}

#[derive(Default)]
pub struct MacosDisplayCapture {
    sampler: Mutex<ScreenPixelSampler>,
}

impl MacosDisplayCapture {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn global_screen_bounds(&self) -> Option<ScreenBounds> {
        let rects =
            active_displays().into_iter().map(CGDisplayBounds).map(rect_edges).collect::<Vec<_>>();
        macos_global_screen_bounds(&rects)
    }
}

impl DisplayCapture for MacosDisplayCapture {
    fn active_monitor_bounds(&self) -> Option<ScreenBounds> {
        let (_, bounds) = active_monitor()?;
        bounds_from_rect(bounds)
    }

    fn active_monitor_refresh_rate_hz(&self) -> Option<i32> {
        active_monitor_refresh_rate()
    }

    fn cursor_position(&self) -> Option<CursorPosition> {
        let point = current_cursor_point()?;
        if !point.x.is_finite()
            || !point.y.is_finite()
            || point.x < f64::from(i32::MIN)
            || point.x > f64::from(i32::MAX)
            || point.y < f64::from(i32::MIN)
            || point.y > f64::from(i32::MAX)
        {
            return None;
        }
        #[allow(clippy::cast_possible_truncation)]
        Some(CursorPosition { x: point.x.round() as i32, y: point.y.round() as i32 })
    }

    fn pointer_unavailable_reason(&self) -> String {
        ACCESSIBILITY_PERMISSION_REASON.to_owned()
    }

    fn request_pixel_access_for_script(
        &self,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<(), DisplayError> {
        if CGPreflightScreenCaptureAccess() {
            return Ok(());
        }
        if cancelled() {
            return Err(DisplayError::new(
                "macOS Screen Recording permission request was cancelled.",
            ));
        }
        if CGRequestScreenCaptureAccess() && CGPreflightScreenCaptureAccess() {
            Ok(())
        } else {
            Err(DisplayError::new(SCREEN_RECORDING_PERMISSION_REASON))
        }
    }

    fn request_absolute_pointer_access_for_script(
        &self,
        cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<(), DisplayError> {
        if accessibility_trusted() {
            return Ok(());
        }
        if cancelled() {
            return Err(DisplayError::new("macOS Accessibility permission request was cancelled."));
        }
        prompt_for_accessibility();
        if accessibility_trusted() {
            Ok(())
        } else {
            Err(DisplayError::new(ACCESSIBILITY_PERMISSION_REASON))
        }
    }

    fn sample_pixel(&self, x: i32, y: i32) -> Result<PixelColor, DisplayError> {
        lock(&self.sampler).sample(x, y)
    }

    fn unavailable_reason(&self) -> String {
        SCREEN_RECORDING_PERMISSION_REASON.to_owned()
    }
}
