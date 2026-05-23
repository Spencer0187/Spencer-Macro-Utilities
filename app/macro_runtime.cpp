#include "macro_runtime.h"

#include "app_profile_bridge.h"
#include "input_actions.h"
#include "notification_suppression.h"
#include "script_manager.h"
#include "../core/key_codes.h"
#include "../platform/input_backend.h"
#include "../platform/logging.h"
#include "../platform/network_backend.h"
#include "../platform/platform_capabilities.h"
#include "../platform/process_backend.h"
#include "../platform/text_input_backend.h"

#include "../core/app_state.h"
#include "../core/legacy_globals.h"

#if defined(_WIN32)
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#include "../platform/windows/admin_elevation.h"
#endif

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <sstream>
#include <string>

namespace smu::app {
namespace {

using namespace std::chrono_literals;
using namespace Globals;

#if defined(_WIN32)
enum class RuntimeProfileSection : std::size_t {
    ResetInputCache,
    RefreshProcesses,
    HotkeySuppression,
    PruneWorkers,
    Freeze,
    ItemDesync,
    PressKey,
    Wallhop,
    WallessLhj,
    Speedglitch,
    Hhj,
    HhjMotion,
    ItemUnequip,
    ItemClip,
    LaughClip,
    WallWalk,
    SpamKey,
    LedgeBounce,
    Bunnyhop,
    FloorBounce,
    LagSwitch,
    ImportedScripts,
    Count
};

constexpr std::array<const char*, static_cast<std::size_t>(RuntimeProfileSection::Count)> kRuntimeProfileSectionNames = {{
    "reset_input_cache",
    "refresh_processes",
    "hotkey_suppression",
    "prune_workers",
    "freeze",
    "item_desync",
    "press_key",
    "wallhop",
    "walless_lhj",
    "speedglitch",
    "hhj",
    "hhj_motion",
    "item_unequip",
    "item_clip",
    "laugh_clip",
    "wall_walk",
    "spam_key",
    "ledge_bounce",
    "bunnyhop",
    "floor_bounce",
    "lag_switch",
    "imported_scripts"
}};

unsigned long long FileTimeToUnsignedLongLong(const FILETIME& fileTime)
{
    return (static_cast<unsigned long long>(fileTime.dwHighDateTime) << 32) |
        static_cast<unsigned long long>(fileTime.dwLowDateTime);
}

unsigned long long CurrentThreadCpu100ns()
{
    FILETIME creation = {};
    FILETIME exitTime = {};
    FILETIME kernel = {};
    FILETIME user = {};
    if (!GetThreadTimes(GetCurrentThread(), &creation, &exitTime, &kernel, &user)) {
        return 0;
    }

    return FileTimeToUnsignedLongLong(kernel) + FileTimeToUnsignedLongLong(user);
}

std::string RuntimeProfilerTimestamp()
{
    SYSTEMTIME time = {};
    GetLocalTime(&time);

    char buffer[64] = {};
    std::snprintf(buffer, sizeof(buffer), "%04u-%02u-%02u %02u:%02u:%02u",
        static_cast<unsigned int>(time.wYear),
        static_cast<unsigned int>(time.wMonth),
        static_cast<unsigned int>(time.wDay),
        static_cast<unsigned int>(time.wHour),
        static_cast<unsigned int>(time.wMinute),
        static_cast<unsigned int>(time.wSecond));
    return buffer;
}

void AppendRuntimeProfilerLog(const std::string& message)
{
    std::ofstream file("SMC.log", std::ios::app);
    if (file) {
        file << "[" << RuntimeProfilerTimestamp() << "] [INFO] " << message << '\n';
    }
    OutputDebugStringA((message + "\n").c_str());
}

bool IsRuntimePerformanceLoggingEnabled()
{
    const char* value = std::getenv("SMU_MACRORUNTIME_PERF");
    if (!value) {
        return false;
    }

    return std::strcmp(value, "1") == 0 ||
        std::strcmp(value, "true") == 0 ||
        std::strcmp(value, "TRUE") == 0 ||
        std::strcmp(value, "on") == 0 ||
        std::strcmp(value, "ON") == 0;
}

class RuntimeThreadProfiler {
public:
    RuntimeThreadProfiler()
    {
        enabled_ = IsRuntimePerformanceLoggingEnabled();
        if (enabled_) {
            reset();
        }
    }

    bool enabled() const
    {
        return enabled_;
    }

    void add(RuntimeProfileSection section, unsigned long long cpu100ns, long long wallNs)
    {
        if (!enabled_) {
            return;
        }

        const std::size_t index = static_cast<std::size_t>(section);
        cpu100ns_[index] += cpu100ns;
        wallNs_[index] += static_cast<unsigned long long>(std::max<long long>(0, wallNs));
        calls_[index] += 1;
    }

    void finishLoop()
    {
        if (!enabled_) {
            return;
        }

        ++loops_;

        const auto now = std::chrono::steady_clock::now();
        if (now - startedAt_ < 5s) {
            return;
        }

        const unsigned long long totalCpu100ns = CurrentThreadCpu100ns() - startedCpu100ns_;
        const auto elapsedNs = std::chrono::duration_cast<std::chrono::nanoseconds>(now - startedAt_).count();
        if (elapsedNs <= 0) {
            reset();
            return;
        }

        unsigned long long accountedCpu100ns = 0;
        for (unsigned long long value : cpu100ns_) {
            accountedCpu100ns += value;
        }

        std::array<std::size_t, static_cast<std::size_t>(RuntimeProfileSection::Count)> order = {};
        for (std::size_t i = 0; i < order.size(); ++i) {
            order[i] = i;
        }
        std::sort(order.begin(), order.end(), [&](std::size_t left, std::size_t right) {
            return cpu100ns_[left] > cpu100ns_[right];
        });

        const double elapsedMs = static_cast<double>(elapsedNs) / 1000000.0;
        const double elapsedSeconds = static_cast<double>(elapsedNs) / 1000000000.0;
        const double totalCpuMs = static_cast<double>(totalCpu100ns) / 10000.0;
        const double accountedCpuMs = static_cast<double>(accountedCpu100ns) / 10000.0;
        const double unaccountedCpuMs = static_cast<double>(
            totalCpu100ns > accountedCpu100ns ? totalCpu100ns - accountedCpu100ns : 0) / 10000.0;

        std::ostringstream stream;
        stream.setf(std::ios::fixed);
        stream.precision(3);
        stream << "SMU MacroRuntime profile: window_ms=" << elapsedMs
               << " loops=" << loops_
               << " loop_hz=" << (static_cast<double>(loops_) / elapsedSeconds)
               << " thread_cpu_ms=" << totalCpuMs
               << " thread_cpu_one_core_pct=" << (totalCpuMs * 100.0 / elapsedMs)
               << " accounted_cpu_ms=" << accountedCpuMs
               << " unaccounted_cpu_ms=" << unaccountedCpuMs;

        for (std::size_t rank = 0; rank < order.size(); ++rank) {
            const std::size_t index = order[rank];
            if (calls_[index] == 0 && cpu100ns_[index] == 0 && wallNs_[index] == 0) {
                continue;
            }

            const double sectionCpuMs = static_cast<double>(cpu100ns_[index]) / 10000.0;
            const double sectionWallMs = static_cast<double>(wallNs_[index]) / 1000000.0;
            stream << "\n  section=" << kRuntimeProfileSectionNames[index]
                   << " calls=" << calls_[index]
                   << " cpu_ms=" << sectionCpuMs
                   << " cpu_pct_of_thread=" << (totalCpuMs > 0.0 ? sectionCpuMs * 100.0 / totalCpuMs : 0.0)
                   << " wall_ms=" << sectionWallMs;
        }

        AppendRuntimeProfilerLog(stream.str());
        reset();
    }

private:
    void reset()
    {
        cpu100ns_.fill(0);
        wallNs_.fill(0);
        calls_.fill(0);
        loops_ = 0;
        startedAt_ = std::chrono::steady_clock::now();
        startedCpu100ns_ = CurrentThreadCpu100ns();
    }

    std::array<unsigned long long, static_cast<std::size_t>(RuntimeProfileSection::Count)> cpu100ns_{};
    std::array<unsigned long long, static_cast<std::size_t>(RuntimeProfileSection::Count)> wallNs_{};
    std::array<unsigned long long, static_cast<std::size_t>(RuntimeProfileSection::Count)> calls_{};
    bool enabled_ = false;
    unsigned long long loops_ = 0;
    std::chrono::steady_clock::time_point startedAt_{};
    unsigned long long startedCpu100ns_ = 0;
};

class ScopedRuntimeProfile {
public:
    ScopedRuntimeProfile(RuntimeThreadProfiler& profiler, RuntimeProfileSection section)
        : profiler_(profiler), section_(section), enabled_(profiler.enabled())
    {
        if (enabled_) {
            startedAt_ = std::chrono::steady_clock::now();
            startedCpu100ns_ = CurrentThreadCpu100ns();
        }
    }

    ~ScopedRuntimeProfile()
    {
        if (!enabled_) {
            return;
        }

        const unsigned long long endedCpu100ns = CurrentThreadCpu100ns();
        const auto endedAt = std::chrono::steady_clock::now();
        profiler_.add(section_,
            endedCpu100ns > startedCpu100ns_ ? endedCpu100ns - startedCpu100ns_ : 0,
            std::chrono::duration_cast<std::chrono::nanoseconds>(endedAt - startedAt_).count());
    }

private:
    RuntimeThreadProfiler& profiler_;
    RuntimeProfileSection section_;
    bool enabled_ = false;
    std::chrono::steady_clock::time_point startedAt_;
    unsigned long long startedCpu100ns_ = 0;
};

#define SMU_CONCAT_RUNTIME_PROFILE_NAME_INNER(left, right) left##right
#define SMU_CONCAT_RUNTIME_PROFILE_NAME(left, right) SMU_CONCAT_RUNTIME_PROFILE_NAME_INNER(left, right)
#define SMU_PROFILE_RUNTIME_SECTION(profiler, section) \
    ScopedRuntimeProfile SMU_CONCAT_RUNTIME_PROFILE_NAME(smuRuntimeProfileScope, __LINE__)((profiler), (section))
#endif

#if defined(_WIN32)
struct ProcessWindowSearchContext {
    const std::vector<unsigned int>* pids = nullptr;
    HWND window = nullptr;
    FILETIME creationTime = {};
    bool foundAny = false;
};

bool IsMainWindow(HWND hwnd)
{
    return IsWindowVisible(hwnd) && GetWindow(hwnd, GW_OWNER) == nullptr;
}

BOOL CALLBACK FindNewestProcessWindowCallback(HWND hwnd, LPARAM lParam)
{
    auto* context = reinterpret_cast<ProcessWindowSearchContext*>(lParam);
    if (!IsMainWindow(hwnd)) {
        return TRUE;
    }

    DWORD windowPid = 0;
    GetWindowThreadProcessId(hwnd, &windowPid);
    if (!context->pids) {
        return TRUE;
    }

    if (std::find(context->pids->begin(), context->pids->end(), static_cast<unsigned int>(windowPid)) == context->pids->end()) {
        return TRUE;
    }

    HANDLE process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, windowPid);
    if (!process) {
        return TRUE;
    }

    FILETIME creation = {};
    FILETIME exitTime = {};
    FILETIME kernel = {};
    FILETIME user = {};
    if (GetProcessTimes(process, &creation, &exitTime, &kernel, &user)) {
        if (!context->foundAny ||
            CompareFileTime(&creation, &context->creationTime) > 0) {
            context->creationTime = creation;
            context->window = hwnd;
            context->foundAny = true;
        }
    }

    CloseHandle(process);
    return TRUE;
}

HWND FindNewestProcessWindow(const std::vector<unsigned int>& pids)
{
    ProcessWindowSearchContext context{};
    context.pids = &pids;
    EnumWindows(FindNewestProcessWindowCallback, reinterpret_cast<LPARAM>(&context));
    return context.window;
}

bool TapWindowCenter(HWND window)
{
    if (!window) {
        return false;
    }

    RECT rect = {};
    if (!GetWindowRect(window, &rect)) {
        return false;
    }

    POINT originalMousePos = {};
    const bool hasOriginalMousePos = GetCursorPos(&originalMousePos) != FALSE;

    const int centerX = rect.left + (rect.right - rect.left) / 2;
    const int centerY = rect.top + (rect.bottom - rect.top) / 2;
    SetCursorPos(centerX, centerY);

    INPUT mouseInput = {};
    mouseInput.type = INPUT_MOUSE;
    mouseInput.mi.dwFlags = MOUSEEVENTF_LEFTDOWN;
    SendInput(1, &mouseInput, sizeof(INPUT));
    std::this_thread::sleep_for(50ms);
    mouseInput.mi.dwFlags = MOUSEEVENTF_LEFTUP;
    SendInput(1, &mouseInput, sizeof(INPUT));

    if (hasOriginalMousePos) {
        std::this_thread::sleep_for(100ms);
        SetCursorPos(originalMousePos.x, originalMousePos.y);
    }
    return true;
}
#endif

bool ShouldKeepRunning()
{
    return running.load(std::memory_order_acquire) && !done.load(std::memory_order_acquire);
}

unsigned int InventorySlotKey(int slot)
{
    return static_cast<unsigned int>(std::clamp(slot, 0, 9) + smu::core::SMU_VK_0);
}

std::pair<int, int> FrameDelaysForFps(unsigned int fps)
{
    const unsigned int safeFps = std::max(1u, fps);
    const float delayFloat = 1000.0f / static_cast<float>(safeFps);
    const int delayFloor = std::max(1, static_cast<int>(delayFloat));
    const int delayCeil = delayFloor + 1;
    const float fractional = delayFloat - static_cast<float>(delayFloor);
    constexpr float epsilon = 0.008f;

    if (fractional < 0.33f - epsilon) {
        return {delayFloor, delayFloor};
    }
    if (fractional > 0.66f + epsilon) {
        return {delayCeil, delayCeil};
    }
    return {delayFloor, delayCeil};
}

void ReleaseZoomOrShift()
{
    if (!globalzoomin) {
        ReleaseKeyBinded(vk_shiftkey);
    }
}

bool LagSwitchConfigsEqual(const smu::platform::LagSwitchConfig& left, const smu::platform::LagSwitchConfig& right)
{
    return left.enabled == right.enabled &&
        left.inboundHardBlock == right.inboundHardBlock &&
        left.outboundHardBlock == right.outboundHardBlock &&
        left.fakeLagEnabled == right.fakeLagEnabled &&
        left.inboundFakeLag == right.inboundFakeLag &&
        left.outboundFakeLag == right.outboundFakeLag &&
        left.fakeLagDelayMs == right.fakeLagDelayMs &&
        left.targetRobloxOnly == right.targetRobloxOnly &&
        left.useUdp == right.useUdp &&
        left.useTcp == right.useTcp &&
        left.preventDisconnect == right.preventDisconnect &&
        left.autoUnblock == right.autoUnblock &&
        left.maxDurationSeconds == right.maxDurationSeconds &&
        left.unblockDurationMs == right.unblockDurationMs &&
        left.targetMode == right.targetMode &&
        left.remoteIps == right.remoteIps &&
        left.remotePorts == right.remotePorts &&
        left.includeRobloxDynamicIps == right.includeRobloxDynamicIps;
}

} // namespace

MacroRuntime::~MacroRuntime()
{
    stop();
}

void MacroRuntime::resetInputPollCache()
{
    inputPollBackend_ = smu::platform::GetInputBackend();
    inputPollKnown_.fill(0);
    inputPollPressed_.fill(0);
    inputPollCacheActive_ = true;
}

bool MacroRuntime::cachedIsKeyPressed(unsigned int key) const
{
    if (key == smu::core::SMU_VK_MOUSE_WHEEL_UP || key == smu::core::SMU_VK_MOUSE_WHEEL_DOWN) {
        return false;
    }

    if (!inputPollCacheActive_) {
        inputPollBackend_ = smu::platform::GetInputBackend();
        inputPollKnown_.fill(0);
        inputPollPressed_.fill(0);
        inputPollCacheActive_ = true;
    }

    if (!inputPollBackend_) {
        return false;
    }

    if (key >= inputPollKnown_.size()) {
        return inputPollBackend_->isKeyPressed(key);
    }

    if (inputPollKnown_[key] == 0) {
        inputPollPressed_[key] = inputPollBackend_->isKeyPressed(key) ? 1 : 0;
        inputPollKnown_[key] = 1;
    }
    return inputPollPressed_[key] != 0;
}

bool MacroRuntime::isModifierPressed(unsigned int key) const
{
    switch (key) {
    case VK_SHIFT:
        return cachedIsKeyPressed(VK_SHIFT) || cachedIsKeyPressed(VK_LSHIFT) || cachedIsKeyPressed(VK_RSHIFT);
    case VK_CONTROL:
        return cachedIsKeyPressed(VK_CONTROL) || cachedIsKeyPressed(VK_LCONTROL) || cachedIsKeyPressed(VK_RCONTROL);
    case VK_MENU:
        return cachedIsKeyPressed(VK_MENU) || cachedIsKeyPressed(VK_LMENU) || cachedIsKeyPressed(VK_RMENU);
    case VK_LWIN:
        return cachedIsKeyPressed(VK_LWIN) || cachedIsKeyPressed(VK_RWIN);
    default:
        return cachedIsKeyPressed(key);
    }
}

bool MacroRuntime::areHotkeyModifiersPressed(unsigned int combinedKey) const
{
    if ((combinedKey & HOTKEY_MASK_WIN) && !isModifierPressed(VK_LWIN)) return false;
    if ((combinedKey & HOTKEY_MASK_CTRL) && !isModifierPressed(VK_CONTROL)) return false;
    if ((combinedKey & HOTKEY_MASK_ALT) && !isModifierPressed(VK_MENU)) return false;
    if ((combinedKey & HOTKEY_MASK_SHIFT) && !isModifierPressed(VK_SHIFT)) return false;
    return true;
}

bool MacroRuntime::anyInputPressedForHotkeySuppression() const
{
    for (unsigned int key = 1; key <= 0xFF; ++key) {
        if (cachedIsKeyPressed(key)) {
            return true;
        }
    }

    return false;
}

void MacroRuntime::start()
{
    bool expected = false;
    if (!running_.compare_exchange_strong(expected, true, std::memory_order_acq_rel)) {
        return;
    }

    antiAfkTimerArmed_ = false;
    antiAfkLastKeyboardSerialSeen_ = 0;
    LogInfo("Portable macro runtime starting.");
    controllerThread_ = std::thread(&MacroRuntime::controllerLoop, this);
}

void MacroRuntime::stop()
{
    bool expected = true;
    if (!running_.compare_exchange_strong(expected, false, std::memory_order_acq_rel)) {
        return;
    }

    isbhoploop.store(false, std::memory_order_release);
    g_isVk_BunnyhopHeldDown.store(false, std::memory_order_release);
    bunnyhopWorkerActive_.store(false, std::memory_order_release);
    antiAfkTimerArmed_ = false;
    antiAfkLastKeyboardSerialSeen_ = 0;

    if (controllerThread_.joinable()) {
        controllerThread_.join();
    }

    setTargetSuspended(false);
    ScriptManager::Get().shutdown();

    std::lock_guard<std::mutex> lock(workerMutex_);
    for (auto& worker : workers_) {
        if (worker && worker->thread.joinable()) {
            worker->thread.join();
        }
    }
    workers_.clear();
    LogInfo("Portable macro runtime stopped.");
}

void MacroRuntime::controllerLoop()
{
    nextProcessRefresh_ = std::chrono::steady_clock::now();
#if defined(_WIN32)
    RuntimeThreadProfiler runtimeProfiler;
#endif

    while (running_.load(std::memory_order_acquire) && ShouldKeepRunning()) {
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::ResetInputCache);
#endif
            resetInputPollCache();
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::RefreshProcesses);
#endif
            refreshTargetProcesses();
        }

        if (g_keybindCaptureActive.load(std::memory_order_acquire)) {
            if (freezeSuspended_) {
                setTargetSuspended(false);
            }
            std::this_thread::sleep_for(5ms);
#if defined(_WIN32)
            runtimeProfiler.finishLoop();
#endif
            continue;
        }

        if (g_suppressHotkeysUntilRelease.load(std::memory_order_acquire)) {
            bool anyInputPressed = false;
            {
#if defined(_WIN32)
                SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::HotkeySuppression);
#endif
                anyInputPressed = anyInputPressedForHotkeySuppression();
            }
            if (anyInputPressed) {
                if (freezeSuspended_) {
                    setTargetSuspended(false);
                }
                std::this_thread::sleep_for(5ms);
#if defined(_WIN32)
                runtimeProfiler.finishLoop();
#endif
                continue;
            }

            g_suppressHotkeysUntilRelease.store(false, std::memory_order_release);
            notbinding.store(true, std::memory_order_release);
        }

        processAntiAfkMacro();

        if (!macrotoggled || !notbinding.load(std::memory_order_acquire)) {
            if (freezeSuspended_) {
                setTargetSuspended(false);
            }
            std::this_thread::sleep_for(5ms);
#if defined(_WIN32)
            runtimeProfiler.finishLoop();
#endif
            continue;
        }

        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::PruneWorkers);
#endif
            pruneFinishedWorkers();
        }

        auto foregroundForEnabledSection = [&](int sectionIndex) {
            return !section_toggles[sectionIndex] || foregroundAllows(disable_outside_roblox[sectionIndex]);
        };

        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::Freeze);
#endif
            processFreezeMacro(foregroundForEnabledSection(0));
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::ItemDesync);
#endif
            processItemDesyncMacro(foregroundForEnabledSection(1));
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::PressKey);
#endif
            processPressKeyMacros(true);
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::Wallhop);
#endif
            processWallhopMacros(true);
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::WallessLhj);
#endif
            processWallessLhjMacro(foregroundForEnabledSection(7));
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::Speedglitch);
#endif
            processSpeedglitchMacro(foregroundForEnabledSection(3));
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::Hhj);
#endif
            processHhjMacro(foregroundForEnabledSection(2));
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::HhjMotion);
#endif
            processHhjMotionLoop();
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::ItemUnequip);
#endif
            processItemUnequipComOffsetMacro(foregroundForEnabledSection(4));
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::ItemClip);
#endif
            processItemClipMacro(foregroundForEnabledSection(8));
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::LaughClip);
#endif
            processLaughClipMacro(foregroundForEnabledSection(9));
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::WallWalk);
#endif
            processWallWalkMacro(foregroundForEnabledSection(10));
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::SpamKey);
#endif
            processSpamKeyMacros();
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::LedgeBounce);
#endif
            processLedgeBounceMacro(foregroundForEnabledSection(12));
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::Bunnyhop);
#endif
            processBunnyhopMacro(foregroundForEnabledSection(13));
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::FloorBounce);
#endif
            processFloorBounceMacro(foregroundForEnabledSection(14));
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::LagSwitch);
#endif
            processLagSwitchMacro(foregroundForEnabledSection(15));
        }
        {
#if defined(_WIN32)
            SMU_PROFILE_RUNTIME_SECTION(runtimeProfiler, RuntimeProfileSection::ImportedScripts);
#endif
            processImportedScripts();
        }

        std::this_thread::sleep_for(2ms);
#if defined(_WIN32)
        runtimeProfiler.finishLoop();
#endif
    }

    if (freezeSuspended_) {
        setTargetSuspended(false);
    }
}

void MacroRuntime::refreshTargetProcesses(bool force)
{
    const auto now = std::chrono::steady_clock::now();
    if (!force && now < nextProcessRefresh_) {
        return;
    }
    nextProcessRefresh_ = now + 1s;

    auto backend = smu::platform::GetProcessBackend();
    if (!backend) {
        targetPIDs.clear();
        processFound = false;
        return;
    }

    std::vector<smu::platform::PlatformPid> pids;
    if (takeallprocessids) {
        pids = backend->findAllProcesses(settingsBuffer);
    } else if (auto pid = backend->findMainProcess(settingsBuffer)) {
        pids.push_back(*pid);
    }

    const bool pidsChanged =
        targetPIDs.size() != pids.size() ||
        !std::equal(targetPIDs.begin(), targetPIDs.end(), pids.begin());

    targetPIDs.assign(pids.begin(), pids.end());
    processFound = !targetPIDs.empty();

    if (pidsChanged) {
        nextForegroundCheck_ = std::chrono::steady_clock::time_point{};
    }
}

bool MacroRuntime::foregroundAllows(bool disableOutsideRoblox)
{
    if (!disableOutsideRoblox) {
        return true;
    }

    const auto now = std::chrono::steady_clock::now();
    if (now < nextForegroundCheck_) {
        return cachedForegroundAllowed_;
    }

    const smu::platform::PlatformCapabilities capabilities = smu::platform::GetPlatformCapabilities();
    if (!capabilities.canDetectForegroundProcess) {
        cachedForegroundAllowed_ = true;
        nextForegroundCheck_ = now + 25ms;
        return true;
    }

    auto backend = smu::platform::GetProcessBackend();
    if (!backend) {
        cachedForegroundAllowed_ = false;
        nextForegroundCheck_ = now + 25ms;
        return false;
    }

    for (unsigned int pid : targetPIDs) {
        if (backend->isForegroundProcess(pid)) {
            cachedForegroundAllowed_ = true;
            nextForegroundCheck_ = now + 25ms;
            return true;
        }
    }
    cachedForegroundAllowed_ = false;
    nextForegroundCheck_ = now + 25ms;
    return false;
}

bool MacroRuntime::isHotkeyPressed(unsigned int combinedKey) const
{
    if (g_keybindCaptureActive.load(std::memory_order_acquire) ||
        g_suppressHotkeysUntilRelease.load(std::memory_order_acquire)) {
        return false;
    }

    const unsigned int key = combinedKey & HOTKEY_KEY_MASK;
    if (key == 0 || key == smu::core::SMU_VK_MOUSE_WHEEL_UP || key == smu::core::SMU_VK_MOUSE_WHEEL_DOWN) {
        return false;
    }

    if (!areHotkeyModifiersPressed(combinedKey)) {
        return false;
    }
    return isModifierPressed(key);
}

void MacroRuntime::processFreezeMacro(bool foregroundAllowed)
{
    if (!section_toggles[0]) {
        if (freezeSuspended_) {
            setTargetSuspended(false);
        }
        freezeWasPressed_ = false;
        return;
    }

    const bool pressed = isHotkeyPressed(vk_mbutton);
    if (isfreezeswitch) {
        if (pressed && !freezeWasPressed_ && foregroundAllowed) {
            setTargetSuspended(!freezeSuspended_);
        }
    } else if (pressed && foregroundAllowed) {
        if (!freezeSuspended_) {
            setTargetSuspended(true);
        }
    } else if (freezeSuspended_) {
        setTargetSuspended(false);
    }
    freezeWasPressed_ = pressed;

    if (!freezeSuspended_) {
        return;
    }

    const auto elapsedMs = std::chrono::duration_cast<std::chrono::milliseconds>(
        std::chrono::steady_clock::now() - freezeStartTime_).count();
    if (elapsedMs >= static_cast<long long>(maxfreezetime * 1000.0f)) {
        setTargetSuspended(false);
        std::this_thread::sleep_for(std::chrono::milliseconds(std::max(0, maxfreezeoverride)));
        if (running_.load(std::memory_order_acquire) && (isfreezeswitch || isHotkeyPressed(vk_mbutton))) {
            setTargetSuspended(true);
        }
    }
}

void MacroRuntime::processItemDesyncMacro(bool foregroundAllowed)
{
    const bool pressed = foregroundAllowed && section_toggles[1] && isHotkeyPressed(vk_f5);
    isdesyncloop.store(pressed, std::memory_order_release);
    desyncWasPressed_ = pressed;

    if (!pressed) {
        return;
    }

    const unsigned int slotKey = InventorySlotKey(desync_slot);
    HoldKey(slotKey);
    ReleaseKey(slotKey);
    HoldKey(slotKey);
    ReleaseKey(slotKey);
}

void MacroRuntime::processPressKeyMacros(bool foregroundAllowed)
{
    if (pressKeyWasPressed_.size() != presskey_instances.size()) {
        pressKeyWasPressed_.assign(presskey_instances.size(), false);
    }

    for (std::size_t index = 0; index < presskey_instances.size(); ++index) {
        auto& inst = presskey_instances[index];
        if (!inst.section_enabled) {
            pressKeyWasPressed_[index] = false;
            continue;
        }
        const bool allowed = foregroundAllows(inst.presskeyinroblox) && foregroundAllowed;
        const bool pressed = allowed && isHotkeyPressed(inst.vk_trigger);
        const bool edge = pressed && !pressKeyWasPressed_[index];
        pressKeyWasPressed_[index] = pressed;
        if (!edge) {
            continue;
        }

        const unsigned int trigger = inst.vk_trigger;
        const unsigned int output = inst.vk_presskey;
        const int bonusDelay = std::max(0, inst.PressKeyBonusDelay);
        const int pressDelay = std::max(1, inst.PressKeyDelay);
        runWorker([this, trigger, output, bonusDelay, pressDelay] {
            if (trigger == output) {
                ReleaseKeyBinded(trigger);
            }
            if (bonusDelay > 0) {
                std::this_thread::sleep_for(std::chrono::milliseconds(bonusDelay));
            }
            if (!running_.load(std::memory_order_acquire)) return;
            HoldKeyBinded(output);
            std::this_thread::sleep_for(std::chrono::milliseconds(pressDelay));
            ReleaseKeyBinded(output);
        });
    }
}

void MacroRuntime::processWallhopMacros(bool foregroundAllowed)
{
    if (wallhopWasPressed_.size() != wallhop_instances.size()) {
        wallhopWasPressed_.assign(wallhop_instances.size(), false);
    }

    for (std::size_t index = 0; index < wallhop_instances.size(); ++index) {
        auto& inst = wallhop_instances[index];
        if (!inst.section_enabled) {
            wallhopWasPressed_[index] = false;
            continue;
        }
        const bool allowed = foregroundAllows(inst.disable_outside_roblox) && foregroundAllowed;
        const bool pressed = allowed && isHotkeyPressed(inst.vk_trigger);
        const bool edge = pressed && !wallhopWasPressed_[index];
        wallhopWasPressed_[index] = pressed;
        if (!edge) {
            continue;
        }

        const int dx = inst.wallhop_dx;
        const int dy = inst.wallhop_dy;
        const int vertical = inst.wallhop_vertical;
        const int delay = std::max(1, inst.WallhopDelay);
        const int bonusDelay = std::max(0, inst.WallhopBonusDelay);
        const bool left = inst.wallhopswitch;
        const bool jump = inst.toggle_jump;
        const bool flickBack = inst.toggle_flick;
        const unsigned int jumpKey = inst.vk_jumpkey;

        runWorker([dx, dy, vertical, delay, bonusDelay, left, jump, flickBack, jumpKey] {
            MoveMouse(left ? -dx : dx, vertical);
            if (flickBack) {
                if (bonusDelay > 0 && bonusDelay < delay) {
                    std::this_thread::sleep_for(std::chrono::milliseconds(bonusDelay));
                    if (jump) HoldKeyBinded(jumpKey);
                    std::this_thread::sleep_for(std::chrono::milliseconds(delay - bonusDelay));
                } else {
                    if (jump) HoldKeyBinded(jumpKey);
                    std::this_thread::sleep_for(std::chrono::milliseconds(delay));
                }
                MoveMouse(left ? -dy : dy, -vertical);
            } else if (jump) {
                HoldKeyBinded(jumpKey);
            }

            if (jump) {
                if (100 - delay > 0) {
                    std::this_thread::sleep_for(std::chrono::milliseconds(100 - delay));
                }
                ReleaseKeyBinded(jumpKey);
            }
        });
    }
}

void MacroRuntime::processWallessLhjMacro(bool foregroundAllowed)
{
    const bool pressed = foregroundAllowed && section_toggles[7] && isHotkeyPressed(vk_f6);
    const bool edge = pressed && !wallessLhjWasPressed_;
    wallessLhjWasPressed_ = pressed;
    if (!edge) {
        return;
    }

    const auto pids = currentTargetPids();
    runWorker([this, pids] {
        const unsigned int sideKey = wallesslhjswitch ? smu::core::SMU_VK_A : smu::core::SMU_VK_D;
        HoldKey(sideKey);
        std::this_thread::sleep_for(15ms);
        HoldKey(smu::core::SMU_VK_SPACE);
        std::this_thread::sleep_for(30ms);

        setPidsSuspended(pids, true);
        std::this_thread::sleep_for(50ms);
        ReleaseKey(sideKey);
        std::this_thread::sleep_for(500ms);

        if (!globalzoomin) {
            HoldKeyBinded(vk_shiftkey);
        } else {
            HoldKeyBinded(globalzoominreverse ? smu::core::SMU_VK_MOUSE_WHEEL_DOWN : smu::core::SMU_VK_MOUSE_WHEEL_UP);
        }
        setPidsSuspended(pids, false);
        std::this_thread::sleep_for(50ms);
        ReleaseZoomOrShift();
        ReleaseKey(smu::core::SMU_VK_SPACE);
    });
}

void MacroRuntime::processSpeedglitchMacro(bool foregroundAllowed)
{
    const bool pressed = foregroundAllowed && section_toggles[3] && isHotkeyPressed(vk_xkey);
    if (pressed && !speedWasPressed_) {
        isspeed.store(!isspeed.load(std::memory_order_acquire), std::memory_order_release);
    } else if (!pressed && isspeedswitch) {
        isspeed.store(false, std::memory_order_release);
    }
    speedWasPressed_ = pressed;

    if (!foregroundAllowed || !section_toggles[3] || !isspeed.load(std::memory_order_acquire)) {
        return;
    }

    const auto [sleep1, sleep2] = FrameDelaysForFps(RobloxFPS.load(std::memory_order_relaxed));
    MoveMouse(speed_strengthx, 0);
    std::this_thread::sleep_for(std::chrono::milliseconds(sleep1));
    MoveMouse(speed_strengthy, 0);
    std::this_thread::sleep_for(std::chrono::milliseconds(sleep2));
}

void MacroRuntime::processHhjMacro(bool foregroundAllowed)
{
    const bool pressed = foregroundAllowed && section_toggles[2] && isHotkeyPressed(vk_xbutton1);
    const bool edge = pressed && !hhjWasPressed_;
    hhjWasPressed_ = pressed;
    if (!edge) {
        return;
    }

    const auto pids = currentTargetPids();
    runWorker([this, pids] {
        if (autotoggle) {
            HoldKeyBinded(vk_autohhjkey1);
            std::this_thread::sleep_for(std::chrono::milliseconds(std::max(0, AutoHHJKey1Time)));
            HoldKeyBinded(vk_autohhjkey2);
            std::this_thread::sleep_for(std::chrono::milliseconds(std::max(0, AutoHHJKey2Time)));
        }

        setPidsSuspended(pids, true);
        if (!HHJFreezeDelayApply) {
            if (!fasthhj) {
                std::this_thread::sleep_for(300ms);
            }
            std::this_thread::sleep_for(200ms);
        } else if (HHJFreezeDelayOverride > 0) {
            std::this_thread::sleep_for(std::chrono::milliseconds(HHJFreezeDelayOverride));
        }

        if (autotoggle) {
            ReleaseKeyBinded(vk_autohhjkey1);
            ReleaseKeyBinded(vk_autohhjkey2);
        }
        setPidsSuspended(pids, false);

        std::this_thread::sleep_for(std::chrono::milliseconds(std::max(0, HHJDelay1)));
        if (!globalzoomin) {
            HoldKeyBinded(vk_shiftkey);
        } else {
            HoldKeyBinded(globalzoominreverse ? smu::core::SMU_VK_MOUSE_WHEEL_DOWN : smu::core::SMU_VK_MOUSE_WHEEL_UP);
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(std::max(0, HHJDelay2)));
        if (HHJLength > 0) {
            isHHJ.store(true, std::memory_order_release);
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(std::max(0, HHJDelay3)));
        ReleaseZoomOrShift();
        if (HHJLength > 0) {
            std::this_thread::sleep_for(std::chrono::milliseconds(HHJLength));
            isHHJ.store(false, std::memory_order_release);
        }
    });
}

void MacroRuntime::processHhjMotionLoop()
{
    if (!section_toggles[2] || !isHHJ.load(std::memory_order_acquire)) {
        return;
    }

    const auto [sleep1, sleep2] = FrameDelaysForFps(RobloxFPS.load(std::memory_order_relaxed));
    MoveMouse(speed_strengthx, 0);
    std::this_thread::sleep_for(std::chrono::milliseconds(sleep1));
    MoveMouse(speed_strengthy, 0);
    std::this_thread::sleep_for(std::chrono::milliseconds(sleep2));
}

void MacroRuntime::processItemUnequipComOffsetMacro(bool foregroundAllowed)
{
    const bool pressed = foregroundAllowed && section_toggles[4] && isHotkeyPressed(vk_f8);
    const bool edge = pressed && !itemUnequipWasPressed_;
    itemUnequipWasPressed_ = pressed;
    if (!edge) {
        return;
    }

    bool expected = false;
    if (!itemUnequipWorkerActive_.compare_exchange_strong(expected, true, std::memory_order_acq_rel)) {
        return;
    }

    const std::string customText = CustomTextChar;
    const std::string emoteText = text;
    runWorker([this, customText, emoteText] {
        HoldKeyBinded(vk_chatkey);
        std::this_thread::sleep_for(50ms);
        ReleaseKeyBinded(vk_chatkey);
        std::this_thread::sleep_for(25ms);

        const bool custom = !customText.empty();
        smu::platform::pasteText(custom ? customText : emoteText, std::max(0, PasteDelay));
        std::this_thread::sleep_for(100ms);
        HoldKeyBinded(vk_enterkey);
        std::this_thread::sleep_for(35ms);
        ReleaseKeyBinded(vk_enterkey);

        if (custom) {
            itemUnequipWorkerActive_.store(false, std::memory_order_release);
            return;
        }

        if (selected_dropdown == 2) {
            std::this_thread::sleep_for(16ms);
        } else {
            std::this_thread::sleep_for(65ms);
        }

        if (selected_dropdown == 0) {
            std::this_thread::sleep_for(815ms);
        }
        if (selected_dropdown == 1) {
            std::this_thread::sleep_for(175ms);
        }

        const unsigned int slotKey = InventorySlotKey(speed_slot);
        HoldKey(slotKey);
        std::this_thread::sleep_for(4ms);
        ReleaseKey(slotKey);
        std::this_thread::sleep_for(4ms);
        if (!unequiptoggle) {
            HoldKey(slotKey);
        }
        ReleaseKey(slotKey);
        itemUnequipWorkerActive_.store(false, std::memory_order_release);
    });
}

void MacroRuntime::processItemClipMacro(bool foregroundAllowed)
{
    const bool pressed = foregroundAllowed && section_toggles[8] && isHotkeyPressed(vk_clipkey);
    if (pressed && !itemClipWasPressed_) {
        isitemloop.store(!isitemloop.load(std::memory_order_acquire), std::memory_order_release);
    } else if (!pressed && isitemclipswitch) {
        isitemloop.store(false, std::memory_order_release);
    }
    itemClipWasPressed_ = pressed;

    if (!foregroundAllowed || !section_toggles[8] || !isitemloop.load(std::memory_order_acquire)) {
        return;
    }

    const int halfDelay = std::max(1, clip_delay / 2);
    const unsigned int slotKey = InventorySlotKey(clip_slot);
    HoldKey(slotKey);
    std::this_thread::sleep_for(std::chrono::milliseconds(halfDelay));
    ReleaseKey(slotKey);
    std::this_thread::sleep_for(std::chrono::milliseconds(halfDelay));
}

void MacroRuntime::processWallWalkMacro(bool foregroundAllowed)
{
    const bool pressed = foregroundAllowed && section_toggles[10] && isHotkeyPressed(vk_wallkey);
    if (pressed && !wallWalkWasPressed_) {
        iswallwalkloop.store(!iswallwalkloop.load(std::memory_order_acquire), std::memory_order_release);
    } else if (!pressed && iswallwalkswitch) {
        iswallwalkloop.store(false, std::memory_order_release);
    }
    wallWalkWasPressed_ = pressed;

    if (!foregroundAllowed || !section_toggles[10] || !iswallwalkloop.load(std::memory_order_acquire)) {
        return;
    }

    const int delay = std::max(1, static_cast<int>(((1000.0f / std::max(1u, RobloxFPS.load(std::memory_order_relaxed))) + 0.5f) * 1.1f));
    const int firstMove = wallwalktoggleside ? -wallwalk_strengthx : wallwalk_strengthx;
    const int secondMove = wallwalktoggleside ? -wallwalk_strengthy : wallwalk_strengthy;
    MoveMouse(firstMove, 0);
    std::this_thread::sleep_for(std::chrono::milliseconds(delay));
    MoveMouse(secondMove, 0);
    std::this_thread::sleep_for(std::chrono::microseconds(std::max(0, RobloxWallWalkValueDelay)));
}

void MacroRuntime::processLaughClipMacro(bool foregroundAllowed)
{
    const bool pressed = foregroundAllowed && section_toggles[9] && isHotkeyPressed(vk_laughkey);
    const bool edge = pressed && !laughClipWasPressed_;
    laughClipWasPressed_ = pressed;
    if (!edge) {
        return;
    }

    bool expected = false;
    if (!laughClipWorkerActive_.compare_exchange_strong(expected, true, std::memory_order_acq_rel)) {
        return;
    }

    runWorker([this] {
        HoldKeyBinded(vk_chatkey);
        std::this_thread::sleep_for(50ms);
        ReleaseKeyBinded(vk_chatkey);
        std::this_thread::sleep_for(25ms);
        smu::platform::pasteText("/e laugh", std::max(0, PasteDelay));
        std::this_thread::sleep_for(100ms);
        HoldKeyBinded(vk_enterkey);
        std::this_thread::sleep_for(35ms);
        ReleaseKeyBinded(vk_enterkey);

        std::this_thread::sleep_for(248ms);

        HoldKey(smu::core::SMU_VK_SPACE);
        if (!globalzoomin) {
            HoldKeyBinded(vk_shiftkey);
        } else {
            HoldKeyBinded(globalzoominreverse ? smu::core::SMU_VK_MOUSE_WHEEL_DOWN : smu::core::SMU_VK_MOUSE_WHEEL_UP);
        }

        if (!laughmoveswitch) {
            HoldKey(smu::core::SMU_VK_S);
        }

        std::this_thread::sleep_for(25ms);
        ReleaseKey(smu::core::SMU_VK_SPACE);
        ReleaseZoomOrShift();
        std::this_thread::sleep_for(25ms);

        if (!laughmoveswitch) {
            ReleaseKey(smu::core::SMU_VK_S);
        }
        laughClipWorkerActive_.store(false, std::memory_order_release);
    });
}

void MacroRuntime::processSpamKeyMacros()
{
    if (spamKeyWasPressed_.size() != spamkey_instances.size()) {
        spamKeyWasPressed_.assign(spamkey_instances.size(), false);
    }

    for (std::size_t index = 0; index < spamkey_instances.size(); ++index) {
        auto& inst = spamkey_instances[index];
        if (!inst.section_enabled) {
            spamKeyWasPressed_[index] = false;
            inst.thread_active.store(false, std::memory_order_release);
            continue;
        }
        const bool allowed = foregroundAllows(inst.disable_outside_roblox);
        const bool pressed = allowed && isHotkeyPressed(inst.vk_trigger);
        const bool edge = pressed && !spamKeyWasPressed_[index];
        spamKeyWasPressed_[index] = pressed;

        if (!allowed || inst.isspamswitch) {
            inst.thread_active.store(false, std::memory_order_release);
        }
        if (!edge) {
            continue;
        }

        const bool newActive = !inst.thread_active.load(std::memory_order_acquire);
        inst.thread_active.store(newActive, std::memory_order_release);
        if (!newActive) {
            continue;
        }

        runWorker([this, &inst] {
            while (running_.load(std::memory_order_acquire) &&
                   ShouldKeepRunning() &&
                   inst.thread_active.load(std::memory_order_acquire) &&
                   macrotoggled &&
                   notbinding.load(std::memory_order_acquire) &&
                   inst.section_enabled) {
                const int delay = std::max(1, inst.real_delay);
                HoldKeyBinded(inst.vk_spamkey);
                std::this_thread::sleep_for(std::chrono::milliseconds(delay));
                ReleaseKeyBinded(inst.vk_spamkey);
                std::this_thread::sleep_for(std::chrono::milliseconds(delay));
            }
            inst.thread_active.store(false, std::memory_order_release);
        });
    }
}

void MacroRuntime::processLedgeBounceMacro(bool foregroundAllowed)
{
    const bool pressed = foregroundAllowed && section_toggles[12] && isHotkeyPressed(vk_bouncekey);
    const bool edge = pressed && !ledgeBounceWasPressed_;
    ledgeBounceWasPressed_ = pressed;
    if (!edge) {
        return;
    }

    runWorker([] {
        const float sensitivity = std::max(0.01f, static_cast<float>(std::atof(RobloxSensValue)));
        int turn90 = static_cast<int>((camfixtoggle ? 250.0f : 180.0f) / sensitivity);
        unsigned int sideKey = smu::core::SMU_VK_D;
        if (bouncesidetoggle) {
            turn90 = -turn90;
            sideKey = smu::core::SMU_VK_A;
        }

        MoveMouse(-turn90, 0);
        std::this_thread::sleep_for(90ms);
        HoldKey(smu::core::SMU_VK_S);
        std::this_thread::sleep_for(40ms);
        ReleaseKey(smu::core::SMU_VK_S);
        MoveMouse(turn90, 0);
        HoldKey(sideKey);
        std::this_thread::sleep_for(16ms);

        if (bouncerealignsideways) {
            ReleaseKey(sideKey);
            HoldKey(smu::core::SMU_VK_W);
            MoveMouse(turn90, 0);
            std::this_thread::sleep_for(70ms);
            ReleaseKey(smu::core::SMU_VK_W);
            MoveMouse(-turn90, 0);
            if (bounceautohold) {
                HoldKey(sideKey);
            }
        } else {
            ReleaseKey(sideKey);
            if (bounceautohold) {
                HoldKey(smu::core::SMU_VK_W);
            }
            MoveMouse(turn90, 0);
        }
    });
}


bool MacroRuntime::isBunnyhopPhysicallyHeld() const
{
    const unsigned int key = vk_bunnyhopkey & HOTKEY_KEY_MASK;
    if (key == 0 ||
        key == smu::core::SMU_VK_MOUSE_WHEEL_UP ||
        key == smu::core::SMU_VK_MOUSE_WHEEL_DOWN) {
        return false;
    }

    if (!areHotkeyModifiersPressed(vk_bunnyhopkey)) {
        return false;
    }

    // Mouse buttons do not pass through the Windows low-level keyboard hook.
    // For mouse-triggered bhop binds, keep using the backend's physical state.
    if (key >= VK_LBUTTON && key <= VK_XBUTTON2) {
        return isModifierPressed(key);
    }

    // Keyboard-triggered bhop must use a physical-key latch, not the raw
    // backend state. The macro injects the same key it uses as its trigger.
    // If we read the normal key state on Windows, our injected key-up event
    // cancels the user's held Space/key and the loop stops after one hop.
    //
    // On Linux, this latch is updated from real evdev devices only; the SMU
    // uinput virtual device is explicitly excluded by the Linux backend.
    return g_isVk_BunnyhopHeldDown.load(std::memory_order_acquire);
}

void MacroRuntime::runBunnyhopWorker()
{
    while (running_.load(std::memory_order_acquire) &&
           ShouldKeepRunning() &&
           isbhoploop.load(std::memory_order_acquire) &&
           macrotoggled &&
           notbinding.load(std::memory_order_acquire) &&
           section_toggles[13]) {
        const unsigned int outputKey = vk_bunnyhopkey;
        const int halfDelay = std::max(1, BunnyHopDelay / 2);

        HoldKeyBinded(outputKey);
        std::this_thread::sleep_for(std::chrono::milliseconds(halfDelay));

        if (!running_.load(std::memory_order_acquire) ||
            !ShouldKeepRunning() ||
            !isbhoploop.load(std::memory_order_acquire)) {
            ReleaseKeyBinded(outputKey);
            break;
        }

        ReleaseKeyBinded(outputKey);
        std::this_thread::sleep_for(std::chrono::milliseconds(halfDelay));
    }

    isbhoploop.store(false, std::memory_order_release);
    bunnyhopWorkerActive_.store(false, std::memory_order_release);
}

void MacroRuntime::processBunnyhopMacro(bool foregroundAllowed)
{
    if (!foregroundAllowed || !section_toggles[13] || !macrotoggled || !notbinding.load(std::memory_order_acquire)) {
        isbhoploop.store(false, std::memory_order_release);
        bunnyhopRunning_ = false;
        return;
    }

    if (isHotkeyPressed(vk_chatkey)) {
        bunnyhopChatLocked_ = true;
    }
    if (bunnyhopChatLocked_ && (isHotkeyPressed(VK_RETURN) || isHotkeyPressed(VK_LBUTTON))) {
        bunnyhopChatLocked_ = false;
    }

    const bool held = isBunnyhopPhysicallyHeld();
    const bool shouldHop = held && (!bunnyhopsmart || !bunnyhopChatLocked_);
    isbhoploop.store(shouldHop, std::memory_order_release);

    if (!shouldHop) {
        bunnyhopRunning_ = false;
        return;
    }

    if (!bunnyhopRunning_) {
        LogInfo("Smart Bunnyhop macro active.");
        bunnyhopRunning_ = true;
    }

    bool expected = false;
    if (bunnyhopWorkerActive_.compare_exchange_strong(expected, true, std::memory_order_acq_rel)) {
        runWorker([this] {
            runBunnyhopWorker();
        });
    }
}

void MacroRuntime::processAntiAfkMacro()
{
#if defined(_WIN32)
    if (!antiafktoggle || !processFound || !notbinding.load(std::memory_order_acquire)) {
        antiAfkTimerArmed_ = false;
        antiAfkLastKeyboardSerialSeen_ = 0;
        return;
    }

    auto backend = smu::platform::GetProcessBackend();
    if (!backend) {
        antiAfkTimerArmed_ = false;
        antiAfkLastKeyboardSerialSeen_ = 0;
        return;
    }

    const auto pids = currentTargetPids();
    if (pids.empty()) {
        antiAfkTimerArmed_ = false;
        antiAfkLastKeyboardSerialSeen_ = 0;
        return;
    }

    bool robloxForeground = false;
    for (unsigned int pid : pids) {
        if (backend->isForegroundProcess(pid)) {
            robloxForeground = true;
            break;
        }
    }

    const auto now = std::chrono::steady_clock::now();
    const std::uint64_t keyboardSerial = g_realKeyboardActivitySerial.load(std::memory_order_acquire);
    if (!antiAfkTimerArmed_) {
        antiAfkTimerArmed_ = true;
        antiAfkLastActivity_ = now;
        antiAfkLastKeyboardSerialSeen_ = keyboardSerial;
    }

    if (robloxForeground && keyboardSerial != antiAfkLastKeyboardSerialSeen_) {
        antiAfkLastKeyboardSerialSeen_ = keyboardSerial;
        antiAfkLastActivity_ = now;
    }

    const int antiAfkMinutes = std::max(1, AntiAFKTime);
    const auto elapsedMinutes = std::chrono::duration_cast<std::chrono::minutes>(now - antiAfkLastActivity_).count();
    if (elapsedMinutes < antiAfkMinutes) {
        return;
    }

    const unsigned int afkKey = vk_afkkey;
    if ((afkKey & HOTKEY_KEY_MASK) == 0) {
        antiAfkLastActivity_ = now;
        return;
    }

    auto pressAntiAfkKey = [this, afkKey]() {
        if (doublepressafkkey) {
            HoldKeyBinded(afkKey);
            std::this_thread::sleep_for(20ms);
            ReleaseKeyBinded(afkKey);
            std::this_thread::sleep_for(20ms);
            HoldKeyBinded(afkKey);
            std::this_thread::sleep_for(20ms);
            ReleaseKeyBinded(afkKey);
        } else {
            HoldKeyBinded(afkKey);
            std::this_thread::sleep_for(20ms);
            ReleaseKeyBinded(afkKey);
        }
    };

    if (robloxForeground) {
        pressAntiAfkKey();
        antiAfkLastActivity_ = std::chrono::steady_clock::now();
        antiAfkLastKeyboardSerialSeen_ = keyboardSerial;
        return;
    }

    HWND window = FindNewestProcessWindow(pids);
    if (!window) {
        antiAfkLastActivity_ = now;
        return;
    }

    HWND originalForeground = GetForegroundWindow();
    POINT originalMousePos = {};
    const bool hasMousePos = GetCursorPos(&originalMousePos) != FALSE;

    ShowWindow(window, SW_RESTORE);
    SetWindowPos(window, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
    SetWindowPos(window, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
    TapWindowCenter(window);
    std::this_thread::sleep_for(500ms);
    pressAntiAfkKey();
    std::this_thread::sleep_for(200ms);

    if (originalForeground) {
        SetWindowPos(originalForeground, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        SetWindowPos(originalForeground, HWND_NOTOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE);
        std::this_thread::sleep_for(50ms);
        SetForegroundWindow(originalForeground);
        std::this_thread::sleep_for(1s);
    }
    if (hasMousePos) {
        SetCursorPos(originalMousePos.x, originalMousePos.y);
    }

    antiAfkLastActivity_ = std::chrono::steady_clock::now();
#else
    antiAfkTimerArmed_ = false;
    antiAfkLastKeyboardSerialSeen_ = 0;
#endif
}

void MacroRuntime::processFloorBounceMacro(bool foregroundAllowed)
{
    const bool pressed = foregroundAllowed && section_toggles[14] && isHotkeyPressed(vk_floorbouncekey);
    const bool edge = pressed && !floorBounceWasPressed_;
    floorBounceWasPressed_ = pressed;
    if (!edge) {
        return;
    }

    const auto pids = currentTargetPids();
    runWorker([this, pids] {
        HoldKey(smu::core::SMU_VK_SPACE);
        std::this_thread::sleep_for(521ms);
        setPidsSuspended(pids, true);
        std::this_thread::sleep_for(72ms);
        setPidsSuspended(pids, false);
        std::this_thread::sleep_for(72ms);
        setPidsSuspended(pids, true);
        std::this_thread::sleep_for(72ms);
        setPidsSuspended(pids, false);
        ReleaseKey(smu::core::SMU_VK_SPACE);

        if (floorbouncehhj) {
            std::this_thread::sleep_for(std::chrono::milliseconds(std::max(0, FloorBounceDelay1)));
            HoldKeyBinded(vk_shiftkey);
            std::this_thread::sleep_for(std::chrono::milliseconds(std::max(0, FloorBounceDelay2)));
            isHHJ.store(true, std::memory_order_release);
            std::this_thread::sleep_for(std::chrono::milliseconds(std::max(0, FloorBounceDelay3)));
            isHHJ.store(false, std::memory_order_release);
            std::this_thread::sleep_for(std::chrono::milliseconds(100));
            ReleaseKeyBinded(vk_shiftkey);
        }
    });
}

void MacroRuntime::processLagSwitchMacro(bool foregroundAllowed)
{
    if (!section_toggles[15]) {
        if (lagSwitchWasPressed_ || hasLastLagSwitchConfig_) {
            if (auto backend = smu::platform::GetNetworkLagBackend()) {
                backend->setBlockingActive(false);
            }
        }
        lagSwitchWasPressed_ = false;
        hasLastLagSwitchConfig_ = false;
        return;
    }

    auto backend = smu::platform::GetNetworkLagBackend();
    if (!backend) {
        return;
    }

    if (!foregroundAllowed) {
        if (lagSwitchWasPressed_ || backend->isBaseBlockingActive()) {
            backend->setBlockingActive(false);
        }
        lagSwitchWasPressed_ = false;
        return;
    }

    const bool pressed = isHotkeyPressed(vk_lagswitchkey);
    bool shouldInitializeBackend = false;
    if (islagswitchswitch) {
        shouldInitializeBackend = pressed && !lagSwitchWasPressed_ && !backend->isBaseBlockingActive();
    } else {
        shouldInitializeBackend = pressed;
    }

    if (shouldInitializeBackend && !bWinDivertEnabled) {
        std::string backendError;
        if (!backend->init(&backendError)) {
            if (!backendError.empty()) {
                LogWarning(backendError);
            }
#if defined(_WIN32)
            if (!smu::platform::windows::IsRunAsAdmin()) {
                if (IsNotificationSuppressed(kAdminElevationWarningId)) {
                    smu::app::SaveSharedProfilesNow();
                    if (smu::platform::windows::RestartAsAdmin()) {
                        done.store(true, std::memory_order_release);
                        running.store(false, std::memory_order_release);
                        auto& appState = smu::core::GetAppState();
                        appState.done.store(true, std::memory_order_release);
                        appState.running.store(false, std::memory_order_release);
                    }
                } else {
                    bShowAdminPopup = true;
                }
            }
#endif
            lagSwitchWasPressed_ = pressed;
            return;
        }
    }

    smu::platform::LagSwitchConfig config;
    config.enabled = bWinDivertEnabled;
    config.inboundHardBlock = lagswitchinbound;
    config.outboundHardBlock = lagswitchoutbound;
    config.fakeLagEnabled = lagswitchlag;
    config.inboundFakeLag = lagswitchlaginbound;
    config.outboundFakeLag = lagswitchlagoutbound;
    config.fakeLagDelayMs = lagswitchlagdelay;
    config.targetRobloxOnly = lagswitchtargetroblox;
    config.targetMode = lagswitchtargetroblox ? smu::platform::LagSwitchTargetMode::Roblox : smu::platform::LagSwitchTargetMode::All;
    config.useTcp = lagswitchusetcp;
    config.useUdp = true;
    config.preventDisconnect = prevent_disconnect;
    config.autoUnblock = lagswitch_autounblock;
    config.maxDurationSeconds = lagswitch_max_duration;
    config.unblockDurationMs = lagswitch_unblock_ms;
    if (!hasLastLagSwitchConfig_ || !LagSwitchConfigsEqual(config, lastLagSwitchConfig_)) {
        backend->setConfig(config);
        lastLagSwitchConfig_ = config;
        hasLastLagSwitchConfig_ = true;
    }

    if (islagswitchswitch) {
        if (pressed && !lagSwitchWasPressed_) {
            const bool nextActive = !backend->isBaseBlockingActive() || lagSwitchUnblocking_;
            if (!lagSwitchUnblocking_) {
                backend->setBlockingActive(nextActive);
            }
            if (nextActive && !lagSwitchUnblocking_) {
                lagSwitchStartTime_ = std::chrono::steady_clock::now();
                lagSwitchLastUnblockTime_ = lagSwitchStartTime_;
                lagSwitchUnblocking_ = false;
            }
        }
    } else {
        if (!lagSwitchUnblocking_) {
            if (pressed && !backend->isBaseBlockingActive()) {
                lagSwitchStartTime_ = std::chrono::steady_clock::now();
                lagSwitchLastUnblockTime_ = lagSwitchStartTime_;
            }
            backend->setBlockingActive(pressed);
        }
    }
    lagSwitchWasPressed_ = pressed;

    if (lagswitch_autounblock && (backend->isBaseBlockingActive() || lagSwitchUnblocking_)) {
        const bool shouldPulse = islagswitchswitch
            ? backend->isBaseBlockingActive() || lagSwitchUnblocking_
            : pressed && (backend->isBaseBlockingActive() || lagSwitchUnblocking_);

        if (!shouldPulse) {
            lagSwitchUnblocking_ = false;
            backend->setBlockingActive(false);
        } else if (!lagSwitchUnblocking_) {
            const auto sinceLastUnblock = std::chrono::duration<float>(
                std::chrono::steady_clock::now() - lagSwitchLastUnblockTime_).count();
            if (sinceLastUnblock >= lagswitch_max_duration) {
                lagSwitchUnblocking_ = true;
                lagSwitchUnblockStartTime_ = std::chrono::steady_clock::now();
                backend->setBlockingActive(false);
            }
        } else {
            const auto unblockElapsed = std::chrono::duration_cast<std::chrono::milliseconds>(
                std::chrono::steady_clock::now() - lagSwitchUnblockStartTime_).count();
            if (unblockElapsed >= lagswitch_unblock_ms) {
                lagSwitchUnblocking_ = false;
                lagSwitchLastUnblockTime_ = std::chrono::steady_clock::now();
                backend->setBlockingActive(true);
            }
        }
    }
    }

void MacroRuntime::processImportedScripts()
{
    ScriptManager& manager = ScriptManager::Get();
    const auto scripts = manager.snapshot();
    if (importedScriptWasPressed_.size() != scripts.size()) {
        importedScriptWasPressed_.assign(scripts.size(), false);
    }

    for (std::size_t index = 0; index < scripts.size(); ++index) {
        const auto& script = scripts[index];
        if (!script) {
            continue;
        }

        const bool enabled = script->enabled.load(std::memory_order_acquire);
        const bool loaded = script->loaded.load(std::memory_order_acquire);
        const bool missing = script->missing.load(std::memory_order_acquire);
        const bool running = script->running.load(std::memory_order_acquire);
        const bool disableOutside = script->disableOutsideRoblox.load(std::memory_order_acquire);
        const unsigned int hotkey = script->hotkey.load(std::memory_order_acquire);

        if (!loaded || missing || !IsScriptHotkeyBound(hotkey)) {
            importedScriptWasPressed_[index] = false;
            continue;
        }

        if (!enabled && !running) {
            importedScriptWasPressed_[index] = false;
            continue;
        }

        const bool pressed = isHotkeyPressed(hotkey);
        const bool edge = pressed && !importedScriptWasPressed_[index];
        importedScriptWasPressed_[index] = pressed;
        if (running) {
            if (edge) {
                ScriptManager::Get().forceStopScript(index);
            }
            continue;
        }

        if (disableOutside && !foregroundAllows(true)) {
            importedScriptWasPressed_[index] = false;
            continue;
        }

        if (!edge) {
            continue;
        }

        runWorker([index] {
            ScriptManager::Get().executeScript(index);
        });
    }
}

void MacroRuntime::setTargetSuspended(bool suspended)
{
    auto backend = smu::platform::GetProcessBackend();
    if (!backend) {
        return;
    }

    if (suspended) {
        refreshTargetProcesses(true);
        if (targetPIDs.empty()) {
            return;
        }
        frozenPids_ = targetPIDs;
    }

    bool anySuccess = false;
    for (unsigned int pid : frozenPids_) {
        anySuccess = (suspended ? backend->suspend(pid) : backend->resume(pid)) || anySuccess;
    }

    if (anySuccess) {
        freezeSuspended_ = suspended;
        if (suspended) {
            freezeStartTime_ = std::chrono::steady_clock::now();
        } else {
            frozenPids_.clear();
        }
    } else if (!frozenPids_.empty()) {
        LogWarning(suspended ? "Freeze macro failed to suspend target process(es)." : "Freeze macro failed to resume target process(es).");
    }
}

std::vector<unsigned int> MacroRuntime::currentTargetPids()
{
    refreshTargetProcesses(true);
    return targetPIDs;
}

void MacroRuntime::setPidsSuspended(const std::vector<unsigned int>& pids, bool suspended)
{
    auto backend = smu::platform::GetProcessBackend();
    if (!backend) {
        return;
    }

    for (unsigned int pid : pids) {
        if (suspended) {
            backend->suspend(pid);
        } else {
            backend->resume(pid);
        }
    }
}

void MacroRuntime::runWorker(std::function<void()> task)
{
    auto slot = std::make_unique<WorkerSlot>();
    WorkerSlot* rawSlot = slot.get();
    rawSlot->thread = std::thread([rawSlot, task = std::move(task)]() mutable {
        task();
        rawSlot->done.store(true, std::memory_order_release);
    });

    std::lock_guard<std::mutex> lock(workerMutex_);
    workers_.push_back(std::move(slot));
}

void MacroRuntime::pruneFinishedWorkers()
{
    std::lock_guard<std::mutex> lock(workerMutex_);
    for (auto it = workers_.begin(); it != workers_.end();) {
        WorkerSlot& slot = **it;
        if (!slot.done.load(std::memory_order_acquire)) {
            ++it;
            continue;
        }
        if (slot.thread.joinable()) {
            slot.thread.join();
        }
        it = workers_.erase(it);
    }
}

} // namespace smu::app
