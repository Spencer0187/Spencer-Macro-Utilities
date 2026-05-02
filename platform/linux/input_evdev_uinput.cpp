#include "input_evdev_uinput.h"

#include "../logging.h"
#include "../../visual studio/Resource Files/keymapping.h"

#include <algorithm>
#include <cerrno>
#include <chrono>
#include <cstring>
#include <cstdio>
#include <dirent.h>
#include <fcntl.h>
#include <initializer_list>
#include <linux/input.h>
#include <linux/uinput.h>
#include <map>
#include <optional>
#include <poll.h>
#include <set>
#include <sstream>
#include <string>
#include <sys/epoll.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <thread>
#include <unordered_map>
#include <unistd.h>

namespace smu::platform::linux {
namespace {

constexpr PlatformKeyCode kVkLButton = 0x01;
constexpr PlatformKeyCode kVkRButton = 0x02;
constexpr PlatformKeyCode kVkMButton = 0x04;
constexpr PlatformKeyCode kVkXButton1 = 0x05;
constexpr PlatformKeyCode kVkXButton2 = 0x06;
constexpr PlatformKeyCode kVkBack = 0x08;
constexpr PlatformKeyCode kVkTab = 0x09;
constexpr PlatformKeyCode kVkReturn = 0x0D;
constexpr PlatformKeyCode kVkShift = 0x10;
constexpr PlatformKeyCode kVkControl = 0x11;
constexpr PlatformKeyCode kVkMenu = 0x12;
constexpr PlatformKeyCode kVkPause = 0x13;
constexpr PlatformKeyCode kVkCapital = 0x14;
constexpr PlatformKeyCode kVkEscape = 0x1B;
constexpr PlatformKeyCode kVkSpace = 0x20;
constexpr PlatformKeyCode kVkPrior = 0x21;
constexpr PlatformKeyCode kVkNext = 0x22;
constexpr PlatformKeyCode kVkEnd = 0x23;
constexpr PlatformKeyCode kVkHome = 0x24;
constexpr PlatformKeyCode kVkLeft = 0x25;
constexpr PlatformKeyCode kVkUp = 0x26;
constexpr PlatformKeyCode kVkRight = 0x27;
constexpr PlatformKeyCode kVkDown = 0x28;
constexpr PlatformKeyCode kVkSnapshot = 0x2C;
constexpr PlatformKeyCode kVkInsert = 0x2D;
constexpr PlatformKeyCode kVkDelete = 0x2E;
constexpr PlatformKeyCode kVkLWin = 0x5B;
constexpr PlatformKeyCode kVkRWin = 0x5C;
constexpr PlatformKeyCode kVkApps = 0x5D;
constexpr PlatformKeyCode kVkNumpad0 = 0x60;
constexpr PlatformKeyCode kVkF1 = 0x70;
constexpr PlatformKeyCode kVkNumlock = 0x90;
constexpr PlatformKeyCode kVkScroll = 0x91;
constexpr PlatformKeyCode kVkLShift = 0xA0;
constexpr PlatformKeyCode kVkRShift = 0xA1;
constexpr PlatformKeyCode kVkLControl = 0xA2;
constexpr PlatformKeyCode kVkRControl = 0xA3;
constexpr PlatformKeyCode kVkLMenu = 0xA4;
constexpr PlatformKeyCode kVkRMenu = 0xA5;
constexpr PlatformKeyCode kVkOem1 = 0xBA;
constexpr PlatformKeyCode kVkOemPlus = 0xBB;
constexpr PlatformKeyCode kVkOemComma = 0xBC;
constexpr PlatformKeyCode kVkOemMinus = 0xBD;
constexpr PlatformKeyCode kVkOemPeriod = 0xBE;
constexpr PlatformKeyCode kVkOem2 = 0xBF;
constexpr PlatformKeyCode kVkOem3 = 0xC0;
constexpr PlatformKeyCode kVkOem4 = 0xDB;
constexpr PlatformKeyCode kVkOem5 = 0xDC;
constexpr PlatformKeyCode kVkOem6 = 0xDD;
constexpr PlatformKeyCode kVkOem7 = 0xDE;

bool TestBit(const unsigned long* bits, int bit)
{
    return (bits[bit / (8 * static_cast<int>(sizeof(unsigned long)))] &
            (1UL << (bit % (8 * static_cast<int>(sizeof(unsigned long)))))) != 0;
}

std::string ErrnoText(const std::string& prefix)
{
    return prefix + ": " + std::strerror(errno);
}

std::vector<std::string> EnumerateEventDevices()
{
    std::vector<std::string> devices;
    DIR* inputDir = opendir("/dev/input");
    if (!inputDir) {
        return devices;
    }

    while (dirent* entry = readdir(inputDir)) {
        if (std::strncmp(entry->d_name, "event", 5) == 0) {
            devices.push_back("/dev/input/" + std::string(entry->d_name));
        }
    }

    closedir(inputDir);
    std::sort(devices.begin(), devices.end());
    return devices;
}

bool DeviceHasKeys(int fd, std::initializer_list<int> keys)
{
    unsigned long keyBits[(KEY_MAX / (8 * sizeof(unsigned long))) + 1] = {};
    if (ioctl(fd, EVIOCGBIT(EV_KEY, sizeof(keyBits)), keyBits) < 0) {
        return false;
    }

    for (int key : keys) {
        if (TestBit(keyBits, key)) {
            return true;
        }
    }
    return false;
}

bool DeviceHasRel(int fd, std::initializer_list<int> rels)
{
    unsigned long relBits[(REL_MAX / (8 * sizeof(unsigned long))) + 1] = {};
    if (ioctl(fd, EVIOCGBIT(EV_REL, sizeof(relBits)), relBits) < 0) {
        return false;
    }

    for (int rel : rels) {
        if (TestBit(relBits, rel)) {
            return true;
        }
    }
    return false;
}

std::string DeviceName(int fd)
{
    char name[256] = {};
    if (ioctl(fd, EVIOCGNAME(sizeof(name)), name) < 0) {
        return {};
    }
    return name;
}

bool IsOwnVirtualDevice(int fd)
{
    return DeviceName(fd) == "SMU Native Linux Input";
}

bool LooksLikeKeyboard(int fd)
{
    if (IsOwnVirtualDevice(fd)) {
        return false;
    }
    return DeviceHasKeys(fd, {KEY_A, KEY_SPACE, KEY_ENTER, KEY_ESC});
}

bool LooksLikeMouse(int fd)
{
    if (IsOwnVirtualDevice(fd)) {
        return false;
    }
    return DeviceHasRel(fd, {REL_X, REL_Y}) && DeviceHasKeys(fd, {BTN_LEFT, BTN_RIGHT, BTN_MIDDLE});
}

std::optional<PlatformKeyCode> WinScanToVk(PlatformKeyCode scanCode)
{
    static const std::unordered_map<PlatformKeyCode, PlatformKeyCode> scanToVk = {
        {0x01, kVkEscape}, {0x02, '1'}, {0x03, '2'}, {0x04, '3'}, {0x05, '4'},
        {0x06, '5'}, {0x07, '6'}, {0x08, '7'}, {0x09, '8'}, {0x0A, '9'},
        {0x0B, '0'}, {0x0C, kVkOemMinus}, {0x0D, kVkOemPlus}, {0x0E, kVkBack},
        {0x0F, kVkTab}, {0x10, 'Q'}, {0x11, 'W'}, {0x12, 'E'}, {0x13, 'R'},
        {0x14, 'T'}, {0x15, 'Y'}, {0x16, 'U'}, {0x17, 'I'}, {0x18, 'O'},
        {0x19, 'P'}, {0x1A, kVkOem4}, {0x1B, kVkOem6}, {0x1C, kVkReturn},
        {0x1D, kVkControl}, {0x1E, 'A'}, {0x1F, 'S'}, {0x20, 'D'}, {0x21, 'F'},
        {0x22, 'G'}, {0x23, 'H'}, {0x24, 'J'}, {0x25, 'K'}, {0x26, 'L'},
        {0x27, kVkOem1}, {0x28, kVkOem7}, {0x29, kVkOem3}, {0x2A, kVkShift},
        {0x2B, kVkOem5}, {0x2C, 'Z'}, {0x2D, 'X'}, {0x2E, 'C'}, {0x2F, 'V'},
        {0x30, 'B'}, {0x31, 'N'}, {0x32, 'M'}, {0x33, kVkOemComma},
        {0x34, kVkOemPeriod}, {0x35, kVkOem2}, {0x36, kVkShift}, {0x37, 0x6A},
        {0x38, kVkMenu}, {0x39, kVkSpace}, {0x3A, kVkCapital},
        {0x3B, kVkF1}, {0x3C, kVkF1 + 1}, {0x3D, kVkF1 + 2}, {0x3E, kVkF1 + 3},
        {0x3F, kVkF1 + 4}, {0x40, kVkF1 + 5}, {0x41, kVkF1 + 6}, {0x42, kVkF1 + 7},
        {0x43, kVkF1 + 8}, {0x44, kVkF1 + 9}, {0x45, kVkNumlock}, {0x46, kVkScroll},
        {0x47, 0x67}, {0x48, 0x68}, {0x49, 0x69}, {0x4A, 0x6D}, {0x4B, 0x64},
        {0x4C, 0x65}, {0x4D, 0x66}, {0x4E, 0x6B}, {0x4F, 0x61}, {0x50, 0x62},
        {0x51, 0x63}, {0x52, kVkNumpad0}, {0x53, 0x6E}, {0x57, kVkF1 + 10},
        {0x58, kVkF1 + 11}
    };

    auto it = scanToVk.find(scanCode);
    if (it == scanToVk.end()) {
        return std::nullopt;
    }
    return it->second;
}

std::optional<int> VkToEvdev(PlatformKeyCode vk)
{
    if (vk == kVkLShift) return KEY_LEFTSHIFT;
    if (vk == kVkRShift) return KEY_RIGHTSHIFT;
    if (vk == kVkLControl) return KEY_LEFTCTRL;
    if (vk == kVkRControl) return KEY_RIGHTCTRL;
    if (vk == kVkLMenu) return KEY_LEFTALT;
    if (vk == kVkRMenu) return KEY_RIGHTALT;

    const uint16_t evdev = win_vkey_to_evdev_key(static_cast<uint8_t>(vk));
    if (evdev == EVDEV_UNASSIGNED) {
        return std::nullopt;
    }
    return static_cast<int>(evdev);
}

std::optional<int> ScanToEvdev(PlatformKeyCode scanCode)
{
    auto vk = WinScanToVk(scanCode);
    if (!vk) {
        return std::nullopt;
    }
    return VkToEvdev(*vk);
}

PlatformKeyCode EvdevToVk(unsigned int evdevCode)
{
    switch (evdevCode) {
    case KEY_LEFTSHIFT: return kVkShift;
    case KEY_RIGHTSHIFT: return kVkShift;
    case KEY_LEFTCTRL: return kVkControl;
    case KEY_RIGHTCTRL: return kVkControl;
    case KEY_LEFTALT: return kVkMenu;
    case KEY_RIGHTALT: return kVkMenu;
    default:
        return evdev_to_win_vkey(static_cast<uint16_t>(evdevCode));
    }
}

std::optional<int> ChordKeyToEvdev(PlatformKeyCode key)
{
    if (key == kMouseWheelUp || key == kMouseWheelDown) {
        return std::nullopt;
    }
    return VkToEvdev(key);
}

std::string KeyNameFallback(PlatformKeyCode key)
{
    static const std::unordered_map<PlatformKeyCode, std::string> names = {
        {kVkLButton, "Mouse Left"}, {kVkRButton, "Mouse Right"}, {kVkMButton, "Mouse Middle"},
        {kVkXButton1, "Mouse X1"}, {kVkXButton2, "Mouse X2"}, {kVkBack, "Backspace"},
        {kVkTab, "Tab"}, {kVkReturn, "Enter"}, {kVkShift, "Shift"}, {kVkControl, "Ctrl"},
        {kVkMenu, "Alt"}, {kVkPause, "Pause"}, {kVkCapital, "Caps Lock"}, {kVkEscape, "Escape"},
        {kVkSpace, "Space"}, {kVkPrior, "Page Up"}, {kVkNext, "Page Down"}, {kVkEnd, "End"},
        {kVkHome, "Home"}, {kVkLeft, "Left"}, {kVkUp, "Up"}, {kVkRight, "Right"},
        {kVkDown, "Down"}, {kVkSnapshot, "Print Screen"}, {kVkInsert, "Insert"},
        {kVkDelete, "Delete"}, {kVkLWin, "Left Super"}, {kVkRWin, "Right Super"},
        {kVkApps, "Menu"}, {kVkNumlock, "Num Lock"}, {kVkScroll, "Scroll Lock"},
        {kVkLShift, "Left Shift"}, {kVkRShift, "Right Shift"}, {kVkLControl, "Left Ctrl"},
        {kVkRControl, "Right Ctrl"}, {kVkLMenu, "Left Alt"}, {kVkRMenu, "Right Alt"},
        {kVkOem1, ";"}, {kVkOemPlus, "="}, {kVkOemComma, ","}, {kVkOemMinus, "-"},
        {kVkOemPeriod, "."}, {kVkOem2, "/"}, {kVkOem3, "`"}, {kVkOem4, "["},
        {kVkOem5, "\\"}, {kVkOem6, "]"}, {kVkOem7, "'"}, {kMouseWheelUp, "Mouse Wheel Up"},
        {kMouseWheelDown, "Mouse Wheel Down"}
    };

    if (key >= 'A' && key <= 'Z') {
        return std::string(1, static_cast<char>(key));
    }
    if (key >= '0' && key <= '9') {
        return std::string(1, static_cast<char>(key));
    }
    if (key >= kVkF1 && key <= kVkF1 + 23) {
        return "F" + std::to_string(key - kVkF1 + 1);
    }
    if (key >= kVkNumpad0 && key <= kVkNumpad0 + 9) {
        return "Numpad " + std::to_string(key - kVkNumpad0);
    }

    auto it = names.find(key);
    if (it != names.end()) {
        return it->second;
    }

    std::ostringstream stream;
    stream << "0x" << std::hex << std::uppercase << key;
    return stream.str();
}

bool AppendInteractiveDetection(std::set<std::string>& devicePaths, std::string* errorMessage)
{
    int epollFd = epoll_create1(EPOLL_CLOEXEC);
    if (epollFd < 0) {
        if (errorMessage) {
            *errorMessage = ErrnoText("Linux input backend failed to create epoll instance");
        }
        return false;
    }

    std::map<int, std::string> fdToPath;
    for (const std::string& path : EnumerateEventDevices()) {
        int fd = open(path.c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
        if (fd < 0) {
            continue;
        }
        if (IsOwnVirtualDevice(fd)) {
            close(fd);
            continue;
        }

        epoll_event event {};
        event.events = EPOLLIN;
        event.data.fd = fd;
        if (epoll_ctl(epollFd, EPOLL_CTL_ADD, fd, &event) == 0) {
            fdToPath[fd] = path;
        } else {
            close(fd);
        }
    }

    bool sawKeyboard = false;
    bool sawMouse = false;
    epoll_event events[16] = {};
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(12);
    while ((!sawKeyboard || !sawMouse) && std::chrono::steady_clock::now() < deadline) {
        const int waitMs = 250;
        const int count = epoll_wait(epollFd, events, 16, waitMs);
        if (count < 0) {
            if (errno == EINTR) {
                continue;
            }
            break;
        }

        for (int index = 0; index < count; ++index) {
            input_event input {};
            while (read(events[index].data.fd, &input, sizeof(input)) == sizeof(input)) {
                const std::string& path = fdToPath[events[index].data.fd];
                if (!sawKeyboard && input.type == EV_KEY && input.value == 1 && input.code < BTN_MOUSE) {
                    devicePaths.insert(path);
                    sawKeyboard = true;
                }

                if (!sawMouse &&
                    ((input.type == EV_REL && (input.code == REL_X || input.code == REL_Y || input.code == REL_WHEEL)) ||
                     (input.type == EV_KEY && input.code >= BTN_MOUSE && input.code <= BTN_TASK))) {
                    devicePaths.insert(path);
                    sawMouse = true;
                }
            }
        }
    }

    for (const auto& [fd, path] : fdToPath) {
        (void)path;
        close(fd);
    }
    close(epollFd);

    if (!sawKeyboard || !sawMouse) {
        if (errorMessage) {
            *errorMessage = "Linux input backend could not detect both keyboard and mouse event devices.";
        }
        return false;
    }

    return true;
}

} // namespace

EvdevUinputInputBackend::EvdevUinputInputBackend()
{
    for (auto& state : keyStates_) {
        state.store(false, std::memory_order_relaxed);
    }
}

EvdevUinputInputBackend::~EvdevUinputInputBackend()
{
    shutdown();
}

bool EvdevUinputInputBackend::init(std::string* errorMessage)
{
    if (errorMessage) {
        errorMessage->clear();
    }

    if (initialized_.load(std::memory_order_acquire)) {
        return true;
    }

    struct stat inputStat {};
    if (stat("/dev/input", &inputStat) != 0) {
        const std::string message = ErrnoText("Linux input backend cannot access /dev/input");
        if (errorMessage) {
            *errorMessage = message;
        }
        smu::log::LogWarning(message);
        return false;
    }

    if (!setupUinput(errorMessage)) {
        if (errorMessage && !errorMessage->empty()) {
            smu::log::LogWarning(*errorMessage);
        }
        shutdown();
        return false;
    }

    if (!detectInputDevices(errorMessage)) {
        if (errorMessage && !errorMessage->empty()) {
            smu::log::LogWarning(*errorMessage);
        }
        shutdown();
        return false;
    }

    running_.store(true, std::memory_order_release);
    for (const std::string& path : devicePaths_) {
        readerThreads_.emplace_back(&EvdevUinputInputBackend::readerThread, this, path);
    }

    initialized_.store(true, std::memory_order_release);
    smu::log::LogInfo("Linux input backend initialized with " + std::to_string(devicePaths_.size()) + " event device(s).");
    logMappingDiagnostics();
    return true;
}

void EvdevUinputInputBackend::shutdown()
{
    running_.store(false, std::memory_order_release);

    for (std::thread& thread : readerThreads_) {
        if (thread.joinable()) {
            thread.join();
        }
    }
    readerThreads_.clear();
    devicePaths_.clear();

    {
        std::lock_guard<std::mutex> lock(emitMutex_);
        if (uinputFd_ >= 0) {
            ioctl(uinputFd_, UI_DEV_DESTROY);
            close(uinputFd_);
            uinputFd_ = -1;
        }
    }

    for (auto& state : keyStates_) {
        state.store(false, std::memory_order_relaxed);
    }
    initialized_.store(false, std::memory_order_release);
}

bool EvdevUinputInputBackend::isKeyPressed(PlatformKeyCode key) const
{
    if (key >= keyStates_.size()) {
        return false;
    }
    return keyStates_[key].load(std::memory_order_acquire);
}

void EvdevUinputInputBackend::holdKey(PlatformKeyCode key, bool)
{
    auto evdev = evdevForVirtualKey(key);
    if (!evdev) {
        smu::log::LogWarning("Linux input backend cannot inject unmapped virtual key " + KeyNameFallback(key) + ".");
        return;
    }

    emit(EV_KEY, *evdev, 1);
    emitSyn();
}

void EvdevUinputInputBackend::releaseKey(PlatformKeyCode key, bool)
{
    auto evdev = evdevForVirtualKey(key);
    if (!evdev) {
        smu::log::LogWarning("Linux input backend cannot release unmapped virtual key " + KeyNameFallback(key) + ".");
        return;
    }

    emit(EV_KEY, *evdev, 0);
    emitSyn();
}

void EvdevUinputInputBackend::pressKey(PlatformKeyCode key, int delayMs)
{
    holdKey(key);
    std::this_thread::sleep_for(std::chrono::milliseconds(delayMs));
    releaseKey(key);
}

void EvdevUinputInputBackend::holdKeyChord(PlatformKeyCode combinedKey)
{
    const PlatformKeyCode vk = combinedKey & 0xFFFFu;
    const bool useShift = (combinedKey & 0x10000u) != 0;
    const bool useCtrl = (combinedKey & 0x20000u) != 0;
    const bool useAlt = (combinedKey & 0x40000u) != 0;
    const bool useWin = (combinedKey & 0x80000u) != 0;

    if (vk == kMouseWheelUp || vk == kMouseWheelDown) {
        mouseWheel(vk == kMouseWheelUp ? 1 : -1);
        return;
    }

    if (useWin) emit(EV_KEY, KEY_LEFTMETA, 1);
    if (useCtrl) emit(EV_KEY, KEY_LEFTCTRL, 1);
    if (useAlt) emit(EV_KEY, KEY_LEFTALT, 1);
    if (useShift) emit(EV_KEY, KEY_LEFTSHIFT, 1);

    if (auto evdev = ChordKeyToEvdev(vk)) {
        emit(EV_KEY, *evdev, 1);
    }
    emitSyn();
}

void EvdevUinputInputBackend::releaseKeyChord(PlatformKeyCode combinedKey)
{
    const PlatformKeyCode vk = combinedKey & 0xFFFFu;
    const bool useShift = (combinedKey & 0x10000u) != 0;
    const bool useCtrl = (combinedKey & 0x20000u) != 0;
    const bool useAlt = (combinedKey & 0x40000u) != 0;
    const bool useWin = (combinedKey & 0x80000u) != 0;

    if (auto evdev = ChordKeyToEvdev(vk)) {
        emit(EV_KEY, *evdev, 0);
    }

    if (useShift) emit(EV_KEY, KEY_LEFTSHIFT, 0);
    if (useAlt) emit(EV_KEY, KEY_LEFTALT, 0);
    if (useCtrl) emit(EV_KEY, KEY_LEFTCTRL, 0);
    if (useWin) emit(EV_KEY, KEY_LEFTMETA, 0);
    emitSyn();
}

void EvdevUinputInputBackend::moveMouse(int dx, int dy)
{
    emit(EV_REL, REL_X, dx);
    emit(EV_REL, REL_Y, dy);
    emitSyn();
}

void EvdevUinputInputBackend::mouseWheel(int delta)
{
    emit(EV_REL, REL_WHEEL, delta >= 0 ? 1 : -1);
    emitSyn();
}

std::optional<PlatformKeyCode> EvdevUinputInputBackend::getCurrentPressedKey() const
{
    for (PlatformKeyCode key = 1; key < keyStates_.size(); ++key) {
        if (keyStates_[key].load(std::memory_order_acquire)) {
            return key;
        }
    }
    return std::nullopt;
}

std::string EvdevUinputInputBackend::formatKeyName(PlatformKeyCode key) const
{
    return KeyNameFallback(key);
}

std::optional<int> EvdevUinputInputBackend::evdevForVirtualKey(PlatformKeyCode key) const
{
    return VkToEvdev(key);
}

std::optional<int> EvdevUinputInputBackend::evdevForScanCode(PlatformKeyCode scanCode) const
{
    return ScanToEvdev(scanCode);
}

void EvdevUinputInputBackend::logMappingDiagnostics() const
{
    const std::initializer_list<PlatformKeyCode> keys = {
        kVkSpace,
        kVkMButton,
        'X',
        kVkF1 + 4,
        kVkLShift,
        kVkControl,
        kVkMenu,
    };

    for (PlatformKeyCode key : keys) {
        if (auto evdev = evdevForVirtualKey(key)) {
            smu::log::LogInfo("Linux input mapping: " + KeyNameFallback(key) + " -> evdev " + std::to_string(*evdev) + ".");
        } else {
            smu::log::LogWarning("Linux input mapping missing for " + KeyNameFallback(key) + ".");
        }
    }
}

void EvdevUinputInputBackend::readerThread(std::string devicePath)
{
    int fd = open(devicePath.c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0) {
        smu::log::LogWarning(ErrnoText("Linux input backend failed to open " + devicePath));
        return;
    }

    // Native mode reads passively and injects through uinput. The older helper's
    // grab-and-reemit path is riskier inside the GUI process and remains in Wine.
    while (running_.load(std::memory_order_acquire)) {
        input_event event {};
        const ssize_t bytesRead = read(fd, &event, sizeof(event));
        if (bytesRead == sizeof(event)) {
            if (event.type == EV_KEY) {
                setKeyStateFromEvdev(event.code, event.value != 0);
            }
            continue;
        }

        if (bytesRead < 0 && errno != EAGAIN && errno != EINTR) {
            smu::log::LogError(ErrnoText("Linux input backend read failed for " + devicePath));
            break;
        }

        pollfd pfd {};
        pfd.fd = fd;
        pfd.events = POLLIN;
        poll(&pfd, 1, 50);
    }

    close(fd);
}

void EvdevUinputInputBackend::emit(int type, int code, int value)
{
    std::lock_guard<std::mutex> lock(emitMutex_);
    if (uinputFd_ < 0) {
        return;
    }

    input_event event {};
    event.type = static_cast<__u16>(type);
    event.code = static_cast<__u16>(code);
    event.value = value;
    if (write(uinputFd_, &event, sizeof(event)) != sizeof(event)) {
        smu::log::LogError(ErrnoText("Linux input backend failed to write uinput event"));
    }
}

void EvdevUinputInputBackend::emitSyn()
{
    emit(EV_SYN, SYN_REPORT, 0);
}

void EvdevUinputInputBackend::setKeyStateFromEvdev(unsigned int evdevCode, bool pressed)
{
    const PlatformKeyCode vk = EvdevToVk(evdevCode);
    if (vk < keyStates_.size()) {
        keyStates_[vk].store(pressed, std::memory_order_release);
    }
}

bool EvdevUinputInputBackend::setupUinput(std::string* errorMessage)
{
    uinputFd_ = open("/dev/uinput", O_WRONLY | O_NONBLOCK | O_CLOEXEC);
    if (uinputFd_ < 0) {
        if (errorMessage) {
            *errorMessage = ErrnoText("Linux input backend failed to open /dev/uinput");
        }
        return false;
    }

    auto requireIoctl = [this, errorMessage](unsigned long request, int value, const std::string& what) {
        if (ioctl(uinputFd_, request, value) < 0) {
            if (errorMessage) {
                *errorMessage = ErrnoText("Linux input backend failed to configure uinput " + what);
            }
            return false;
        }
        return true;
    };

    if (!requireIoctl(UI_SET_EVBIT, EV_KEY, "EV_KEY")) return false;
    for (int key = 0; key < KEY_MAX; ++key) {
        if (!requireIoctl(UI_SET_KEYBIT, key, "key " + std::to_string(key))) return false;
    }

    if (!requireIoctl(UI_SET_EVBIT, EV_REL, "EV_REL")) return false;
    if (!requireIoctl(UI_SET_RELBIT, REL_X, "REL_X")) return false;
    if (!requireIoctl(UI_SET_RELBIT, REL_Y, "REL_Y")) return false;
    if (!requireIoctl(UI_SET_RELBIT, REL_WHEEL, "REL_WHEEL")) return false;

    uinput_user_dev device {};
    std::snprintf(device.name, UINPUT_MAX_NAME_SIZE, "SMU Native Linux Input");
    device.id.bustype = BUS_USB;
    device.id.vendor = 0x534d;
    device.id.product = 0x4301;
    device.id.version = 1;

    if (write(uinputFd_, &device, sizeof(device)) != sizeof(device)) {
        if (errorMessage) {
            *errorMessage = ErrnoText("Linux input backend failed to write uinput device descriptor");
        }
        return false;
    }

    if (ioctl(uinputFd_, UI_DEV_CREATE) < 0) {
        if (errorMessage) {
            *errorMessage = ErrnoText("Linux input backend failed to create uinput virtual device");
        }
        return false;
    }

    std::this_thread::sleep_for(std::chrono::milliseconds(150));
    return true;
}

bool EvdevUinputInputBackend::detectInputDevices(std::string* errorMessage)
{
    std::set<std::string> paths;
    bool hasKeyboard = false;
    bool hasMouse = false;

    for (const std::string& path : EnumerateEventDevices()) {
        int fd = open(path.c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC);
        if (fd < 0) {
            continue;
        }

        const bool keyboard = LooksLikeKeyboard(fd);
        const bool mouse = LooksLikeMouse(fd);
        close(fd);

        if (keyboard || mouse) {
            paths.insert(path);
        }
        hasKeyboard = hasKeyboard || keyboard;
        hasMouse = hasMouse || mouse;
    }

    if (!hasKeyboard || !hasMouse) {
        if (!AppendInteractiveDetection(paths, errorMessage)) {
            return false;
        }
    }

    if (paths.empty()) {
        if (errorMessage && errorMessage->empty()) {
            *errorMessage = "Linux input backend did not find readable /dev/input/event* keyboard or mouse devices.";
        }
        return false;
    }

    devicePaths_.assign(paths.begin(), paths.end());
    return true;
}

std::shared_ptr<InputBackend> CreateEvdevUinputInputBackend()
{
    return std::make_shared<EvdevUinputInputBackend>();
}

} // namespace smu::platform::linux
