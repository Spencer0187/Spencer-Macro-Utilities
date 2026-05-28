#include "input_cgevent.h"

#if defined(__APPLE__)

#include "permissions_macos.h"

#include <ApplicationServices/ApplicationServices.h>
#include <CoreFoundation/CoreFoundation.h>
#include <CoreGraphics/CoreGraphics.h>

#include "../logging.h"
#include "../../core/key_codes.h"
#include "../../core/keymapping.h"
#include "../../core/legacy_globals.h"

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <future>
#include <limits>
#include <mutex>
#include <optional>
#include <string>
#include <thread>
#include <vector>

namespace smu::platform::macos {
namespace {

constexpr std::int64_t kMacosInjectedEventTag = 0x534d554d41434f53LL;
constexpr const char kMacosEventTapDisabledWarningId[] = "macos_event_tap_disabled";
constexpr const char kMacosEventTapInitWarningId[] = "macos_event_tap_init_failed";

std::atomic_bool g_outputInitialized{false};
std::atomic_bool g_readLoopInitialized{false};

std::string KeyName(PlatformKeyCode key)
{
    return smu::core::FormatKeyCodeFallback(key);
}

std::optional<CGPoint> CurrentCursorPoint()
{
    CGEventRef probe = CGEventCreate(nullptr);
    if (!probe) {
        return std::nullopt;
    }

    const CGPoint point = CGEventGetLocation(probe);
    CFRelease(probe);
    return point;
}

bool PointInside(CGPoint point, CGRect bounds)
{
    return point.x >= CGRectGetMinX(bounds) && point.x < CGRectGetMaxX(bounds) &&
        point.y >= CGRectGetMinY(bounds) && point.y < CGRectGetMaxY(bounds);
}

std::vector<CGDirectDisplayID> ActiveDisplays()
{
    uint32_t displayCount = 0;
    if (CGGetActiveDisplayList(0, nullptr, &displayCount) != kCGErrorSuccess ||
        displayCount == 0) {
        return {};
    }

    std::vector<CGDirectDisplayID> displays(displayCount);
    if (CGGetActiveDisplayList(displayCount, displays.data(), &displayCount) != kCGErrorSuccess) {
        return {};
    }
    displays.resize(displayCount);
    return displays;
}

std::optional<ScreenBounds> ScreenBoundsFromRect(CGRect rect)
{
    if (CGRectIsNull(rect) || CGRectIsEmpty(rect)) {
        return std::nullopt;
    }

    const double left = std::floor(CGRectGetMinX(rect));
    const double top = std::floor(CGRectGetMinY(rect));
    const double right = std::ceil(CGRectGetMaxX(rect));
    const double bottom = std::ceil(CGRectGetMaxY(rect));
    if (left < std::numeric_limits<int>::min() || top < std::numeric_limits<int>::min() ||
        right > std::numeric_limits<int>::max() || bottom > std::numeric_limits<int>::max()) {
        return std::nullopt;
    }

    const int width = static_cast<int>(right - left);
    const int height = static_cast<int>(bottom - top);
    if (width <= 0 || height <= 0) {
        return std::nullopt;
    }
    return ScreenBounds{static_cast<int>(left), static_cast<int>(top), width, height};
}

long double DisplayDistanceSquared(CGPoint point, CGRect bounds)
{
    const long double clampedX = std::clamp<long double>(
        point.x,
        CGRectGetMinX(bounds),
        CGRectGetMaxX(bounds));
    const long double clampedY = std::clamp<long double>(
        point.y,
        CGRectGetMinY(bounds),
        CGRectGetMaxY(bounds));
    const long double dx = static_cast<long double>(point.x) - clampedX;
    const long double dy = static_cast<long double>(point.y) - clampedY;
    return dx * dx + dy * dy;
}

std::optional<CGDirectDisplayID> DisplayForPoint(CGPoint point)
{
    std::optional<CGDirectDisplayID> closestDisplay;
    long double closestDistance = std::numeric_limits<long double>::max();

    for (CGDirectDisplayID display : ActiveDisplays()) {
        const CGRect bounds = CGDisplayBounds(display);
        if (PointInside(point, bounds)) {
            return display;
        }

        const long double distance = DisplayDistanceSquared(point, bounds);
        if (!closestDisplay || distance < closestDistance) {
            closestDisplay = display;
            closestDistance = distance;
        }
    }

    return closestDisplay;
}

std::optional<ScreenBounds> GlobalScreenBounds()
{
    std::optional<CGRect> unionRect;
    for (CGDirectDisplayID display : ActiveDisplays()) {
        const CGRect bounds = CGDisplayBounds(display);
        if (CGRectIsNull(bounds) || CGRectIsEmpty(bounds)) {
            continue;
        }
        unionRect = unionRect ? CGRectUnion(*unionRect, bounds) : bounds;
    }
    return unionRect ? ScreenBoundsFromRect(*unionRect) : std::nullopt;
}

std::optional<ScreenBounds> ActiveMonitorBounds()
{
    const auto cursor = CurrentCursorPoint();
    if (!cursor) {
        return std::nullopt;
    }
    const auto display = DisplayForPoint(*cursor);
    if (!display) {
        return std::nullopt;
    }
    return ScreenBoundsFromRect(CGDisplayBounds(*display));
}

std::optional<int> RefreshRateForActiveMonitor()
{
    const auto cursor = CurrentCursorPoint();
    const auto display = cursor ? DisplayForPoint(*cursor) : std::nullopt;
    if (!display) {
        return std::nullopt;
    }

    CGDisplayModeRef mode = CGDisplayCopyDisplayMode(*display);
    if (!mode) {
        return std::nullopt;
    }
    const double refreshRate = CGDisplayModeGetRefreshRate(mode);
    CGDisplayModeRelease(mode);
    if (!std::isfinite(refreshRate) || refreshRate <= 0.0 ||
        refreshRate > static_cast<double>(std::numeric_limits<int>::max())) {
        return std::nullopt;
    }
    return static_cast<int>(std::llround(refreshRate));
}

std::string ScreenRecordingPermissionReason()
{
    return "macOS Screen Recording permission is required before SMU can read screen pixels.";
}

class MacosScreenPixelSampler {
public:
    MacosScreenPixelSampler() = default;
    MacosScreenPixelSampler(const MacosScreenPixelSampler&) = delete;
    MacosScreenPixelSampler& operator=(const MacosScreenPixelSampler&) = delete;

    ~MacosScreenPixelSampler()
    {
        resetImage();
    }

    std::optional<PixelColor> sample(int x, int y, std::string* errorMessage)
    {
        if (!HasScreenRecordingPermission()) {
            if (errorMessage) {
                *errorMessage = ScreenRecordingPermissionReason();
            }
            return std::nullopt;
        }

        const CGPoint point = CGPointMake(static_cast<double>(x) + 0.5, static_cast<double>(y) + 0.5);
        const auto display = DisplayForPoint(point);
        if (!display) {
            if (errorMessage) {
                *errorMessage = "macOS could not resolve the display containing the requested pixel.";
            }
            return std::nullopt;
        }

        std::lock_guard<std::mutex> lock(mutex_);
        const CGRect displayBounds = CGDisplayBounds(*display);
        if (!PointInside(point, displayBounds)) {
            if (errorMessage) {
                *errorMessage = "requested pixel is outside the active macOS display bounds.";
            }
            return std::nullopt;
        }
        if (!captureDisplayIfNeeded(*display, displayBounds, errorMessage)) {
            return std::nullopt;
        }

        const double localX = point.x - CGRectGetMinX(cachedBounds_);
        const double localY = point.y - CGRectGetMinY(cachedBounds_);
        const double xScale = static_cast<double>(CGImageGetWidth(image_)) / CGRectGetWidth(cachedBounds_);
        const double yScale = static_cast<double>(CGImageGetHeight(image_)) / CGRectGetHeight(cachedBounds_);
        const int pixelX = std::clamp(
            static_cast<int>(std::floor(localX * xScale)),
            0,
            static_cast<int>(CGImageGetWidth(image_)) - 1);
        const int pixelY = std::clamp(
            static_cast<int>(std::floor(localY * yScale)),
            0,
            static_cast<int>(CGImageGetHeight(image_)) - 1);
        return sampleImagePixel(pixelX, pixelY, errorMessage);
    }

private:
    bool captureDisplayIfNeeded(
        CGDirectDisplayID display,
        CGRect bounds,
        std::string* errorMessage)
    {
        const auto now = std::chrono::steady_clock::now();
        if (image_ && display == cachedDisplay_ &&
            CGRectEqualToRect(bounds, cachedBounds_) &&
            now - captureTime_ < std::chrono::milliseconds(16)) {
            return true;
        }

        resetImage();
        // This synchronous CoreGraphics path keeps 10.15 compatibility. Replace it
        // with ScreenCaptureKit when the project raises its macOS deployment target.
        image_ = CGDisplayCreateImage(display);
        if (!image_) {
            if (errorMessage) {
                *errorMessage =
                    "macOS could not capture the requested display. Check Screen Recording permission.";
            }
            return false;
        }

        cachedDisplay_ = display;
        cachedBounds_ = bounds;
        captureTime_ = now;
        return true;
    }

    std::optional<PixelColor> sampleImagePixel(
        int pixelX,
        int pixelY,
        std::string* errorMessage) const
    {
        if (!image_) {
            return std::nullopt;
        }

        CGImageRef pixelImage = CGImageCreateWithImageInRect(
            image_,
            CGRectMake(pixelX, pixelY, 1.0, 1.0));
        if (!pixelImage) {
            if (errorMessage) {
                *errorMessage = "macOS could not isolate the requested display pixel.";
            }
            return std::nullopt;
        }

        std::array<std::uint8_t, 4> rgba{};
        CGColorSpaceRef colorSpace = CGColorSpaceCreateDeviceRGB();
        CGContextRef context = colorSpace
            ? CGBitmapContextCreate(
                rgba.data(),
                1,
                1,
                8,
                rgba.size(),
                colorSpace,
                static_cast<CGBitmapInfo>(
                    static_cast<std::uint32_t>(kCGImageAlphaPremultipliedLast) |
                    static_cast<std::uint32_t>(kCGBitmapByteOrder32Big)))
            : nullptr;
        if (!context) {
            if (colorSpace) {
                CGColorSpaceRelease(colorSpace);
            }
            CGImageRelease(pixelImage);
            if (errorMessage) {
                *errorMessage = "macOS could not create a pixel sampling bitmap context.";
            }
            return std::nullopt;
        }

        CGContextDrawImage(context, CGRectMake(0.0, 0.0, 1.0, 1.0), pixelImage);
        CGContextRelease(context);
        CGColorSpaceRelease(colorSpace);
        CGImageRelease(pixelImage);
        return PixelColor{rgba[0], rgba[1], rgba[2]};
    }

    void resetImage()
    {
        if (image_) {
            CGImageRelease(image_);
        }
        image_ = nullptr;
        cachedDisplay_ = kCGNullDirectDisplay;
        cachedBounds_ = CGRectNull;
        captureTime_ = {};
    }

    std::mutex mutex_;
    CGImageRef image_ = nullptr;
    CGDirectDisplayID cachedDisplay_ = kCGNullDirectDisplay;
    CGRect cachedBounds_ = CGRectNull;
    std::chrono::steady_clock::time_point captureTime_{};
};

MacosScreenPixelSampler& ScreenPixelSampler()
{
    static MacosScreenPixelSampler sampler;
    return sampler;
}

bool AccessibilityAllowsOutput()
{
    return g_outputInitialized.load(std::memory_order_acquire) && IsAccessibilityTrusted();
}

bool PostTaggedEvent(CGEventRef event)
{
    if (!event) {
        return false;
    }
    CGEventSetIntegerValueField(event, kCGEventSourceUserData, kMacosInjectedEventTag);
    CGEventPost(kCGHIDEventTap, event);
    CFRelease(event);
    return true;
}

bool PostKeyboardEvent(PlatformKeyCode key, bool down)
{
    if (!AccessibilityAllowsOutput()) {
        return false;
    }

    const uint16_t macKey = win_vkey_to_macos_key(static_cast<uint8_t>(key));
    if (macKey == MACOS_UNASSIGNED || macKey >= MACOS_MOUSE_X2) {
        return false;
    }
    return PostTaggedEvent(CGEventCreateKeyboardEvent(nullptr, macKey, down));
}

struct MouseButtonDescription {
    CGEventType downType = kCGEventNull;
    CGEventType upType = kCGEventNull;
    CGMouseButton button = kCGMouseButtonLeft;
    int buttonNumber = 0;
};

std::optional<MouseButtonDescription> MouseButtonForMacKey(uint16_t macKey)
{
    switch (macKey) {
    case MACOS_MOUSE_LEFT:
        return MouseButtonDescription{kCGEventLeftMouseDown, kCGEventLeftMouseUp, kCGMouseButtonLeft, 0};
    case MACOS_MOUSE_RIGHT:
        return MouseButtonDescription{kCGEventRightMouseDown, kCGEventRightMouseUp, kCGMouseButtonRight, 1};
    case MACOS_MOUSE_MIDDLE:
        return MouseButtonDescription{kCGEventOtherMouseDown, kCGEventOtherMouseUp, kCGMouseButtonCenter, 2};
    case MACOS_MOUSE_X1:
        return MouseButtonDescription{kCGEventOtherMouseDown, kCGEventOtherMouseUp, static_cast<CGMouseButton>(3), 3};
    case MACOS_MOUSE_X2:
        return MouseButtonDescription{kCGEventOtherMouseDown, kCGEventOtherMouseUp, static_cast<CGMouseButton>(4), 4};
    default:
        return std::nullopt;
    }
}

bool PostMouseButtonEvent(PlatformKeyCode key, bool down)
{
    if (!AccessibilityAllowsOutput()) {
        return false;
    }

    const uint16_t macKey = win_vkey_to_macos_key(static_cast<uint8_t>(key));
    const auto button = MouseButtonForMacKey(macKey);
    const auto cursor = CurrentCursorPoint();
    if (!button || !cursor) {
        return false;
    }

    CGEventRef event = CGEventCreateMouseEvent(
        nullptr,
        down ? button->downType : button->upType,
        *cursor,
        button->button);
    if (!event) {
        return false;
    }
    if (button->downType == kCGEventOtherMouseDown) {
        CGEventSetIntegerValueField(event, kCGMouseEventButtonNumber, button->buttonNumber);
    }
    return PostTaggedEvent(event);
}

bool PostMouseMove(CGPoint target,
                   std::optional<int> deltaX = std::nullopt,
                   std::optional<int> deltaY = std::nullopt)
{
    if (!AccessibilityAllowsOutput()) {
        return false;
    }
    CGEventRef event = CGEventCreateMouseEvent(
        nullptr,
        kCGEventMouseMoved,
        target,
        kCGMouseButtonLeft);
    if (!event) {
        return false;
    }
    if (deltaX) {
        CGEventSetIntegerValueField(event, kCGMouseEventDeltaX, *deltaX);
    }
    if (deltaY) {
        CGEventSetIntegerValueField(event, kCGMouseEventDeltaY, *deltaY);
    }
    return PostTaggedEvent(event);
}

PlatformKeyCode WinVirtualKeyForMacKey(uint16_t macKey)
{
    if (macKey == 0x36) {
        return smu::core::SMU_VK_RWIN;
    }
    return macos_to_win_vkey(macKey);
}

std::optional<PlatformKeyCode> MouseVirtualKeyForEvent(CGEventType type, CGEventRef event)
{
    switch (type) {
    case kCGEventLeftMouseDown:
    case kCGEventLeftMouseUp:
        return smu::core::SMU_VK_LBUTTON;
    case kCGEventRightMouseDown:
    case kCGEventRightMouseUp:
        return smu::core::SMU_VK_RBUTTON;
    case kCGEventOtherMouseDown:
    case kCGEventOtherMouseUp: {
        const int64_t buttonNumber = CGEventGetIntegerValueField(event, kCGMouseEventButtonNumber);
        if (buttonNumber == 2) {
            return smu::core::SMU_VK_MBUTTON;
        }
        if (buttonNumber == 3) {
            return smu::core::SMU_VK_XBUTTON1;
        }
        if (buttonNumber == 4) {
            return smu::core::SMU_VK_XBUTTON2;
        }
        return std::nullopt;
    }
    default:
        return std::nullopt;
    }
}

bool ModifierDownForFlagsChanged(PlatformKeyCode key, CGEventFlags flags)
{
    switch (key) {
    case smu::core::SMU_VK_SHIFT:
    case smu::core::SMU_VK_LSHIFT:
    case smu::core::SMU_VK_RSHIFT:
        return (flags & kCGEventFlagMaskShift) != 0;
    case smu::core::SMU_VK_CONTROL:
    case smu::core::SMU_VK_LCONTROL:
    case smu::core::SMU_VK_RCONTROL:
        return (flags & kCGEventFlagMaskControl) != 0;
    case smu::core::SMU_VK_MENU:
    case smu::core::SMU_VK_LMENU:
    case smu::core::SMU_VK_RMENU:
        return (flags & kCGEventFlagMaskAlternate) != 0;
    case smu::core::SMU_VK_LWIN:
    case smu::core::SMU_VK_RWIN:
        return (flags & kCGEventFlagMaskCommand) != 0;
    case smu::core::SMU_VK_CAPITAL:
        return (flags & kCGEventFlagMaskAlphaShift) != 0;
    default:
        return false;
    }
}

class MacosCGEventInputBackend final : public InputBackend {
public:
    MacosCGEventInputBackend()
    {
        clearKeyStates();
    }

    ~MacosCGEventInputBackend() override
    {
        shutdown();
    }

    bool init(std::string* errorMessage = nullptr) override
    {
        if (errorMessage) {
            errorMessage->clear();
        }

        if (!IsAccessibilityTrusted()) {
            if (errorMessage) {
                *errorMessage =
                    "macOS Accessibility permission is required to send global keyboard/mouse input.";
            }
            return false;
        }

        initialized_.store(true, std::memory_order_release);
        g_outputInitialized.store(true, std::memory_order_release);

        std::string readError;
        if (!startReadLoop(&readError)) {
            initialized_.store(false, std::memory_order_release);
            g_outputInitialized.store(false, std::memory_order_release);
            if (errorMessage) {
                *errorMessage = readError.empty()
                    ? "macOS event tap could not start for global input reading."
                    : readError;
            }
            LogWarning(errorMessage && !errorMessage->empty()
                    ? *errorMessage
                    : "macOS event tap could not start for global input reading.",
                kMacosEventTapInitWarningId,
                true);
            return false;
        }
        return true;
    }

    void shutdown() override
    {
        initialized_.store(false, std::memory_order_release);
        g_outputInitialized.store(false, std::memory_order_release);
        stopReadLoop();
        clearKeyStates();
        Globals::g_isVk_BunnyhopHeldDown.store(false, std::memory_order_release);
    }

    bool isKeyPressed(PlatformKeyCode key) const override
    {
        if (!readInitialized_.load(std::memory_order_acquire) || key >= keyStates_.size()) {
            return false;
        }
        return keyStates_[key].load(std::memory_order_acquire);
    }

    void holdKey(PlatformKeyCode key, bool) override
    {
        if (PostMouseButtonEvent(key, true) || PostKeyboardEvent(key, true)) {
            return;
        }
        if (initialized_.load(std::memory_order_acquire) && key < smu::core::SMU_VK_MOUSE_WHEEL_UP) {
            LogWarning("macOS input backend cannot inject virtual key " + KeyName(key) + ".");
        }
    }

    void releaseKey(PlatformKeyCode key, bool) override
    {
        if (PostMouseButtonEvent(key, false) || PostKeyboardEvent(key, false)) {
            return;
        }
        if (initialized_.load(std::memory_order_acquire) && key < smu::core::SMU_VK_MOUSE_WHEEL_UP) {
            LogWarning("macOS input backend cannot release virtual key " + KeyName(key) + ".");
        }
    }

    void pressKey(PlatformKeyCode key, int delayMs) override
    {
        holdKey(key, false);
        std::this_thread::sleep_for(std::chrono::milliseconds(std::max(delayMs, 0)));
        releaseKey(key, false);
    }

    void holdKeyChord(PlatformKeyCode combinedKey) override
    {
        const PlatformKeyCode key = combinedKey & smu::core::HOTKEY_KEY_MASK;
        if (key == kMouseWheelUp || key == kMouseWheelDown) {
            mouseWheel(key == kMouseWheelUp ? 1 : -1);
            return;
        }

        if ((combinedKey & smu::core::HOTKEY_MASK_WIN) != 0) holdKey(smu::core::SMU_VK_LWIN, false);
        if ((combinedKey & smu::core::HOTKEY_MASK_CTRL) != 0) holdKey(smu::core::SMU_VK_CONTROL, false);
        if ((combinedKey & smu::core::HOTKEY_MASK_ALT) != 0) holdKey(smu::core::SMU_VK_MENU, false);
        if ((combinedKey & smu::core::HOTKEY_MASK_SHIFT) != 0) holdKey(smu::core::SMU_VK_SHIFT, false);
        if (key != kNoKey) holdKey(key, false);
    }

    void releaseKeyChord(PlatformKeyCode combinedKey) override
    {
        const PlatformKeyCode key = combinedKey & smu::core::HOTKEY_KEY_MASK;
        if (key != kNoKey && key != kMouseWheelUp && key != kMouseWheelDown) releaseKey(key, false);
        if ((combinedKey & smu::core::HOTKEY_MASK_SHIFT) != 0) releaseKey(smu::core::SMU_VK_SHIFT, false);
        if ((combinedKey & smu::core::HOTKEY_MASK_ALT) != 0) releaseKey(smu::core::SMU_VK_MENU, false);
        if ((combinedKey & smu::core::HOTKEY_MASK_CTRL) != 0) releaseKey(smu::core::SMU_VK_CONTROL, false);
        if ((combinedKey & smu::core::HOTKEY_MASK_WIN) != 0) releaseKey(smu::core::SMU_VK_LWIN, false);
    }

    void moveMouse(int dx, int dy) override
    {
        moveMouseRaw(dx, dy);
    }

    void moveMouseRaw(int dx, int dy) override
    {
        const auto cursor = CurrentCursorPoint();
        if (!cursor) {
            return;
        }
        if (Globals::macos_cursor_movement) {
            PostMouseMove(CGPointMake(cursor->x + dx, cursor->y + dy));
        } else {
            PostMouseMove(*cursor, dx, dy);
        }
    }

    bool moveMouseAbsolute(int x, int y, std::string* errorMessage = nullptr) override
    {
        if (errorMessage) {
            errorMessage->clear();
        }
        if (!AccessibilityAllowsOutput()) {
            if (errorMessage) {
                *errorMessage =
                    "macOS Accessibility permission is required to move the global cursor.";
            }
            return false;
        }
        if (!PostMouseMove(CGPointMake(x, y))) {
            if (errorMessage) {
                *errorMessage = "macOS failed to create a global absolute mouse movement event.";
            }
            return false;
        }
        return true;
    }

    void mouseWheel(int delta) override
    {
        if (!AccessibilityAllowsOutput() || delta == 0) {
            return;
        }
        PostTaggedEvent(CGEventCreateScrollWheelEvent(
            nullptr,
            kCGScrollEventUnitLine,
            1,
            delta));
    }

    std::optional<CursorPosition> getCursorPosition() const override
    {
        const auto point = CurrentCursorPoint();
        if (!point) {
            return std::nullopt;
        }
        return CursorPosition{
            static_cast<int>(std::llround(point->x)),
            static_cast<int>(std::llround(point->y)),
        };
    }

    std::optional<ScreenBounds> getScreenBounds() const override
    {
        return GlobalScreenBounds();
    }

    std::optional<ScreenBounds> getActiveMonitorBounds() const override
    {
        return ActiveMonitorBounds();
    }

    std::optional<int> getActiveMonitorRefreshRateHz() const override
    {
        return RefreshRateForActiveMonitor();
    }

    std::string absolutePointerUnavailableReason() const override
    {
        if (!IsAccessibilityTrusted()) {
            return "macOS Accessibility permission is required for absolute cursor movement.";
        }
        return "macOS could not resolve the current cursor or display bounds.";
    }

    std::optional<PixelColor> getPixelColor(int x, int y, std::string* errorMessage = nullptr) const override
    {
        return ScreenPixelSampler().sample(x, y, errorMessage);
    }

    std::string screenReadUnavailableReason() const override
    {
        if (!HasScreenRecordingPermission()) {
            return ScreenRecordingPermissionReason();
        }
        return "macOS could not read screen pixels in the current desktop session.";
    }

    std::optional<PlatformKeyCode> getCurrentPressedKey() const override
    {
        if (!readInitialized_.load(std::memory_order_acquire)) {
            return std::nullopt;
        }
        for (PlatformKeyCode key = 1; key < keyStates_.size(); ++key) {
            if (keyStates_[key].load(std::memory_order_acquire)) {
                return key;
            }
        }
        return std::nullopt;
    }

    std::string formatKeyName(PlatformKeyCode key) const override
    {
        return KeyName(key);
    }

private:
    static CGEventRef EventTapCallback(
        CGEventTapProxy,
        CGEventType type,
        CGEventRef event,
        void* userInfo)
    {
        auto* backend = static_cast<MacosCGEventInputBackend*>(userInfo);
        return backend ? backend->handleEventTap(type, event) : event;
    }

    CGEventRef handleEventTap(CGEventType type, CGEventRef event)
    {
        if (type == kCGEventTapDisabledByTimeout || type == kCGEventTapDisabledByUserInput) {
            if (eventTap_) {
                CGEventTapEnable(eventTap_, true);
                LogWarning("macOS global input event tap was disabled and has been re-enabled.",
                    kMacosEventTapDisabledWarningId,
                    false);
            }
            return event;
        }

        if (!event ||
            CGEventGetIntegerValueField(event, kCGEventSourceUserData) == kMacosInjectedEventTag) {
            return event;
        }

        switch (type) {
        case kCGEventKeyDown:
        case kCGEventKeyUp: {
            const auto macKey = static_cast<uint16_t>(
                CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode));
            setKeyState(WinVirtualKeyForMacKey(macKey), type == kCGEventKeyDown);
            break;
        }
        case kCGEventFlagsChanged: {
            const auto macKey = static_cast<uint16_t>(
                CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode));
            const PlatformKeyCode key = WinVirtualKeyForMacKey(macKey);
            setKeyState(key, ModifierDownForFlagsChanged(key, CGEventGetFlags(event)));
            break;
        }
        case kCGEventLeftMouseDown:
        case kCGEventRightMouseDown:
        case kCGEventOtherMouseDown:
        case kCGEventLeftMouseUp:
        case kCGEventRightMouseUp:
        case kCGEventOtherMouseUp:
            if (const auto key = MouseVirtualKeyForEvent(type, event)) {
                setKeyState(*key,
                    type == kCGEventLeftMouseDown ||
                    type == kCGEventRightMouseDown ||
                    type == kCGEventOtherMouseDown);
            }
            break;
        default:
            break;
        }
        return event;
    }

    void setKeyState(PlatformKeyCode key, bool down)
    {
        if (key == VK_UNASSIGNED || key >= keyStates_.size()) {
            return;
        }
        keyStates_[key].store(down, std::memory_order_release);

        const PlatformKeyCode bunnyhopKey = Globals::vk_bunnyhopkey & Globals::HOTKEY_KEY_MASK;
        if (key == bunnyhopKey) {
            Globals::g_isVk_BunnyhopHeldDown.store(down, std::memory_order_release);
        }
    }

    bool startReadLoop(std::string* errorMessage)
    {
        if (readInitialized_.load(std::memory_order_acquire)) {
            return true;
        }
        if (runLoopThread_.joinable()) {
            return false;
        }

        std::promise<bool> started;
        std::future<bool> startedFuture = started.get_future();
        running_.store(true, std::memory_order_release);
        runLoopThread_ = std::thread(
            [this, started = std::move(started)]() mutable {
                eventTapRunLoop(std::move(started));
            });

        if (!startedFuture.get()) {
            running_.store(false, std::memory_order_release);
            if (runLoopThread_.joinable()) {
                runLoopThread_.join();
            }
            if (errorMessage && errorMessage->empty()) {
                *errorMessage =
                    "macOS could not create the global input event tap. Check Accessibility permission, then restart SMU.";
            }
            return false;
        }
        return true;
    }

    void stopReadLoop()
    {
        running_.store(false, std::memory_order_release);
        {
            std::lock_guard<std::mutex> lock(runLoopMutex_);
            if (runLoop_) {
                CFRunLoopStop(runLoop_);
                CFRunLoopWakeUp(runLoop_);
            }
        }
        if (runLoopThread_.joinable()) {
            runLoopThread_.join();
        }
        readInitialized_.store(false, std::memory_order_release);
        g_readLoopInitialized.store(false, std::memory_order_release);
    }

    void eventTapRunLoop(std::promise<bool> started)
    {
        const CGEventMask mask =
            CGEventMaskBit(kCGEventKeyDown) |
            CGEventMaskBit(kCGEventKeyUp) |
            CGEventMaskBit(kCGEventFlagsChanged) |
            CGEventMaskBit(kCGEventLeftMouseDown) |
            CGEventMaskBit(kCGEventLeftMouseUp) |
            CGEventMaskBit(kCGEventRightMouseDown) |
            CGEventMaskBit(kCGEventRightMouseUp) |
            CGEventMaskBit(kCGEventOtherMouseDown) |
            CGEventMaskBit(kCGEventOtherMouseUp);

        CFMachPortRef eventTap = CGEventTapCreate(
            kCGSessionEventTap,
            kCGHeadInsertEventTap,
            kCGEventTapOptionListenOnly,
            mask,
            EventTapCallback,
            this);
        if (!eventTap) {
            started.set_value(false);
            return;
        }

        CFRunLoopSourceRef source = CFMachPortCreateRunLoopSource(kCFAllocatorDefault, eventTap, 0);
        if (!source) {
            CFRelease(eventTap);
            started.set_value(false);
            return;
        }

        {
            std::lock_guard<std::mutex> lock(runLoopMutex_);
            eventTap_ = eventTap;
            runLoopSource_ = source;
            runLoop_ = CFRunLoopGetCurrent();
            CFRetain(runLoop_);
        }

        CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
        CGEventTapEnable(eventTap, true);
        readInitialized_.store(true, std::memory_order_release);
        g_readLoopInitialized.store(true, std::memory_order_release);
        LogInfo("macOS global input event tap started.");
        started.set_value(true);

        CFRunLoopRun();

        readInitialized_.store(false, std::memory_order_release);
        g_readLoopInitialized.store(false, std::memory_order_release);
        CFRunLoopRemoveSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
        {
            std::lock_guard<std::mutex> lock(runLoopMutex_);
            if (runLoop_) {
                CFRelease(runLoop_);
            }
            runLoop_ = nullptr;
            runLoopSource_ = nullptr;
            eventTap_ = nullptr;
        }
        CFRelease(source);
        CFRelease(eventTap);
    }

    void clearKeyStates()
    {
        for (auto& state : keyStates_) {
            state.store(false, std::memory_order_relaxed);
        }
    }

    std::atomic_bool running_{false};
    std::atomic_bool initialized_{false};
    std::atomic_bool readInitialized_{false};
    std::array<std::atomic_bool, 258> keyStates_{};
    std::thread runLoopThread_;
    std::mutex runLoopMutex_;
    CFRunLoopRef runLoop_ = nullptr;
    CFMachPortRef eventTap_ = nullptr;
    CFRunLoopSourceRef runLoopSource_ = nullptr;
};

} // namespace

std::shared_ptr<smu::platform::InputBackend> CreateMacosInputBackend()
{
    return std::make_shared<MacosCGEventInputBackend>();
}

bool IsMacosInputOutputInitialized()
{
    return g_outputInitialized.load(std::memory_order_acquire);
}

bool IsMacosInputReadLoopInitialized()
{
    return g_readLoopInitialized.load(std::memory_order_acquire);
}

} // namespace smu::platform::macos

#endif
