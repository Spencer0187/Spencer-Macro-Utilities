#include "network_windivert.h"

#if defined(_WIN32)

#include "../../core/legacy_globals.h"
#include "../process_backend.h"
#include "RoLogParser.h"
#include "resource.h"

#include <iphlpapi.h>
#include <iprtrmib.h>
#include <mutex>
#include <iostream>
#include <fstream>
#include <filesystem>
#include <string> 
#include <shlobj.h>
#include <atomic>
#include <memory>
#include <set>

// NEW INCLUDES FOR LAG QUEUE
#include <queue>
#include <vector>
#include <condition_variable>
#include <chrono>
#include <thread>
#include <vector>

using namespace Globals;

namespace smu::platform::windows {
namespace {

std::atomic<bool> g_is_using_rcc = false;
std::mutex g_lag_config_mutex;
smu::platform::LagSwitchConfig g_base_lag_config;
smu::platform::LagSwitchConfig g_script_lag_config;
std::uintptr_t g_script_config_owner = 0;
std::uintptr_t g_script_blocking_owner = 0;
bool g_has_script_lag_config = false;
bool g_base_lag_blocking = false;
bool g_script_lag_blocking = false;
std::mutex g_process_udp_ports_mutex;
std::set<int> g_process_udp_ports;
#ifdef AF_INET6
constexpr ULONG kAddressFamilyInet6 = AF_INET6;
#else
constexpr ULONG kAddressFamilyInet6 = 23;
#endif

struct Udp6RowOwnerPid {
    UCHAR localAddr[16];
    DWORD localScopeId;
    DWORD localPort;
    DWORD owningPid;
};

struct Udp6TableOwnerPid {
    DWORD numEntries;
    Udp6RowOwnerPid table[1];
};

// DELAY / LAG SYSTEM DEFINITIONS

struct DelayedPacket {
    std::vector<char> data;
    UINT len;
    WINDIVERT_ADDRESS addr;
    std::chrono::steady_clock::time_point send_time;
};

// Queue to hold packets waiting to be sent
std::deque<DelayedPacket> g_delayed_packet_queue;
std::mutex g_delay_queue_mutex;
std::condition_variable g_delay_cv;
std::atomic<bool> g_delay_thread_running = false;

void SafeCloseWinDivert()
{
	std::lock_guard<std::mutex> lock(g_windivert_handle_mutex);
	if (hWindivert != INVALID_HANDLE_VALUE && pWinDivertClose) {
		pWinDivertClose(hWindivert);
		hWindivert = INVALID_HANDLE_VALUE;
	}
}

bool LagSwitchTargetUsesRoblox(const smu::platform::LagSwitchConfig& config)
{
    return config.targetMode == smu::platform::LagSwitchTargetMode::Roblox ||
        (config.targetMode == smu::platform::LagSwitchTargetMode::Custom && config.includeRobloxDynamicIps);
}

int UdpTablePortToHostOrder(DWORD networkOrderPort)
{
    const DWORD port = networkOrderPort & 0xFFFF;
    return static_cast<int>(((port & 0x00FF) << 8) | ((port & 0xFF00) >> 8));
}

std::vector<smu::platform::PlatformPid> CurrentTargetProcessIds()
{
    auto processBackend = smu::platform::GetProcessBackend();
    if (!processBackend || settingsBuffer[0] == '\0') {
        return {};
    }

    if (takeallprocessids) {
        return processBackend->findAllProcesses(settingsBuffer);
    }

    if (auto pid = processBackend->findMainProcess(settingsBuffer)) {
        return {*pid};
    }
    return {};
}

void CollectUdp4PortsForPids(const std::set<DWORD>& targetPids, std::set<int>& ports)
{
    ULONG bufferSize = 0;
    DWORD result = GetExtendedUdpTable(nullptr, &bufferSize, FALSE, AF_INET, UDP_TABLE_OWNER_PID, 0);
    if (result != ERROR_INSUFFICIENT_BUFFER || bufferSize == 0) {
        return;
    }

    std::vector<unsigned char> buffer(bufferSize);
    auto* table = reinterpret_cast<PMIB_UDPTABLE_OWNER_PID>(buffer.data());
    result = GetExtendedUdpTable(table, &bufferSize, FALSE, AF_INET, UDP_TABLE_OWNER_PID, 0);
    if (result != NO_ERROR) {
        return;
    }

    for (DWORD index = 0; index < table->dwNumEntries; ++index) {
        const MIB_UDPROW_OWNER_PID& row = table->table[index];
        if (targetPids.find(row.dwOwningPid) != targetPids.end()) {
            const int port = UdpTablePortToHostOrder(row.dwLocalPort);
            if (port > 0 && port <= 65535) {
                ports.insert(port);
            }
        }
    }
}

void CollectUdp6PortsForPids(const std::set<DWORD>& targetPids, std::set<int>& ports)
{
    ULONG bufferSize = 0;
    DWORD result = GetExtendedUdpTable(nullptr, &bufferSize, FALSE, kAddressFamilyInet6, UDP_TABLE_OWNER_PID, 0);
    if (result != ERROR_INSUFFICIENT_BUFFER || bufferSize == 0) {
        return;
    }

    std::vector<unsigned char> buffer(bufferSize);
    auto* table = reinterpret_cast<Udp6TableOwnerPid*>(buffer.data());
    result = GetExtendedUdpTable(table, &bufferSize, FALSE, kAddressFamilyInet6, UDP_TABLE_OWNER_PID, 0);
    if (result != NO_ERROR) {
        return;
    }

    for (DWORD index = 0; index < table->numEntries; ++index) {
        const Udp6RowOwnerPid& row = table->table[index];
        if (targetPids.find(row.owningPid) != targetPids.end()) {
            const int port = UdpTablePortToHostOrder(row.localPort);
            if (port > 0 && port <= 65535) {
                ports.insert(port);
            }
        }
    }
}

std::set<int> QueryTargetUdpPorts()
{
    const std::vector<smu::platform::PlatformPid> pids = CurrentTargetProcessIds();
    if (pids.empty()) {
        return {};
    }

    std::set<DWORD> nativePids;
    for (smu::platform::PlatformPid pid : pids) {
        nativePids.insert(static_cast<DWORD>(pid));
    }

    std::set<int> ports;
    CollectUdp4PortsForPids(nativePids, ports);
    CollectUdp6PortsForPids(nativePids, ports);
    return ports;
}

bool RefreshTargetUdpPorts()
{
    const std::set<int> ports = QueryTargetUdpPorts();
    std::lock_guard<std::mutex> lock(g_process_udp_ports_mutex);
    if (ports == g_process_udp_ports) {
        return false;
    }
    g_process_udp_ports = ports;
    return true;
}

std::string FindNewestRobloxLogFile(const std::filesystem::path& logsFolder)
{
    namespace fs = std::filesystem;

    std::error_code ec;
    if (!fs::exists(logsFolder, ec) || ec || !fs::is_directory(logsFolder, ec) || ec) {
        return {};
    }

    std::string newestLogFile;
    fs::file_time_type newestTime{};
    bool foundFile = false;

    try {
        for (const auto& entry : fs::directory_iterator(logsFolder, ec)) {
            if (ec) {
                return {};
            }

            if (!entry.is_regular_file(ec) || ec) {
                continue;
            }

            const auto fileTime = entry.last_write_time(ec);
            if (ec) {
                continue;
            }

            if (!foundFile || fileTime > newestTime) {
                newestLogFile = entry.path().string();
                newestTime = fileTime;
                foundFile = true;
            }
        }
    } catch (const fs::filesystem_error& e) {
        std::cerr << "[RoLogParser] Filesystem error: " << e.what() << std::endl;
        return {};
    }

    return newestLogFile;
}

std::set<int> TargetUdpPortsSnapshot()
{
    std::lock_guard<std::mutex> lock(g_process_udp_ports_mutex);
    return g_process_udp_ports;
}

smu::platform::LagSwitchConfig EffectiveLagSwitchConfigLocked()
{
    smu::platform::LagSwitchConfig effective = g_has_script_lag_config ? g_script_lag_config : g_base_lag_config;
    effective.currentlyBlocking = g_base_lag_blocking || g_script_lag_blocking;
    effective.enabled = effective.enabled || effective.currentlyBlocking;
    return effective;
}

void PublishEffectiveLagSwitchStateLocked()
{
    const smu::platform::LagSwitchConfig effective = EffectiveLagSwitchConfigLocked();
    g_windivert_blocking.store(effective.currentlyBlocking, std::memory_order_relaxed);
    g_log_thread_running.store(LagSwitchTargetUsesRoblox(effective), std::memory_order_relaxed);
}

smu::platform::LagSwitchConfig EffectiveLagSwitchConfig()
{
    std::lock_guard<std::mutex> lock(g_lag_config_mutex);
    return EffectiveLagSwitchConfigLocked();
}

std::string BuildProtocolFilter(const smu::platform::LagSwitchConfig& config)
{
    if (config.useUdp && config.useTcp) {
        return "(udp or tcp)";
    }
    if (config.useUdp) {
        return "udp";
    }
    if (config.useTcp) {
        return "tcp";
    }
    return "false";
}

std::string BuildProcessUdpPortFilter()
{
    const std::set<int> ports = TargetUdpPortsSnapshot();
    if (ports.empty()) {
        return {};
    }

    std::string filter;
    for (int port : ports) {
        const std::string portText = std::to_string(port);
        const std::string clause = "((outbound and udp.SrcPort == " + portText + ") or (inbound and udp.DstPort == " + portText + "))";
        if (filter.empty()) {
            filter = clause;
        } else {
            filter += " or " + clause;
        }
    }
    return "(" + filter + ")";
}

std::string BuildRobloxIpFilter()
{
    std::string combined_ip_filter = ROBLOX_RANGE_FILTER;
    std::shared_lock lock(g_ip_mutex);
    for (const auto& ip : g_roblox_dynamic_ips) {
        combined_ip_filter += " or (ip.SrcAddr == " + ip + " or ip.DstAddr == " + ip + ")";
    }
    return "(" + combined_ip_filter + ")";
}

std::string BuildRobloxFilter()
{
    const std::string processPortFilter = BuildProcessUdpPortFilter();
    if (!processPortFilter.empty()) {
        return processPortFilter;
    }

    if (CurrentTargetProcessIds().empty()) {
        return "false";
    }

    return BuildRobloxIpFilter();
}

std::string BuildCustomTargetFilter(const smu::platform::LagSwitchConfig& config)
{
    std::vector<std::string> clauses;
    clauses.reserve(config.remoteIps.size() + config.remotePorts.size() + 1);

    if (config.includeRobloxDynamicIps) {
        clauses.push_back(BuildRobloxIpFilter());
    }

    for (const std::string& ip : config.remoteIps) {
        clauses.push_back("((outbound and ip.DstAddr == " + ip + ") or (inbound and ip.SrcAddr == " + ip + "))");
    }

    for (int port : config.remotePorts) {
        std::vector<std::string> portClauses;
        if (config.useUdp) {
            portClauses.push_back("((outbound and udp.DstPort == " + std::to_string(port) + ") or (inbound and udp.SrcPort == " + std::to_string(port) + "))");
        }
        if (config.useTcp) {
            portClauses.push_back("((outbound and tcp.DstPort == " + std::to_string(port) + ") or (inbound and tcp.SrcPort == " + std::to_string(port) + "))");
        }
        if (portClauses.size() == 1) {
            clauses.push_back(portClauses.front());
        } else if (portClauses.size() == 2) {
            clauses.push_back("(" + portClauses[0] + " or " + portClauses[1] + ")");
        }
    }

    if (clauses.empty()) {
        return "false";
    }

    std::string filter = "(" + clauses.front();
    for (std::size_t index = 1; index < clauses.size(); ++index) {
        filter += " or " + clauses[index];
    }
    filter += ")";
    return filter;
}

std::string BuildTargetFilter(const smu::platform::LagSwitchConfig& config)
{
    switch (config.targetMode) {
    case smu::platform::LagSwitchTargetMode::All:
        return "true";
    case smu::platform::LagSwitchTargetMode::Custom:
        return BuildCustomTargetFilter(config);
    case smu::platform::LagSwitchTargetMode::Roblox:
    default:
        return BuildRobloxFilter();
    }
}

std::string BuildWindDivertFilter(const smu::platform::LagSwitchConfig& config)
{
    const bool captureInbound = config.inboundHardBlock || (config.fakeLagEnabled && config.inboundFakeLag);
    const bool captureOutbound = config.outboundHardBlock || (config.fakeLagEnabled && config.outboundFakeLag);

    std::string directionFilter;
    if (captureInbound && captureOutbound) {
        directionFilter = "(inbound or outbound)";
    } else if (captureInbound) {
        directionFilter = "inbound";
    } else if (captureOutbound) {
        directionFilter = "outbound";
    } else {
        directionFilter = "false";
    }

    return directionFilter + " and " + BuildTargetFilter(config) + " and " + BuildProtocolFilter(config);
}

// ExtractResource function required for dynamic WinDivert Usage
bool ExtractResource(int resourceId, const char* resourceType, const std::string& outputFilename) {
    // Check if file already exists
    DWORD attrib = GetFileAttributesA(outputFilename.c_str());
    if (attrib != INVALID_FILE_ATTRIBUTES && !(attrib & FILE_ATTRIBUTE_DIRECTORY)) {
        return true; // File exists, no need to extract
    }

    std::cout << "Extracting " << outputFilename << "..." << std::endl;

    HMODULE hModule = GetModuleHandle(NULL);
    HRSRC hRes = FindResourceA(hModule, MAKEINTRESOURCEA(resourceId), resourceType);
    if (!hRes) {
        std::cerr << "Failed to find resource ID: " << resourceId << " Type: " << resourceType << std::endl;
        return false;
    }

    HGLOBAL hMem = LoadResource(hModule, hRes);
    if (!hMem) return false;

    DWORD size = SizeofResource(hModule, hRes);
    void* data = LockResource(hMem);

    if (!data || size == 0) return false;

    std::ofstream file(outputFilename, std::ios::binary);
    if (!file) {
        std::cerr << "Failed to write file: " << outputFilename << std::endl;
        return false;
    }

    file.write(static_cast<const char*>(data), size);
    file.close();

    return true;
}

static const wchar_t* GetWinDivertErrorDescription(DWORD errorCode) {
    switch (errorCode) {
        case ERROR_FILE_NOT_FOUND:          // 2
            return L"The WinDivert driver (SMCWinDivert.sys or WinDivert64.sys) could not be found.\n"
                   L"Ensure the .sys file is in the same directory as the DLL/exe.";
        case ERROR_ACCESS_DENIED:           // 5
            return L"Access denied. The program must be run as Administrator.\n"
                   L"Additionally, the driver may be blocked by signature enforcement or antivirus.";
        case ERROR_INVALID_PARAMETER:       // 87
            return L"Invalid parameter passed to WinDivertOpen (e.g., bad filter string, invalid layer, priority, or flags).";
        case ERROR_INVALID_IMAGE_HASH:      // 577 (common on newer Windows)
            return L"Driver signature verification failed.\n"
                   L"The WinDivert driver is not signed or signature enforcement is enabled.\n"
                   L"Try disabling Secure Boot, Driver Signature Enforcement, or use a signed version.";
        case ERROR_SERVICE_DOES_NOT_EXIST:  // 1060 (driver service issue)
            return L"The WinDivert service does not exist or failed to start properly.";
        case ERROR_IO_DEVICE:               // 1117 (generic driver load failure)
            return L"I/O device error - the driver failed to load or initialize.";
        case ERROR_NO_DATA:                 // 232 (e.g., after shutdown recv)
            return L"No more data available (queue empty after shutdown).";
        case ERROR_HOST_UNREACHABLE:        // Often used for certain injection failures
            return L"Host unreachable - common during certain packet reinjection scenarios.";
        case ERROR_INVALID_HANDLE:          // 6 (bad handle)
            return L"Invalid handle passed to a WinDivert function.";
        default:
            return L"Unknown or generic Windows error.\nMost likely because the WinDivert Service hasn't started yet.\nRestart the program.";
    }
}

std::string WideToUtf8(const wchar_t* value)
{
    if (!value) {
        return {};
    }

    const int needed = WideCharToMultiByte(CP_UTF8, 0, value, -1, nullptr, 0, nullptr, nullptr);
    if (needed <= 1) {
        return {};
    }

    std::string out(static_cast<std::size_t>(needed - 1), '\0');
    WideCharToMultiByte(CP_UTF8, 0, value, -1, out.data(), needed, nullptr, nullptr);
    return out;
}

bool TryLoadWinDivert(std::string* errorMessage, DWORD* win32ErrorCode) {
	wchar_t buffer[256] = {0};
	// Check if file exists before attempting extraction or loading
	if (bDependenciesLoaded) return true; 

    // 1. Extract SYS (Resource ID from previous code)
    if (!ExtractResource(IDR_SMC_WINDIVERT_SYS1, "SMC_WINDIVERT_SYS", SYS_NAME)) {
        if (errorMessage) {
            *errorMessage = "Failed to extract WinDivert driver (.sys) into the working directory.";
        }
        return false;
    }
    
    // 2. Extract DLL
    if (!ExtractResource(IDR_SMC_WINDIVERT_DLL1, "SMC_WINDIVERT_DLL", DLL_NAME)) {
        if (errorMessage) {
            *errorMessage = "Failed to extract SMCWinDivert.dll into the working directory.";
        }
        return false;
    }

    // For some godforsaken reason, running sc QUERY on WinDivert updates its status from "Stopped" to "Non Existent", which fixes all of our problems.
    quiet_system("sc query WinDivert >nul 2>&1");

    // 3. Load Library
    HMODULE hWinDivertDll = LoadLibraryA("SMCWinDivert.dll");
    if (!hWinDivertDll) {
        const DWORD err = GetLastError();
        if (win32ErrorCode) {
            *win32ErrorCode = err;
        }
        if (errorMessage) {
            const std::string desc = WideToUtf8(GetWinDivertErrorDescription(err));
            *errorMessage = "Failed to load SMCWinDivert.dll (LoadLibrary error " + std::to_string(err) + "). " + desc;
        }
        return false;
    }

    // 4. Map Functions
    pWinDivertOpen = (tWinDivertOpen)GetProcAddress(hWinDivertDll, "WinDivertOpen");
    pWinDivertRecv = (tWinDivertRecv)GetProcAddress(hWinDivertDll, "WinDivertRecv");
    pWinDivertSend = (tWinDivertSend)GetProcAddress(hWinDivertDll, "WinDivertSend");
    pWinDivertClose = (tWinDivertClose)GetProcAddress(hWinDivertDll, "WinDivertClose");

	pWinDivertHelperParsePacket = (tWinDivertHelperParsePacket)GetProcAddress(hWinDivertDll, "WinDivertHelperParsePacket");
    pWinDivertHelperCalcChecksums = (tWinDivertHelperCalcChecksums)GetProcAddress(hWinDivertDll, "WinDivertHelperCalcChecksums");

	// Check missing symbols (same collection as before)
    bool allLoaded = true;
    wchar_t missingFuncs[512] = L"";

	if (!pWinDivertOpen) {
        wcscat_s(missingFuncs, L"WinDivertOpen\n");
        allLoaded = false;
    }
    if (!pWinDivertRecv) {
        wcscat_s(missingFuncs, L"WinDivertRecv\n");
        allLoaded = false;
    }
    if (!pWinDivertSend) {
        wcscat_s(missingFuncs, L"WinDivertSend\n");
        allLoaded = false;
    }
    if (!pWinDivertClose) {
        wcscat_s(missingFuncs, L"WinDivertClose\n");
        allLoaded = false;
    }
    // Optional functions (not fatal, but still report if missing)
    if (!pWinDivertHelperParsePacket) {
        wcscat_s(missingFuncs, L"WinDivertHelperParsePacket (optional)\n");
	    allLoaded = false;
    }
    if (!pWinDivertHelperCalcChecksums) {
        wcscat_s(missingFuncs, L"WinDivertHelperCalcChecksums (optional)\n");
    }

    if (!allLoaded) {
        if (errorMessage) {
            // missingFuncs is wide; keep the message short and actionable.
            *errorMessage = "Failed to resolve required WinDivert exports from SMCWinDivert.dll.";
        }
        FreeLibrary(hWinDivertDll);
        return false;
    }

	HANDLE testHandle = pWinDivertOpen("false", WINDIVERT_LAYER_NETWORK, 0, WINDIVERT_FLAG_SNIFF);
    if (testHandle == INVALID_HANDLE_VALUE) {
        const DWORD err = GetLastError();
        if (win32ErrorCode) {
            *win32ErrorCode = err;
        }
        if (errorMessage) {
            const std::string desc = WideToUtf8(GetWinDivertErrorDescription(err));
            *errorMessage = "WinDivertOpen failed (error " + std::to_string(err) + "). " + desc;
        }
        FreeLibrary(hWinDivertDll);
        return false;
    }

    // Success, close the test handle
    SafeCloseWinDivert();

    if (pWinDivertOpen && pWinDivertRecv && pWinDivertSend && pWinDivertClose) {
        bDependenciesLoaded = true;
        return true;
    }
    return false;
}

bool IsValidRobloxIP(const std::string& ip_str) {
    if (ip_str.empty()) return false;
    // 127.x.x.x (Localhost)
    if (ip_str.rfind("127.", 0) == 0) return false;
    // 10.x.x.x (Private Class A)
    if (ip_str.rfind("10.", 0) == 0) return false;
    // 192.168.x.x (Private Class C)
    if (ip_str.rfind("192.168.", 0) == 0) return false;
    // 172.16.x.x - 172.31.x.x (Private Class B)
    if (ip_str.rfind("172.", 0) == 0) return false;
    // 0.0.0.0
    if (ip_str == "0.0.0.0") return false;
    return true;
}

void DelaySenderWorker() {
    while (g_delay_thread_running) {
        std::unique_lock<std::mutex> lock(g_delay_queue_mutex);
        
        // Wait until queue is not empty or thread stops
        if (g_delayed_packet_queue.empty()) {
            g_delay_cv.wait(lock, [] { return !g_delayed_packet_queue.empty() || !g_delay_thread_running; });
        }

        if (!g_delay_thread_running) break;

        // Check the packet at the front
        auto now = std::chrono::steady_clock::now();
        DelayedPacket& front_packet = g_delayed_packet_queue.front();

        if (now >= front_packet.send_time) {
            // It is time to send
            {
                // Lock the handle to prevent conflict with the main thread or closing
                std::lock_guard<std::mutex> handleLock(g_windivert_handle_mutex);
                if (hWindivert != INVALID_HANDLE_VALUE && pWinDivertSend) {
                    pWinDivertSend(hWindivert, front_packet.data.data(), front_packet.len, NULL, &front_packet.addr);
                }
            }
            g_delayed_packet_queue.pop_front();
        } else {
            // Not time yet, sleep until the timestamp of the first packet
            // We use wait_until to sleep efficiently but wake up if a new packet (with potentially earlier time?) comes in
            // though usually packets come in order.
            g_delay_cv.wait_until(lock, front_packet.send_time);
        }
    }

    // Cleanup queue on exit
    std::unique_lock<std::mutex> lock(g_delay_queue_mutex);
    g_delayed_packet_queue.clear();
}

void RobloxLogScannerThread() {
    char localAppData[MAX_PATH];

    if (SHGetFolderPathA(NULL, CSIDL_LOCAL_APPDATA, NULL, 0, localAppData) != S_OK) {
        std::cerr << "Could not find LocalAppData for RoLogParser configuration." << std::endl;
        return;
    }

    const std::filesystem::path logsFolder = std::filesystem::path(localAppData) / "Roblox" / "logs";
    std::string activeLogFile;
    std::unique_ptr<RoLogObject, decltype(&RoLogFreeObject)> logObject(nullptr, RoLogFreeObject);

    while (g_windivert_running.load(std::memory_order_relaxed))
    {
        while (g_windivert_running.load(std::memory_order_relaxed) &&
               !g_log_thread_running.load(std::memory_order_relaxed)) 
            std::this_thread::sleep_for(std::chrono::milliseconds(200));
        
        if (!g_windivert_running.load(std::memory_order_relaxed)) break;

        if (EffectiveLagSwitchConfig().targetMode == smu::platform::LagSwitchTargetMode::Roblox &&
            RefreshTargetUdpPorts() && bWinDivertEnabled) {
            SafeCloseWinDivert();
        }

        const std::string newestLogFile = FindNewestRobloxLogFile(logsFolder);
        if (newestLogFile.empty()) {
            std::this_thread::sleep_for(std::chrono::seconds(1));
            continue;
        }

        if (!logObject || newestLogFile != activeLogFile) {
            activeLogFile = newestLogFile;
            logObject.reset(RoLogCreateObject(activeLogFile.c_str()));
            if (!logObject) {
                std::this_thread::sleep_for(std::chrono::seconds(1));
                continue;
            }
        }

        const RoLogParseRes parseResult = RoLogParse(logObject.get());
        if (parseResult != ROLOGSUCCESS) {
            std::cerr << "[RoLogParser] Could not parse Roblox log: " << activeLogFile << std::endl;
            logObject.reset();
            activeLogFile.clear();
            std::this_thread::sleep_for(std::chrono::seconds(1));
            continue;
        }

        if (logObject->current_state == ROLOG_IN_GAME) {
            bool new_ip_found = false;

            // Update Connection Type State
            if (logObject->serverObj && logObject->serverObj->serverType == ROLOGUDMUXSERVER) {
                g_is_using_rcc = false;
            } else if (logObject->serverObj && logObject->serverObj->serverType == ROLOGRCCSERVER) {
                std::cout << "RCC Server Found, Switching logic to ten-second pulse logic";
                g_is_using_rcc = true;
            }

            // Helper lambda to process IPs with specific labels
            auto process_ip = [&](const char* ip, const std::string& label) {
                if (!ip) return;
                if (!IsValidRobloxIP(ip)) return;

                std::unique_lock lock(g_ip_mutex);
                if (g_roblox_dynamic_ips.find(ip) == g_roblox_dynamic_ips.end()) {
                    g_roblox_dynamic_ips.insert(ip);
                    new_ip_found = true;
                    std::cout << "Found Roblox Server (" << label << "): " << ip << std::endl;
                }
            };

            // Process UDMUX
            if (logObject->serverObj && logObject->serverObj->serverType == ROLOGUDMUXSERVER) {
                process_ip(logObject->serverObj->serverIPUDMUX, "UDMUX");
            }

            // Process RCC (RakNet)
            if (logObject->serverObj) {
                process_ip(logObject->serverObj->serverIPRCC, "RCC");
            }

            if (new_ip_found && bWinDivertEnabled) {
                SafeCloseWinDivert();
            }
        }
        
        // Poll every 1 second
        std::this_thread::sleep_for(std::chrono::seconds(1));
    }
}

void WindivertWorkerThread() {
    WINDIVERT_ADDRESS addr;
    std::unique_ptr<char[]> packet = std::make_unique<char[]>(WINDIVERT_MTU_MAX);
    UINT packetLen;

    // Helper structures for parsing
    PWINDIVERT_IPHDR ip_header;
    PWINDIVERT_IPV6HDR ipv6_header;
    PWINDIVERT_ICMPHDR icmp_header;
    PWINDIVERT_ICMPV6HDR icmpv6_header;
    PWINDIVERT_TCPHDR tcp_header;
    PWINDIVERT_UDPHDR udp_header;
    PVOID payload = nullptr;
    UINT payload_len = 0;
    
    UINT8 protocol;
    PVOID next;
    UINT next_len;

    auto last_safety_pulse_time = std::chrono::steady_clock::now();
    bool safety_pulse_active = false;

    if (!pWinDivertOpen) return;

    // Start the delay sender thread
    g_delay_thread_running = true;
    std::thread senderThread(DelaySenderWorker);

    while (g_windivert_running) {
        smu::platform::LagSwitchConfig filterConfig = EffectiveLagSwitchConfig();
        if (filterConfig.targetMode == smu::platform::LagSwitchTargetMode::Roblox) {
            RefreshTargetUdpPorts();
        }
        std::string final_filter = BuildWindDivertFilter(filterConfig);

        g_current_windivert_filter = final_filter; 

        {
            std::lock_guard<std::mutex> lock(g_windivert_handle_mutex);
            hWindivert = pWinDivertOpen(final_filter.c_str(), WINDIVERT_LAYER_NETWORK, 0, 0);
        }

        if (hWindivert == INVALID_HANDLE_VALUE) {
            // Handle error / reset IPs if parameter invalid
            if (GetLastError() == ERROR_INVALID_PARAMETER && !g_roblox_dynamic_ips.empty()) {
                 std::unique_lock lock(g_ip_mutex);
                 g_roblox_dynamic_ips.clear();
            }
            std::this_thread::sleep_for(std::chrono::milliseconds(500));
            continue; 
        }

        last_safety_pulse_time = std::chrono::steady_clock::now();

        // --- INNER LOOP ---
        while (g_windivert_running) {
            if (!pWinDivertRecv(hWindivert, packet.get(), WINDIVERT_MTU_MAX, &packetLen, &addr)) {
                break;
            }
            const smu::platform::LagSwitchConfig packetConfig = EffectiveLagSwitchConfig();
            // We only manipulate packets if the Lagswitch is currently ACTIVE (Toggle On / Key Held)
            if (packetConfig.currentlyBlocking) {

                // 1. HARD BLOCKING CHECK
                bool should_hard_block = false;
                if (addr.Outbound && packetConfig.outboundHardBlock) should_hard_block = true;
                if (!addr.Outbound && packetConfig.inboundHardBlock) should_hard_block = true;

                if (should_hard_block) {
                    if (packetConfig.preventDisconnect) {
                        // Pulse Logic
                        long long interval_ms = 0;
                        bool is_rcc = g_is_using_rcc.load();

                        if (is_rcc) interval_ms = 9500;
                        else {
                            if (packetConfig.inboundHardBlock && packetConfig.outboundHardBlock) interval_ms = 19900;
                            else if (packetConfig.outboundHardBlock) interval_ms = 290000;
                            else interval_ms = 0;
                        }

                        if (interval_ms > 0) {
                            auto now = std::chrono::steady_clock::now();
                            auto elapsed_ms = std::chrono::duration_cast<std::chrono::milliseconds>(now - last_safety_pulse_time).count();

                            if (elapsed_ms >= interval_ms) safety_pulse_active = true;

                            if (safety_pulse_active) {
                                if (elapsed_ms >= (interval_ms + 250)) {
                                    last_safety_pulse_time = std::chrono::steady_clock::now();
                                    safety_pulse_active = false;
                                } else {
                                    // Send safety packet
                                    if (pWinDivertHelperParsePacket(packet.get(), packetLen, 
                                        &ip_header, &ipv6_header, &protocol, 
                                        &icmp_header, &icmpv6_header, &tcp_header, &udp_header, 
                                        &payload, &payload_len, &next, &next_len) && payload != nullptr) 
                                    {
                                        pWinDivertSend(hWindivert, packet.get(), packetLen, NULL, &addr);
                                        continue; 
									}
                                }
                            }
                        }
                        if (is_rcc) continue; // Drop if not pulsing

                        // Smart Drop Logic
                        bool drop_smart = false; 
                        if (pWinDivertHelperParsePacket(packet.get(), packetLen, 
                            &ip_header, &ipv6_header, &protocol, 
                            &icmp_header, &icmpv6_header, &tcp_header, &udp_header, 
                            &payload, &payload_len, &next, &next_len) && payload != nullptr) 
                        {
                            if (payload_len >= 90) {
                                unsigned char* p = (unsigned char*)payload;
                                if (p[0] == 0x01 && p[1] == 0x00 && p[2] == 0x00 && p[4] == 0x01 && p[5] == 0x11) {
                                    if (!addr.Outbound && p[3] == 0x17) drop_smart = true;
                                    else if (addr.Outbound && p[3] == 0x1F) drop_smart = true;
                                }
                            }
                        }

                        if (drop_smart) continue; // Drop
                        else {
                            pWinDivertSend(hWindivert, packet.get(), packetLen, NULL, &addr); // Send Heartbeat
                            continue;
                        }
                    }
                    
                    // If prevent_disconnect is off -> Pure Drop
                    continue; 
                }

                // 2. FAKE LAG (LATENCY) CHECK
                if (packetConfig.fakeLagEnabled) {
                    bool should_delay = false;
                    if (addr.Outbound && packetConfig.outboundFakeLag) should_delay = true;
                    if (!addr.Outbound && packetConfig.inboundFakeLag) should_delay = true;

                    if (should_delay && packetConfig.fakeLagDelayMs > 0) {
                        DelayedPacket p;
                        p.data.assign(packet.get(), packet.get() + packetLen);
                        p.len = packetLen;
                        p.addr = addr;
                        p.send_time = std::chrono::steady_clock::now() + std::chrono::milliseconds(packetConfig.fakeLagDelayMs);

                        {
                            std::lock_guard<std::mutex> qLock(g_delay_queue_mutex);
                            g_delayed_packet_queue.push_back(std::move(p));
                        }
                        g_delay_cv.notify_one();
                        
                        // Consumed by queue, do not send immediately
                        continue; 
                    }
                }
            }

            // 3. PASSTHROUGH
            last_safety_pulse_time = std::chrono::steady_clock::now();
            pWinDivertSend(hWindivert, packet.get(), packetLen, NULL, &addr);
        }

        SafeCloseWinDivert();
    }

    g_delay_thread_running = false;
    g_delay_cv.notify_all();
    if (senderThread.joinable()) {
        senderThread.join();
    }
}

smu::platform::LagSwitchConfig LagSwitchConfigFromGlobals()
{
    smu::platform::LagSwitchConfig config;
    config.enabled = bWinDivertEnabled;
    config.currentlyBlocking = g_windivert_blocking.load(std::memory_order_relaxed);
    config.inboundHardBlock = lagswitchinbound;
    config.outboundHardBlock = lagswitchoutbound;
    config.fakeLagEnabled = lagswitchlag;
    config.inboundFakeLag = lagswitchlaginbound;
    config.outboundFakeLag = lagswitchlagoutbound;
    config.fakeLagDelayMs = lagswitchlagdelay;
    config.targetRobloxOnly = lagswitchtargetroblox;
    config.useUdp = true;
    config.useTcp = lagswitchusetcp;
    config.preventDisconnect = prevent_disconnect;
    config.autoUnblock = lagswitch_autounblock;
    config.maxDurationSeconds = lagswitch_max_duration;
    config.unblockDurationMs = lagswitch_unblock_ms;
    config.targetMode = lagswitchtargetroblox ? smu::platform::LagSwitchTargetMode::Roblox : smu::platform::LagSwitchTargetMode::All;
    return config;
}

void ApplyLagSwitchConfigToGlobals(const smu::platform::LagSwitchConfig& config)
{
    lagswitchinbound = config.inboundHardBlock;
    lagswitchoutbound = config.outboundHardBlock;
    lagswitchlag = config.fakeLagEnabled;
    lagswitchlaginbound = config.inboundFakeLag;
    lagswitchlagoutbound = config.outboundFakeLag;
    lagswitchlagdelay = config.fakeLagDelayMs;
    lagswitchtargetroblox = config.targetRobloxOnly;
    lagswitchusetcp = config.useTcp;
    prevent_disconnect = config.preventDisconnect;
    lagswitch_autounblock = config.autoUnblock;
    lagswitch_max_duration = config.maxDurationSeconds;
    lagswitch_unblock_ms = config.unblockDurationMs;
}

void SetBaseLagSwitchConfig(const smu::platform::LagSwitchConfig& config)
{
    bool shouldRestart = false;
    {
        std::lock_guard<std::mutex> lock(g_lag_config_mutex);
        const std::string oldFilter = BuildWindDivertFilter(EffectiveLagSwitchConfigLocked());
        g_base_lag_config = config;
        g_base_lag_config.currentlyBlocking = g_base_lag_blocking;
        ApplyLagSwitchConfigToGlobals(config);
        PublishEffectiveLagSwitchStateLocked();
        const std::string newFilter = BuildWindDivertFilter(EffectiveLagSwitchConfigLocked());
        shouldRestart = oldFilter != newFilter;
    }
    if (shouldRestart) {
        SafeCloseWinDivert();
    }
}

void SetBaseLagSwitchBlocking(bool active)
{
    std::lock_guard<std::mutex> lock(g_lag_config_mutex);
    g_base_lag_blocking = active;
    g_base_lag_config.currentlyBlocking = active;
    PublishEffectiveLagSwitchStateLocked();
}

void SetScriptLagSwitchConfig(std::uintptr_t ownerToken, const smu::platform::LagSwitchConfig& config)
{
    bool shouldRestart = false;
    {
        std::lock_guard<std::mutex> lock(g_lag_config_mutex);
        const std::string oldFilter = BuildWindDivertFilter(EffectiveLagSwitchConfigLocked());
        g_script_config_owner = ownerToken;
        g_script_lag_config = config;
        g_script_lag_config.currentlyBlocking = g_script_lag_blocking;
        g_has_script_lag_config = true;
        PublishEffectiveLagSwitchStateLocked();
        const std::string newFilter = BuildWindDivertFilter(EffectiveLagSwitchConfigLocked());
        shouldRestart = oldFilter != newFilter;
    }
    if (shouldRestart) {
        SafeCloseWinDivert();
    }
}

void ClearScriptLagSwitchConfig(std::uintptr_t ownerToken)
{
    bool cleared = false;
    bool shouldRestart = false;
    {
        std::lock_guard<std::mutex> lock(g_lag_config_mutex);
        const std::string oldFilter = BuildWindDivertFilter(EffectiveLagSwitchConfigLocked());
        if (g_has_script_lag_config && g_script_config_owner == ownerToken) {
            g_has_script_lag_config = false;
            g_script_lag_config = {};
            cleared = true;
            PublishEffectiveLagSwitchStateLocked();
        }
        const std::string newFilter = BuildWindDivertFilter(EffectiveLagSwitchConfigLocked());
        shouldRestart = oldFilter != newFilter;
    }
    if (cleared && shouldRestart) {
        SafeCloseWinDivert();
    }
}

void SetScriptLagSwitchBlocking(std::uintptr_t ownerToken, bool active)
{
    std::lock_guard<std::mutex> lock(g_lag_config_mutex);
    g_script_blocking_owner = ownerToken;
    g_script_lag_blocking = active;
    PublishEffectiveLagSwitchStateLocked();
}

void ClearScriptLagSwitchState(std::uintptr_t ownerToken)
{
    bool clearedConfig = false;
    bool shouldRestart = false;
    {
        std::lock_guard<std::mutex> lock(g_lag_config_mutex);
        const std::string oldFilter = BuildWindDivertFilter(EffectiveLagSwitchConfigLocked());
        if (g_has_script_lag_config && g_script_config_owner == ownerToken) {
            g_has_script_lag_config = false;
            g_script_lag_config = {};
            clearedConfig = true;
        }
        if (g_script_blocking_owner == ownerToken) {
            g_script_lag_blocking = false;
        }
        PublishEffectiveLagSwitchStateLocked();
        const std::string newFilter = BuildWindDivertFilter(EffectiveLagSwitchConfigLocked());
        shouldRestart = oldFilter != newFilter;
    }
    if (clearedConfig && shouldRestart) {
        SafeCloseWinDivert();
    }
}

class WinDivertNetworkLagBackend final : public smu::platform::NetworkLagBackend {
public:
    WinDivertNetworkLagBackend()
    {
        std::lock_guard<std::mutex> lock(g_lag_config_mutex);
        g_base_lag_config = LagSwitchConfigFromGlobals();
        g_base_lag_blocking = g_base_lag_config.currentlyBlocking;
        PublishEffectiveLagSwitchStateLocked();
    }

    ~WinDivertNetworkLagBackend() override
    {
        shutdown();
    }

    bool init(std::string* errorMessage = nullptr) override
    {
        if (errorMessage) {
            errorMessage->clear();
        }

        if (bWinDivertEnabled) {
            return true;
        }

        DWORD win32Error = 0;
        if (!TryLoadWinDivert(errorMessage, &win32Error)) {
            if (win32Error == ERROR_ACCESS_DENIED && errorMessage) {
                // Access denied is expected when not elevated. The caller is responsible
                // for offering an elevation prompt instead of showing a generic critical dialog.
                errorMessage->clear();
            }
            return false;
        }

        bWinDivertEnabled = true;
        g_windivert_running.store(true, std::memory_order_relaxed);
        {
            std::lock_guard<std::mutex> lock(g_lag_config_mutex);
            g_base_lag_config.enabled = true;
            PublishEffectiveLagSwitchStateLocked();
        }
        if (!workerThread_.joinable()) {
            workerThread_ = std::thread(WindivertWorkerThread);
        }
        if (!logScannerThread_.joinable()) {
            logScannerThread_ = std::thread(RobloxLogScannerThread);
        }
        return true;
    }

    void shutdown() override
    {
        if (!bWinDivertEnabled && !workerThread_.joinable()) {
            return;
        }

        bWinDivertEnabled = false;
        g_windivert_running.store(false, std::memory_order_relaxed);
        {
            std::lock_guard<std::mutex> lock(g_lag_config_mutex);
            g_base_lag_config.enabled = false;
            g_base_lag_blocking = false;
            g_script_lag_blocking = false;
            PublishEffectiveLagSwitchStateLocked();
        }
        g_log_thread_running.store(false, std::memory_order_relaxed);
        {
            std::lock_guard<std::mutex> lock(g_process_udp_ports_mutex);
            g_process_udp_ports.clear();
        }
        SafeCloseWinDivert();

        if (workerThread_.joinable()) {
            workerThread_.join();
        }
        if (logScannerThread_.joinable()) {
            logScannerThread_.join();
        }
    }

    bool isAvailable() const override
    {
        return true;
    }

    bool isBlockingActive() const override
    {
        return effectiveConfig().currentlyBlocking;
    }

    bool isBaseBlockingActive() const override
    {
        std::lock_guard<std::mutex> lock(g_lag_config_mutex);
        return g_base_lag_blocking;
    }

    void setBlockingActive(bool active) override
    {
        SetBaseLagSwitchBlocking(active);
    }

    void setScriptBlockingActive(std::uintptr_t ownerToken, bool active) override
    {
        SetScriptLagSwitchBlocking(ownerToken, active);
    }

    void setConfig(const smu::platform::LagSwitchConfig& config) override
    {
        SetBaseLagSwitchConfig(config);
    }

    void setScriptConfigOverride(std::uintptr_t ownerToken, const smu::platform::LagSwitchConfig& config) override
    {
        SetScriptLagSwitchConfig(ownerToken, config);
    }

    void clearScriptConfigOverride(std::uintptr_t ownerToken) override
    {
        ClearScriptLagSwitchConfig(ownerToken);
    }

    void clearScriptState(std::uintptr_t ownerToken) override
    {
        ClearScriptLagSwitchState(ownerToken);
    }

    smu::platform::LagSwitchConfig config() const override
    {
        std::lock_guard<std::mutex> lock(g_lag_config_mutex);
        smu::platform::LagSwitchConfig config = g_base_lag_config;
        config.currentlyBlocking = g_base_lag_blocking;
        return config;
    }

    smu::platform::LagSwitchConfig effectiveConfig() const override
    {
        return EffectiveLagSwitchConfig();
    }

    void restartCapture() override
    {
        if (bWinDivertEnabled) {
            SafeCloseWinDivert();
        }
    }

    std::string unsupportedReason() const override
    {
        return {};
    }

private:
    std::thread workerThread_;
    std::thread logScannerThread_;
};

} // namespace

std::shared_ptr<smu::platform::NetworkLagBackend> CreateWinDivertNetworkLagBackend()
{
    return std::make_shared<WinDivertNetworkLagBackend>();
}

} // namespace smu::platform::windows

#endif
