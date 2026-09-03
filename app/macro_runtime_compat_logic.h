#pragma once

#include <algorithm>
#include <utility>

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
