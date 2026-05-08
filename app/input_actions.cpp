#include "input_actions.h"

#include "../platform/input_backend.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <limits>
#include <optional>
#include <string>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#endif

namespace smu::app {
namespace {

struct ScaledModeBase {
    double width = 0.0;
    double height = 0.0;
};

std::optional<ScaledModeBase> GetScaledModeBase(const std::string& mode)
{
    if (mode == "scaled720p") {
        return ScaledModeBase{1280.0, 720.0};
    }
    if (mode == "scaled1080p") {
        return ScaledModeBase{1920.0, 1080.0};
    }
    if (mode == "scaled1440p") {
        return ScaledModeBase{2560.0, 1440.0};
    }
    if (mode == "scaled2160p") {
        return ScaledModeBase{3840.0, 2160.0};
    }
    return std::nullopt;
}

bool RoundToInt(double value, int& result)
{
    if (!std::isfinite(value)) {
        return false;
    }
    const double rounded = std::round(value);
    if (rounded < static_cast<double>(std::numeric_limits<int>::min()) ||
        rounded > static_cast<double>(std::numeric_limits<int>::max())) {
        return false;
    }
    result = static_cast<int>(rounded);
    return true;
}

void SetError(std::string* errorMessage, const char* message)
{
    if (errorMessage) {
        *errorMessage = message;
    }
}

void SetError(std::string* errorMessage, const std::string& message)
{
    if (errorMessage) {
        *errorMessage = message;
    }
}

bool ResolveAbsolutePoint(const smu::platform::ScreenBounds& bounds,
                          double x,
                          double y,
                          const std::string& mode,
                          int& targetX,
                          int& targetY,
                          std::string* errorMessage)
{
    if (!bounds.valid()) {
        SetError(errorMessage, "active monitor bounds are invalid");
        return false;
    }

    double targetXValue = 0.0;
    double targetYValue = 0.0;

    if (mode == "pixels") {
        targetXValue = static_cast<double>(bounds.x) + x;
        targetYValue = static_cast<double>(bounds.y) + y;
    } else if (mode == "percent") {
        if (x < 0.0 || x > 100.0 || y < 0.0 || y > 100.0) {
            SetError(errorMessage, "percent mode requires x and y to be in the range 0 through 100");
            return false;
        }
        targetXValue = static_cast<double>(bounds.x) + (x / 100.0) * static_cast<double>(bounds.width - 1);
        targetYValue = static_cast<double>(bounds.y) + (y / 100.0) * static_cast<double>(bounds.height - 1);
    } else if (const auto scaledBase = GetScaledModeBase(mode)) {
        targetXValue = static_cast<double>(bounds.x) + x * static_cast<double>(bounds.width) / scaledBase->width;
        targetYValue = static_cast<double>(bounds.y) + y * static_cast<double>(bounds.height) / scaledBase->height;
    } else {
        SetError(errorMessage, "mode must be one of: pixels, percent, scaled720p, scaled1080p, scaled1440p, scaled2160p");
        return false;
    }

    if (!RoundToInt(targetXValue, targetX) || !RoundToInt(targetYValue, targetY)) {
        SetError(errorMessage, "target coordinate is outside the supported integer range");
        return false;
    }

    const int minX = bounds.x;
    const int minY = bounds.y;
    const int maxX = bounds.x + bounds.width - 1;
    const int maxY = bounds.y + bounds.height - 1;
    targetX = std::clamp(targetX, minX, maxX);
    targetY = std::clamp(targetY, minY, maxY);
    return true;
}

std::optional<smu::platform::ScreenBounds> GetActiveMonitorBounds(smu::platform::InputBackend& backend,
                                                                   std::string* errorMessage)
{
    const auto bounds = backend.getActiveMonitorBounds();
    if (!bounds || !bounds->valid()) {
        const std::string reason = backend.absolutePointerUnavailableReason();
        SetError(errorMessage, reason.empty() ? "active monitor bounds are not available on this platform/session" : reason);
        return std::nullopt;
    }
    return bounds;
}

std::string FormatColorHex(const smu::platform::PixelColor& color)
{
    char buffer[8] = {};
    std::snprintf(buffer, sizeof(buffer), "#%02X%02X%02X", static_cast<unsigned int>(color.r), static_cast<unsigned int>(color.g), static_cast<unsigned int>(color.b));
    return std::string(buffer);
}

} // namespace

bool IsKeyPressed(core::KeyCode key)
{
    if (auto backend = platform::GetInputBackend()) {
        return backend->isKeyPressed(key);
    }
    return false;
}

void HoldKey(core::KeyCode key, bool extended)
{
    if (auto backend = platform::GetInputBackend()) {
        backend->holdKey(key, extended);
    }
}

void ReleaseKey(core::KeyCode key, bool extended)
{
    if (auto backend = platform::GetInputBackend()) {
        backend->releaseKey(key, extended);
    }
}

void PressKey(core::KeyCode key, int delayMs)
{
    if (auto backend = platform::GetInputBackend()) {
        backend->pressKey(key, delayMs);
    }
}

void HoldKeyBinded(core::KeyCode combinedKey)
{
    if (auto backend = platform::GetInputBackend()) {
        backend->holdKeyChord(combinedKey);
    }
}

void ReleaseKeyBinded(core::KeyCode combinedKey)
{
    if (auto backend = platform::GetInputBackend()) {
        backend->releaseKeyChord(combinedKey);
    }
}

void MoveMouse(int dx, int dy)
{
    if (auto backend = platform::GetInputBackend()) {
        backend->moveMouse(dx, dy);
    }
}

bool MoveMouseAbsoluteDelta(int dx, int dy, std::string* errorMessage)
{
    if (errorMessage) {
        errorMessage->clear();
    }

    auto backend = platform::GetInputBackend();
    if (!backend) {
        SetError(errorMessage, "input backend is not available");
        return false;
    }

    const auto cursor = backend->getCursorPosition();
    if (!cursor) {
        const std::string reason = backend->absolutePointerUnavailableReason();
        SetError(errorMessage, reason.empty() ? "cursor position is not available on this platform/session" : reason);
        return false;
    }

    const long long targetX = static_cast<long long>(cursor->x) + static_cast<long long>(dx);
    const long long targetY = static_cast<long long>(cursor->y) + static_cast<long long>(dy);
    if (targetX < static_cast<long long>(std::numeric_limits<int>::min()) ||
        targetX > static_cast<long long>(std::numeric_limits<int>::max()) ||
        targetY < static_cast<long long>(std::numeric_limits<int>::min()) ||
        targetY > static_cast<long long>(std::numeric_limits<int>::max())) {
        SetError(errorMessage, "absolute movement target is outside the supported integer range");
        return false;
    }

    std::string backendError;
    if (!backend->moveMouseAbsolute(static_cast<int>(targetX), static_cast<int>(targetY), &backendError)) {
        SetError(errorMessage, backendError.empty() ? backend->absolutePointerUnavailableReason() : backendError);
        return false;
    }
    return true;
}

bool MoveMouseAbs(double x, double y, const std::string& mode, bool useAbsoluteMotion, std::string* errorMessage)
{
    if (errorMessage) {
        errorMessage->clear();
    }

    auto backend = platform::GetInputBackend();
    if (!backend) {
        SetError(errorMessage, "input backend is not available");
        return false;
    }

    const auto bounds = GetActiveMonitorBounds(*backend, errorMessage);
    if (!bounds) {
        return false;
    }

    const auto cursor = backend->getCursorPosition();
    if (!cursor) {
        const std::string reason = backend->absolutePointerUnavailableReason();
        SetError(errorMessage, reason.empty() ? "cursor position is not available on this platform/session" : reason);
        return false;
    }

    int targetX = 0;
    int targetY = 0;
    if (!ResolveAbsolutePoint(*bounds, x, y, mode, targetX, targetY, errorMessage)) {
        return false;
    }

    if (useAbsoluteMotion) {
        std::string backendError;
        if (!backend->moveMouseAbsolute(targetX, targetY, &backendError)) {
            SetError(errorMessage, backendError.empty() ? backend->absolutePointerUnavailableReason() : backendError);
            return false;
        }
    } else {
        const long long deltaX = static_cast<long long>(targetX) - static_cast<long long>(cursor->x);
        const long long deltaY = static_cast<long long>(targetY) - static_cast<long long>(cursor->y);
        if (deltaX < static_cast<long long>(std::numeric_limits<int>::min()) ||
            deltaX > static_cast<long long>(std::numeric_limits<int>::max()) ||
            deltaY < static_cast<long long>(std::numeric_limits<int>::min()) ||
            deltaY > static_cast<long long>(std::numeric_limits<int>::max())) {
            SetError(errorMessage, "relative movement delta is outside the supported integer range");
            return false;
        }
        backend->moveMouse(static_cast<int>(deltaX), static_cast<int>(deltaY));
    }
    return true;
}

std::optional<smu::platform::PixelColor> GetPixelColor(double x, double y, const std::string& mode, std::string* errorMessage)
{
    if (errorMessage) {
        errorMessage->clear();
    }

    auto backend = platform::GetInputBackend();
    if (!backend) {
        SetError(errorMessage, "input backend is not available");
        return std::nullopt;
    }

    const auto bounds = GetActiveMonitorBounds(*backend, errorMessage);
    if (!bounds) {
        return std::nullopt;
    }

    int targetX = 0;
    int targetY = 0;
    if (!ResolveAbsolutePoint(*bounds, x, y, mode, targetX, targetY, errorMessage)) {
        return std::nullopt;
    }

    std::string sampleError;
    const auto color = backend->getPixelColor(targetX, targetY, &sampleError);
    if (!color) {
        if (sampleError.empty()) {
            sampleError = backend->screenReadUnavailableReason();
        }
        if (sampleError.empty()) {
            sampleError = "screen pixel color sampling failed";
        }
        SetError(errorMessage, sampleError);
        return std::nullopt;
    }

    return color;
}

std::optional<std::vector<std::vector<smu::platform::PixelColor>>> GetPixelRect(
    double x1,
    double y1,
    double x2,
    double y2,
    const std::string& mode,
    std::string* errorMessage)
{
    if (errorMessage) {
        errorMessage->clear();
    }

    auto backend = platform::GetInputBackend();
    if (!backend) {
        SetError(errorMessage, "input backend is not available");
        return std::nullopt;
    }

    const auto bounds = GetActiveMonitorBounds(*backend, errorMessage);
    if (!bounds) {
        return std::nullopt;
    }

    int left = 0;
    int top = 0;
    int right = 0;
    int bottom = 0;
    if (!ResolveAbsolutePoint(*bounds, x1, y1, mode, left, top, errorMessage)) {
        return std::nullopt;
    }
    if (!ResolveAbsolutePoint(*bounds, x2, y2, mode, right, bottom, errorMessage)) {
        return std::nullopt;
    }

    if (left > right) {
        std::swap(left, right);
    }
    if (top > bottom) {
        std::swap(top, bottom);
    }

    const int width = std::max(1, right - left + 1);
    const int height = std::max(1, bottom - top + 1);

    std::vector<std::vector<smu::platform::PixelColor>> pixels;
    pixels.reserve(static_cast<std::size_t>(height));
    for (int row = 0; row < height; ++row) {
        std::vector<smu::platform::PixelColor> values;
        values.reserve(static_cast<std::size_t>(width));
        for (int col = 0; col < width; ++col) {
            std::string sampleError;
            const auto color = backend->getPixelColor(left + col, top + row, &sampleError);
            if (!color) {
                if (sampleError.empty()) {
                    sampleError = backend->screenReadUnavailableReason();
                }
                if (sampleError.empty()) {
                    sampleError = "screen pixel color sampling failed";
                }
                SetError(errorMessage, sampleError);
                return std::nullopt;
            }
            values.push_back(*color);
        }
        pixels.push_back(std::move(values));
    }

    return pixels;
}

void MouseWheel(int delta)
{
    if (auto backend = platform::GetInputBackend()) {
        backend->mouseWheel(delta);
    }
}

} // namespace smu::app
