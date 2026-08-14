#pragma once

#if defined(_WIN32)

#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include <atomic>
#include <memory>
#include <set>
#include <string>
#include <shared_mutex>
#include <chrono>

#include "../network_backend.h"

namespace smu::platform::windows {

std::shared_ptr<smu::platform::NetworkLagBackend> CreateWinDivertNetworkLagBackend();

} // namespace smu::platform::windows

#endif
