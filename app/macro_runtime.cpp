#include "macro_runtime.h"

#include "input_actions.h"
#include "../core/key_codes.h"
#include "../platform/input_backend.h"
#include "../platform/logging.h"
#include "../platform/platform_capabilities.h"
#include "../platform/process_backend.h"

#ifndef SMU_PORTABLE_GLOBALS
#define SMU_PORTABLE_GLOBALS
#endif
#include "Resource Files/globals.h"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstring>
#include <string>

namespace smu::app {
namespace {

using namespace std::chrono_literals;
using namespace Globals;

bool IsModifierPressed(const smu::platform::InputBackend& input, unsigned int key)
{
    switch (key) {
    case VK_SHIFT:
        return input.isKeyPressed(VK_SHIFT) || input.isKeyPressed(VK_LSHIFT) || input.isKeyPressed(VK_RSHIFT);
    case VK_CONTROL:
        return input.isKeyPressed(VK_CONTROL) || input.isKeyPressed(VK_LCONTROL) || input.isKeyPressed(VK_RCONTROL);
    case VK_MENU:
        return input.isKeyPressed(VK_MENU) || input.isKeyPressed(VK_LMENU) || input.isKeyPressed(VK_RMENU);
    case VK_LWIN:
        return input.isKeyPressed(VK_LWIN) || input.isKeyPressed(VK_RWIN);
    default:
        return input.isKeyPressed(key);
    }
}

bool ShouldKeepRunning()
{
    return running.load(std::memory_order_acquire) && !done.load(std::memory_order_acquire);
}

} // namespace

MacroRuntime::~MacroRuntime()
{
    stop();
}

void MacroRuntime::start()
{
    bool expected = false;
    if (!running_.compare_exchange_strong(expected, true, std::memory_order_acq_rel)) {
        return;
    }

    LogInfo("Portable macro runtime starting.");
    controllerThread_ = std::thread(&MacroRuntime::controllerLoop, this);
}

void MacroRuntime::stop()
{
    bool expected = true;
    if (!running_.compare_exchange_strong(expected, false, std::memory_order_acq_rel)) {
        return;
    }

    if (controllerThread_.joinable()) {
        controllerThread_.join();
    }

    setTargetSuspended(false);

    std::lock_guard<std::mutex> lock(workerMutex_);
    for (std::thread& worker : workers_) {
        if (worker.joinable()) {
            worker.join();
        }
    }
    workers_.clear();
    LogInfo("Portable macro runtime stopped.");
}

void MacroRuntime::controllerLoop()
{
    nextProcessRefresh_ = std::chrono::steady_clock::now();

    while (running_.load(std::memory_order_acquire) && ShouldKeepRunning()) {
        refreshTargetProcesses();

        if (!macrotoggled || !notbinding) {
            if (freezeSuspended_) {
                setTargetSuspended(false);
            }
            std::this_thread::sleep_for(5ms);
            continue;
        }

        processFreezeMacro(foregroundAllows(disable_outside_roblox[0]));
        processPressKeyMacros(true);
        processWallhopMacros(true);
        processBunnyhopMacro(foregroundAllows(disable_outside_roblox[13]));

        std::this_thread::sleep_for(2ms);
    }

    if (freezeSuspended_) {
        setTargetSuspended(false);
    }
}

void MacroRuntime::refreshTargetProcesses()
{
    const auto now = std::chrono::steady_clock::now();
    if (now < nextProcessRefresh_) {
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

    targetPIDs.assign(pids.begin(), pids.end());
    processFound = !targetPIDs.empty();
}

bool MacroRuntime::foregroundAllows(bool disableOutsideRoblox)
{
    if (!disableOutsideRoblox) {
        return true;
    }

    const smu::platform::PlatformCapabilities capabilities = smu::platform::GetPlatformCapabilities();
    if (!capabilities.canDetectForegroundProcess) {
        return true;
    }

    auto backend = smu::platform::GetProcessBackend();
    if (!backend) {
        return false;
    }

    for (DWORD pid : targetPIDs) {
        if (backend->isForegroundProcess(pid)) {
            return true;
        }
    }
    return false;
}

bool MacroRuntime::isHotkeyPressed(unsigned int combinedKey) const
{
    auto input = smu::platform::GetInputBackend();
    if (!input) {
        return false;
    }

    const unsigned int key = combinedKey & HOTKEY_KEY_MASK;
    if (key == 0 || key == smu::core::SMU_VK_MOUSE_WHEEL_UP || key == smu::core::SMU_VK_MOUSE_WHEEL_DOWN) {
        return false;
    }

    if ((combinedKey & HOTKEY_MASK_WIN) && !IsModifierPressed(*input, VK_LWIN)) return false;
    if ((combinedKey & HOTKEY_MASK_CTRL) && !IsModifierPressed(*input, VK_CONTROL)) return false;
    if ((combinedKey & HOTKEY_MASK_ALT) && !IsModifierPressed(*input, VK_MENU)) return false;
    if ((combinedKey & HOTKEY_MASK_SHIFT) && !IsModifierPressed(*input, VK_SHIFT)) return false;
    return IsModifierPressed(*input, key);
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

void MacroRuntime::processPressKeyMacros(bool foregroundAllowed)
{
    if (pressKeyWasPressed_.size() != presskey_instances.size()) {
        pressKeyWasPressed_.assign(presskey_instances.size(), false);
    }

    for (std::size_t index = 0; index < presskey_instances.size(); ++index) {
        auto& inst = presskey_instances[index];
        const bool allowed = foregroundAllows(inst.presskeyinroblox) && foregroundAllowed;
        const bool pressed = inst.section_enabled && allowed && isHotkeyPressed(inst.vk_trigger);
        const bool edge = pressed && !pressKeyWasPressed_[index];
        pressKeyWasPressed_[index] = pressed;
        if (!edge) {
            continue;
        }

        const unsigned int trigger = inst.vk_trigger;
        const unsigned int output = inst.vk_presskey;
        const int bonusDelay = std::max(0, inst.PressKeyBonusDelay);
        const int pressDelay = std::max(1, inst.PressKeyDelay);
        runWorker(std::thread([this, trigger, output, bonusDelay, pressDelay] {
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
        }));
    }
}

void MacroRuntime::processWallhopMacros(bool foregroundAllowed)
{
    if (wallhopWasPressed_.size() != wallhop_instances.size()) {
        wallhopWasPressed_.assign(wallhop_instances.size(), false);
    }

    for (std::size_t index = 0; index < wallhop_instances.size(); ++index) {
        auto& inst = wallhop_instances[index];
        const bool allowed = foregroundAllows(inst.disable_outside_roblox) && foregroundAllowed;
        const bool pressed = inst.section_enabled && allowed && isHotkeyPressed(inst.vk_trigger);
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

        runWorker(std::thread([this, dx, dy, vertical, delay, bonusDelay, left, jump, flickBack, jumpKey] {
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
            (void)this;
        }));
    }
}

void MacroRuntime::processBunnyhopMacro(bool foregroundAllowed)
{
    const bool canProcess = foregroundAllowed && section_toggles[13] && macrotoggled && notbinding;
    if (isHotkeyPressed(vk_chatkey)) {
        bunnyhopChatLocked_ = true;
    }
    if (bunnyhopChatLocked_ && (isHotkeyPressed(VK_RETURN) || isHotkeyPressed(VK_LBUTTON))) {
        bunnyhopChatLocked_ = false;
    }

    const bool held = isHotkeyPressed(vk_bunnyhopkey);
    const bool shouldHop = canProcess && held && (!bunnyhopsmart || !bunnyhopChatLocked_);
    isbhoploop.store(shouldHop, std::memory_order_release);
    if (!shouldHop) {
        bunnyhopRunning_ = false;
        return;
    }

    if (!bunnyhopRunning_) {
        LogInfo("Smart Bunnyhop macro active.");
        bunnyhopRunning_ = true;
    }

    const int halfDelay = std::max(1, BunnyHopDelay / 2);
    HoldKeyBinded(vk_bunnyhopkey);
    std::this_thread::sleep_for(std::chrono::milliseconds(halfDelay));
    ReleaseKeyBinded(vk_bunnyhopkey);
    std::this_thread::sleep_for(std::chrono::milliseconds(halfDelay));
}

void MacroRuntime::setTargetSuspended(bool suspended)
{
    auto backend = smu::platform::GetProcessBackend();
    if (!backend) {
        return;
    }

    if (suspended) {
        refreshTargetProcesses();
        if (targetPIDs.empty()) {
            LogWarning("Freeze macro could not find target process from current settings.");
            return;
        }
        frozenPids_ = targetPIDs;
    }

    bool anySuccess = false;
    for (DWORD pid : frozenPids_) {
        anySuccess = (suspended ? backend->suspend(pid) : backend->resume(pid)) || anySuccess;
    }

    if (anySuccess) {
        freezeSuspended_ = suspended;
        if (suspended) {
            freezeStartTime_ = std::chrono::steady_clock::now();
            LogInfo("Freeze macro suspended " + std::to_string(frozenPids_.size()) + " target process(es).");
        } else {
            LogInfo("Freeze macro resumed target process(es).");
            frozenPids_.clear();
        }
    } else if (!frozenPids_.empty()) {
        LogWarning(suspended ? "Freeze macro failed to suspend target process(es)." : "Freeze macro failed to resume target process(es).");
    }
}

void MacroRuntime::runWorker(std::thread worker)
{
    std::lock_guard<std::mutex> lock(workerMutex_);
    workers_.push_back(std::move(worker));
}

} // namespace smu::app
