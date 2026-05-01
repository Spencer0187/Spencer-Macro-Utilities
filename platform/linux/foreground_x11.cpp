#include "foreground_x11.h"

#include <algorithm>
#include <cstddef>
#include <cstring>

#if defined(__linux__) && defined(SMU_HAS_X11) && SMU_HAS_X11
#include <X11/Xatom.h>
#include <X11/Xlib.h>
#endif

namespace smu::platform::linux {

bool IsX11ForegroundDetectionAvailable()
{
#if defined(__linux__) && defined(SMU_HAS_X11) && SMU_HAS_X11
    return true;
#else
    return false;
#endif
}

std::optional<PlatformPid> GetX11ForegroundProcess(std::string* errorMessage)
{
    if (errorMessage) {
        errorMessage->clear();
    }

#if defined(__linux__) && defined(SMU_HAS_X11) && SMU_HAS_X11
    Display* display = XOpenDisplay(nullptr);
    if (!display) {
        if (errorMessage) {
            *errorMessage = "XOpenDisplay failed; X11 foreground process detection is unavailable.";
        }
        return std::nullopt;
    }

    auto closeDisplay = [&display]() {
        if (display) {
            XCloseDisplay(display);
            display = nullptr;
        }
    };

    const Window root = DefaultRootWindow(display);
    const Atom activeWindowAtom = XInternAtom(display, "_NET_ACTIVE_WINDOW", True);
    const Atom wmPidAtom = XInternAtom(display, "_NET_WM_PID", True);
    if (activeWindowAtom == None || wmPidAtom == None) {
        if (errorMessage) {
            *errorMessage = "X11 window manager does not expose _NET_ACTIVE_WINDOW or _NET_WM_PID.";
        }
        closeDisplay();
        return std::nullopt;
    }

    Atom actualType = None;
    int actualFormat = 0;
    unsigned long itemCount = 0;
    unsigned long bytesAfter = 0;
    unsigned char* property = nullptr;

    int status = XGetWindowProperty(
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
        if (errorMessage) {
            *errorMessage = "Could not read _NET_ACTIVE_WINDOW from the X11 root window.";
        }
        closeDisplay();
        return std::nullopt;
    }

    Window activeWindow = 0;
    if (actualFormat == 32) {
        activeWindow = static_cast<Window>(reinterpret_cast<unsigned long*>(property)[0]);
    } else {
        std::memcpy(&activeWindow, property, std::min(sizeof(activeWindow), static_cast<std::size_t>(actualFormat / 8)));
    }
    XFree(property);
    property = nullptr;

    if (activeWindow == 0) {
        if (errorMessage) {
            *errorMessage = "X11 reports no active window.";
        }
        closeDisplay();
        return std::nullopt;
    }

    status = XGetWindowProperty(
        display,
        activeWindow,
        wmPidAtom,
        0,
        1,
        False,
        XA_CARDINAL,
        &actualType,
        &actualFormat,
        &itemCount,
        &bytesAfter,
        &property);

    if (status != Success || !property || itemCount == 0) {
        if (property) {
            XFree(property);
        }
        if (errorMessage) {
            *errorMessage = "Could not read _NET_WM_PID from the active X11 window.";
        }
        closeDisplay();
        return std::nullopt;
    }

    unsigned long pidValue = 0;
    if (actualFormat == 32) {
        pidValue = reinterpret_cast<unsigned long*>(property)[0];
    } else {
        std::memcpy(&pidValue, property, std::min(sizeof(pidValue), static_cast<std::size_t>(actualFormat / 8)));
    }
    XFree(property);
    closeDisplay();

    if (pidValue == 0) {
        if (errorMessage) {
            *errorMessage = "Active X11 window did not expose a valid process ID.";
        }
        return std::nullopt;
    }

    return static_cast<PlatformPid>(pidValue);
#else
    if (errorMessage) {
        *errorMessage = "X11 support was not available at build time; foreground process detection is unsupported.";
    }
    return std::nullopt;
#endif
}

bool IsX11ForegroundProcess(const std::vector<PlatformPid>& targetPids, std::string* errorMessage)
{
    auto foregroundPid = GetX11ForegroundProcess(errorMessage);
    if (!foregroundPid) {
        return false;
    }

    return std::find(targetPids.begin(), targetPids.end(), *foregroundPid) != targetPids.end();
}

} // namespace smu::platform::linux
