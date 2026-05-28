#pragma once

#include "../input_backend.h"

#include <memory>

namespace smu::platform::macos {

std::shared_ptr<smu::platform::InputBackend> CreateMacosInputBackend();
bool IsMacosInputOutputInitialized();
bool IsMacosInputReadLoopInitialized();

} // namespace smu::platform::macos
