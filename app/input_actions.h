#pragma once

#include "../core/key_codes.h"

#include <optional>
#include <string>

namespace smu::app {

bool IsKeyPressed(core::KeyCode key);
void HoldKey(core::KeyCode key, bool extended = false);
void ReleaseKey(core::KeyCode key, bool extended = false);
void PressKey(core::KeyCode key, int delayMs = 50);
void HoldKeyBinded(core::KeyCode combinedKey);
void ReleaseKeyBinded(core::KeyCode combinedKey);
void MoveMouse(int dx, int dy);
bool MoveMouseAbs(double x, double y, const std::string& mode, std::string* errorMessage = nullptr);
std::optional<std::string> GetPixelColorHex(double x, double y, const std::string& mode, std::string* errorMessage = nullptr);
void MouseWheel(int delta);

} // namespace smu::app
