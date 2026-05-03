#!/usr/bin/env python3
from pathlib import Path
import re
import sys


FOREGROUND = Path("platform/linux/foreground_x11.cpp")
APP_UI = Path("app/app_ui.cpp")
MACRO_RUNTIME = Path("app/macro_runtime.cpp")


def backup(path: Path, original: str) -> None:
    path.with_suffix(path.suffix + ".pre_x11_immediate_fallback.bak").write_text(original, encoding="utf-8")


def find_function_bounds(text: str, signature_start: str) -> tuple[int, int]:
    start = text.find(signature_start)
    if start == -1:
        raise RuntimeError(f"Could not find function signature: {signature_start}")

    brace = text.find("{", start)
    if brace == -1:
        raise RuntimeError(f"Could not find opening brace for: {signature_start}")

    depth = 0
    i = brace
    while i < len(text):
        if text[i] == "{":
            depth += 1
        elif text[i] == "}":
            depth -= 1
            if depth == 0:
                return start, i + 1
        i += 1

    raise RuntimeError(f"Could not find closing brace for: {signature_start}")


def replace_function(text: str, signature_start: str, replacement: str) -> str:
    start, end = find_function_bounds(text, signature_start)
    return text[:start] + replacement + text[end:]


def patch_foreground_x11() -> bool:
    path = FOREGROUND
    original = path.read_text(encoding="utf-8")
    text = original

    text = text.replace(
        "constexpr int kX11ForegroundFailureDisableThreshold = 5;",
        "constexpr int kX11ForegroundFailureDisableThreshold = 1;",
    )

    new_available = r'''bool IsX11ForegroundDetectionAvailable(std::string* errorMessage)
{
#if defined(__linux__) && defined(SMU_HAS_X11) && SMU_HAS_X11
    if (errorMessage) {
        errorMessage->clear();
    }

    int state = g_x11ForegroundDetectionState.load(std::memory_order_acquire);
    if (state == 1) {
        return true;
    }
    if (state == 0) {
        if (errorMessage) {
            std::lock_guard<std::mutex> lock(g_x11ForegroundDetectionMutex);
            *errorMessage = g_x11ForegroundDetectionError;
        }
        return false;
    }

    std::lock_guard<std::mutex> lock(g_x11ForegroundDetectionMutex);
    state = g_x11ForegroundDetectionState.load(std::memory_order_acquire);
    if (state == 1) {
        return true;
    }
    if (state == 0) {
        if (errorMessage) {
            *errorMessage = g_x11ForegroundDetectionError;
        }
        return false;
    }

    Display* display = XOpenDisplay(nullptr);
    if (!display) {
        const std::string message = "XOpenDisplay failed; X11 foreground process detection is unavailable.";
        g_x11ForegroundDetectionError = message;
        g_x11ForegroundDetectionState.store(0, std::memory_order_release);
        if (errorMessage) {
            *errorMessage = message;
        }
        return false;
    }

    const Window root = DefaultRootWindow(display);
    const Atom activeWindowAtom = XInternAtom(display, "_NET_ACTIVE_WINDOW", True);
    const Atom wmPidAtom = XInternAtom(display, "_NET_WM_PID", True);

    if (activeWindowAtom == None || wmPidAtom == None) {
        XCloseDisplay(display);

        const std::string message = "X11 window manager does not expose _NET_ACTIVE_WINDOW or _NET_WM_PID.";
        g_x11ForegroundDetectionError = message;
        g_x11ForegroundDetectionState.store(0, std::memory_order_release);
        if (errorMessage) {
            *errorMessage = message;
        }
        return false;
    }

    Atom actualType = None;
    int actualFormat = 0;
    unsigned long itemCount = 0;
    unsigned long bytesAfter = 0;
    unsigned char* property = nullptr;

    const int status = XGetWindowProperty(
        display,
        root,
        activeWindowAtom,
        0,
        1,
        False,
        XA_WINDOW,
        &actualType,
        &actualFormat,
        &itemCount,
        &bytesAfter,
        &property);

    if (status != Success || !property || itemCount == 0) {
        if (property) {
            XFree(property);
        }
        XCloseDisplay(display);

        const std::string message =
            "X11 foreground process detection is unavailable because _NET_ACTIVE_WINDOW "
            "could not be read from the X11 root window.";
        g_x11ForegroundDetectionError = message;
        g_x11ForegroundDetectionState.store(0, std::memory_order_release);
        if (errorMessage) {
            *errorMessage = message;
        }
        return false;
    }

    Window activeWindow = 0;
    if (actualFormat == 32) {
        activeWindow = static_cast<Window>(reinterpret_cast<unsigned long*>(property)[0]);
    } else {
        std::memcpy(&activeWindow, property, std::min(sizeof(activeWindow), static_cast<std::size_t>(actualFormat / 8)));
    }

    XFree(property);
    XCloseDisplay(display);

    if (activeWindow == 0) {
        const std::string message =
            "X11 foreground process detection is unavailable because _NET_ACTIVE_WINDOW "
            "is empty. This X11 window manager/session is not exposing a usable active-window property.";

        g_x11ForegroundDetectionError = message;
        g_x11EmptyActiveWindowFailures.store(0, std::memory_order_release);
        g_x11MissingPidFailures.store(0, std::memory_order_release);
        g_x11ForegroundDetectionState.store(0, std::memory_order_release);

        if (errorMessage) {
            *errorMessage = message;
        }
        return false;
    }

    g_x11EmptyActiveWindowFailures.store(0, std::memory_order_release);
    g_x11MissingPidFailures.store(0, std::memory_order_release);
    g_x11ForegroundDetectionState.store(1, std::memory_order_release);
    return true;
#else
    if (errorMessage) {
        *errorMessage = "X11 support was not available at build time; foreground process detection is unsupported.";
    }
    return false;
#endif
}'''

    text = replace_function(
        text,
        "bool IsX11ForegroundDetectionAvailable(std::string* errorMessage)",
        new_available,
    )

    start, end = find_function_bounds(text, "std::optional<PlatformPid> GetX11ForegroundProcess")
    fn = text[start:end]

    old_active_zero = re.compile(
        r'''    if \(activeWindow == 0\) \{\n'''
        r'''        closeDisplay\(\);\n\n'''
        r'''        const int failures = g_x11EmptyActiveWindowFailures\.fetch_add\(1, std::memory_order_acq_rel\) \+ 1;\n'''
        r'''        if \(failures >= kX11ForegroundFailureDisableThreshold\) \{\n'''
        r'''            const std::string message =\n'''
        r'''                "X11 foreground process detection failed repeatedly because _NET_ACTIVE_WINDOW "\n'''
        r'''                "was empty\. The current X11 window manager/session is not exposing a usable "\n'''
        r'''                "active-window property\.";\n\n'''
        r'''            DisableX11ForegroundDetection\(message\);\n'''
        r'''            if \(errorMessage\) \{\n'''
        r'''                \*errorMessage = message;\n'''
        r'''            \}\n'''
        r'''        \}\n\n'''
        r'''        return std::nullopt;\n'''
        r'''    \}\n''',
        re.MULTILINE,
    )

    new_active_zero = r'''    if (activeWindow == 0) {
        closeDisplay();

        const std::string message =
            "X11 foreground process detection is unavailable because _NET_ACTIVE_WINDOW "
            "is empty. This X11 window manager/session is not exposing a usable active-window property.";

        DisableX11ForegroundDetection(message);
        if (errorMessage) {
            *errorMessage = message;
        }

        return std::nullopt;
    }
'''

    fn2, count = old_active_zero.subn(new_active_zero, fn, count=1)
    if count == 0 and "This X11 window manager/session is not exposing a usable active-window property." not in fn:
        raise RuntimeError("Could not replace GetX11ForegroundProcess activeWindow == 0 block.")

    text = text[:start] + fn2 + text[end:]

    if text != original:
        backup(path, original)
        path.write_text(text, encoding="utf-8")
        return True

    return False


def patch_app_ui_refresh() -> bool:
    path = APP_UI
    original = path.read_text(encoding="utf-8")
    text = original

    if "RefreshPlatformCapabilitiesForUi" not in text:
        insertion_point = "UpdateUiState g_updateUiState;\n"
        helper = r'''
void RefreshPlatformCapabilitiesForUi(AppContext& context)
{
    static double nextCapabilityRefreshTime = 0.0;
    const double now = ImGui::GetTime();
    if (now < nextCapabilityRefreshTime) {
        return;
    }

    nextCapabilityRefreshTime = now + 0.25;

    const bool wasFallbackActive = IsForegroundDetectionFallbackActive(context);
    context.capabilities = smu::platform::GetPlatformCapabilities();

    if (!wasFallbackActive && IsForegroundDetectionFallbackActive(context)) {
        context.foregroundFallbackWarningShown = false;
        MaybeWarnForegroundDetectionFallback(context);
    }
}
'''
        if insertion_point not in text:
            raise RuntimeError("Could not find app_ui.cpp UpdateUiState insertion point.")

        text = text.replace(insertion_point, insertion_point + helper, 1)

    signature = "void RenderAppUi(AppContext& context)"
    start, end = find_function_bounds(text, signature)
    fn = text[start:end]

    if "RefreshPlatformCapabilitiesForUi(context);" not in fn:
        brace = fn.find("{")
        fn = fn[:brace + 1] + "\n    RefreshPlatformCapabilitiesForUi(context);\n" + fn[brace + 1:]
        text = text[:start] + fn + text[end:]

    if text != original:
        backup(path, original)
        path.write_text(text, encoding="utf-8")
        return True

    return False


def patch_macro_runtime_cache_reset() -> bool:
    path = MACRO_RUNTIME
    original = path.read_text(encoding="utf-8")
    text = original

    old = r'''    targetPIDs.assign(pids.begin(), pids.end());
    processFound = !targetPIDs.empty();
    nextForegroundCheck_ = std::chrono::steady_clock::time_point{};
'''

    new = r'''    const bool pidsChanged =
        targetPIDs.size() != pids.size() ||
        !std::equal(targetPIDs.begin(), targetPIDs.end(), pids.begin());

    targetPIDs.assign(pids.begin(), pids.end());
    processFound = !targetPIDs.empty();

    if (pidsChanged) {
        nextForegroundCheck_ = std::chrono::steady_clock::time_point{};
    }
'''

    if old in text:
        text = text.replace(old, new, 1)

    if text != original:
        backup(path, original)
        path.write_text(text, encoding="utf-8")
        return True

    return False


def main() -> int:
    for path in (FOREGROUND, APP_UI, MACRO_RUNTIME):
        if not path.exists():
            print(f"error: missing {path}. Run this from the repository root.", file=sys.stderr)
            return 1

    try:
        changed = []
        if patch_foreground_x11():
            changed.append(str(FOREGROUND))
        if patch_app_ui_refresh():
            changed.append(str(APP_UI))
        if patch_macro_runtime_cache_reset():
            changed.append(str(MACRO_RUNTIME))

        if changed:
            print("Patched:")
            for item in changed:
                print(f"  - {item}")
            print("Backups created as *.pre_x11_immediate_fallback.bak")
        else:
            print("No changes made. Files already appear patched.")

        return 0

    except Exception as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())