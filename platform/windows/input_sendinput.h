#pragma once

#include "../input_backend.h"

#include <memory>
#include <string_view>

namespace smu::platform::windows {

std::shared_ptr<smu::platform::InputBackend> CreateWindowsInputBackend();
bool TypeUnicodeText(std::string_view text, int delayMs);
bool PressCharacter(char character, int delayMs);

} // namespace smu::platform::windows
