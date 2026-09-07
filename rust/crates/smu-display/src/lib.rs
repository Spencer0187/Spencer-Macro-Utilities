//! Display geometry and screen-read services for SMU.
//!
//! The legacy C++ `InputBackend` mixed these APIs with keyboard/mouse injection. Rust keeps them
//! separate so capture/portal lifetimes and input-device lifetimes can evolve independently.

use std::fmt;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScreenBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl ScreenBounds {
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.width > 0 && self.height > 0
    }
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn macos_screen_bounds_from_edges(
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
) -> Option<ScreenBounds> {
    if ![min_x, min_y, max_x, max_y].into_iter().all(f64::is_finite) {
        return None;
    }
    let left = min_x.floor();
    let top = min_y.floor();
    let right = max_x.ceil();
    let bottom = max_y.ceil();
    if left < f64::from(i32::MIN)
        || top < f64::from(i32::MIN)
        || right > f64::from(i32::MAX)
        || bottom > f64::from(i32::MAX)
    {
        return None;
    }
    let width = right - left;
    let height = bottom - top;
    if width <= 0.0 || height <= 0.0 || width > f64::from(i32::MAX) || height > f64::from(i32::MAX)
    {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some(ScreenBounds { x: left as i32, y: top as i32, width: width as i32, height: height as i32 })
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn macos_display_index_for_point(
    point_x: f64,
    point_y: f64,
    rects: &[(f64, f64, f64, f64)],
) -> Option<usize> {
    let mut closest = None;
    let mut closest_distance = f64::INFINITY;
    for (index, &(min_x, min_y, max_x, max_y)) in rects.iter().enumerate() {
        if ![min_x, min_y, max_x, max_y].into_iter().all(f64::is_finite)
            || max_x <= min_x
            || max_y <= min_y
        {
            continue;
        }
        if point_x >= min_x && point_x < max_x && point_y >= min_y && point_y < max_y {
            return Some(index);
        }
        let clamped_x = point_x.clamp(min_x, max_x);
        let clamped_y = point_y.clamp(min_y, max_y);
        let dx = point_x - clamped_x;
        let dy = point_y - clamped_y;
        #[allow(clippy::suboptimal_flops)]
        let distance = dx * dx + dy * dy;
        if closest.is_none() || distance < closest_distance {
            closest = Some(index);
            closest_distance = distance;
        }
    }
    closest
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn macos_global_screen_bounds(rects: &[(f64, f64, f64, f64)]) -> Option<ScreenBounds> {
    let mut union: Option<(f64, f64, f64, f64)> = None;
    for &(min_x, min_y, max_x, max_y) in rects {
        if macos_screen_bounds_from_edges(min_x, min_y, max_x, max_y).is_none() {
            continue;
        }
        union = Some(match union {
            Some((left, top, right, bottom)) => {
                (left.min(min_x), top.min(min_y), right.max(max_x), bottom.max(max_y))
            }
            None => (min_x, min_y, max_x, max_y),
        });
    }
    union.and_then(|(left, top, right, bottom)| {
        macos_screen_bounds_from_edges(left, top, right, bottom)
    })
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn windows_monitor_bounds(
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
) -> Option<ScreenBounds> {
    let width = i64::from(right) - i64::from(left);
    let height = i64::from(bottom) - i64::from(top);
    if width <= 0 || height <= 0 || width > i64::from(i32::MAX) || height > i64::from(i32::MAX) {
        return None;
    }
    Some(ScreenBounds {
        x: left,
        y: top,
        width: i32::try_from(width).ok()?,
        height: i32::try_from(height).ok()?,
    })
}

#[cfg(any(target_os = "windows", test))]
pub(crate) fn windows_refresh_interval(refresh_hz: Option<i32>) -> std::time::Duration {
    let hz = refresh_hz.filter(|hz| *hz > 1).unwrap_or(60);
    let micros = (1_000_000_i64 / i64::from(hz)).max(1);
    std::time::Duration::from_micros(u64::try_from(micros).unwrap_or(16_666))
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CursorPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PixelColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl PixelColor {
    /// Matches C++ `FormatColorHex`: uppercase `#RRGGBB`.
    #[must_use]
    pub fn legacy_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinateMode {
    Pixels,
    Percent,
    Scaled720p,
    Scaled1080p,
    Scaled1440p,
    Scaled2160p,
}

impl CoordinateMode {
    /// Parse exactly the mode strings accepted by the C++ script API.
    ///
    /// # Errors
    ///
    /// Returns the frozen compatibility message for an unsupported mode.
    pub fn parse(value: &str) -> Result<Self, DisplayError> {
        match value {
            "pixels" => Ok(Self::Pixels),
            "percent" => Ok(Self::Percent),
            "scaled720p" => Ok(Self::Scaled720p),
            "scaled1080p" => Ok(Self::Scaled1080p),
            "scaled1440p" => Ok(Self::Scaled1440p),
            "scaled2160p" => Ok(Self::Scaled2160p),
            _ => Err(DisplayError::new(
                "mode must be one of: pixels, percent, scaled720p, scaled1080p, scaled1440p, scaled2160p",
            )),
        }
    }

    const fn scaled_base(self) -> Option<(f64, f64)> {
        match self {
            Self::Scaled720p => Some((1280.0, 720.0)),
            Self::Scaled1080p => Some((1920.0, 1080.0)),
            Self::Scaled1440p => Some((2560.0, 1440.0)),
            Self::Scaled2160p => Some((3840.0, 2160.0)),
            Self::Pixels | Self::Percent => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DisplayError(String);

impl DisplayError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DisplayError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for DisplayError {}

fn round_to_i32(value: f64) -> Result<i32, DisplayError> {
    if !value.is_finite() {
        return Err(DisplayError::new("target coordinate is outside the supported integer range"));
    }
    let rounded = value.round();
    if rounded < f64::from(i32::MIN) || rounded > f64::from(i32::MAX) {
        return Err(DisplayError::new("target coordinate is outside the supported integer range"));
    }
    #[allow(clippy::cast_possible_truncation)]
    Ok(rounded as i32)
}

/// Resolve one script-visible coordinate into the selected monitor's global pixel space.
///
/// The endpoint behavior intentionally matches C++: percent uses `width - 1`/`height - 1`, scaled
/// modes use full width/height before final clamping, and all modes round before clamping.
///
/// # Errors
///
/// Returns the same validation text as the C++ coordinate resolver.
#[allow(clippy::similar_names)]
pub fn resolve_absolute_point(
    bounds: ScreenBounds,
    x: f64,
    y: f64,
    mode: CoordinateMode,
) -> Result<(i32, i32), DisplayError> {
    if !bounds.is_valid() {
        return Err(DisplayError::new("active monitor bounds are invalid"));
    }

    let (target_x_value, target_y_value) = match mode {
        CoordinateMode::Pixels => (f64::from(bounds.x) + x, f64::from(bounds.y) + y),
        CoordinateMode::Percent => {
            if !(0.0..=100.0).contains(&x) || !(0.0..=100.0).contains(&y) {
                return Err(DisplayError::new(
                    "percent mode requires x and y to be in the range 0 through 100",
                ));
            }
            // Preserve the C++ multiply/add evaluation order instead of fusing with `mul_add`.
            #[allow(clippy::suboptimal_flops)]
            let target_x = f64::from(bounds.x) + (x / 100.0) * f64::from(bounds.width - 1);
            #[allow(clippy::suboptimal_flops)]
            let target_y = f64::from(bounds.y) + (y / 100.0) * f64::from(bounds.height - 1);
            (target_x, target_y)
        }
        scaled_mode => {
            let Some((base_width, base_height)) = scaled_mode.scaled_base() else {
                return Err(DisplayError::new("unsupported coordinate mode"));
            };
            (
                f64::from(bounds.x) + x * f64::from(bounds.width) / base_width,
                f64::from(bounds.y) + y * f64::from(bounds.height) / base_height,
            )
        }
    };

    let target_x = round_to_i32(target_x_value)?;
    let target_y = round_to_i32(target_y_value)?;
    let min_x = bounds.x;
    let min_y = bounds.y;
    let max_x = bounds.x.saturating_add(bounds.width).saturating_sub(1);
    let max_y = bounds.y.saturating_add(bounds.height).saturating_sub(1);
    Ok((target_x.clamp(min_x, max_x), target_y.clamp(min_y, max_y)))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PixelRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl PixelRect {
    #[must_use]
    pub fn width(self) -> usize {
        usize::try_from(i64::from(self.right) - i64::from(self.left) + 1).unwrap_or(usize::MAX)
    }

    #[must_use]
    pub fn height(self) -> usize {
        usize::try_from(i64::from(self.bottom) - i64::from(self.top) + 1).unwrap_or(usize::MAX)
    }
}

/// Resolve and normalize two script coordinates exactly as C++ `GetPixelRect` does.
///
/// # Errors
///
/// Propagates coordinate validation failures.
pub fn resolve_pixel_rect(
    bounds: ScreenBounds,
    first: (f64, f64),
    second: (f64, f64),
    mode: CoordinateMode,
) -> Result<PixelRect, DisplayError> {
    let (mut left, mut top) = resolve_absolute_point(bounds, first.0, first.1, mode)?;
    let (mut right, mut bottom) = resolve_absolute_point(bounds, second.0, second.1, mode)?;
    if left > right {
        std::mem::swap(&mut left, &mut right);
    }
    if top > bottom {
        std::mem::swap(&mut top, &mut bottom);
    }
    Ok(PixelRect { left, top, right, bottom })
}

/// Screen capture/read capability independent of keyboard/mouse injection.
pub trait DisplayCapture: Send + Sync {
    fn active_monitor_bounds(&self) -> Option<ScreenBounds>;

    fn active_monitor_refresh_rate_hz(&self) -> Option<i32> {
        None
    }

    fn cursor_position(&self) -> Option<CursorPosition> {
        None
    }

    fn pointer_unavailable_reason(&self) -> String {
        "cursor/screen position is not available on this platform/session".to_owned()
    }

    /// Allow an interactive platform service to obtain pixel-capture permission before a script
    /// begins a screen read. Non-interactive backends are already ready and return `Ok(())`.
    ///
    /// # Errors
    ///
    /// Returns a user-decline, cancellation, or platform activation failure.
    fn request_pixel_access_for_script(
        &self,
        _cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<(), DisplayError> {
        Ok(())
    }

    /// Allow an interactive platform service to obtain absolute-pointer permission before a script
    /// begins direct pointer positioning. Non-interactive backends return `Ok(())`.
    ///
    /// # Errors
    ///
    /// Returns a user-decline, cancellation, or platform activation failure.
    fn request_absolute_pointer_access_for_script(
        &self,
        _cancelled: &(dyn Fn() -> bool + Send + Sync),
    ) -> Result<(), DisplayError> {
        Ok(())
    }

    /// Sample one global screen coordinate.
    ///
    /// # Errors
    ///
    /// Returns the platform/session capture failure for an unavailable or unreadable pixel.
    fn sample_pixel(&self, x: i32, y: i32) -> Result<PixelColor, DisplayError>;

    fn unavailable_reason(&self) -> String {
        "screen pixel color sampling is not available on this platform/session".to_owned()
    }
}

/// Read a normalized rectangle row-major using the historical fail-fast behavior.
///
/// # Errors
///
/// Stops on the first pixel sample error, like C++ `GetPixelRect`.
pub fn sample_pixel_rect(
    capture: &dyn DisplayCapture,
    rect: PixelRect,
) -> Result<Vec<Vec<PixelColor>>, DisplayError> {
    let width = rect.width();
    let height = rect.height();
    let mut rows = Vec::with_capacity(height);
    for row in 0..height {
        let row_offset = i32::try_from(row)
            .map_err(|_| DisplayError::new("screen pixel rectangle is too large"))?;
        let mut values = Vec::with_capacity(width);
        for column in 0..width {
            let column_offset = i32::try_from(column)
                .map_err(|_| DisplayError::new("screen pixel rectangle is too large"))?;
            values.push(capture.sample_pixel(
                rect.left.saturating_add(column_offset),
                rect.top.saturating_add(row_offset),
            )?);
        }
        rows.push(values);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    const BOUNDS: ScreenBounds = ScreenBounds { x: -1920, y: 0, width: 1920, height: 1080 };

    #[test]
    fn screen_bounds_validity_matches_cpp_contract() {
        assert!(BOUNDS.is_valid());
        assert!(!ScreenBounds { width: 0, ..BOUNDS }.is_valid());
        assert!(!ScreenBounds { height: -1, ..BOUNDS }.is_valid());
    }

    #[test]
    fn macos_screen_bounds_floor_ceil_union_and_nearest_display_match_cpp_rules() {
        assert_eq!(
            macos_screen_bounds_from_edges(-1920.4, -0.2, -0.1, 1080.1),
            Some(ScreenBounds { x: -1921, y: -1, width: 1921, height: 1082 })
        );
        assert_eq!(macos_screen_bounds_from_edges(0.0, 0.0, 0.0, 1080.0), None);
        assert_eq!(macos_screen_bounds_from_edges(f64::NAN, 0.0, 10.0, 10.0), None);

        let rects = [(-1920.0, 0.0, 0.0, 1080.0), (0.0, 0.0, 2560.0, 1440.0)];
        assert_eq!(macos_display_index_for_point(-100.0, 500.0, &rects), Some(0));
        assert_eq!(macos_display_index_for_point(2000.0, 1000.0, &rects), Some(1));
        assert_eq!(macos_display_index_for_point(3000.0, 500.0, &rects), Some(1));
        assert_eq!(
            macos_global_screen_bounds(&rects),
            Some(ScreenBounds { x: -1920, y: 0, width: 4480, height: 1440 })
        );
    }

    #[test]
    fn windows_monitor_bounds_and_refresh_interval_match_cpp_rules() {
        assert_eq!(
            windows_monitor_bounds(-1920, 0, 0, 1080),
            Some(ScreenBounds { x: -1920, y: 0, width: 1920, height: 1080 })
        );
        assert_eq!(windows_monitor_bounds(0, 0, 0, 1080), None);
        assert_eq!(windows_refresh_interval(Some(120)), std::time::Duration::from_micros(8_333));
        assert_eq!(windows_refresh_interval(Some(1)), std::time::Duration::from_micros(16_666));
        assert_eq!(windows_refresh_interval(None), std::time::Duration::from_micros(16_666));
    }

    #[test]
    fn mode_parser_accepts_only_cpp_spellings() {
        assert_eq!(CoordinateMode::parse("pixels"), Ok(CoordinateMode::Pixels));
        assert_eq!(CoordinateMode::parse("scaled2160p"), Ok(CoordinateMode::Scaled2160p));
        assert_eq!(
            CoordinateMode::parse("PIXELS").map_err(|error| error.to_string()),
            Err("mode must be one of: pixels, percent, scaled720p, scaled1080p, scaled1440p, scaled2160p".to_owned())
        );
    }

    #[test]
    fn percent_mode_uses_inclusive_monitor_endpoints() {
        assert_eq!(
            resolve_absolute_point(BOUNDS, 0.0, 0.0, CoordinateMode::Percent),
            Ok((-1920, 0))
        );
        assert_eq!(
            resolve_absolute_point(BOUNDS, 100.0, 100.0, CoordinateMode::Percent),
            Ok((-1, 1079))
        );
        assert_eq!(
            resolve_absolute_point(BOUNDS, 50.0, 50.0, CoordinateMode::Percent),
            Ok((-961, 540))
        );
    }

    #[test]
    fn pixels_and_scaled_modes_round_then_clamp() {
        assert_eq!(
            resolve_absolute_point(BOUNDS, 20_000.0, -5.0, CoordinateMode::Pixels),
            Ok((-1, 0))
        );
        assert_eq!(
            resolve_absolute_point(BOUNDS, 1280.0, 720.0, CoordinateMode::Scaled720p),
            Ok((-1, 1079))
        );
        assert_eq!(
            resolve_absolute_point(
                ScreenBounds { x: 0, y: 0, width: 1280, height: 720 },
                0.5,
                0.5,
                CoordinateMode::Pixels
            ),
            Ok((1, 1))
        );
    }

    #[test]
    fn invalid_percent_and_nonfinite_values_match_cpp_errors() {
        assert_eq!(
            resolve_absolute_point(BOUNDS, -0.01, 50.0, CoordinateMode::Percent)
                .map_err(|error| error.to_string()),
            Err("percent mode requires x and y to be in the range 0 through 100".to_owned())
        );
        assert_eq!(
            resolve_absolute_point(BOUNDS, f64::NAN, 0.0, CoordinateMode::Pixels)
                .map_err(|error| error.to_string()),
            Err("target coordinate is outside the supported integer range".to_owned())
        );
    }

    #[test]
    fn rectangle_resolution_swaps_reversed_corners() {
        assert_eq!(
            resolve_pixel_rect(BOUNDS, (100.0, 80.0), (10.0, 20.0), CoordinateMode::Pixels),
            Ok(PixelRect { left: -1910, top: 20, right: -1820, bottom: 80 })
        );
    }

    #[test]
    fn color_hex_matches_cpp_uppercase_output() {
        assert_eq!(PixelColor { r: 0x0A, g: 0xB5, b: 0xFF }.legacy_hex(), "#0AB5FF");
    }

    struct TestCapture;

    impl DisplayCapture for TestCapture {
        fn active_monitor_bounds(&self) -> Option<ScreenBounds> {
            Some(ScreenBounds { x: 0, y: 0, width: 2, height: 2 })
        }

        fn sample_pixel(&self, x: i32, y: i32) -> Result<PixelColor, DisplayError> {
            Ok(PixelColor {
                r: u8::try_from(x).expect("test x coordinate should fit"),
                g: u8::try_from(y).expect("test y coordinate should fit"),
                b: 0,
            })
        }
    }

    #[test]
    fn rectangle_sampling_is_row_major() {
        let pixels =
            sample_pixel_rect(&TestCapture, PixelRect { left: 0, top: 0, right: 1, bottom: 1 })
                .expect("small test rectangle should sample");
        assert_eq!(
            pixels,
            [
                [PixelColor { r: 0, g: 0, b: 0 }, PixelColor { r: 1, g: 0, b: 0 }],
                [PixelColor { r: 0, g: 1, b: 0 }, PixelColor { r: 1, g: 1, b: 0 }],
            ]
        );
    }
}
