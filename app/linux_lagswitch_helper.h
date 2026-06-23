#pragma once

#include <string>

namespace smu::app {

#if defined(__linux__)
bool StartLinuxNetworkHelperWithGraphicalPkexec(std::string* errorMessage = nullptr);
#endif

} // namespace smu::app
