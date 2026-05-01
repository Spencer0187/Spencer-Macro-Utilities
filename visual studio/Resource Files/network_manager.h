#pragma once
#include <windows.h>
#include <atomic>
#include <memory>
#include <set>
#include <string>
#include <shared_mutex>
#include <chrono>
#include "windivert-files/windivert.h"

// Required for logzz
#include "Resource Files/json.hpp"
#include "Resource Files/logzz.hpp"

// --- Functions ---
bool TryLoadWinDivert();
void WindivertWorkerThread();
void RobloxLogScannerThread();
void SafeCloseWinDivert();
void DelaySenderWorker();

namespace smu::platform {
class NetworkLagBackend;
}

std::shared_ptr<smu::platform::NetworkLagBackend> CreateWinDivertNetworkLagBackend();

static const wchar_t *GetWinDivertErrorDescription(DWORD errorCode);
static bool ExtractResource(int resourceId, const char* resourceType, const std::string& outputFilename);
