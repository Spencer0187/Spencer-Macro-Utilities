#pragma once

#include <string>

namespace smu::app {

#if defined(__linux__)
bool StartLinuxProcessHelperWithGraphicalPkexec(std::string* errorMessage);
#endif

} // namespace smu::app
