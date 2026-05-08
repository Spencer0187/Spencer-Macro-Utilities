#pragma once

#include "../core/key_codes.h"
#include "../platform/platform_types.h"

#include <optional>
#include <vector>
#include <string>

namespace smu::app {

bool IsKeyPressed(core::KeyCode key);
void HoldKey(core::KeyCode key, bool extended = false);
void ReleaseKey(core::KeyCode key, bool extended = false);
void PressKey(core::KeyCode key, int delayMs = 50);
void HoldKeyBinded(core::KeyCode combinedKey);
void ReleaseKeyBinded(core::KeyCode combinedKey);
void MoveMouse(int dx, int dy);
bool MoveMouseAbsoluteDelta(int dx, int dy, std::string* errorMessage = nullptr);
bool MoveMouseAbs(double x, double y, const std::string& mode, bool useAbsoluteMotion, std::string* errorMessage = nullptr);
std::optional<smu::platform::PixelColor> GetPixelColor(double x, double y, const std::string& mode, std::string* errorMessage = nullptr);
std::optional<std::vector<std::vector<smu::platform::PixelColor>>> GetPixelRect(
    double x1,
    double y1,
    double x2,
    double y2,
    const std::string& mode,
    std::string* errorMessage = nullptr);
void MouseWheel(int delta);

} // namespace smu::app
