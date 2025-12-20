#include "Resource Files/overlay.h"
#include <string>
#include <atomic>

#include "Resource Files/globals.h"
using namespace Globals;

HWND overlayHwnd = NULL;
HANDLE g_overlayFontHandle = NULL;

// Win32 Window Procedure for the Overlay
LRESULT CALLBACK OverlayWndProc(HWND hWnd, UINT msg, WPARAM wParam, LPARAM lParam)
{
	switch (msg) {
	case WM_DESTROY:
		return 0;
	}
	return DefWindowProc(hWnd, msg, wParam, lParam);
}

void UpdateLagswitchOverlay()
{
	// 1. Determine Visibility
	bool shouldExist = show_lag_overlay && bWinDivertEnabled;
	if (overlay_hide_inactive && !g_windivert_blocking.load())
		shouldExist = false;

	if (!shouldExist) {
		if (overlayHwnd) {
			DestroyWindow(overlayHwnd);
			overlayHwnd = NULL;
		}
		return;
	}

	// 2. Create Window and Register Font if needed
	if (overlayHwnd == NULL) {
		HINSTANCE hInstance = GetModuleHandle(NULL);

		WNDCLASSEXW owc = {sizeof(WNDCLASSEXW),
				   CS_HREDRAW | CS_VREDRAW,
				   OverlayWndProc,
				   0,
				   0,
				   hInstance,
				   NULL,
				   NULL,
				   NULL,
				   NULL,
				   L"LS_Overlay",
				   NULL};
		RegisterClassExW(&owc);

		if (overlay_x == -1)
			overlay_x = (int)(GetSystemMetrics(SM_CXSCREEN) * 0.8f);

		overlayHwnd = CreateWindowExW(WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT |
						      WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
					      L"LS_Overlay", L"", WS_POPUP, overlay_x, overlay_y, 1,
					      1, NULL, NULL, hInstance, NULL);

		ShowWindow(overlayHwnd, SW_SHOWNOACTIVATE);

		// Register your embedded font for GDI
		if (!g_overlayFontHandle) {
			HRSRC hRes = FindResource(NULL, TEXT("LSANS_TTF"), RT_RCDATA);
			if (hRes) {
				HGLOBAL hMem = LoadResource(NULL, hRes);
				void *pData = LockResource(hMem);
				DWORD len = SizeofResource(NULL, hRes);
				DWORD nFonts = 0;
				g_overlayFontHandle =
					AddFontMemResourceEx(pData, len, NULL, &nFonts);
			}
		}
	}

	// 3. Prepare HDCs
	HDC hdc = GetDC(overlayHwnd);
	HDC memDC = CreateCompatibleDC(hdc);

	// 4. Set Font and Measure Anchor ("Lagswitch OFF")
	HFONT hFont = CreateFontA(overlay_size + 15, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE,
				  DEFAULT_CHARSET, OUT_OUTLINE_PRECIS, CLIP_DEFAULT_PRECIS,
				  CLEARTYPE_QUALITY, VARIABLE_PITCH, "Lucida Sans");
	SelectObject(memDC, hFont);

	const char *anchorText = "Lagswitch OFF";
	SIZE anchorSize;
	GetTextExtentPoint32A(memDC, anchorText, (int)strlen(anchorText), &anchorSize);

	// 5. Calculate Padding & Box Dimensions
	int padding = (int)(((anchorSize.cx + anchorSize.cy) / 2.0f) / 25.0f);
	if (padding < 2)
		padding = 2;

	int boxW = anchorSize.cx + (padding * 2);
	int boxH = anchorSize.cy + (padding * 2);

	// 6. Drawing
	HBITMAP hBitmap = CreateCompatibleBitmap(hdc, boxW, boxH);
	SelectObject(memDC, hBitmap);

	RECT rect = {0, 0, boxW, boxH};
	if (overlay_use_bg) {
		HBRUSH hBrush = CreateSolidBrush(
			RGB(overlay_bg_r * 255, overlay_bg_g * 255, overlay_bg_b * 255));
		FillRect(memDC, &rect, hBrush);
		DeleteObject(hBrush);
	} else {
		HBRUSH hBrush = CreateSolidBrush(RGB(0, 0, 0)); // Transparency Key
		FillRect(memDC, &rect, hBrush);
		DeleteObject(hBrush);
	}

	const char *currentText = g_windivert_blocking.load() ? "Lagswitch ON" : "Lagswitch OFF";
	SIZE currentSize;
	GetTextExtentPoint32A(memDC, currentText, (int)strlen(currentText), &currentSize);

	SetBkMode(memDC, TRANSPARENT);
	SetTextColor(memDC, g_windivert_blocking.load() ? RGB(0, 255, 0) : RGB(255, 255, 255));

	// Centering calculation
	TextOutA(memDC, (boxW - currentSize.cx) / 2, (boxH - currentSize.cy) / 2, currentText,
		 (int)strlen(currentText));

	// 7. Apply to Layered Window
	POINT ptSrc = {0, 0};
	POINT ptDst = {overlay_x, overlay_y};
	SIZE winSize = {boxW, boxH};
	BLENDFUNCTION blend = {AC_SRC_OVER, 0, 255, AC_SRC_ALPHA};
	UpdateLayeredWindow(overlayHwnd, hdc, &ptDst, &winSize, memDC, &ptSrc, RGB(0, 0, 0), &blend,
			    ULW_COLORKEY);

	// 8. Cleanup
	DeleteObject(hFont);
	DeleteObject(hBitmap);
	DeleteDC(memDC);
	ReleaseDC(overlayHwnd, hdc);
}

void CleanupOverlay()
{
	if (overlayHwnd) {
		DestroyWindow(overlayHwnd);
		overlayHwnd = NULL;
	}
	if (g_overlayFontHandle) {
		RemoveFontMemResourceEx(g_overlayFontHandle);
		g_overlayFontHandle = NULL;
	}
}