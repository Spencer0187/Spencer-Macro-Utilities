#pragma once

#include "../core/key_codes.h"

namespace smu::app {

bool IsKeyPressed(core::KeyCode key);
void HoldKey(core::KeyCode key, bool extended = false);
void ReleaseKey(core::KeyCode key, bool extended = false);
void PressKey(core::KeyCode key, int delayMs = 50);
void HoldKeyBinded(core::KeyCode combinedKey);
void ReleaseKeyBinded(core::KeyCode combinedKey);
void MoveMouse(int dx, int dy);
void MouseWheel(int delta);

} // namespace smu::app
