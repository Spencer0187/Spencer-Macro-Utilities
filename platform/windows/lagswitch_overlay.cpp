#if defined(_WIN32)

#include "lagswitch_overlay.h"

#include "../../core/legacy_globals.h"

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <atomic>
#include <cstring>

namespace smu::platform::windows {
namespace {

using namespace Globals;

HWND g_overlayHwnd = nullptr;
HANDLE g_overlayFontHandle = nullptr;

LRESULT CALLBACK OverlayWndProc(HWND hwnd, UINT msg, WPARAM wparam, LPARAM lparam)
{
    (void)wparam;
    (void)lparam;
    switch (msg) {
    case WM_DESTROY:
        return 0;
    default:
        break;
    }
    return DefWindowProc(hwnd, msg, wparam, lparam);
}

void DestroyOverlayWindow()
{
    if (g_overlayHwnd) {
        DestroyWindow(g_overlayHwnd);
        g_overlayHwnd = nullptr;
    }
}

} // namespace

void UpdateLagswitchOverlay()
{
    bool shouldExist = show_lag_overlay && bWinDivertEnabled;
    if (overlay_hide_inactive && !g_windivert_blocking.load(std::memory_order_relaxed)) {
        shouldExist = false;
    }

    if (!shouldExist) {
        DestroyOverlayWindow();
        return;
    }

    if (!g_overlayHwnd) {
        HINSTANCE instance = GetModuleHandle(nullptr);
        WNDCLASSEXW wc = {sizeof(WNDCLASSEXW)};
        wc.style = CS_HREDRAW | CS_VREDRAW;
        wc.lpfnWndProc = OverlayWndProc;
        wc.hInstance = instance;
        wc.lpszClassName = L"LS_Overlay";
        RegisterClassExW(&wc);

        if (overlay_x == -1) {
            overlay_x = static_cast<int>(GetSystemMetrics(SM_CXSCREEN) * 0.8f);
        }

        g_overlayHwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            L"LS_Overlay",
            L"",
            WS_POPUP,
            overlay_x,
            overlay_y,
            1,
            1,
            nullptr,
            nullptr,
            instance,
            nullptr);

        ShowWindow(g_overlayHwnd, SW_SHOWNOACTIVATE);

        if (!g_overlayFontHandle) {
            HRSRC res = FindResource(nullptr, TEXT("LSANS_TTF"), RT_RCDATA);
            if (res) {
                HGLOBAL mem = LoadResource(nullptr, res);
                void* data = LockResource(mem);
                const DWORD len = SizeofResource(nullptr, res);
                DWORD fontCount = 0;
                g_overlayFontHandle = AddFontMemResourceEx(data, len, nullptr, &fontCount);
            }
        }
    }

    HDC hdc = GetDC(g_overlayHwnd);
    HDC memDc = CreateCompatibleDC(hdc);

    const DWORD quality = overlay_use_bg ? CLEARTYPE_QUALITY : ANTIALIASED_QUALITY;
    HFONT font = CreateFontA(
        overlay_size + 15,
        0,
        0,
        0,
        FW_BOLD,
        FALSE,
        FALSE,
        FALSE,
        DEFAULT_CHARSET,
        OUT_OUTLINE_PRECIS,
        CLIP_DEFAULT_PRECIS,
        quality,
        VARIABLE_PITCH,
        "Lucida Sans");
    SelectObject(memDc, font);

    const char* anchorText = "Lagswitch OFF";
    SIZE anchorSize {};
    GetTextExtentPoint32A(memDc, anchorText, static_cast<int>(std::strlen(anchorText)), &anchorSize);

    int padding = static_cast<int>(((anchorSize.cx + anchorSize.cy) / 2.0f) / 25.0f);
    if (padding < 2) padding = 2;

    const int boxW = anchorSize.cx + (padding * 2);
    const int boxH = anchorSize.cy + (padding * 2);

    BITMAPINFO bmi {};
    bmi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    bmi.bmiHeader.biWidth = boxW;
    bmi.bmiHeader.biHeight = boxH;
    bmi.bmiHeader.biPlanes = 1;
    bmi.bmiHeader.biBitCount = 32;
    bmi.bmiHeader.biCompression = BI_RGB;

    void* bits = nullptr;
    HBITMAP bitmap = CreateDIBSection(memDc, &bmi, DIB_RGB_COLORS, &bits, nullptr, 0);
    SelectObject(memDc, bitmap);

    RECT rect {0, 0, boxW, boxH};
    const bool isActive = g_windivert_blocking.load(std::memory_order_relaxed);
    const char* currentText = isActive ? "Lagswitch ON" : "Lagswitch OFF";

    if (overlay_use_bg) {
        HBRUSH brush = CreateSolidBrush(RGB(
            static_cast<int>(overlay_bg_r * 255),
            static_cast<int>(overlay_bg_g * 255),
            static_cast<int>(overlay_bg_b * 255)));
        FillRect(memDc, &rect, brush);
        DeleteObject(brush);

        SetBkMode(memDc, TRANSPARENT);
        SetTextColor(memDc, isActive ? RGB(0, 255, 0) : RGB(255, 255, 255));

        SIZE currentSize {};
        GetTextExtentPoint32A(memDc, currentText, static_cast<int>(std::strlen(currentText)), &currentSize);
        TextOutA(memDc, (boxW - currentSize.cx) / 2, (boxH - currentSize.cy) / 2, currentText,
            static_cast<int>(std::strlen(currentText)));

        if (bits) {
            BYTE* pixels = static_cast<BYTE*>(bits);
            const int totalPixels = boxW * boxH;
            for (int i = 0; i < totalPixels; ++i) {
                pixels[i * 4 + 3] = 255;
            }
        }
    } else {
        HBRUSH brush = CreateSolidBrush(RGB(0, 0, 0));
        FillRect(memDc, &rect, brush);
        DeleteObject(brush);

        SetBkMode(memDc, TRANSPARENT);
        SetTextColor(memDc, RGB(255, 255, 255));

        SIZE currentSize {};
        GetTextExtentPoint32A(memDc, currentText, static_cast<int>(std::strlen(currentText)), &currentSize);
        TextOutA(memDc, (boxW - currentSize.cx) / 2, (boxH - currentSize.cy) / 2, currentText,
            static_cast<int>(std::strlen(currentText)));

        if (bits) {
            BYTE* pixels = static_cast<BYTE*>(bits);
            const int totalPixels = boxW * boxH;

            const BYTE targetR = isActive ? 0 : 255;
            const BYTE targetG = 255;
            const BYTE targetB = isActive ? 0 : 255;

            for (int i = 0; i < totalPixels; ++i) {
                const BYTE intensity = pixels[i * 4 + 2];
                if (intensity > 0) {
                    pixels[i * 4 + 0] = static_cast<BYTE>((targetB * intensity) / 255);
                    pixels[i * 4 + 1] = static_cast<BYTE>((targetG * intensity) / 255);
                    pixels[i * 4 + 2] = static_cast<BYTE>((targetR * intensity) / 255);
                    pixels[i * 4 + 3] = intensity;
                } else {
                    pixels[i * 4 + 3] = 0;
                }
            }
        }
    }

    POINT ptSrc {0, 0};
    POINT ptDst {overlay_x, overlay_y};
    SIZE winSize {boxW, boxH};
    BLENDFUNCTION blend {AC_SRC_OVER, 0, 255, AC_SRC_ALPHA};

    UpdateLayeredWindow(g_overlayHwnd, hdc, &ptDst, &winSize, memDc, &ptSrc, 0, &blend, ULW_ALPHA);

    DeleteObject(font);
    DeleteObject(bitmap);
    DeleteDC(memDc);
    ReleaseDC(g_overlayHwnd, hdc);
}

void CleanupLagswitchOverlay()
{
    DestroyOverlayWindow();
    if (g_overlayFontHandle) {
        RemoveFontMemResourceEx(g_overlayFontHandle);
        g_overlayFontHandle = nullptr;
    }
}

} // namespace smu::platform::windows

#endif

