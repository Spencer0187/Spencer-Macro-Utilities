#include <cstdlib>
#include <iostream>
#include <vector>
#include <windows.h>
#include <Psapi.h>
#include <tchar.h>
#include <fcntl.h>
#include <io.h>
#include <conio.h>
#include <thread>
#include <chrono>
#include <cwctype>
#include <string>
#include <sstream>
#include <atomic>
#include <mutex>
#include <array>
#include <algorithm>
#include <cctype>

// TO ENABLE FREEZING, CREATE A .CMD in the same folder that just does (nameofthisexe).exe RobloxPlayerBeta.exe (or whatever your roblox process is called)


// TO PUT IN A KEYBOARD KEY, GO TO https://www.millisecond.com/support/docs/current/html/language/scancodes.htm
// Convert the scancode into hexadecimal before putting it into the HoldKey or ReleaseKey functions
// Ex: E = 18 = 0x12 = HoldKey(0x12)

// If you want to create custom HOTKEYS for stuff that isn't an alphabet/function key, go to https://learn.microsoft.com/en-us/windows/win32/inputdev/virtual-key-codes and get your virtual key code or value

std::string text = "/e dance2"; // INSERT CUSTOM TEXT HERE

std::atomic<bool> isclip2(false); // Set the variable used for the alternate thread
std::atomic<bool> isspeed(false);
std::atomic<bool> isHHJ(false);

const DWORD SCAN_CODE_FLAGS = KEYEVENTF_SCANCODE;
const DWORD RELEASE_FLAGS = KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP;

INPUT inputkey = {};
INPUT inputhold = {};
INPUT inputrelease = {};

// Translate all VK's into variables so it's less annoying to debug it
unsigned int vk_f5 = VK_F5;
unsigned int vk_f6 = VK_F6;
unsigned int vk_f8 = VK_F8;
unsigned int vk_mbutton = VK_MBUTTON;
unsigned int vk_xbutton1 = VK_XBUTTON1;
unsigned int vk_xbutton2 = VK_XBUTTON2;
unsigned int vk_zkey = VkKeyScanEx('Z', GetKeyboardLayout(0)) & 0xFF; // Use this for alphabet keys so it works across layouts
unsigned int vk_xkey = VkKeyScanEx('X', GetKeyboardLayout(0)) & 0xFF;
unsigned int vk_wkey = VkKeyScanEx('W', GetKeyboardLayout(0)) & 0xFF;
unsigned int vk_slashkeyinit = VkKeyScanEx('/', GetKeyboardLayout(0)) & 0xFF;
unsigned int vk_slashkey = MapVirtualKey(vk_slashkeyinit, MAPVK_VK_TO_VSC);

int wallhop_dx = 300;
int wallhop_dy = -300;
int speed_strengthx = 959;
int speedoffsetx = 0;
int speed_strengthy = -959;
int speedoffsety = 0;
auto suspendStartTime = std::chrono::steady_clock::time_point();
const int suspendDuration = 9000;
const int unsuspendTime = 50;

int clip_slot = 6; // MUST BE ONE ABOVE YOUR ACTUAL SLOT NUMBER

typedef LONG(NTAPI *NtSuspendProcess)(HANDLE ProcessHandle);
typedef LONG(NTAPI *NtResumeProcess)(HANDLE ProcessHandle);

// Function to suspend or resume a process
void SuspendOrResumeProcess(NtSuspendProcess pfnSuspend, NtResumeProcess pfnResume, HANDLE hProcess,
			    bool suspend)
{
	if (suspend) {
		pfnSuspend(hProcess);
	} else {
		pfnResume(hProcess);
	}
}

bool ProcessKey(const std::wstring &key, unsigned int &custom_vk)
{
	// Convert the key to uppercase to handle cases like "f6"
	std::wstring uppercaseKey = key;
	std::transform(uppercaseKey.begin(), uppercaseKey.end(), uppercaseKey.begin(), ::towupper);

	// Check if the key is a single alphabet character
	if (uppercaseKey.length() == 1 && std::iswalpha(uppercaseKey[0])) {
		// Convert to uppercase
		custom_vk = VkKeyScanEx(uppercaseKey[0], GetKeyboardLayout(0)) & 0xFF;
		return true;
	}
	// Check if the key is a function key (F1-F12)
	else if (uppercaseKey.length() >= 2 && uppercaseKey[0] == L'F' &&
		 std::isdigit(uppercaseKey[1])) {
		int funcKeyNumber = std::stoi(uppercaseKey.substr(1));
		if (funcKeyNumber >= 1 && funcKeyNumber <= 12) {
			custom_vk = VK_F1 + (funcKeyNumber - 1);
			return true;
		}
	}
	// Check if the key is a virtual key code in hexadecimal format (e.g., "0x11" for Control)
	else if (uppercaseKey.length() >= 3 && uppercaseKey.substr(0, 2) == L"0X") {
		std::wstringstream ss;
		ss << std::hex << uppercaseKey.substr(2);
		ss >> custom_vk;
		return true;
	}

	// If it's not a valid alphabet key, function key, or virtual key code, return false
	return false;
}


INPUT createInput()
{
	INPUT inputkey = {};
	inputkey.type = INPUT_KEYBOARD;
	return inputkey;
}

// Hold a key down, self explanatory
void HoldKey(WORD scanCode)
{
	INPUT input = {};
	input.type = INPUT_KEYBOARD;
	input.ki.wScan = scanCode;
	input.ki.dwFlags = SCAN_CODE_FLAGS;

	SendInput(1, &input, sizeof(INPUT));
}

void ReleaseKey(WORD scanCode)
{
	INPUT input = {};
	input.type = INPUT_KEYBOARD;
	input.ki.wScan = scanCode;
	input.ki.dwFlags = RELEASE_FLAGS;

	SendInput(1, &input, sizeof(INPUT));
}

// Move your mouse to any coordinate
void MoveMouse(int dx, int dy)
{
	INPUT input = {0};
	input.type = INPUT_MOUSE;
	input.mi.dx = dx;
	input.mi.dy = dy;
	input.mi.dwFlags = MOUSEEVENTF_MOVE;

	SendInput(1, &input, sizeof(INPUT));
}

void PasteText(const std::string &text) // To run, just do PasteText(text), which is the name of the variable above at the beginning
{
	for (char c : text) {
		INPUT input = {0};
		input.type = INPUT_KEYBOARD;
		input.ki.wVk = 0;
		input.ki.wScan = c;
		input.ki.dwFlags = KEYEVENTF_UNICODE;

		SendInput(1, &input, sizeof(INPUT));
	}
}

// This is ran in a separate thread to avoid interfering with other functions
void ItemClipLoop2()
{
	while (true) { // Efficient variable checking method
		while (!isclip2) {
			std::this_thread::sleep_for(std::chrono::microseconds(100));
		}
		HoldKey(clip_slot);
		ReleaseKey(clip_slot);
		HoldKey(clip_slot);
		ReleaseKey(clip_slot);
	}
}

void Speedglitchloop()
{
	while (true) {
		while (!isspeed) {
			std::this_thread::sleep_for(std::chrono::microseconds(50));
		}
		MoveMouse(speed_strengthx, 0);
		std::this_thread::sleep_for(std::chrono::microseconds(6060));
		MoveMouse(speed_strengthy, 0);
		std::this_thread::sleep_for(std::chrono::microseconds(6060));
	}
}

void SpeedglitchloopHHJ()
{
	speedoffsetx = speed_strengthx * 1.04;
	speedoffsety = speed_strengthy * 1.04;
	while (true) {
		while (!isHHJ) {
			std::this_thread::sleep_for(std::chrono::microseconds(50));
		}
		MoveMouse(speedoffsetx, 0);
		std::this_thread::sleep_for(std::chrono::microseconds(3030));
		MoveMouse(speedoffsety, 0);
		std::this_thread::sleep_for(std::chrono::microseconds(3030));
	}
}

HWND FindWindowByProcessHandle(HANDLE hProcess)
{
	DWORD targetPID = GetProcessId(hProcess);
	HWND hwnd = FindWindowEx(NULL, NULL, NULL, NULL);
	while (hwnd != NULL) {
		DWORD windowPID = 0;
		GetWindowThreadProcessId(hwnd, &windowPID);
		if (windowPID == targetPID) {
			return hwnd;
		}
		hwnd = FindWindowEx(NULL, hwnd, NULL, NULL);
	}
	return NULL;
}

bool processFound = false; // Initialize as no process found

bool IsForegroundWindowProcess(const TCHAR *processName) // Check the current active window
{
	HWND hwnd2 = GetForegroundWindow();
	if (hwnd2 == nullptr) {
		return false;
	}

	DWORD processID;
	GetWindowThreadProcessId(hwnd2, &processID);

	HANDLE hProcess =
		OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, FALSE, processID);
	if (hProcess == nullptr) {
		return false;
	}

	TCHAR windowProcessName[MAX_PATH];
	bool result = false;

	if (GetModuleBaseName(hProcess, nullptr, windowProcessName,
			      sizeof(windowProcessName) / sizeof(TCHAR))) {
		result = (_tcscmp(windowProcessName, processName) == 0);
	}

	CloseHandle(hProcess);
	return result;
}

int wmain(int argc, wchar_t *__restrict argv[])
{

	const HMODULE hNtdll = GetModuleHandleA("ntdll"); // START OF SUSPENDING GOBBLEDYGOOK (DONT TOUCH)
	NtSuspendProcess pfnSuspend =
		reinterpret_cast<NtSuspendProcess>(GetProcAddress(hNtdll, "NtSuspendProcess"));
	NtResumeProcess pfnResume =
		reinterpret_cast<NtResumeProcess>(GetProcAddress(hNtdll, "NtResumeProcess"));

	wchar_t szProcessName[MAX_PATH];
	DWORD aProcesses[1024];
	DWORD cbNeeded;

	std::vector<wchar_t *> nameList;
	nameList.reserve(argc - 1);
	std::vector<DWORD> processList;

	_setmode(_fileno(stdout), _O_WTEXT);

	if (!EnumProcesses(aProcesses, sizeof(aProcesses), &cbNeeded)) {
		return 1;
	}

	const DWORD cProcesses = cbNeeded / sizeof(DWORD);

	for (wchar_t **arg = argv + 1; arg < argv + argc; ++arg) {
		nameList.push_back(*arg);
	}

	HANDLE hProcess = nullptr;
	for (DWORD i = 1; i < cProcesses; ++i) {
		hProcess = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SUSPEND_RESUME,
				       FALSE, aProcesses[i]);
		if (hProcess && GetProcessImageFileNameW(hProcess, szProcessName, MAX_PATH)) {
			wchar_t *exeName = wcsrchr(szProcessName, L'\\') + 1;
			if (!nameList.empty() && !wcscmp(exeName, nameList[0])) {
				processFound = true;
				break;
			}
			CloseHandle(hProcess);
			hProcess = nullptr;
		}
	} // END OF SUSPENDING GOBBLEDYGOOK (DONT TOUCH)

	std::wcout << L"Processes to be managed:\n";
	if (processFound) {
		std::wcout << L"Program found!" << L"\n";
		std::wcout << L"Press F5 to item desync (keep it on for 3-4 seconds and equip your item beforehand in your 5 slot and have it\nactively equipped)" << L"\n";
		std::wcout << L"Press Z to press the \"D\" key for one frame" << L"\n";
		std::wcout << L"Press X to speedglitch (must have com offset and 60 fps), configure this value until it is a 180 degree turn\nthen subtract a few pixels from that to let you move forwards instead of backwards" << L"\n";
		std::wcout << L"Press Xbutton1 to do a Helicopter High Jump, if you offset your center of mass horizontally, and then turn\ninto a wall to get your legs stuck inside of it, and then activate the macro, you will fly upwards" << L"\n";
		std::wcout << L"However, you need to be on 60-120 FPS (120 seems to work best) and have your speedglitch set up PROPERLY" << L"\n";
		std::wcout << L"Press Xbutton2 to automatically wallhop" << L"\n";
		std::wcout << L"Press middle mouse button to automatically tab glitch (freeze roblox)" << L"\n";
		std::wcout << L"Press F6 to Walless LHJ (SHIFTLOCK OFF) (REQUIRES A SIDEWAYS COM OFFSET BY EQUIPPING ITEM OR RESPAWN COM AND .5 STUDS OF A FOOT ON A PLATFORM)" << L"\n";
		std::wcout << L"Press F8 to automatically /e dance2 and equip and equip the item in your \"3\" slot to get a weaker, but itemless version of the speedglitch." << L"\n";
		std::wcout << L"Change the -'s inside of your .cmd file to whatever ALPHABET hotkey, F1-F12, or a windows.h key VALUE" << L"\n";
		std::wcout << L"\nThe order of them is: Process name > Wallhop Strength > Speedglitch strength > Freezing > Item Spam Desync > Helicopter High Jump (HHJ) > Speedglitch > Unequip Speed > Press D > Wallhop > Walless LHJ" << L"\n";
	}
	if (!processFound) {
		std::wcout << "TO ENABLE FREEZING, CREATE A .CMD in the same folder that just does (nameofthisexe).exe RobloxPlayerBeta.exe (or whatever your roblox process is called)" << std::endl;
		std::wcout << "ALERT! Roblox has NOT been found, run the .cmd file and NOT the .exe file as you need to put the roblox process as your parameter!" << std::endl;
		std::wcout << "If you did do this, check for spelling and make sure its CASE SENSITIVE AND HAS THE .exe AT THE END!" << std::endl;
		std::wcout << "The program will still run, but you will not be able to suspend/freeze Roblox at all, so relaunch it properly." << std::endl;
		std::wcout << L"Change the -'s inside of your .cmd file to whatever ALPHABET hotkey, F1-F12, or a windows.h key VALUE" << L"\n";
		std::wcout << L"\nThe order of them is: Process name > Wallhop Strength > Speedglitch strength > Freezing > Item Spam Desync > Helicopter High Jump (HHJ) > Speedglitch > Unequip Speed > Press D > Wallhop > Walless LHJ" << L"\n";
		std::wcout << "Program will open in 1 second." << std::endl;
		std::this_thread::sleep_for(std::chrono::seconds(1));
		}


	if (argc > 2) { // If the amount of arguments in the file is greater than two (the list starts at 1, and the .exe name is 2, so your wallhop should be 3, BUT the argv list doesnt include the .exe, so its 2
		wallhop_dx = std::wcstol(argv[2], nullptr, 10); // Wallhop dx is your wallhop strength, the angle that it rotates
		wallhop_dy = wallhop_dx * -1;
	} else {
		std::wcout << L"SOME OF YOUR -'s OR HOTKEYS ARE MISSING!" << std::endl;
	}

	
	if (argc > 3) { 
		speed_strengthx = std::wcstol(argv[3], nullptr, 10);
		speed_strengthy = speed_strengthx * -1;
	} else {
		std::wcout << L"SOME OF YOUR -'s OR HOTKEYS ARE MISSING!" << std::endl;
	}



	if (argc > 4) { // Freeze Hotkey (Alphabet or function)
		std::wstring key = argv[4];

		if (ProcessKey(key, vk_mbutton)) {
			std::wcout << L"\nCustom Freeze Hotkey Key Worked: " << argv[4]
				   << std::endl;
		} else {
			std::wcout << L"Using default Freeze Hotkey" << std::endl;
		}
	} else {
		std::wcout << L"SOME OF YOUR -'s OR HOTKEYS ARE MISSING!" << std::endl;
	}

	if (argc > 5) { // Item Desync
		std::wstring key = argv[5];

		if (ProcessKey(key, vk_f5)) {
			std::wcout << L"Custom Desync Hotkey Key Worked: " << argv[5] << std::endl;
		} else {
			std::wcout << L"Using default Desync Hotkey" << std::endl;
		}
	} else {
		std::wcout << L"SOME OF YOUR -'s OR HOTKEYS ARE MISSING!" << std::endl;
	}

	if (argc > 6) { // HHJ
		std::wstring key = argv[6];

		if (ProcessKey(key, vk_xbutton1)) {
			std::wcout << L"Custom HHJ Hotkey Key Worked: " << argv[6] << std::endl;
		} else {
			std::wcout << L"Using default HHJ Hotkey" << std::endl;
		}
	} else {
		std::wcout << L"SOME OF YOUR -'s OR HOTKEYS ARE MISSING!" << std::endl;
	}

	if (argc > 7) { // Speedglitch
		std::wstring key = argv[7];

		if (ProcessKey(key, vk_xkey)) {
			std::wcout << L"Custom Speedglitch Hotkey Key Worked: " << argv[7] << " Current Strength: " << speed_strengthx << std::endl;
		} else {
			std::wcout << L"Using default Speedglitch Hotkey with current Strength: " << speed_strengthx << std::endl;
		}
	} else {
		std::wcout << L"SOME OF YOUR -'s OR HOTKEYS ARE MISSING!" << std::endl;
	}

	if (argc > 8) { // Auto-Unequip Speed
		std::wstring key = argv[8];

		if (ProcessKey(key, vk_f8)) {
			std::wcout << L"Custom Unequip Speed Hotkey Key Worked: " << argv[8] << std::endl;
		} else {
			std::wcout << L"Using default Unequip Speed Hotkey" << std::endl;
		}
	} else {
		std::wcout << L"SOME OF YOUR -'s OR HOTKEYS ARE MISSING!" << std::endl;
	}

	if (argc > 9) { // Press D for one frame
		std::wstring key = argv[9];

		if (ProcessKey(key, vk_zkey)) {
			std::wcout << L"Custom Press D Hotkey Key Worked: " << argv[9] << std::endl;
		} else {
			std::wcout << L"Using default D Hotkey" << std::endl;
		}
	} else {
		std::wcout << L"SOME OF YOUR -'s OR HOTKEYS ARE MISSING!" << std::endl;
	}

	if (argc > 10) { // Wallhop
		std::wstring key = argv[10];

		if (ProcessKey(key, vk_xbutton2)) {
			std::wcout << L"Custom WallHop Hotkey Key Worked: " << argv[10] << " Current Strength: " << wallhop_dx << std::endl;
		} else {
			std::wcout << L"Using default WallHop Hotkey with current Strength: " << wallhop_dx << std::endl;
		}
	} else {
		std::wcout << L"SOME OF YOUR -'s OR HOTKEYS ARE MISSING!" << std::endl;
	}

	if (argc > 11) { // Walless LHJ
		std::wstring key = argv[11];

		if (ProcessKey(key, vk_f6)) {
			std::wcout << L"Custom Walless LHJ Hotkey Key Worked: " << argv[11]
				   << std::endl;
		} else {
			std::wcout << L"Using default LHJ Hotkey" << std::endl;
		}
	} else {
		std::wcout << L"SOME OF YOUR -'s OR HOTKEYS ARE MISSING!" << std::endl;
	}

	std::thread actionThread(Speedglitchloop); // Start a separate thread for item desync loop, ONLY do this for SPAMMERS that need to run ALONGSIDE other functions
	std::thread actionThread2(ItemClipLoop2);
	std::thread actionThread3(SpeedglitchloopHHJ);

	bool isdesync = false; // These variables are used for "one-click" functionalies for macros, so you can press a key and it runs every time that key is pressed (without overlapping itself)
	bool isSuspended = false; 
	bool islhj = false;
	bool ispressd = false;
	bool iswallhop = false;
	bool isspeedglitch = false;
	bool isspeed2 = false;
	int speed_slot = 3;
	bool HHJ = false;
	auto lastPressTime = std::chrono::steady_clock::now();
	HWND hwnd = FindWindowByProcessHandle(hProcess);

	while (true) {
		if (GetAsyncKeyState(vk_mbutton) & 0x8000) {
			if (!isSuspended) {
				SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, true); // Suspend the program on press
				isSuspended = true;
				suspendStartTime = std::chrono::steady_clock::now(); // Start the timer
			} else {
				// Check if 9 seconds have passed since suspension
				auto elapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
								   std::chrono::steady_clock::now() - suspendStartTime)
								   .count();
				if (elapsed >= suspendDuration) {
					// Unsuspend for 50 ms
					SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, false);
					std::this_thread::sleep_for(std::chrono::milliseconds(unsuspendTime));
					SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, true);

					// Reset the timer
					suspendStartTime = std::chrono::steady_clock::now();
				}
			}
		} else {
			if (isSuspended) {
				SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, false); // Unsuspend when button is released
				isSuspended = false;
			}
		}

		if ((GetAsyncKeyState(vk_f5) & 0x8000) && IsForegroundWindowProcess((nameList[0]))) { // Item Desync Macro with anti-idiot design
			if (!isdesync) {
				isclip2 = true;
				isdesync = true;
			}
		} else {
			isclip2 = false;
			isdesync = false;
		}

		if (GetAsyncKeyState(vk_zkey) & 0x8000) { // Press D for one frame
			if (!ispressd) {
				HoldKey(0x20);
				std::this_thread::sleep_for(std::chrono::milliseconds(6));
				ReleaseKey(0x20);
				ispressd = true;
			}
		} else {
			ispressd = false;
		}

		if (GetAsyncKeyState(vk_xbutton2) & 0x8000) { // Wallhop
			if (!iswallhop) {
				HoldKey(0x39);
				MoveMouse(wallhop_dx, 0);
				std::this_thread::sleep_for(std::chrono::milliseconds(75));
				MoveMouse(wallhop_dy, 0);
				ReleaseKey(0x39);
				iswallhop = true;
			}
		} else {
			iswallhop = false;
		}

		if (GetAsyncKeyState(vk_f6) & 0x8000) { // Walless LHJ (REQUIRES COM OFFSET AND .5 STUDS OF A FOOT ON A PLATFORM)
			if (!islhj) {
				HoldKey(0x20);
				std::this_thread::sleep_for(std::chrono::milliseconds(15));
				HoldKey(0x39);
				std::this_thread::sleep_for(std::chrono::milliseconds(20));
				SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, true);
				std::this_thread::sleep_for(std::chrono::milliseconds(50));
				ReleaseKey(0x20);
				std::this_thread::sleep_for(std::chrono::milliseconds(500));
				HoldKey(0x2A);
				SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, false);
				std::this_thread::sleep_for(std::chrono::milliseconds(50));
				ReleaseKey(0x2A);
				ReleaseKey(0x39);
				islhj = true;
			}
		} else {
			islhj = false;
		}

		if (GetAsyncKeyState(vk_xkey) & 0x8000) { // Speedglitch
			if (!isspeedglitch) {
				isspeed = !isspeed;
				isspeedglitch = true;
			}
		} else {
			isspeedglitch = false;
		}

		if (GetAsyncKeyState(vk_f8) & 0x8000) { // Unequip Speed
			if (!isspeed2) {
				HoldKey(vk_slashkey);
				std::this_thread::sleep_for(std::chrono::milliseconds(300));
				PasteText(text);
				std::this_thread::sleep_for(std::chrono::milliseconds(100));
				ReleaseKey(vk_slashkey);
				HoldKey(0x1C);
				std::this_thread::sleep_for(std::chrono::milliseconds(50));
				ReleaseKey(0x1C);
				std::this_thread::sleep_for(std::chrono::milliseconds(920));
				HoldKey(speed_slot + 1);
				ReleaseKey(speed_slot + 1);
				std::this_thread::sleep_for(std::chrono::milliseconds(10));
				SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, true);
				std::this_thread::sleep_for(std::chrono::milliseconds(200));
				HoldKey(speed_slot + 1);
				ReleaseKey(speed_slot + 1);
				std::this_thread::sleep_for(std::chrono::milliseconds(50));
				SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, false);
				isspeed2 = true;
			}
		} else {
			isspeed2 = false;
		}

		if (GetAsyncKeyState(vk_xbutton1) & 0x8000) { // Helicopter High jump
			if (!HHJ) {
				SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, true);
				std::this_thread::sleep_for(std::chrono::milliseconds(50));
				INPUT input = {0};
				input.type = INPUT_MOUSE;
				input.mi.dwFlags = MOUSEEVENTF_RIGHTDOWN;
				SendInput(1, &input, sizeof(INPUT));
				std::this_thread::sleep_for(std::chrono::milliseconds(450));
				SuspendOrResumeProcess(pfnSuspend, pfnResume, hProcess, false);
				MoveMouse(speed_strengthx, 0);
				std::this_thread::sleep_for(std::chrono::milliseconds(16));
				HoldKey(0x2A);
				input.mi.dwFlags = MOUSEEVENTF_RIGHTUP;
				SendInput(1, &input, sizeof(INPUT));
				std::this_thread::sleep_for(std::chrono::milliseconds(16));
				isHHJ = true;
				ReleaseKey(0x2A);
				std::this_thread::sleep_for(std::chrono::milliseconds(240));
				MoveMouse(speed_strengthy, 0);
				isHHJ = false;
				HHJ = true;
			}
		} else {
			HHJ = false;
		}

		if (GetAsyncKeyState('W') & 0x8000 && IsForegroundWindowProcess((nameList[0]))) { // Anti AFK (MUST STAY AT THE LOWEST PRIORITY!!!)
			// W is pressed, update the last press time to now
			lastPressTime = std::chrono::steady_clock::now();
		} else {
			// Check if 15 minutes have passed
			auto now = std::chrono::steady_clock::now();
			auto elapsedMinutes = std::chrono::duration_cast<std::chrono::minutes>(now - lastPressTime).count();
			if (elapsedMinutes >= 15) {
				HWND originalHwnd = GetForegroundWindow();
				INPUT input = {0};
				input.type = INPUT_MOUSE;
				input.mi.dwFlags = MOUSEEVENTF_LEFTDOWN;
				if (IsForegroundWindowProcess(nameList[0]) == 0) { // Extremely long process to simulate going to roblox and typing .
					SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
					SetWindowPos(hwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
					Sleep(1000);
					SendInput(1, &input, sizeof(INPUT));
					input.mi.dwFlags = MOUSEEVENTF_LEFTUP;
					Sleep(50);
					SendInput(1, &input, sizeof(INPUT));
					Sleep(500);
					HoldKey(0x35);
					Sleep(20);
					ReleaseKey(0x35);
					Sleep(20);
					HoldKey(0x34);
					Sleep(20);
					ReleaseKey(0x34);
					Sleep(20);
					HoldKey(0x1C);
					Sleep(20);
					ReleaseKey(0x1C);
					Sleep(500);
					SetWindowPos(originalHwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
					SetWindowPos(originalHwnd, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
					Sleep(1000);
					SetForegroundWindow(originalHwnd);
					lastPressTime = std::chrono::steady_clock::now();
				}
				if (IsForegroundWindowProcess(nameList[0]) == 1) {
					HoldKey(0x35);
					Sleep(20);
					ReleaseKey(0x35);
					Sleep(20);
					HoldKey(0x34);
					Sleep(20);
					ReleaseKey(0x34);
					Sleep(20);
					HoldKey(0x1C);
					Sleep(20);
					ReleaseKey(0x1C);
					Sleep(500);
					lastPressTime = std::chrono::steady_clock::now();
					}
				}
        }

		std::this_thread::sleep_for(std::chrono::microseconds(50)); // Delay between checking for keys (dont touch pls)
	}

	return 0;
}	