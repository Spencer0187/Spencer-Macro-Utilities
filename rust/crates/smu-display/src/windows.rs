use crate::{
    CursorPosition, DisplayCapture, DisplayError, PixelColor, ScreenBounds, windows_monitor_bounds,
    windows_refresh_interval,
};
use std::{
    ffi::c_void,
    mem::size_of,
    ptr,
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{GetLastError, POINT},
    Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC,
        CreateDIBSection, DEVMODEA, DIB_RGB_COLORS, DeleteDC, DeleteObject, ENUM_CURRENT_SETTINGS,
        EnumDisplaySettingsA, GetDC, GetDeviceCaps, GetMonitorInfoA, HDC, HGDIOBJ, HMONITOR,
        MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXA, MonitorFromPoint, ReleaseDC,
        SRCCOPY, SelectObject, VREFRESH,
    },
    UI::WindowsAndMessaging::GetCursorPos,
};

#[derive(Clone, Copy, Debug)]
struct MonitorCaptureInfo {
    monitor: usize,
    bounds: ScreenBounds,
    refresh_interval: Duration,
}

struct ScreenDc(HDC);

impl ScreenDc {
    fn acquire() -> Option<Self> {
        // SAFETY: GetDC with a null HWND acquires the desktop DC and retains no Rust pointers.
        let dc = unsafe { GetDC(ptr::null_mut()) };
        (!dc.is_null()).then_some(Self(dc))
    }
}

impl Drop for ScreenDc {
    fn drop(&mut self) {
        // SAFETY: this DC was acquired by GetDC(nullptr) and is released exactly once here.
        unsafe {
            let _ = ReleaseDC(ptr::null_mut(), self.0);
        }
    }
}

fn monitor_info(monitor: HMONITOR) -> Option<MONITORINFOEXA> {
    if monitor.is_null() {
        return None;
    }
    let mut info = MONITORINFOEXA::default();
    info.monitorInfo.cbSize = u32::try_from(size_of::<MONITORINFOEXA>()).ok()?;
    // SAFETY: MONITORINFOEXA starts with MONITORINFO and cbSize advertises the full structure size.
    let ok = unsafe {
        GetMonitorInfoA(monitor, (&mut info as *mut MONITORINFOEXA).cast::<MONITORINFO>())
    };
    (ok != 0).then_some(info)
}

fn monitor_refresh_rate(monitor: HMONITOR) -> Option<i32> {
    if let Some(info) = monitor_info(monitor) {
        let mut mode = DEVMODEA::default();
        mode.dmSize = u16::try_from(size_of::<DEVMODEA>()).ok()?;
        // SAFETY: szDevice is NUL-terminated Win32 device storage returned by GetMonitorInfoA and
        // mode points to writable DEVMODEA storage for this call.
        let ok = unsafe {
            EnumDisplaySettingsA(
                info.szDevice.as_ptr().cast::<u8>(),
                ENUM_CURRENT_SETTINGS,
                &mut mode,
            )
        };
        if ok != 0 && mode.dmDisplayFrequency > 1 {
            return i32::try_from(mode.dmDisplayFrequency).ok();
        }
    }

    let dc = ScreenDc::acquire()?;
    // SAFETY: GetDeviceCaps reads the desktop DC only.
    let refresh = unsafe { GetDeviceCaps(dc.0, VREFRESH as i32) };
    (refresh > 1).then_some(refresh)
}

fn resolve_monitor_capture_info(x: i32, y: i32) -> Result<MonitorCaptureInfo, DisplayError> {
    // SAFETY: MonitorFromPoint retains no pointers.
    let monitor = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    if monitor.is_null() {
        return Err(DisplayError::new("failed to resolve the monitor for the requested pixel"));
    }
    let info = monitor_info(monitor).ok_or_else(|| {
        DisplayError::new("GetMonitorInfo failed while preparing screen pixel sampling")
    })?;
    let bounds = windows_monitor_bounds(
        info.monitorInfo.rcMonitor.left,
        info.monitorInfo.rcMonitor.top,
        info.monitorInfo.rcMonitor.right,
        info.monitorInfo.rcMonitor.bottom,
    )
    .ok_or_else(|| {
        DisplayError::new("monitor bounds were invalid while preparing screen pixel sampling")
    })?;
    let refresh_rate_hz = monitor_refresh_rate(monitor);
    Ok(MonitorCaptureInfo {
        monitor: monitor as usize,
        bounds,
        refresh_interval: windows_refresh_interval(refresh_rate_hz),
    })
}

#[derive(Debug)]
struct ScreenPixelSampler {
    mem_dc: usize,
    bitmap: usize,
    old_bitmap: usize,
    pixel_bits: usize,
    bitmap_width: i32,
    bitmap_height: i32,
    cached_monitor: usize,
    cached_bounds: ScreenBounds,
    cache_capture_time: Option<Instant>,
    cache_refresh_interval: Duration,
    cache_valid: bool,
}

impl Default for ScreenPixelSampler {
    fn default() -> Self {
        Self {
            mem_dc: 0,
            bitmap: 0,
            old_bitmap: 0,
            pixel_bits: 0,
            bitmap_width: 0,
            bitmap_height: 0,
            cached_monitor: 0,
            cached_bounds: ScreenBounds::default(),
            cache_capture_time: None,
            cache_refresh_interval: Duration::ZERO,
            cache_valid: false,
        }
    }
}

impl ScreenPixelSampler {
    fn sample(&mut self, x: i32, y: i32) -> Result<PixelColor, DisplayError> {
        let info = resolve_monitor_capture_info(x, y)?;
        self.ensure_initialized(&info)?;
        if self.should_refresh_frame(&info) {
            self.capture_frame(&info)?;
        }
        self.sample_cached_pixel(x, y)
    }

    fn reset(&mut self) {
        // SAFETY: handles stored here are owned by this sampler and restored/deleted exactly once.
        unsafe {
            if self.mem_dc != 0 {
                let mem_dc = self.mem_dc as HDC;
                if self.old_bitmap != 0 {
                    let _ = SelectObject(mem_dc, self.old_bitmap as HGDIOBJ);
                }
                let _ = DeleteDC(mem_dc);
            }
            if self.bitmap != 0 {
                let _ = DeleteObject(self.bitmap as HGDIOBJ);
            }
        }
        self.mem_dc = 0;
        self.bitmap = 0;
        self.old_bitmap = 0;
        self.pixel_bits = 0;
        self.bitmap_width = 0;
        self.bitmap_height = 0;
        self.cached_monitor = 0;
        self.cached_bounds = ScreenBounds::default();
        self.cache_capture_time = None;
        self.cache_refresh_interval = Duration::ZERO;
        self.cache_valid = false;
    }

    fn ensure_initialized(&mut self, info: &MonitorCaptureInfo) -> Result<(), DisplayError> {
        if self.mem_dc != 0
            && self.bitmap != 0
            && self.pixel_bits != 0
            && self.bitmap_width == info.bounds.width
            && self.bitmap_height == info.bounds.height
        {
            return Ok(());
        }
        let screen = ScreenDc::acquire().ok_or_else(|| {
            DisplayError::new("GetDC failed while preparing screen pixel sampling")
        })?;
        self.reset();

        // SAFETY: screen.0 is a live desktop DC.
        let mem_dc = unsafe { CreateCompatibleDC(screen.0) };
        if mem_dc.is_null() {
            return Err(DisplayError::new(
                "CreateCompatibleDC failed while preparing screen pixel sampling",
            ));
        }
        self.mem_dc = mem_dc as usize;

        let mut bitmap_info = BITMAPINFO::default();
        bitmap_info.bmiHeader = BITMAPINFOHEADER {
            biSize: u32::try_from(size_of::<BITMAPINFOHEADER>()).unwrap_or_default(),
            biWidth: info.bounds.width,
            biHeight: -info.bounds.height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..BITMAPINFOHEADER::default()
        };
        let mut bits: *mut c_void = ptr::null_mut();
        // SAFETY: bitmap_info and bits are valid for the duration of the call; no section handle is used.
        let bitmap = unsafe {
            CreateDIBSection(screen.0, &bitmap_info, DIB_RGB_COLORS, &mut bits, ptr::null_mut(), 0)
        };
        self.bitmap = bitmap as usize;
        self.pixel_bits = bits as usize;
        if bitmap.is_null() || bits.is_null() {
            self.reset();
            return Err(DisplayError::new(
                "CreateDIBSection failed while preparing screen pixel sampling",
            ));
        }

        // SAFETY: mem_dc and bitmap are live handles owned by this sampler.
        let old = unsafe { SelectObject(mem_dc, bitmap as HGDIOBJ) };
        if old.is_null() || old as isize == -1 {
            self.reset();
            return Err(DisplayError::new(
                "SelectObject failed while preparing screen pixel sampling",
            ));
        }
        self.old_bitmap = old as usize;
        self.bitmap_width = info.bounds.width;
        self.bitmap_height = info.bounds.height;
        Ok(())
    }

    fn should_refresh_frame(&self, info: &MonitorCaptureInfo) -> bool {
        !self.cache_valid
            || self.cached_monitor != info.monitor
            || self.cached_bounds != info.bounds
            || self.cache_refresh_interval != info.refresh_interval
            || self
                .cache_capture_time
                .is_none_or(|captured| captured.elapsed() >= info.refresh_interval)
    }

    fn capture_frame(&mut self, info: &MonitorCaptureInfo) -> Result<(), DisplayError> {
        if self.mem_dc == 0 || self.bitmap == 0 || self.pixel_bits == 0 {
            return Err(DisplayError::new("screen pixel sampling resources are not ready"));
        }
        let screen = ScreenDc::acquire().ok_or_else(|| {
            self.cache_valid = false;
            DisplayError::new("GetDC failed while sampling the screen frame")
        })?;
        // SAFETY: source and destination DCs are live and the destination DIB matches the monitor size.
        let ok = unsafe {
            BitBlt(
                self.mem_dc as HDC,
                0,
                0,
                info.bounds.width,
                info.bounds.height,
                screen.0,
                info.bounds.x,
                info.bounds.y,
                SRCCOPY | CAPTUREBLT,
            )
        };
        if ok == 0 {
            self.cache_valid = false;
            // SAFETY: GetLastError has no pointer arguments.
            let last_error = unsafe { GetLastError() };
            return Err(if last_error == 0 {
                DisplayError::new("BitBlt failed while sampling the screen frame")
            } else {
                DisplayError::new(format!(
                    "BitBlt failed while sampling the screen frame (GetLastError={last_error})"
                ))
            });
        }
        self.cached_monitor = info.monitor;
        self.cached_bounds = info.bounds;
        self.cache_refresh_interval = info.refresh_interval;
        self.cache_capture_time = Some(Instant::now());
        self.cache_valid = true;
        Ok(())
    }

    fn sample_cached_pixel(&self, x: i32, y: i32) -> Result<PixelColor, DisplayError> {
        if !self.cache_valid
            || self.pixel_bits == 0
            || self.bitmap_width <= 0
            || self.bitmap_height <= 0
        {
            return Err(DisplayError::new("screen pixel cache is not ready"));
        }
        let local_x = x - self.cached_bounds.x;
        let local_y = y - self.cached_bounds.y;
        if local_x < 0
            || local_y < 0
            || local_x >= self.bitmap_width
            || local_y >= self.bitmap_height
        {
            return Err(DisplayError::new("requested pixel is outside the cached monitor bounds"));
        }
        let offset = (usize::try_from(local_y).unwrap_or_default()
            * usize::try_from(self.bitmap_width).unwrap_or_default()
            + usize::try_from(local_x).unwrap_or_default())
            * 4;
        let bytes = self.pixel_bits as *const u8;
        // SAFETY: the validated offset lies inside the DIB allocated for bitmap_width*bitmap_height*4.
        unsafe {
            Ok(PixelColor {
                r: *bytes.add(offset + 2),
                g: *bytes.add(offset + 1),
                b: *bytes.add(offset),
            })
        }
    }
}

impl Drop for ScreenPixelSampler {
    fn drop(&mut self) {
        self.reset();
    }
}

#[derive(Debug, Default)]
pub struct WindowsDisplayCapture {
    sampler: Mutex<ScreenPixelSampler>,
}

impl WindowsDisplayCapture {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn active_monitor() -> Option<(HMONITOR, MONITORINFOEXA)> {
    let mut point = POINT::default();
    // SAFETY: point is valid writable storage and MonitorFromPoint retains no pointers.
    if unsafe { GetCursorPos(&mut point) } == 0 {
        return None;
    }
    let monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    let info = monitor_info(monitor)?;
    Some((monitor, info))
}

impl DisplayCapture for WindowsDisplayCapture {
    fn active_monitor_bounds(&self) -> Option<ScreenBounds> {
        let (_, info) = active_monitor()?;
        windows_monitor_bounds(
            info.monitorInfo.rcMonitor.left,
            info.monitorInfo.rcMonitor.top,
            info.monitorInfo.rcMonitor.right,
            info.monitorInfo.rcMonitor.bottom,
        )
    }

    fn active_monitor_refresh_rate_hz(&self) -> Option<i32> {
        let (monitor, _) = active_monitor()?;
        monitor_refresh_rate(monitor)
    }

    fn cursor_position(&self) -> Option<CursorPosition> {
        let mut point = POINT::default();
        // SAFETY: point is valid writable storage.
        (unsafe { GetCursorPos(&mut point) } != 0)
            .then_some(CursorPosition { x: point.x, y: point.y })
    }

    fn sample_pixel(&self, x: i32, y: i32) -> Result<PixelColor, DisplayError> {
        lock(&self.sampler).sample(x, y)
    }

    fn unavailable_reason(&self) -> String {
        "screen pixel color sampling is not available in the current Windows session".to_owned()
    }
}
