#pragma once

#include <cstdint>

namespace smu::platform {

using PlatformPid = std::uint32_t;
using PlatformKeyCode = std::uint32_t;

struct PlatformProcessHandle {
    std::uintptr_t native = 0;

    constexpr bool valid() const noexcept
    {
        return native != 0;
    }
};

struct PlatformWindowHandle {
    std::uintptr_t native = 0;

    constexpr bool valid() const noexcept
    {
        return native != 0;
    }
};

struct CursorPosition {
    int x = 0;
    int y = 0;
};

struct ScreenBounds {
    int x = 0;
    int y = 0;
    int width = 0;
    int height = 0;

    constexpr bool valid() const noexcept
    {
        return width > 0 && height > 0;
    }
};

struct PixelColor {
    std::uint8_t r = 0;
    std::uint8_t g = 0;
    std::uint8_t b = 0;
};

constexpr PlatformKeyCode kNoKey = 0;
constexpr PlatformKeyCode kMouseWheelUp = 256;
constexpr PlatformKeyCode kMouseWheelDown = 257;

} // namespace smu::platform
