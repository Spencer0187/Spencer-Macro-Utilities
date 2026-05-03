#include "foreground_x11.h"

#include <algorithm>
#include <atomic>
#include <cstddef>
#include <cstring>
#include <mutex>

#if defined(__linux__) && defined(SMU_HAS_X11) && SMU_HAS_X11
#include <X11/Xatom.h>
#include <X11/Xlib.h>
#endif

namespace smu::platform::linux {
namespace {

#if defined(__linux__) && defined(SMU_HAS_X11) && SMU_HAS_X11
constexpr int kX11ForegroundFailureDisableThreshold = 1;

std::atomic<int> g_x11ForegroundDetectionState{-1};
std::atomic<int> g_x11EmptyActiveWindowFailures{0};
std::atomic<int> g_x11MissingPidFailures{0};
std::mutex g_x11ForegroundDetectionMutex;
std::string g_x11ForegroundDetectionError;
#endif

} // namespace

bool IsX11ForegroundDetectionAvailable(std::string* errorMessage)
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
}

bool DisableX11ForegroundDetection(const std::string& reason)
{
#if defined(__linux__) && defined(SMU_HAS_X11) && SMU_HAS_X11
    std::lock_guard<std::mutex> lock(g_x11ForegroundDetectionMutex);
    if (g_x11ForegroundDetectionState.load(std::memory_order_acquire) == 0) {
        return false;
    }

    g_x11ForegroundDetectionError = reason;
    g_x11EmptyActiveWindowFailures.store(0, std::memory_order_release);
    g_x11MissingPidFailures.store(0, std::memory_order_release);
    g_x11ForegroundDetectionState.store(0, std::memory_order_release);
    return true;
#else
    (void)reason;
    return false;
#endif
}

std::optional<PlatformPid> GetX11ForegroundProcess(std::string* errorMessage)
{
    if (errorMessage) {
        errorMessage->clear();
    }

#if defined(__linux__) && defined(SMU_HAS_X11) && SMU_HAS_X11
    std::string availabilityError;
    if (!IsX11ForegroundDetectionAvailable(&availabilityError)) {
        if (errorMessage) {
            *errorMessage = availabilityError;
        }
        return std::nullopt;
    }

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

    g_x11EmptyActiveWindowFailures.store(0, std::memory_order_release);

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

        closeDisplay();

        const int failures = g_x11MissingPidFailures.fetch_add(1, std::memory_order_acq_rel) + 1;
        const std::string transientMessage =
            "Could not read _NET_WM_PID from the active X11 window.";

        if (failures >= kX11ForegroundFailureDisableThreshold) {
            const std::string message =
                "X11 foreground process detection failed repeatedly because the active window "
                "did not expose _NET_WM_PID. The current X11 window manager/session is not "
                "providing enough information to map the active window to a process.";

            DisableX11ForegroundDetection(message);
            if (errorMessage) {
                *errorMessage = message;
            }
        } else if (errorMessage) {
            *errorMessage = transientMessage;
        }

        return std::nullopt;
    }

    g_x11MissingPidFailures.store(0, std::memory_order_release);

    unsigned long pidValue = 0;
    if (actualFormat == 32) {
        pidValue = reinterpret_cast<unsigned long*>(property)[0];
    } else {
        std::memcpy(&pidValue, property, std::min(sizeof(pidValue), static_cast<std::size_t>(actualFormat / 8)));
    }
    XFree(property);
    closeDisplay();

    if (pidValue == 0) {
        const int failures = g_x11MissingPidFailures.fetch_add(1, std::memory_order_acq_rel) + 1;
        if (failures >= kX11ForegroundFailureDisableThreshold) {
            const std::string message =
                "X11 foreground process detection failed repeatedly because the active window "
                "reported an invalid process ID.";

            DisableX11ForegroundDetection(message);
            if (errorMessage) {
                *errorMessage = message;
            }
        } else if (errorMessage) {
            *errorMessage = "Active X11 window did not expose a valid process ID.";
        }

        return std::nullopt;
    }

    g_x11MissingPidFailures.store(0, std::memory_order_release);
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
