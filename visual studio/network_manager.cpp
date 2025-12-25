/* --- START OF FILE network_manager.cpp --- */

#include "Resource Files/network_manager.h"
#include "resource.h"
#include <mutex>
#include <iostream>
#include <fstream>
#include <filesystem>
#include <string> 
#include <regex>
#include <shlobj.h>
#include <atomic>

// NEW INCLUDES FOR LAG QUEUE
#include <queue>
#include <vector>
#include <condition_variable>
#include <chrono>

// Include windivert
#include "windivert-files/windivert.h"

#include "Resource Files/globals.h"
using namespace Globals;

// Tracks if the current connection is RCC (RakNet) or UDMUX
std::atomic<bool> g_is_using_rcc = false;

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

bool TryLoadWinDivert() {
	if (g_isLinuxWine) {
		return false;
	}

	wchar_t buffer[256] = {0};
	// Check if file exists before attempting extraction or loading
	if (bDependenciesLoaded) return true; 

    // 1. Extract SYS (Resource ID from previous code)
    if (!ExtractResource(IDR_SMC_WINDIVERT_SYS1, "SMC_WINDIVERT_SYS", SYS_NAME)) MessageBoxW(NULL, buffer, L"WinDivert SYS Placement Error", MB_ICONERROR | MB_OK);
    
    // 2. Extract DLL
    if (!ExtractResource(IDR_SMC_WINDIVERT_DLL1, "SMC_WINDIVERT_DLL", DLL_NAME)) MessageBoxW(NULL, buffer, L"SMCWinDivert DLL Placement Error", MB_ICONERROR | MB_OK);

    // For some godforsaken reason, running sc QUERY on WinDivert updates its status from "Stopped" to "Non Existent", which fixes all of our problems.
    quiet_system("sc query WinDivert >nul 2>&1");

    // 3. Load Library
    HMODULE hWinDivertDll = LoadLibraryA("SMCWinDivert.dll");
    if (!hWinDivertDll) {
        DWORD err = GetLastError();
        wchar_t msg[1024];
        const wchar_t* desc = GetWinDivertErrorDescription(err);  // Reuse for common cases
        wsprintfW(msg, L"Failed to load SMCWinDivert.dll\n\n"
                       L"LoadLibrary error code: %lu\n\nDescription:\n%s", err, desc);
        MessageBoxW(NULL, msg, L"WinDivert Load Error", MB_ICONERROR | MB_OK);
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
        wsprintfW(buffer, L"Failed to resolve required WinDivert function(s):\n\n%s", missingFuncs);
        MessageBoxW(NULL, buffer, L"WinDivert Symbol Resolution Error", MB_ICONERROR | MB_OK);
        FreeLibrary(hWinDivertDll);
        return false;
    }

	HANDLE testHandle = pWinDivertOpen("false", WINDIVERT_LAYER_NETWORK, 0, WINDIVERT_FLAG_SNIFF);
    if (testHandle == INVALID_HANDLE_VALUE) {
        DWORD err = GetLastError();
        const wchar_t* desc = GetWinDivertErrorDescription(err);

        wchar_t msg[1024];
        wsprintfW(msg, L"WinDivert DLL loaded successfully, but driver failed to open.\n\n"
                       L"WinDivertOpen error code: %lu\n\n"
                       L"Description:\n%s", err, desc);

        MessageBoxW(NULL, msg, L"WinDivert Driver Error", MB_ICONERROR | MB_OK);

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
    namespace fs = std::filesystem;
    char localAppData[MAX_PATH];
    
    // 1. Configure logzz paths
    if (SHGetFolderPathA(NULL, CSIDL_LOCAL_APPDATA, NULL, 0, localAppData) == S_OK) {
        std::string base = std::string(localAppData) + "\\Roblox";
        logzz::logs_folder_path = base + "\\logs";
        logzz::local_storage_folder_path = base + "\\LocalStorage";
        
        // Optional: Load user info if available/needed
        logzz::load_user_info();
    } else {
        std::cerr << "Could not find LocalAppData for Logzz configuration." << std::endl;
        return;
    }

    while (running)
    {
        while (running && !g_log_thread_running.load(std::memory_order_relaxed)) 
            std::this_thread::sleep_for(std::chrono::milliseconds(200));
        
        if (!running) break;

        // 2. Call Library Handle
        state s = logzz::loop_handle();

        if (s == IN_GAME) {
            bool new_ip_found = false;

            // Update Connection Type State
            if (logzz::server_uses_udmux) {
                g_is_using_rcc = false;
            } else {
                std::cout << "RCC Server Found, Switching logic to ten-second pulse logic";
                g_is_using_rcc = true;
            }

            // Helper lambda to process IPs with specific labels
            auto process_ip = [&](const std::string& ip, const std::string& label) {
                if (!IsValidRobloxIP(ip)) return;

                std::unique_lock lock(g_ip_mutex);
                if (g_roblox_dynamic_ips.find(ip) == g_roblox_dynamic_ips.end()) {
                    g_roblox_dynamic_ips.insert(ip);
                    new_ip_found = true;
                    std::cout << "Found Roblox Server (" << label << "): " << ip << std::endl;
                }
            };

            // Process UDMUX
            if (logzz::server_uses_udmux) {
                process_ip(logzz::server_udmux_address, "UDMUX");
            }

            // Process RCC (RakNet)
            process_ip(logzz::server_rcc_address, "RCC");

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
        
        // Construct Filter
        std::string direction_filter = "";
        
        // We capture packets if EITHER Hard Blocking OR Fake Lag is enabled for a direction
        // This ensures we have the packet in hand to decide what to do with it later
        bool capture_inbound = lagswitchinbound || lagswitchlaginbound;
        bool capture_outbound = lagswitchoutbound || lagswitchlagoutbound;

        // If nothing is selected, we still capture "false" to keep the thread alive but idle
        if (capture_inbound && capture_outbound) direction_filter = "(inbound or outbound)";
        else if (capture_inbound) direction_filter = "inbound";
        else if (capture_outbound) direction_filter = "outbound";
        else direction_filter = "false";

        std::string final_filter = "";
        
        if (lagswitchtargetroblox) {
            std::string combined_ip_filter = ROBLOX_RANGE_FILTER;
            {
                std::shared_lock lock(g_ip_mutex);
                for (const auto& ip : g_roblox_dynamic_ips) {
                    combined_ip_filter += " or (ip.SrcAddr == " + ip + " or ip.DstAddr == " + ip + ")";
                }
            }
            final_filter = direction_filter + " and (" + combined_ip_filter + ")";
        } else {
            final_filter = direction_filter;
        }

		if (lagswitchusetcp) final_filter = final_filter + " and (udp or tcp)";
		else final_filter = final_filter + " and udp";

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
            // We only manipulate packets if the Lagswitch is currently ACTIVE (Toggle On / Key Held)
            if (g_windivert_blocking) {

                // 1. HARD BLOCKING CHECK
                bool should_hard_block = false;
                if (addr.Outbound && lagswitchoutbound) should_hard_block = true;
                if (!addr.Outbound && lagswitchinbound) should_hard_block = true;

                if (should_hard_block) {
                    if (prevent_disconnect) {
                        // Pulse Logic
                        long long interval_ms = 0;
                        bool is_rcc = g_is_using_rcc.load();

                        if (is_rcc) interval_ms = 9500;
                        else {
                            if (lagswitchinbound && lagswitchoutbound) interval_ms = 19900; 
                            else if (lagswitchoutbound) interval_ms = 290000;
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
                if (lagswitchlag) {
                    bool should_delay = false;
                    if (addr.Outbound && lagswitchlagoutbound) should_delay = true;
                    if (!addr.Outbound && lagswitchlaginbound) should_delay = true;

                    if (should_delay && lagswitchlagdelay > 0) {
                        DelayedPacket p;
                        p.data.assign(packet.get(), packet.get() + packetLen);
                        p.len = packetLen;
                        p.addr = addr;
                        p.send_time = std::chrono::steady_clock::now() + std::chrono::milliseconds(lagswitchlagdelay);

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