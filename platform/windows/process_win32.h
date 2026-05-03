#pragma once

#include "../process_backend.h"

#include <memory>

namespace smu::platform::windows {

std::shared_ptr<smu::platform::ProcessBackend> CreateWindowsProcessBackend();

} // namespace smu::platform::windows
