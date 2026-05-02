#include "macro_runtime.h"

#include "input_actions.h"
#include "../core/key_codes.h"
#include "../platform/input_backend.h"
#include "../platform/logging.h"
#include "../platform/network_backend.h"
#include "../platform/platform_capabilities.h"
#include "../platform/process_backend.h"
#include "../platform/text_input_backend.h"

#ifndef SMU_PORTABLE_GLOBALS
#define SMU_PORTABLE_GLOBALS
#endif
#include "Resource Files/globals.h"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstdlib>
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

    while (running_.load(std::memory_order_acquire) && ShouldKeepRunning()) {
        refreshTargetProcesses();

        if (!macrotoggled || !notbinding) {
            if (freezeSuspended_) {
                setTargetSuspended(false);
            }
            std::this_thread::sleep_for(5ms);
            continue;
        }

        pruneFinishedWorkers();

        processFreezeMacro(foregroundAllows(disable_outside_roblox[0]));
        processItemDesyncMacro(foregroundAllows(disable_outside_roblox[1]));
        processPressKeyMacros(true);
        processWallhopMacros(true);
        processWallessLhjMacro(foregroundAllows(disable_outside_roblox[7]));
        processSpeedglitchMacro(foregroundAllows(disable_outside_roblox[3]));
        processHhjMacro(foregroundAllows(disable_outside_roblox[2]));
        processHhjMotionLoop();
        processItemUnequipComOffsetMacro(foregroundAllows(disable_outside_roblox[4]));
        processItemClipMacro(foregroundAllows(disable_outside_roblox[8]));
        processLaughClipMacro(foregroundAllows(disable_outside_roblox[9]));
        processWallWalkMacro(foregroundAllows(disable_outside_roblox[10]));
        processSpamKeyMacros();
        processLedgeBounceMacro(foregroundAllows(disable_outside_roblox[12]));
        processBunnyhopMacro(foregroundAllows(disable_outside_roblox[13]));
        processFloorBounceMacro(foregroundAllows(disable_outside_roblox[14]));
        processLagSwitchMacro(foregroundAllows(disable_outside_roblox[15]));

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

    for (unsigned int pid : targetPIDs) {
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

    const std::string customText = CustomTextChar;
    const std::string emoteText = text;
    runWorker([customText, emoteText] {
        HoldKeyBinded(vk_chatkey);
        std::this_thread::sleep_for(17ms);
        ReleaseKeyBinded(vk_chatkey);
        std::this_thread::sleep_for(17ms);

        const bool custom = !customText.empty();
        smu::platform::pasteText(custom ? customText : emoteText, std::max(0, PasteDelay));
        std::this_thread::sleep_for(25ms);
        HoldKeyBinded(vk_enterkey);
        std::this_thread::sleep_for(35ms);
        ReleaseKeyBinded(vk_enterkey);

        if (custom) {
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

    runWorker([] {
        HoldKeyBinded(vk_chatkey);
        std::this_thread::sleep_for(17ms);
        ReleaseKeyBinded(vk_chatkey);
        std::this_thread::sleep_for(50ms);
        smu::platform::pasteText("/e laugh", std::max(0, PasteDelay));
        std::this_thread::sleep_for(50ms);
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
    });
}

void MacroRuntime::processSpamKeyMacros()
{
    if (spamKeyWasPressed_.size() != spamkey_instances.size()) {
        spamKeyWasPressed_.assign(spamkey_instances.size(), false);
    }

    for (std::size_t index = 0; index < spamkey_instances.size(); ++index) {
        auto& inst = spamkey_instances[index];
        const bool allowed = foregroundAllows(inst.disable_outside_roblox);
        const bool pressed = allowed && inst.section_enabled && isHotkeyPressed(inst.vk_trigger);
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
                   notbinding &&
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
            ReleaseKeyBinded(vk_shiftkey);
        }
    });
}

void MacroRuntime::processLagSwitchMacro(bool foregroundAllowed)
{
    auto backend = smu::platform::GetNetworkLagBackend();
    if (!backend || !backend->isAvailable()) {
        return;
    }

    smu::platform::LagSwitchConfig config = backend->config();
    config.enabled = true;
    config.inboundHardBlock = lagswitchinbound;
    config.outboundHardBlock = lagswitchoutbound;
    config.fakeLagEnabled = lagswitchlag;
    config.inboundFakeLag = lagswitchlaginbound;
    config.outboundFakeLag = lagswitchlagoutbound;
    config.fakeLagDelayMs = lagswitchlagdelay;
    config.targetRobloxOnly = lagswitchtargetroblox;
    config.useTcp = lagswitchusetcp;
    config.useUdp = !lagswitchusetcp;
    config.preventDisconnect = prevent_disconnect;
    config.autoUnblock = lagswitch_autounblock;
    config.maxDurationSeconds = lagswitch_max_duration;
    config.unblockDurationMs = lagswitch_unblock_ms;
    backend->setConfig(config);

    if (!foregroundAllowed || !section_toggles[15]) {
        backend->setBlockingActive(false);
        lagSwitchWasPressed_ = false;
        return;
    }

    const bool pressed = isHotkeyPressed(vk_lagswitchkey);
    if (islagswitchswitch) {
        if (pressed && !lagSwitchWasPressed_) {
            const bool nextActive = !backend->isBlockingActive();
            backend->setBlockingActive(nextActive);
            if (nextActive) {
                lagSwitchStartTime_ = std::chrono::steady_clock::now();
            }
        }
    } else {
        if (pressed && !backend->isBlockingActive()) {
            lagSwitchStartTime_ = std::chrono::steady_clock::now();
        }
        backend->setBlockingActive(pressed);
    }
    lagSwitchWasPressed_ = pressed;

    if (lagswitch_autounblock && backend->isBlockingActive()) {
        const auto elapsed = std::chrono::duration<float>(std::chrono::steady_clock::now() - lagSwitchStartTime_).count();
        if (elapsed >= lagswitch_max_duration) {
            backend->setBlockingActive(false);
        }
    }
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
    refreshTargetProcesses();
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
