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

constexpr PlatformKeyCode kNoKey = 0;
constexpr PlatformKeyCode kMouseWheelUp = 256;
constexpr PlatformKeyCode kMouseWheelDown = 257;

} // namespace smu::platform
