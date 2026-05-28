#pragma once

#include "../process_backend.h"

#include <memory>

namespace smu::platform::macos {

std::shared_ptr<smu::platform::ProcessBackend> CreateMacosProcessBackend();

} // namespace smu::platform::macos
