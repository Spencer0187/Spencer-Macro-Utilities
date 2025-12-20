#pragma once
#include <windows.h>
#include <atomic>
#include <vector>
#include <string>
#include <unordered_map>
#include <chrono>
#include <shared_mutex>
#include <mutex>
#include <set>

#include "windivert-files/windivert.h"

namespace Globals {
    // --- Application & Window State ---
    inline HWND hwnd = NULL;
    inline std::atomic<bool> running = true;
    inline std::atomic<bool> done = false;
    inline std::atomic<unsigned int> RobloxFPS = 120;
    inline std::vector<DWORD> targetPIDs;
    inline std::vector<HANDLE> hProcess;
    inline int screen_width = GetSystemMetrics(SM_CXSCREEN) / 1.5;
    inline int screen_height = GetSystemMetrics(SM_CYSCREEN) / 1.5 + 10;
    inline int raw_window_width = 0;
    inline int raw_window_height = 0;
    inline int WindowPosX = 0;
    inline int WindowPosY = 0;
    inline float windowOpacityPercent = 100.0f;
    inline bool processFound = false;
    inline bool UserOutdated = false;
    inline bool ontoptoggle = false;

    // --- Macro Atomic Flags ---
    inline std::atomic<bool> isdesyncloop{ false };
    inline std::atomic<bool> isspeed{ false };
    inline std::atomic<bool> isHHJ{ false };
    inline std::atomic<bool> isspamloop{ false };
    inline std::atomic<bool> isitemloop{ false };
    inline std::atomic<bool> iswallwalkloop{ false };
    inline std::atomic<bool> isbhoploop{ false };
    inline std::atomic<bool> isafk{ false };
    inline std::atomic<bool> iswallhopthread{ false };
    inline std::atomic<bool> ispresskeythread{ false };
    inline std::atomic<bool> isfloorbouncethread{ false };
    inline std::atomic<bool> g_isVk_BunnyhopHeldDown{ false };

    // --- Keybind Variables (Virtual Keys) ---
    inline unsigned int vk_f5 = VK_F5;
    inline unsigned int vk_f6 = VK_F6;
    inline unsigned int vk_f8 = VK_F8;
    inline unsigned int vk_mbutton = VK_MBUTTON;
    inline unsigned int vk_xbutton1 = VK_XBUTTON1;
    inline unsigned int vk_xbutton2 = VK_XBUTTON2;
    inline unsigned int vk_spamkey = VK_LBUTTON;
    inline unsigned int vk_clipkey = VK_F3;
    inline unsigned int vk_wallkey = VK_F1;
    inline unsigned int vk_laughkey = VK_F7;
    inline unsigned int vk_shiftkey = VK_SHIFT;
    inline unsigned int vk_enterkey = VK_RETURN;
    inline unsigned int vk_floorbouncekey = VK_F4;
    
    // API-initialized keys
    inline unsigned int vk_zkey = VkKeyScanEx('Z', GetKeyboardLayout(0)) & 0xFF;
    inline unsigned int vk_dkey = VkKeyScanEx('D', GetKeyboardLayout(0)) & 0xFF;
    inline unsigned int vk_xkey = VkKeyScanEx('X', GetKeyboardLayout(0)) & 0xFF;
    inline unsigned int vk_wkey = VkKeyScanEx('W', GetKeyboardLayout(0)) & 0xFF;
    inline unsigned int vk_bouncekey = VkKeyScanEx('C', GetKeyboardLayout(0)) & 0xFF;
    inline unsigned int vk_leftbracket = MapVirtualKey(0x1A, MAPVK_VSC_TO_VK);
    inline unsigned int vk_bunnyhopkey = MapVirtualKey(0x39, MAPVK_VSC_TO_VK);
    inline unsigned int vk_chatkey = VkKeyScanEx('/', GetKeyboardLayout(0)) & 0xFF;
    inline unsigned int vk_afkkey = VkKeyScanEx('\\', GetKeyboardLayout(0)) & 0xFF;
    inline unsigned int vk_lagswitchkey = VkKeyScanEx('=', GetKeyboardLayout(0)) & 0xFF;

    // --- Main Toggles & Switches ---
    inline bool g_isLinuxWine = false;
    inline bool notbinding = true;
    inline bool macrotoggled = true;
    inline bool shiftswitch = false;
    inline bool unequiptoggle = false;
    inline bool camfixtoggle = false;
    inline bool wallhopswitch = false;
    inline bool wallwalktoggleside = false;
    inline bool wallhopcamfix = false;
    inline bool chatoverride = true;
    inline bool toggle_jump = true;
    inline bool toggle_flick = true;
    inline bool fasthhj = false;
    inline bool globalzoomin = false;
    inline bool globalzoominreverse = false;
    inline bool wallesslhjswitch = false;
    inline bool autotoggle = false;
    inline bool isspeedswitch = false;
    inline bool isfreezeswitch = false;
    inline bool freezeoutsideroblox = true;
    inline bool iswallwalkswitch = false;
    inline bool isspamswitch = false;
    inline bool isitemclipswitch = false;
    inline bool antiafktoggle = true;
    inline bool bouncesidetoggle = false;
    inline bool bouncerealignsideways = true;
    inline bool bounceautohold = true;
    inline bool laughmoveswitch = false;
    inline bool takeallprocessids = false;
    inline bool bunnyhoptoggled = false;
    inline bool bunnyhopsmart = true;
    inline bool presskeyinroblox = false;
    inline bool unequipinroblox = false;
    inline bool shortdescriptions = false;
    inline bool useoldpaste = true;
    inline bool doublepressafkkey = true;
    inline bool floorbouncehhj = false;
    inline bool showadvancedhhjbounce = false;
    inline bool DontShowAdminWarning = false;
    inline bool show_lag_overlay = false;
    inline bool overlay_hide_inactive = false;
    inline bool overlay_use_bg = false;

    // --- Numeric Settings ---
    inline int wallhop_dx = 300;
    inline int wallhop_dy = -300;
    inline int speed_strengthx = 959;
    inline int speed_strengthy = -959;
    inline int wallwalk_strengthx = 94;
    inline int wallwalk_strengthy = -94;
    inline int speedoffsetx = 0;
    inline int speedoffsety = 0;
    inline int speed_slot = 3;
    inline int desync_slot = 5;
    inline int clip_slot = 7;
    inline int clip_delay = 30;
    inline int BunnyHopDelay = 10;
    inline int RobloxWallWalkValueDelay = 72720;
    inline float spam_delay = 20.0f;
    inline float maxfreezetime = 9.00f;
    inline float PreviousSensValue = -1.0f;
    inline float PreviousWallWalkSide = 0;
    inline float PreviousWallWalkValue = 0.5f;
    inline int maxfreezeoverride = 50;
    inline int real_delay = 1000;
    inline int RobloxPixelValue = 716;
    inline int RobloxWallWalkValue = -94;
    inline int WallhopDelay = 17;
    inline int WallhopBonusDelay = 0;
    inline int AntiAFKTime = 15;
    inline int display_scale = 100;
    inline int PressKeyDelay = 16;
    inline int PressKeyBonusDelay = 0;
    inline int PasteDelay = 1;
    inline int HHJLength = 243;
    inline int HHJFreezeDelayOverride = 0;
    inline int HHJDelay1 = 9;
    inline int HHJDelay2 = 17;
    inline int HHJDelay3 = 16;
    inline int FloorBounceDelay1 = 5;
    inline int FloorBounceDelay2 = 8;
    inline int FloorBounceDelay3 = 100;

    // --- Overlay Ints ---
    inline int overlay_x = -1;
    inline int overlay_y = 50;
    inline int overlay_size = 20;
    inline float overlay_bg_r = 0.0f;
    inline float overlay_bg_g = 0.0f;
    inline float overlay_bg_b = 0.0f;

    // --- Buffers & Strings ---
    inline char settingsBuffer[256] = "RobloxPlayerBeta.exe";
    inline char ItemDesyncSlot[256] = "5";
    inline char ItemSpeedSlot[256] = "3";
    inline char ItemClipSlot[256] = "7";
    inline char ItemClipDelay[256] = "34";
    inline char BunnyHopDelayChar[256] = "10";
    inline char WallhopPixels[256] = "300";
    inline char WallhopDelayChar[256] = "19";
    inline char WallhopBonusDelayChar[256] = "0";
    inline char WallhopDegrees[256] = "150";
    inline char SpamDelay[256] = "20";
    inline char RobloxSensValue[256] = "0.5";
    inline char RobloxPixelValueChar[256] = "716";
    inline char RobloxWallWalkValueChar[256] = "-94";
    inline char RobloxWallWalkValueDelayChar[256] = "72720";
    inline char CustomTextChar[8192] = "";
    inline char RobloxFPSChar[256] = "60";
    inline char AntiAFKTimeChar[256] = "15";
    inline char PressKeyDelayChar[256] = "16";
    inline char PressKeyBonusDelayChar[256] = "0";
    inline char PasteDelayChar[256] = "1";
    inline char HHJLengthChar[16] = "243";
    inline char HHJFreezeDelayOverrideChar[16] = "0";
    inline char HHJDelay1Char[16] = "9";
    inline char HHJDelay2Char[16] = "17";
    inline char HHJDelay3Char[16] = "16";
    inline char FloorBounceDelay1Char[16] = "5";
    inline char FloorBounceDelay2Char[16] = "8";
    inline char FloorBounceDelay3Char[16] = "100";
    inline std::string text = "/e dance2";

    // --- Section Management ---
    inline constexpr int section_amounts = 16;
    inline int section_order[16] = {0, 1, 2, 15, 3, 4, 5, 6, 13, 7, 8, 9, 10, 11, 12, 14};
    inline bool section_toggles[16] = {true, true, true, true, true, false, true, true, true, false, false, false, false, false, false, false};
    inline int selected_section = -1;
    inline int dragged_section = -1;
    inline int selected_dropdown = 0;

    // --- Profile Management Constants ---
    inline const std::string G_SETTINGS_FILEPATH = "RMCSettings.json";
    inline std::string G_CURRENTLY_LOADED_PROFILE_NAME = "";
    inline const std::string METADATA_KEY = "_metadata";
    inline const std::string LAST_ACTIVE_PROFILE_KEY = "last_active_profile";

    // --- Lookup Tables ---
    inline const std::unordered_map<int, std::string> vkToString = {
        {VK_LBUTTON, "VK_LBUTTON"}, {VK_RBUTTON, "VK_RBUTTON"}, {VK_CANCEL, "VK_CANCEL"},
        {VK_MBUTTON, "VK_MBUTTON"}, {VK_XBUTTON1, "VK_XBUTTON1"}, {VK_XBUTTON2, "VK_XBUTTON2"},
        {VK_BACK, "VK_BACK"}, {VK_TAB, "VK_TAB"}, {VK_CLEAR, "VK_CLEAR"}, {VK_RETURN, "VK_RETURN"},
        {VK_SHIFT, "VK_SHIFT"}, {VK_CONTROL, "VK_CONTROL"}, {VK_MENU, "VK_MENU"}, {VK_PAUSE, "VK_PAUSE"},
        {VK_CAPITAL, "VK_CAPITAL"}, {VK_ESCAPE, "VK_ESCAPE"}, {VK_SPACE, "VK_SPACE"}, {VK_PRIOR, "VK_PRIOR"},
        {VK_NEXT, "VK_NEXT"}, {VK_END, "VK_END"}, {VK_HOME, "VK_HOME"}, {VK_LEFT, "VK_LEFT"},
        {VK_UP, "VK_UP"}, {VK_RIGHT, "VK_RIGHT"}, {VK_DOWN, "VK_DOWN"}, {VK_SELECT, "VK_SELECT"},
        {VK_PRINT, "VK_PRINT"}, {VK_EXECUTE, "VK_EXECUTE"}, {VK_SNAPSHOT, "VK_SNAPSHOT"},
        {VK_INSERT, "VK_INSERT"}, {VK_DELETE, "VK_DELETE"}, {VK_HELP, "VK_HELP"}, {VK_LWIN, "VK_LWIN"},
        {VK_RWIN, "VK_RWIN"}, {VK_NUMPAD0, "VK_NUMPAD0"}, {VK_NUMPAD1, "VK_NUMPAD1"},
        {VK_NUMPAD2, "VK_NUMPAD2"}, {VK_NUMPAD3, "VK_NUMPAD3"}, {VK_NUMPAD4, "VK_NUMPAD4"},
        {VK_NUMPAD5, "VK_NUMPAD5"}, {VK_NUMPAD6, "VK_NUMPAD6"}, {VK_NUMPAD7, "VK_NUMPAD7"},
        {VK_NUMPAD8, "VK_NUMPAD8"}, {VK_NUMPAD9, "VK_NUMPAD9"}, {VK_MULTIPLY, "VK_MULTIPLY"},
        {VK_ADD, "VK_ADD"}, {VK_SEPARATOR, "VK_SEPARATOR"}, {VK_SUBTRACT, "VK_SUBTRACT"},
        {VK_DECIMAL, "VK_DECIMAL"}, {VK_DIVIDE, "VK_DIVIDE"}, {VK_LSHIFT, "VK_LSHIFT"},
        {VK_RSHIFT, "VK_RSHIFT"}, {VK_LCONTROL, "VK_LCONTROL"}, {VK_RCONTROL, "VK_RCONTROL"},
        {VK_LMENU, "VK_LMENU"}, {VK_RMENU, "VK_RMENU"}, {VK_OEM_PLUS, "VK_OEM_PLUS"},
        {VK_OEM_COMMA, "VK_OEM_COMMA"}, {VK_OEM_MINUS, "VK_OEM_MINUS"}, {VK_OEM_PERIOD, "VK_OEM_PERIOD"},
        {VK_OEM_2, "VK_OEM_2"}, {VK_OEM_3, "VK_OEM_3"}, {VK_OEM_4, "VK_OEM_4"}, {VK_OEM_5, "VK_OEM_5"},
        {VK_OEM_6, "VK_OEM_6"}, {VK_OEM_7, "VK_OEM_7"}, {VK_OEM_8, "VK_OEM_8"}, {0x0, "RESTART PC"},
        {256, "M_WHEEL_UP"}, {257, "M_WHEEL_DOWN"}
    };

    inline const std::unordered_map<int, unsigned int*> section_to_key = {
        {0, &vk_mbutton}, {1, &vk_f5}, {2, &vk_xbutton1}, {3, &vk_xkey},
        {4, &vk_f8}, {5, &vk_zkey}, {6, &vk_xbutton2}, {7, &vk_f6},
        {8, &vk_clipkey}, {9, &vk_laughkey}, {10, &vk_wallkey}, {11, &vk_leftbracket},
        {12, &vk_bouncekey}, {13, &vk_bunnyhopkey}, {14, &vk_floorbouncekey}, {15, &vk_lagswitchkey}
    };

    // Definitions (the actual storage)
    inline bool islagswitchswitch = true;
    inline bool lagswitchoutbound = true;
    inline bool lagswitchinbound = true;
    inline bool lagswitchtargetroblox = true;
    inline bool prevent_disconnect = true;
    inline bool bShowAdminPopup = false;
    inline bool lagswitch_autounblock = false;
    inline float lagswitch_max_duration = 9.00f;
    inline int lagswitch_unblock_ms = 50;
    inline bool bWinDivertEnabled = false;
    inline bool bDependenciesLoaded = false;

    inline std::string g_current_windivert_filter = "true";

    inline std::atomic<bool> g_windivert_running(false);
    inline std::atomic<bool> g_windivert_blocking(false);
    inline std::atomic<bool> g_log_thread_running(false);
    inline std::atomic<bool> g_ip_list_updated(false);
    inline auto lagswitch_start_time = std::chrono::steady_clock::now();

    inline const std::string ROBLOX_RANGE_FILTER = "((ip.SrcAddr >= 128.116.0.0 and ip.SrcAddr <= 128.116.255.255) or (ip.DstAddr >= 128.116.0.0 and ip.DstAddr <= 128.116.255.255))";

    inline std::set<std::string> g_roblox_dynamic_ips;
    inline std::shared_mutex g_ip_mutex;

    inline HANDLE hWindivert = INVALID_HANDLE_VALUE;
    inline std::mutex g_windivert_handle_mutex;

    // --- WinDivert Function Pointers ---
    typedef HANDLE (*tWinDivertOpen)(const char *filter, WINDIVERT_LAYER layer, INT16 priority, UINT64 flags);
    typedef BOOL (*tWinDivertRecv)(HANDLE handle, PVOID pPacket, UINT packetLen, UINT *pRecvLen, WINDIVERT_ADDRESS *pAddr);
    typedef BOOL (*tWinDivertSend)(HANDLE handle, const PVOID pPacket, UINT packetLen, UINT *pSendLen, const WINDIVERT_ADDRESS *pAddr);
    typedef BOOL (*tWinDivertClose)(HANDLE handle);
    typedef BOOL (*tWinDivertHelperParsePacket)(
	    const void *pPacket, UINT packetLen, PWINDIVERT_IPHDR *ppIpHdr,
	    PWINDIVERT_IPV6HDR *ppIpv6Hdr, UINT8 *pProtocol, PWINDIVERT_ICMPHDR *ppIcmpHdr,
	    PWINDIVERT_ICMPV6HDR *ppIcmpv6Hdr, PWINDIVERT_TCPHDR *ppTcpHdr, PWINDIVERT_UDPHDR *ppUdpHdr,
	    PVOID *ppData, UINT *pDataLen, PVOID *ppNext, UINT *pNextLen);
    typedef UINT64 (*tWinDivertHelperCalcChecksums)(void* pPacket, UINT packetLen, WINDIVERT_ADDRESS* pAddr, UINT64 flags);

    // DLL mapping pointers
    inline tWinDivertOpen pWinDivertOpen = nullptr;
    inline tWinDivertRecv pWinDivertRecv = nullptr;
    inline tWinDivertSend pWinDivertSend = nullptr;
    inline tWinDivertClose pWinDivertClose = nullptr;
    inline tWinDivertHelperParsePacket pWinDivertHelperParsePacket = nullptr;
    inline tWinDivertHelperCalcChecksums pWinDivertHelperCalcChecksums = nullptr;
}