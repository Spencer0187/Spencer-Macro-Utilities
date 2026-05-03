#pragma once

#include "../input_backend.h"

#include <memory>

namespace smu::platform::windows {

std::shared_ptr<smu::platform::InputBackend> CreateWindowsInputBackend();

} // namespace smu::platform::windows
