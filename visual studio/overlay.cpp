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

	// 2. Create Window
	if (overlayHwnd == NULL) {
		HINSTANCE hInstance = GetModuleHandle(NULL);
		WNDCLASSEXW owc = {sizeof(WNDCLASSEXW), CS_HREDRAW | CS_VREDRAW,
				   OverlayWndProc, 0, 0, hInstance, NULL, NULL, NULL, NULL,
				   L"LS_Overlay", NULL};
		RegisterClassExW(&owc);

		if (overlay_x == -1)
			overlay_x = (int)(GetSystemMetrics(SM_CXSCREEN) * 0.8f);

		overlayHwnd = CreateWindowExW(WS_EX_TOPMOST | WS_EX_LAYERED | WS_EX_TRANSPARENT |
						      WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
					      L"LS_Overlay", L"", WS_POPUP, overlay_x, overlay_y, 1,
					      1, NULL, NULL, hInstance, NULL);

		ShowWindow(overlayHwnd, SW_SHOWNOACTIVATE);

		if (!g_overlayFontHandle) {
			HRSRC hRes = FindResource(NULL, TEXT("LSANS_TTF"), RT_RCDATA);
			if (hRes) {
				HGLOBAL hMem = LoadResource(NULL, hRes);
				void *pData = LockResource(hMem);
				DWORD len = SizeofResource(NULL, hRes);
				DWORD nFonts = 0;
				g_overlayFontHandle = AddFontMemResourceEx(pData, len, NULL, &nFonts);
			}
		}
	}

	HDC hdc = GetDC(overlayHwnd);
	HDC memDC = CreateCompatibleDC(hdc);

	// 3. Setup Font
	// If Background is ON: Use ClearType (looks best on solid backgrounds)
	// If Background is OFF: Use Antialiased (required for our transparency trick)
	DWORD dwQuality = overlay_use_bg ? CLEARTYPE_QUALITY : ANTIALIASED_QUALITY;

	HFONT hFont = CreateFontA(overlay_size + 15, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE,
				  DEFAULT_CHARSET, OUT_OUTLINE_PRECIS, CLIP_DEFAULT_PRECIS,
				  dwQuality, VARIABLE_PITCH, "Lucida Sans");
	SelectObject(memDC, hFont);

	const char *anchorText = "Lagswitch OFF";
	SIZE anchorSize;
	GetTextExtentPoint32A(memDC, anchorText, (int)strlen(anchorText), &anchorSize);

	int padding = (int)(((anchorSize.cx + anchorSize.cy) / 2.0f) / 25.0f);
	if (padding < 2) padding = 2;

	int boxW = anchorSize.cx + (padding * 2);
	int boxH = anchorSize.cy + (padding * 2);

	// 4. Create 32-bit DIB
	BITMAPINFO bmi = {0};
	bmi.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
	bmi.bmiHeader.biWidth = boxW;
	bmi.bmiHeader.biHeight = boxH;
	bmi.bmiHeader.biPlanes = 1;
	bmi.bmiHeader.biBitCount = 32;
	bmi.bmiHeader.biCompression = BI_RGB;

	void* pBits = NULL;
	HBITMAP hBitmap = CreateDIBSection(memDC, &bmi, DIB_RGB_COLORS, &pBits, NULL, 0);
	SelectObject(memDC, hBitmap);

	// 5. Drawing Logic
	RECT rect = {0, 0, boxW, boxH};
	bool isActive = g_windivert_blocking.load();
	const char *currentText = isActive ? "Lagswitch ON" : "Lagswitch OFF";

	if (overlay_use_bg) {
		// --- BACKGROUND ENABLED (CLEARTYPE MODE) ---
		
		// 1. Fill Background
		HBRUSH hBrush = CreateSolidBrush(
			RGB(overlay_bg_r * 255, overlay_bg_g * 255, overlay_bg_b * 255));
		FillRect(memDC, &rect, hBrush);
		DeleteObject(hBrush);

		// 2. Draw Text (Standard GDI)
		SetBkMode(memDC, TRANSPARENT);
		SetTextColor(memDC, isActive ? RGB(0, 255, 0) : RGB(255, 255, 255));
		
		SIZE currentSize;
		GetTextExtentPoint32A(memDC, currentText, (int)strlen(currentText), &currentSize);
		TextOutA(memDC, (boxW - currentSize.cx) / 2, (boxH - currentSize.cy) / 2, currentText,
			 (int)strlen(currentText));

		// 3. Force Alpha to 255
		// GDI operations (FillRect/TextOut) leave the Alpha byte as 0. 
		// Since we are using ULW_ALPHA later, we must manually set Alpha to 255
		// to make the window visible.
		if (pBits) {
			BYTE* pixels = (BYTE*)pBits;
			int totalPixels = boxW * boxH;
			for (int i = 0; i < totalPixels; i++) {
				pixels[i * 4 + 3] = 255; // Opaque
			}
		}
	} 
	else {
		// --- BACKGROUND DISABLED (TRANSPARENT MASK MODE) ---
		
		// 1. Fill Black (Mask Base)
		HBRUSH hBrush = CreateSolidBrush(RGB(0, 0, 0));
		FillRect(memDC, &rect, hBrush);
		DeleteObject(hBrush);

		// 2. Draw Text in PURE WHITE (Mask Source)
		SetBkMode(memDC, TRANSPARENT);
		SetTextColor(memDC, RGB(255, 255, 255)); 

		SIZE currentSize;
		GetTextExtentPoint32A(memDC, currentText, (int)strlen(currentText), &currentSize);
		TextOutA(memDC, (boxW - currentSize.cx) / 2, (boxH - currentSize.cy) / 2, currentText,
			 (int)strlen(currentText));

		// 3. Apply Premultiplied Alpha
		if (pBits) {
			BYTE* pixels = (BYTE*)pBits;
			int totalPixels = boxW * boxH;
			
			BYTE targetR = isActive ? 0 : 255;
			BYTE targetG = isActive ? 255 : 255;
			BYTE targetB = isActive ? 0 : 255;

			for (int i = 0; i < totalPixels; i++) {
				// Use Red channel as intensity/alpha map
				BYTE intensity = pixels[i * 4 + 2]; 

				if (intensity > 0) {
					pixels[i * 4 + 0] = (targetB * intensity) / 255; // B
					pixels[i * 4 + 1] = (targetG * intensity) / 255; // G
					pixels[i * 4 + 2] = (targetR * intensity) / 255; // R
					pixels[i * 4 + 3] = intensity; // Alpha
				} else {
					pixels[i * 4 + 3] = 0; // Transparent
				}
			}
		}
	}

	// 6. Update Window
	POINT ptSrc = {0, 0};
	POINT ptDst = {overlay_x, overlay_y};
	SIZE winSize = {boxW, boxH};
	BLENDFUNCTION blend = {AC_SRC_OVER, 0, 255, AC_SRC_ALPHA};

	UpdateLayeredWindow(overlayHwnd, hdc, &ptDst, &winSize, memDC, &ptSrc, 0, &blend, ULW_ALPHA);

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