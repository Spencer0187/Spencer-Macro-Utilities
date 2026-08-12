#pragma once

#include "platform_types.h"

#include <string_view>

namespace smu::platform {

class InputBackend;

struct KeyAction {
    PlatformKeyCode key = kNoKey;
    PlatformKeyCode scanCode = kNoKey;
    bool needsShift = false;
    bool valid = false;
};

KeyAction CharToKeyAction_Compat(char c);
KeyAction CharToKeyAction_Global(char c);
KeyAction charToKeyAction(char c);

bool typeText(InputBackend& input, std::string_view text, int delayMs);
bool pasteText(std::string_view text, int delayMs, bool useUnicode = false);
bool pressCharacter(char character, int delayMs = 50);

} // namespace smu::platform
