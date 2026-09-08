#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowsLagOverlaySettings {
    pub shown: bool,
    pub hide_inactive: bool,
    pub use_background: bool,
    pub x: i32,
    pub y: i32,
    pub text_size: i32,
    pub background: [f32; 3],
}

#[must_use]
const fn should_exist(
    settings: WindowsLagOverlaySettings,
    backend_enabled: bool,
    active: bool,
) -> bool {
    settings.shown && backend_enabled && (!settings.hide_inactive || active)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VirtualDesktopBounds {
    pub min_x: i32,
    pub min_y: i32,
    pub width: i32,
    pub height: i32,
}

impl VirtualDesktopBounds {
    #[must_use]
    pub const fn max_x(self) -> i32 {
        self.min_x.saturating_add(self.width)
    }

    #[must_use]
    pub const fn max_y(self) -> i32 {
        self.min_y.saturating_add(self.height)
    }

    #[must_use]
    pub const fn default_overlay_x(self) -> i32 {
        self.min_x.saturating_add(self.width.saturating_mul(4) / 5)
    }
}

#[must_use]
const fn primary_screen_default_x(screen_width: i32) -> i32 {
    screen_width.saturating_mul(4) / 5
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{POINT, RECT, SIZE};
    use windows_sys::Win32::Graphics::Gdi::{
        AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, AddFontResourceExW, BI_RGB, BITMAPINFO,
        BITMAPINFOHEADER, BLENDFUNCTION, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS,
        CreateCompatibleDC, CreateDIBSection, CreateFontW, CreateSolidBrush, DEFAULT_CHARSET,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, FR_PRIVATE, FW_BOLD, FillRect, GetDC,
        GetTextExtentPoint32W, OUT_OUTLINE_PRECIS, ReleaseDC, RemoveFontResourceExW, SelectObject,
        SetBkMode, SetTextColor, TRANSPARENT, TextOutW, VARIABLE_PITCH,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, GetSystemMetrics, SM_CXSCREEN, SM_CXVIRTUALSCREEN,
        SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SW_SHOWNOACTIVATE, ShowWindow,
        ULW_ALPHA, UpdateLayeredWindow, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
    };

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn wide_path(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
    }

    fn rgb(red: u8, green: u8, blue: u8) -> u32 {
        u32::from(red) | (u32::from(green) << 8) | (u32::from(blue) << 16)
    }

    fn background_channel(value: f32) -> u8 {
        (value.clamp(0.0, 1.0) * 255.0) as u8
    }

    #[must_use]
    pub fn virtual_desktop_bounds() -> VirtualDesktopBounds {
        // SAFETY: GetSystemMetrics has no pointer preconditions.
        unsafe {
            VirtualDesktopBounds {
                min_x: GetSystemMetrics(SM_XVIRTUALSCREEN),
                min_y: GetSystemMetrics(SM_YVIRTUALSCREEN),
                width: GetSystemMetrics(SM_CXVIRTUALSCREEN),
                height: GetSystemMetrics(SM_CYVIRTUALSCREEN),
            }
        }
    }

    #[derive(Debug)]
    pub struct WindowsLagOverlay {
        hwnd: windows_sys::Win32::Foundation::HWND,
        registered_font_path: Option<Vec<u16>>,
    }

    impl WindowsLagOverlay {
        #[must_use]
        pub fn new() -> Self {
            Self { hwnd: null_mut(), registered_font_path: None }
        }

        fn destroy_window(&mut self) {
            if !self.hwnd.is_null() {
                // SAFETY: hwnd was created by this service on the current UI thread and is destroyed at most once.
                let _ = unsafe { DestroyWindow(self.hwnd) };
                self.hwnd = null_mut();
            }
        }

        fn ensure_font(&mut self) {
            if self.registered_font_path.is_some() {
                return;
            }
            let path = crate::theme::font_path();
            if !path.is_file() {
                return;
            }
            let wide_path = wide_path(&path);
            // SAFETY: wide_path is NUL terminated and remains alive for the duration of the call.
            let added = unsafe { AddFontResourceExW(wide_path.as_ptr(), FR_PRIVATE, null()) };
            if added > 0 {
                self.registered_font_path = Some(wide_path);
            }
        }

        fn ensure_window(&mut self, x: i32, y: i32) -> bool {
            if !self.hwnd.is_null() {
                return true;
            }
            self.ensure_font();
            let class_name = wide("STATIC");
            let title = wide("");
            // SAFETY: null module name requests the current process module.
            let instance = unsafe { GetModuleHandleW(null()) };
            // SAFETY: class/title strings are valid NUL-terminated UTF-16. All optional owner/menu/param pointers
            // are null, matching a top-level popup window.
            self.hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_TOPMOST
                        | WS_EX_LAYERED
                        | WS_EX_TRANSPARENT
                        | WS_EX_TOOLWINDOW
                        | WS_EX_NOACTIVATE,
                    class_name.as_ptr(),
                    title.as_ptr(),
                    WS_POPUP,
                    x,
                    y,
                    1,
                    1,
                    null_mut(),
                    null_mut(),
                    instance,
                    null(),
                )
            };
            if self.hwnd.is_null() {
                return false;
            }
            // SAFETY: hwnd is a valid window created immediately above.
            let _ = unsafe { ShowWindow(self.hwnd, SW_SHOWNOACTIVATE) };
            true
        }

        pub fn update(
            &mut self,
            settings: WindowsLagOverlaySettings,
            backend_enabled: bool,
            active: bool,
        ) -> Option<i32> {
            if !should_exist(settings, backend_enabled, active) {
                self.destroy_window();
                return None;
            }

            let resolved_x = if settings.x == -1 {
                // SAFETY: GetSystemMetrics has no pointer preconditions.
                primary_screen_default_x(unsafe { GetSystemMetrics(SM_CXSCREEN) })
            } else {
                settings.x
            };
            let resolved_default = (settings.x == -1).then_some(resolved_x);
            if !self.ensure_window(resolved_x, settings.y) {
                return resolved_default;
            }
            self.render(settings, resolved_x, active);
            resolved_default
        }

        fn render(&self, settings: WindowsLagOverlaySettings, x: i32, active: bool) {
            // SAFETY: self.hwnd is a live window while this method runs.
            let hdc = unsafe { GetDC(self.hwnd) };
            if hdc.is_null() {
                return;
            }
            // SAFETY: hdc is a valid display DC.
            let mem_dc = unsafe { CreateCompatibleDC(hdc) };
            if mem_dc.is_null() {
                // SAFETY: balances GetDC above.
                let _ = unsafe { ReleaseDC(self.hwnd, hdc) };
                return;
            }

            let face = wide("Lucida Sans");
            let quality = if settings.use_background {
                u32::from(CLEARTYPE_QUALITY)
            } else {
                u32::from(ANTIALIASED_QUALITY)
            };
            // SAFETY: face is a valid NUL-terminated UTF-16 string and all numeric font settings match C++.
            let font = unsafe {
                CreateFontW(
                    settings.text_size.saturating_add(15),
                    0,
                    0,
                    0,
                    FW_BOLD as i32,
                    0,
                    0,
                    0,
                    u32::from(DEFAULT_CHARSET),
                    u32::from(OUT_OUTLINE_PRECIS),
                    u32::from(CLIP_DEFAULT_PRECIS),
                    quality,
                    u32::from(VARIABLE_PITCH),
                    face.as_ptr(),
                )
            };
            if font.is_null() {
                // SAFETY: both resources were acquired above.
                unsafe {
                    let _ = DeleteDC(mem_dc);
                    let _ = ReleaseDC(self.hwnd, hdc);
                }
                return;
            }
            // SAFETY: mem_dc and font are valid GDI handles.
            let old_font = unsafe { SelectObject(mem_dc, font) };

            let anchor = wide("Lagswitch OFF");
            let mut anchor_size = SIZE::default();
            // SAFETY: anchor has 13 visible UTF-16 code units before its terminator and anchor_size is writable.
            if unsafe { GetTextExtentPoint32W(mem_dc, anchor.as_ptr(), 13, &raw mut anchor_size) }
                == 0
            {
                self.cleanup_render_handles(hdc, mem_dc, font, old_font, null_mut(), null_mut());
                return;
            }
            let padding = (((anchor_size.cx + anchor_size.cy) as f32 / 2.0) / 25.0) as i32;
            let padding = padding.max(2);
            let box_width = anchor_size.cx.saturating_add(padding.saturating_mul(2));
            let box_height = anchor_size.cy.saturating_add(padding.saturating_mul(2));
            if box_width <= 0 || box_height <= 0 {
                self.cleanup_render_handles(hdc, mem_dc, font, old_font, null_mut(), null_mut());
                return;
            }

            let bitmap_info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: box_width,
                    biHeight: box_height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..BITMAPINFOHEADER::default()
                },
                ..BITMAPINFO::default()
            };
            let mut bits: *mut c_void = null_mut();
            // SAFETY: bitmap_info describes a valid 32-bit RGB DIB and bits points to writable output storage.
            let bitmap = unsafe {
                CreateDIBSection(
                    mem_dc,
                    &raw const bitmap_info,
                    DIB_RGB_COLORS,
                    &raw mut bits,
                    null_mut(),
                    0,
                )
            };
            if bitmap.is_null() {
                self.cleanup_render_handles(hdc, mem_dc, font, old_font, null_mut(), null_mut());
                return;
            }
            // SAFETY: mem_dc and bitmap are valid GDI handles.
            let old_bitmap = unsafe { SelectObject(mem_dc, bitmap) };
            let rect = RECT { left: 0, top: 0, right: box_width, bottom: box_height };
            let text = if active { wide("Lagswitch ON") } else { wide("Lagswitch OFF") };
            let visible_len = if active { 12 } else { 13 };

            let brush_color = if settings.use_background {
                rgb(
                    background_channel(settings.background[0]),
                    background_channel(settings.background[1]),
                    background_channel(settings.background[2]),
                )
            } else {
                rgb(0, 0, 0)
            };
            // SAFETY: color is a valid COLORREF.
            let brush = unsafe { CreateSolidBrush(brush_color) };
            if !brush.is_null() {
                // SAFETY: mem_dc, rect and brush are valid for the duration of the call.
                unsafe {
                    let _ = FillRect(mem_dc, &raw const rect, brush);
                    let _ = DeleteObject(brush);
                }
            }
            // SAFETY: mem_dc is valid; mode and COLORREF values are valid.
            unsafe {
                let _ = SetBkMode(mem_dc, TRANSPARENT as i32);
                let text_color = if settings.use_background && active {
                    rgb(0, 255, 0)
                } else {
                    rgb(255, 255, 255)
                };
                let _ = SetTextColor(mem_dc, text_color);
            }

            let mut current_size = SIZE::default();
            // SAFETY: text and current_size are valid for the stated visible length.
            if unsafe {
                GetTextExtentPoint32W(mem_dc, text.as_ptr(), visible_len, &raw mut current_size)
            } != 0
            {
                let text_x = (box_width - current_size.cx) / 2;
                let text_y = (box_height - current_size.cy) / 2;
                // SAFETY: text is valid for visible_len code units and mem_dc is valid.
                let _ = unsafe { TextOutW(mem_dc, text_x, text_y, text.as_ptr(), visible_len) };
            }

            if !bits.is_null() {
                let pixel_count = usize::try_from(box_width)
                    .ok()
                    .and_then(|w| usize::try_from(box_height).ok().and_then(|h| w.checked_mul(h)));
                if let Some(pixel_count) = pixel_count {
                    // SAFETY: CreateDIBSection allocated pixel_count * 4 bytes for the 32-bit bitmap.
                    let pixels = unsafe {
                        std::slice::from_raw_parts_mut(
                            bits.cast::<u8>(),
                            pixel_count.saturating_mul(4),
                        )
                    };
                    if settings.use_background {
                        for pixel in pixels.chunks_exact_mut(4) {
                            pixel[3] = 255;
                        }
                    } else {
                        let (target_red, target_green, target_blue) =
                            if active { (0_u8, 255_u8, 0_u8) } else { (255, 255, 255) };
                        for pixel in pixels.chunks_exact_mut(4) {
                            let intensity = pixel[2];
                            if intensity > 0 {
                                pixel[0] =
                                    ((u16::from(target_blue) * u16::from(intensity)) / 255) as u8;
                                pixel[1] =
                                    ((u16::from(target_green) * u16::from(intensity)) / 255) as u8;
                                pixel[2] =
                                    ((u16::from(target_red) * u16::from(intensity)) / 255) as u8;
                                pixel[3] = intensity;
                            } else {
                                pixel[3] = 0;
                            }
                        }
                    }
                }
            }

            let source = POINT { x: 0, y: 0 };
            let destination = POINT { x, y: settings.y };
            let window_size = SIZE { cx: box_width, cy: box_height };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            // SAFETY: all DC/window/bitmap geometry pointers remain valid for the synchronous call.
            let _ = unsafe {
                UpdateLayeredWindow(
                    self.hwnd,
                    hdc,
                    &raw const destination,
                    &raw const window_size,
                    mem_dc,
                    &raw const source,
                    0,
                    &raw const blend,
                    ULW_ALPHA,
                )
            };

            self.cleanup_render_handles(hdc, mem_dc, font, old_font, bitmap, old_bitmap);
        }

        fn cleanup_render_handles(
            &self,
            hdc: windows_sys::Win32::Graphics::Gdi::HDC,
            mem_dc: windows_sys::Win32::Graphics::Gdi::HDC,
            font: windows_sys::Win32::Graphics::Gdi::HFONT,
            old_font: windows_sys::Win32::Graphics::Gdi::HGDIOBJ,
            bitmap: windows_sys::Win32::Graphics::Gdi::HBITMAP,
            old_bitmap: windows_sys::Win32::Graphics::Gdi::HGDIOBJ,
        ) {
            // SAFETY: each non-null handle was created/acquired in render. Restore selected objects before delete.
            unsafe {
                if !old_bitmap.is_null() {
                    let _ = SelectObject(mem_dc, old_bitmap);
                }
                if !old_font.is_null() {
                    let _ = SelectObject(mem_dc, old_font);
                }
                if !bitmap.is_null() {
                    let _ = DeleteObject(bitmap);
                }
                if !font.is_null() {
                    let _ = DeleteObject(font);
                }
                let _ = DeleteDC(mem_dc);
                let _ = ReleaseDC(self.hwnd, hdc);
            }
        }

        pub fn shutdown(&mut self) {
            self.destroy_window();
            if let Some(path) = self.registered_font_path.take() {
                // SAFETY: path is the same NUL-terminated path used for AddFontResourceExW.
                let _ = unsafe { RemoveFontResourceExW(path.as_ptr(), FR_PRIVATE, null()) };
            }
        }
    }

    impl Default for WindowsLagOverlay {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Drop for WindowsLagOverlay {
        fn drop(&mut self) {
            self.shutdown();
        }
    }
}

#[cfg(target_os = "windows")]
pub use platform::{WindowsLagOverlay, virtual_desktop_bounds};

#[cfg(test)]
mod tests {
    use super::*;

    const SETTINGS: WindowsLagOverlaySettings = WindowsLagOverlaySettings {
        shown: true,
        hide_inactive: false,
        use_background: false,
        x: -1,
        y: 50,
        text_size: 20,
        background: [60.0 / 255.0, 90.0 / 255.0, 160.0 / 255.0],
    };

    #[test]
    fn visibility_matches_cpp_show_enabled_and_hide_inactive_rules() {
        assert!(!should_exist(SETTINGS, false, true));
        assert!(should_exist(SETTINGS, true, false));
        let hidden_when_inactive = WindowsLagOverlaySettings { hide_inactive: true, ..SETTINGS };
        assert!(!should_exist(hidden_when_inactive, true, false));
        assert!(should_exist(hidden_when_inactive, true, true));
        let hidden = WindowsLagOverlaySettings { shown: false, ..SETTINGS };
        assert!(!should_exist(hidden, true, true));
    }

    #[test]
    fn virtual_desktop_bounds_match_cpp_slider_and_default_x_rules() {
        let bounds = VirtualDesktopBounds { min_x: -2560, min_y: -900, width: 7040, height: 3060 };
        assert_eq!(bounds.max_x(), 4480);
        assert_eq!(bounds.max_y(), 2160);
        assert_eq!(bounds.default_overlay_x(), 3072);
    }

    #[test]
    fn runtime_sentinel_fallback_matches_cpp_primary_screen_rule() {
        assert_eq!(primary_screen_default_x(1920), 1536);
        assert_eq!(primary_screen_default_x(2560), 2048);
    }
}
