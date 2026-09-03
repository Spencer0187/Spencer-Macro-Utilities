#pragma once

#include "key_codes.h"

#include <algorithm>
#include <utility>
#include <vector>

namespace smu::app::compat {

struct RisingEdgeResult {
    bool rising = false;
    bool nextWasPressed = false;
};

inline RisingEdgeResult EvaluateRisingEdge(bool pressed, bool wasPressed)
{
    return RisingEdgeResult{
        .rising = pressed && !wasPressed,
        .nextWasPressed = pressed,
    };
}

inline bool ShouldToggleFreeze(bool configuredToggle, unsigned int freezeKey)
{
    return configuredToggle ||
        freezeKey == smu::core::SMU_VK_MOUSE_WHEEL_UP ||
        freezeKey == smu::core::SMU_VK_MOUSE_WHEEL_DOWN;
}

inline long long FreezeTimeoutThresholdMs(float maxFreezeTimeSeconds)
{
    return static_cast<long long>(maxFreezeTimeSeconds * 1000.0f);
}

struct TargetSuspendResult {
    bool anySuccess = false;
    bool warning = false;
    bool resetFreezeStart = false;
};

template <typename RefreshFn, typename SetPidSuspendedFn>
TargetSuspendResult ExecuteTargetSuspendRequest(
    bool suspended,
    std::vector<unsigned int>& targetPids,
    std::vector<unsigned int>& frozenPids,
    bool& freezeSuspended,
    RefreshFn&& refresh,
    SetPidSuspendedFn&& setPidSuspended)
{
    if (suspended) {
        if (targetPids.empty()) {
            refresh();
        }
        if (targetPids.empty()) {
            return {};
        }
        frozenPids = targetPids;
    }

    bool anySuccess = false;
    for (unsigned int pid : frozenPids) {
        anySuccess = setPidSuspended(pid, suspended) || anySuccess;
    }

    if (!anySuccess && suspended) {
        const auto previousPids = frozenPids;
        refresh();
        if (!targetPids.empty() && targetPids != previousPids) {
            frozenPids = targetPids;
            for (unsigned int pid : frozenPids) {
                anySuccess = setPidSuspended(pid, true) || anySuccess;
            }
        }
    }

    TargetSuspendResult result;
    result.anySuccess = anySuccess;
    if (anySuccess) {
        freezeSuspended = suspended;
        result.resetFreezeStart = suspended;
        if (!suspended) {
            frozenPids.clear();
        }
    } else if (!frozenPids.empty()) {
        result.warning = true;
    }
    return result;
}

struct FreezeStepResult {
    bool nextWasPressed = false;
    bool toggleFreeze = false;
};

template <typename IsSuspendedFn, typename SetSuspendedFn, typename ElapsedMsFn,
          typename SleepFn, typename RunningFn, typename PressedAgainFn>
FreezeStepResult ExecuteFreezeStep(
    bool sectionEnabled,
    bool foregroundAllowed,
    bool pressed,
    bool wasPressed,
    bool configuredToggle,
    unsigned int freezeKey,
    float maxFreezeTimeSeconds,
    int maxFreezeOverrideMs,
    IsSuspendedFn&& isSuspended,
    SetSuspendedFn&& setSuspended,
    ElapsedMsFn&& elapsedMs,
    SleepFn&& sleep,
    RunningFn&& running,
    PressedAgainFn&& pressedAgain)
{
    const bool toggleFreeze = ShouldToggleFreeze(configuredToggle, freezeKey);
    if (!sectionEnabled) {
        if (isSuspended()) {
            setSuspended(false);
        }
        return FreezeStepResult{
            .nextWasPressed = false,
            .toggleFreeze = toggleFreeze,
        };
    }

    if (toggleFreeze) {
        if (pressed && !wasPressed && foregroundAllowed) {
            setSuspended(!isSuspended());
        }
    } else if (pressed && foregroundAllowed) {
        if (!isSuspended()) {
            setSuspended(true);
        }
    } else if (isSuspended()) {
        setSuspended(false);
    }

    if (isSuspended() &&
        elapsedMs() >= FreezeTimeoutThresholdMs(maxFreezeTimeSeconds)) {
        setSuspended(false);
        sleep(std::max(0, maxFreezeOverrideMs));
        if (running() && (toggleFreeze || pressedAgain())) {
            setSuspended(true);
        }
    }

    return FreezeStepResult{
        .nextWasPressed = pressed,
        .toggleFreeze = toggleFreeze,
    };
}

inline unsigned int InventorySlotKey(int slot)
{
    return static_cast<unsigned int>(std::clamp(slot, 0, 9) + smu::core::SMU_VK_0);
}

template <typename HoldFn, typename ReleaseFn>
void ExecuteItemDesyncPulse(unsigned int slotKey, HoldFn&& hold, ReleaseFn&& release)
{
    hold(slotKey);
    release(slotKey);
    hold(slotKey);
    release(slotKey);
}

enum class ToggleLoopStateAction {
    None,
    Toggle,
    Clear,
};

struct ToggleLoopEdgeResult {
    bool pressed = false;
    bool nextWasPressed = false;
    ToggleLoopStateAction stateAction = ToggleLoopStateAction::None;
};

inline ToggleLoopEdgeResult EvaluateToggleLoopEdge(
    bool foregroundAllowed,
    bool sectionEnabled,
    bool hotkeyPressed,
    bool toggleSwitch,
    bool wasPressed)
{
    const bool pressed = foregroundAllowed && sectionEnabled && hotkeyPressed;
    ToggleLoopStateAction action = ToggleLoopStateAction::None;
    if (pressed && !wasPressed) {
        action = ToggleLoopStateAction::Toggle;
    } else if (!pressed && toggleSwitch) {
        action = ToggleLoopStateAction::Clear;
    }

    return ToggleLoopEdgeResult{
        .pressed = pressed,
        .nextWasPressed = pressed,
        .stateAction = action,
    };
}

inline bool ApplyToggleLoopStateAction(ToggleLoopStateAction action, bool active)
{
    switch (action) {
    case ToggleLoopStateAction::Toggle:
        return !active;
    case ToggleLoopStateAction::Clear:
        return false;
    case ToggleLoopStateAction::None:
    default:
        return active;
    }
}

inline int ItemClipHalfDelayMs(int clipDelay)
{
    return std::max(1, clipDelay / 2);
}

template <typename HoldFn, typename SleepFn, typename ReleaseFn>
void ExecuteItemClipCycle(
    unsigned int slotKey,
    int halfDelayMs,
    HoldFn&& hold,
    SleepFn&& sleep,
    ReleaseFn&& release)
{
    hold(slotKey);
    sleep(halfDelayMs);
    release(slotKey);
    sleep(halfDelayMs);
}

inline int WallWalkFrameDelayMs(unsigned int fps)
{
    const unsigned int safeFps = std::max(1u, fps);
    return std::max(1, static_cast<int>(((1000.0f / static_cast<float>(safeFps)) + 0.5f) * 1.1f));
}

template <typename MoveFn, typename SleepMsFn, typename TailDelayUsFn, typename SleepUsFn>
void ExecuteWallWalkCycle(
    int firstMove,
    int secondMove,
    int frameDelayMs,
    MoveFn&& move,
    SleepMsFn&& sleepMs,
    TailDelayUsFn&& tailDelayUs,
    SleepUsFn&& sleepUs)
{
    move(firstMove, 0);
    sleepMs(frameDelayMs);
    move(secondMove, 0);
    sleepUs(std::max(0, tailDelayUs()));
}

struct WallhopWorkerPlan {
    int dx = 0;
    int dy = 0;
    int vertical = 0;
    int delayMs = 1;
    int bonusDelayMs = 0;
    bool left = false;
    bool jump = false;
    bool flickBack = false;
    unsigned int jumpKey = 0;
};

inline WallhopWorkerPlan BuildWallhopWorkerPlan(
    int dx,
    int dy,
    int vertical,
    int delay,
    int bonusDelay,
    bool left,
    bool jump,
    bool flickBack,
    unsigned int jumpKey)
{
    return WallhopWorkerPlan{
        .dx = dx,
        .dy = dy,
        .vertical = vertical,
        .delayMs = std::max(1, delay),
        .bonusDelayMs = std::max(0, bonusDelay),
        .left = left,
        .jump = jump,
        .flickBack = flickBack,
        .jumpKey = jumpKey,
    };
}

template <typename MoveFn, typename SleepFn, typename HoldFn, typename ReleaseFn>
void ExecuteWallhopWorker(
    const WallhopWorkerPlan& plan,
    MoveFn&& move,
    SleepFn&& sleep,
    HoldFn&& hold,
    ReleaseFn&& release)
{
    move(plan.left ? -plan.dx : plan.dx, plan.vertical);
    if (plan.flickBack) {
        if (plan.bonusDelayMs > 0 && plan.bonusDelayMs < plan.delayMs) {
            sleep(plan.bonusDelayMs);
            if (plan.jump) {
                hold(plan.jumpKey);
            }
            sleep(plan.delayMs - plan.bonusDelayMs);
        } else {
            if (plan.jump) {
                hold(plan.jumpKey);
            }
            sleep(plan.delayMs);
        }
        move(plan.left ? -plan.dy : plan.dy, -plan.vertical);
    } else if (plan.jump) {
        hold(plan.jumpKey);
    }

    if (plan.jump) {
        if (100 - plan.delayMs > 0) {
            sleep(100 - plan.delayMs);
        }
        release(plan.jumpKey);
    }
}

template <typename SideLeftFn, typename GlobalZoomActionFn, typename ShiftKeyActionFn,
          typename ZoomReverseFn, typename GlobalZoomReleaseFn, typename ShiftKeyReleaseFn,
          typename HoldDirectFn, typename ReleaseDirectFn, typename HoldBoundFn,
          typename ReleaseBoundFn, typename SetPidsSuspendedFn, typename SleepFn>
void ExecuteWallessLhjWorker(
    const std::vector<unsigned int>& pids,
    SideLeftFn&& sideLeft,
    GlobalZoomActionFn&& globalZoomAction,
    ShiftKeyActionFn&& shiftKeyAction,
    ZoomReverseFn&& zoomReverse,
    GlobalZoomReleaseFn&& globalZoomRelease,
    ShiftKeyReleaseFn&& shiftKeyRelease,
    HoldDirectFn&& holdDirect,
    ReleaseDirectFn&& releaseDirect,
    HoldBoundFn&& holdBound,
    ReleaseBoundFn&& releaseBound,
    SetPidsSuspendedFn&& setPidsSuspended,
    SleepFn&& sleep)
{
    const unsigned int sideKey = sideLeft() ? smu::core::SMU_VK_A : smu::core::SMU_VK_D;
    holdDirect(sideKey);
    sleep(15);
    holdDirect(smu::core::SMU_VK_SPACE);
    sleep(30);

    setPidsSuspended(pids, true);
    sleep(50);
    releaseDirect(sideKey);
    sleep(500);

    if (!globalZoomAction()) {
        holdBound(shiftKeyAction());
    } else {
        holdBound(zoomReverse() ? smu::core::SMU_VK_MOUSE_WHEEL_DOWN : smu::core::SMU_VK_MOUSE_WHEEL_UP);
    }
    setPidsSuspended(pids, false);
    sleep(50);
    if (!globalZoomRelease()) {
        releaseBound(shiftKeyRelease());
    }
    releaseDirect(smu::core::SMU_VK_SPACE);
}

struct PressKeyWorkerPlan {
    unsigned int trigger = 0;
    unsigned int output = 0;
    int bonusDelayMs = 0;
    int pressDelayMs = 1;
    bool releaseTriggerFirst = false;
};

inline PressKeyWorkerPlan BuildPressKeyWorkerPlan(
    unsigned int trigger,
    unsigned int output,
    int bonusDelay,
    int pressDelay)
{
    return PressKeyWorkerPlan{
        .trigger = trigger,
        .output = output,
        .bonusDelayMs = std::max(0, bonusDelay),
        .pressDelayMs = std::max(1, pressDelay),
        .releaseTriggerFirst = trigger == output,
    };
}

template <typename ReleaseFn, typename SleepFn, typename RunningFn, typename HoldFn>
void ExecutePressKeyWorker(
    const PressKeyWorkerPlan& plan,
    ReleaseFn&& release,
    SleepFn&& sleep,
    RunningFn&& running,
    HoldFn&& hold)
{
    if (plan.releaseTriggerFirst) {
        release(plan.trigger);
    }
    if (plan.bonusDelayMs > 0) {
        sleep(plan.bonusDelayMs);
    }
    if (!running()) {
        return;
    }
    hold(plan.output);
    sleep(plan.pressDelayMs);
    release(plan.output);
}

struct FrameDelayPair {
    int firstMs = 1;
    int secondMs = 1;
};

inline FrameDelayPair FrameDelaysForFps(unsigned int fps)
{
    const unsigned int safeFps = std::max(1u, fps);
    const float delayFloat = 1000.0f / static_cast<float>(safeFps);
    const int delayFloor = std::max(1, static_cast<int>(delayFloat));
    const int delayCeil = delayFloor + 1;
    const float fractional = delayFloat - static_cast<float>(delayFloor);
    constexpr float epsilon = 0.008f;

    if (fractional < 0.33f - epsilon) {
        return FrameDelayPair{delayFloor, delayFloor};
    }
    if (fractional > 0.66f + epsilon) {
        return FrameDelayPair{delayCeil, delayCeil};
    }
    return FrameDelayPair{delayFloor, delayCeil};
}

enum class SpeedglitchStateAction {
    None,
    Toggle,
    Clear,
};

struct SpeedglitchEdgeResult {
    bool pressed = false;
    bool nextWasPressed = false;
    SpeedglitchStateAction stateAction = SpeedglitchStateAction::None;
};

inline SpeedglitchEdgeResult EvaluateSpeedglitchEdge(
    bool foregroundAllowed,
    bool sectionEnabled,
    bool hotkeyPressed,
    bool toggleSwitch,
    bool wasPressed)
{
    const bool pressed = foregroundAllowed && sectionEnabled && hotkeyPressed;
    SpeedglitchStateAction action = SpeedglitchStateAction::None;
    if (pressed && !wasPressed) {
        action = SpeedglitchStateAction::Toggle;
    } else if (!pressed && toggleSwitch) {
        action = SpeedglitchStateAction::Clear;
    }

    return SpeedglitchEdgeResult{
        .pressed = pressed,
        .nextWasPressed = pressed,
        .stateAction = action,
    };
}

inline bool ApplySpeedglitchStateAction(SpeedglitchStateAction action, bool isSpeed)
{
    switch (action) {
    case SpeedglitchStateAction::Toggle:
        return !isSpeed;
    case SpeedglitchStateAction::Clear:
        return false;
    case SpeedglitchStateAction::None:
    default:
        return isSpeed;
    }
}

struct SpeedglitchCyclePlan {
    int strengthX = 0;
    int strengthY = 0;
    FrameDelayPair delays{};
};

inline SpeedglitchCyclePlan BuildSpeedglitchCyclePlan(unsigned int fps, int strengthX, int strengthY)
{
    return SpeedglitchCyclePlan{
        .strengthX = strengthX,
        .strengthY = strengthY,
        .delays = FrameDelaysForFps(fps),
    };
}

template <typename FirstMoveFn, typename SleepFn, typename SecondMoveFn>
void ExecuteSpeedglitchCycle(
    const FrameDelayPair& delays,
    FirstMoveFn&& firstMove,
    SleepFn&& sleep,
    SecondMoveFn&& secondMove)
{
    firstMove();
    sleep(delays.firstMs);
    secondMove();
    sleep(delays.secondMs);
}

} // namespace smu::app::compat
