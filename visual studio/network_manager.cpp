#include "Resource Files/network_manager.h"
#include "resource.h"
#include <mutex>
#include <iostream>
#include <fstream>
#include <filesystem>
#include <string> 
#include <regex>
#include <shlobj.h>

// Include windivert
#include "windivert-files/windivert.h"

#include "Resource Files/globals.h"
using namespace Globals;

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
            return L"Unknown or generic Windows error.\nMostly likely because the WinDivert Service hasn't started yet.\nRestart the program.";
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
    if (!ExtractResource(IDR_SMC_WINDIVERT_SYS1, "SMC_WINDIVERT_SYS", "WinDivert64.sys")) MessageBoxW(NULL, buffer, L"WinDivert SYS Placement Error", MB_ICONERROR | MB_OK);
    
    // 2. Extract DLL
    if (!ExtractResource(IDR_SMC_WINDIVERT_DLL1, "SMC_WINDIVERT_DLL", "SMCWinDivert.dll")) MessageBoxW(NULL, buffer, L"SMCWinDivert DLL Placement Error", MB_ICONERROR | MB_OK);

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

void RobloxLogScannerThread() {
    namespace fs = std::filesystem;
    char localAppData[MAX_PATH];
    
    // Regex to find IPs
    std::regex ip_regex(R"(\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b)");

    while (running)
    {
        while (running && !g_log_thread_running.load(std::memory_order_relaxed)) std::this_thread::sleep_for(std::chrono::milliseconds(200));
        if (!running) break;

		if (SHGetFolderPathA(NULL, CSIDL_LOCAL_APPDATA, NULL, 0, localAppData) == S_OK) {
			fs::path logDir = fs::path(localAppData) / "Roblox" / "logs";

			if (fs::exists(logDir)) {
				fs::path newestFile;
				fs::file_time_type newestTime;
				bool found = false;

				// 1. Find Newest Log File
				try {
					for (const auto& entry : fs::directory_iterator(logDir)) {
						if (entry.is_regular_file()) {
							if (!found || entry.last_write_time() > newestTime) {
								newestTime = entry.last_write_time();
								newestFile = entry.path();
								found = true;
							}
						}
					}
				} catch (...) {}

				// 2. Scan File
				if (found) {
					std::ifstream file(newestFile, std::ios::in | std::ios::binary);
					if (file.is_open()) {
                        
						// Optimization: Only read the last 5000KB. 
						// Connection IPs are usually at the end (new connections) or beginning.
						// Reading from the end catches server hops.
						file.seekg(0, std::ios::end);
						size_t size = file.tellg();
                        
						if (size > 5000000) file.seekg(size - 5000000);
						else file.seekg(0);

						std::string buffer(5000000, '\0');
						file.read(&buffer[0], 5000000); 
                        
						auto words_begin = std::sregex_iterator(buffer.begin(), buffer.end(), ip_regex);
						auto words_end = std::sregex_iterator();

						bool new_ip_found = false;

						for (std::sregex_iterator i = words_begin; i != words_end; ++i) {
							std::smatch match = *i;
							std::string ip_str = match.str();

							// --- FILTER OUT JUNK IPs ---
							// 127.x.x.x (Localhost)
							if (ip_str.rfind("127.", 0) == 0) continue;
							// 10.x.x.x (Private Class A)
							if (ip_str.rfind("10.", 0) == 0) continue;
							// 192.168.x.x (Private Class C)
							if (ip_str.rfind("192.168.", 0) == 0) continue;
							// 172.16.x.x - 172.31.x.x (Private Class B - simplified check)
							if (ip_str.rfind("172.", 0) == 0) continue;
							// 0.0.0.0
							if (ip_str == "0.0.0.0") continue;

							// Thread-Safe Insert
							{
								std::unique_lock lock(g_ip_mutex);
								if (g_roblox_dynamic_ips.find(ip_str) == g_roblox_dynamic_ips.end()) {
									g_roblox_dynamic_ips.insert(ip_str);
									new_ip_found = true;
                                    
									std::cout << "Found Roblox Server: " << ip_str << std::endl;
								}
							}
						}

						if (new_ip_found && bWinDivertEnabled) {
							SafeCloseWinDivert();
						}
					}
				}
			}
		}
        
		std::this_thread::sleep_for(std::chrono::seconds(2));
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

    // Safety Timer State
    auto last_safety_pulse_time = std::chrono::steady_clock::now();
    bool safety_pulse_active = false;

    // Ensure all functions are loaded
    if (!pWinDivertOpen || !pWinDivertRecv || !pWinDivertSend || !pWinDivertClose || 
        !pWinDivertHelperParsePacket || !pWinDivertHelperCalcChecksums) {
        std::cout << "WinDivert functions not loaded!" << std::endl;
        return;
    }

    while (g_windivert_running) {
        
        // 1. Construct Filter String
        std::string direction_filter = "";
        if (lagswitchinbound && lagswitchoutbound) direction_filter = "(inbound or outbound)";
        else if (lagswitchinbound) direction_filter = "inbound";
        else if (lagswitchoutbound) direction_filter = "outbound";
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

		if (prevent_disconnect) {
			final_filter = final_filter + " and udp";
		}

        g_current_windivert_filter = final_filter; 

        // Open WinDivert
        {
            std::lock_guard<std::mutex> lock(g_windivert_handle_mutex);
            hWindivert = pWinDivertOpen(final_filter.c_str(), WINDIVERT_LAYER_NETWORK, 0, 0);
        }

        if (hWindivert == INVALID_HANDLE_VALUE) {
            if (GetLastError() == ERROR_INVALID_PARAMETER && !g_roblox_dynamic_ips.empty()) {
                 std::unique_lock lock(g_ip_mutex);
                 g_roblox_dynamic_ips.clear();
            }
            std::this_thread::sleep_for(std::chrono::milliseconds(500));
            continue; 
        }

        // Reset timer on open
        last_safety_pulse_time = std::chrono::steady_clock::now();

        // --- INNER LOOP ---
        while (g_windivert_running) {
            if (!pWinDivertRecv(hWindivert, packet.get(), WINDIVERT_MTU_MAX, &packetLen, &addr)) {
                break;
            }

            if (g_windivert_blocking) {
                if (prevent_disconnect) {
                    
                    // ----------------------------------------------------
                    // DYNAMIC SAFETY PULSE LOGIC
                    // ----------------------------------------------------
                    long long interval_ms = 0;

                    if (lagswitchinbound && lagswitchoutbound) {
                        // BOTH: Unlag every 20 seconds
                        interval_ms = 24000; 
                    } else if (lagswitchoutbound) {
                        // OUTBOUND ONLY: Unlag every 4m 50s (290s)
                        interval_ms = 290000;
                    } else {
                        // INBOUND ONLY: Infinite (0)
                        interval_ms = 0;
                    }

                    if (interval_ms > 0) {
                        auto now = std::chrono::steady_clock::now();
                        auto elapsed_ms = std::chrono::duration_cast<std::chrono::milliseconds>(now - last_safety_pulse_time).count();

                        // Check if time to pulse
                        if (elapsed_ms >= interval_ms) {
                            safety_pulse_active = true;
                        }

                        // If pulse is active, allow traffic for 50ms
                        if (safety_pulse_active) {
                            if (elapsed_ms >= (interval_ms + 50)) {
                                // Pulse Finished -> Reset Timer
                                last_safety_pulse_time = std::chrono::steady_clock::now();
                                safety_pulse_active = false;
                            } else {
                                // Pulse Active -> Force Send Packet
								pWinDivertSend(hWindivert, packet.get(), packetLen, NULL, &addr);
                                continue;
                            }
                        }
                    }
                    // ----------------------------------------------------

                    bool drop_packet = false; // Default: Send (Safe fallback)

                    if (pWinDivertHelperParsePacket(packet.get(), packetLen, 
                        &ip_header, &ipv6_header, &protocol, 
                        &icmp_header, &icmpv6_header, &tcp_header, &udp_header, 
                        &payload, &payload_len, &next, &next_len) && payload != nullptr) 
                    {
                        // Check Header: 01 00 00 [17/1F] 01 11
                        if (payload_len >= 90) {
                            unsigned char* p = (unsigned char*)payload;
                            
                            // 1. Check common bytes
                            if (p[0] == 0x01 && p[1] == 0x00 && p[2] == 0x00 && 
                                p[4] == 0x01 && p[5] == 0x11) 
                            {
                                // 2. Check The Differentiator Byte (Index 3)
                                if (!addr.Outbound) {
                                    // INBOUND: Expect 0x17
                                    if (p[3] == 0x17) drop_packet = true;
                                } 
                                else {
                                    // OUTBOUND: Expect 0x1F
                                    if (p[3] == 0x1F) drop_packet = true;
                                }
                            }
                        }
                    }

                    if (drop_packet) continue; // Drop Physics
                    else {
                        pWinDivertSend(hWindivert, packet.get(), packetLen, NULL, &addr); // Send Heartbeat/Other
                        continue;
                    }
                }
                
                // If Prevent Disconnect is OFF -> Drop Everything
                continue; 
            }

            // Not Blocking (Lag Switch OFF) -> Keep timer fresh & send packets
            last_safety_pulse_time = std::chrono::steady_clock::now();
            pWinDivertSend(hWindivert, packet.get(), packetLen, NULL, &addr);
        }

        SafeCloseWinDivert();
    }
}