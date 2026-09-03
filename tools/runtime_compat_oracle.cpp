#include "macro_runtime_compat_logic.h"
#include "json.hpp"

#include <cstdlib>
#include <iostream>
#include <string_view>

namespace {

using Json = nlohmann::json;

bool ParseBool(const char* text)
{
    return std::string_view(text) == "1" || std::string_view(text) == "true";
}

struct OracleHhjSettings {
    const Json& spec;
    Json& events;
    int autoToggleReads = 0;
    int autoKey1Reads = 0;
    int autoKey2Reads = 0;

    bool readBool(const char* name)
    {
        const bool value = spec.value(name, false);
        events.push_back(Json{{"action", "setting_read"}, {"name", name}, {"value", value}});
        return value;
    }

    int readInt(const char* name)
    {
        const int value = spec.value(name, 0);
        events.push_back(Json{{"action", "setting_read"}, {"name", name}, {"value", value}});
        return value;
    }

    unsigned int readUnsigned(const char* name)
    {
        const auto value = static_cast<unsigned int>(spec.value(name, 0u));
        events.push_back(Json{{"action", "setting_read"}, {"name", name}, {"value", value}});
        return value;
    }

    bool autoToggle()
    {
        return readBool(autoToggleReads++ == 0 ? "auto_toggle_start" : "auto_toggle_release");
    }
    unsigned int autoKey1()
    {
        return readUnsigned(autoKey1Reads++ == 0 ? "auto_key1_action" : "auto_key1_release");
    }
    int autoKey1Time() { return readInt("auto_key1_time"); }
    unsigned int autoKey2()
    {
        return readUnsigned(autoKey2Reads++ == 0 ? "auto_key2_action" : "auto_key2_release");
    }
    int autoKey2Time() { return readInt("auto_key2_time"); }
    bool freezeDelayApply() { return readBool("freeze_delay_apply"); }
    bool fastHhj() { return readBool("fast_hhj"); }
    int freezeDelayOverrideCheck() { return readInt("freeze_delay_override_check"); }
    int freezeDelayOverrideValue() { return readInt("freeze_delay_override_value"); }
    int delay1() { return readInt("delay1"); }
    bool globalZoomAction() { return readBool("global_zoom_action"); }
    unsigned int shiftKeyAction() { return readUnsigned("shift_key_action"); }
    bool zoomReverse() { return readBool("zoom_reverse"); }
    int delay2() { return readInt("delay2"); }
    int lengthActivationCheck() { return readInt("length_activation_check"); }
    int delay3() { return readInt("delay3"); }
    bool globalZoomRelease() { return readBool("global_zoom_release"); }
    unsigned int shiftKeyRelease() { return readUnsigned("shift_key_release"); }
    int lengthFinishCheck() { return readInt("length_finish_check"); }
    int lengthSleepValue() { return readInt("length_sleep_value"); }
};

int Usage()
{
    std::cerr << "usage:\n"
              << "  smu_runtime_compat_oracle frame-delays <fps>\n"
              << "  smu_runtime_compat_oracle presskey-worker <trigger> <output> <bonus-ms> <press-ms> <running-after-bonus>\n"
              << "  smu_runtime_compat_oracle rising-edge <pressed> <was-pressed>\n"
              << "  smu_runtime_compat_oracle freeze-threshold <seconds>\n"
              << "  smu_runtime_compat_oracle freeze-step <section> <foreground> <pressed> <was-pressed> <configured-toggle> <freeze-key> <suspended> <elapsed-ms> <max-seconds> <override-ms> <running-after-override> <pressed-after-override> <suspend-success> <resume-success>\n"
              << "  smu_runtime_compat_oracle target-suspend <json-spec>\n"
              << "  smu_runtime_compat_oracle item-desync <foreground> <section> <hotkey> <slot>\n"
              << "  smu_runtime_compat_oracle wallhop-worker <dx> <dy> <vertical> <delay-ms> <bonus-ms> <left> <jump> <flick-back> <jump-key>\n"
              << "  smu_runtime_compat_oracle walless-worker <json-spec>\n"
              << "  smu_runtime_compat_oracle hhj-worker <json-spec>\n"
              << "  smu_runtime_compat_oracle itemclip-step <foreground> <section-press> <section-execute> <hotkey> <toggle-switch> <was-pressed> <is-loop> <clip-delay> <clip-slot>\n"
              << "  smu_runtime_compat_oracle wallwalk-step <foreground> <section-press> <section-execute> <hotkey> <toggle-switch> <was-pressed> <is-loop> <fps> <side-first> <strength-x> <side-second> <strength-y> <tail-delay-us>\n"
              << "  smu_runtime_compat_oracle speedglitch-step <foreground> <section> <hotkey> <toggle-switch> <was-pressed> <is-speed> <fps> <x> <y>\n";
    return 2;
}

Json Event(const char* action)
{
    return Json{{"action", action}};
}

} // namespace

int main(int argc, char** argv)
{
    using namespace smu::app::compat;

    if (argc < 2) {
        return Usage();
    }

    const std::string_view operation(argv[1]);
    Json result;

    if (operation == "frame-delays" && argc == 3) {
        const auto delays = FrameDelaysForFps(static_cast<unsigned int>(std::strtoul(argv[2], nullptr, 10)));
        result = Json{
            {"operation", "frame-delays"},
            {"first_ms", delays.firstMs},
            {"second_ms", delays.secondMs},
        };
    } else if (operation == "rising-edge" && argc == 4) {
        const auto edge = EvaluateRisingEdge(ParseBool(argv[2]), ParseBool(argv[3]));
        result = Json{
            {"operation", "rising-edge"},
            {"rising", edge.rising},
            {"next_was_pressed", edge.nextWasPressed},
        };
    } else if (operation == "freeze-threshold" && argc == 3) {
        result = Json{
            {"operation", "freeze-threshold"},
            {"threshold_ms", FreezeTimeoutThresholdMs(std::strtof(argv[2], nullptr))},
        };
    } else if (operation == "freeze-step" && argc == 16) {
        const bool sectionEnabled = ParseBool(argv[2]);
        const bool foregroundAllowed = ParseBool(argv[3]);
        const bool pressed = ParseBool(argv[4]);
        const bool wasPressed = ParseBool(argv[5]);
        const bool configuredToggle = ParseBool(argv[6]);
        const auto freezeKey = static_cast<unsigned int>(std::strtoul(argv[7], nullptr, 10));
        bool suspended = ParseBool(argv[8]);
        long long elapsedMs = std::strtoll(argv[9], nullptr, 10);
        const float maxSeconds = std::strtof(argv[10], nullptr);
        const int overrideMs = std::atoi(argv[11]);
        const bool runningAfterOverride = ParseBool(argv[12]);
        const bool pressedAfterOverride = ParseBool(argv[13]);
        const bool suspendSuccess = ParseBool(argv[14]);
        const bool resumeSuccess = ParseBool(argv[15]);
        Json events = Json::array();

        const auto step = ExecuteFreezeStep(
            sectionEnabled,
            foregroundAllowed,
            pressed,
            wasPressed,
            configuredToggle,
            freezeKey,
            maxSeconds,
            overrideMs,
            [&] { return suspended; },
            [&](bool shouldSuspend) {
                const bool success = shouldSuspend ? suspendSuccess : resumeSuccess;
                events.push_back(Json{
                    {"action", "set_suspended"},
                    {"suspended", shouldSuspend},
                    {"success", success},
                });
                if (success) {
                    suspended = shouldSuspend;
                    if (shouldSuspend) {
                        elapsedMs = 0;
                    }
                }
            },
            [&] { return elapsedMs; },
            [&](int delayMs) {
                events.push_back(Json{{"action", "wait"}, {"duration_ms", delayMs}, {"kind", "coarse"}});
                elapsedMs += delayMs;
            },
            [&] {
                events.push_back(Json{{"action", "running_check"}, {"running", runningAfterOverride}});
                return runningAfterOverride;
            },
            [&] {
                events.push_back(Json{{"action", "hotkey_recheck"}, {"pressed", pressedAfterOverride}});
                return pressedAfterOverride;
            });

        result = Json{
            {"operation", "freeze-step"},
            {"toggle_freeze", step.toggleFreeze},
            {"next_was_pressed", step.nextWasPressed},
            {"final_suspended", suspended},
            {"events", std::move(events)},
        };
    } else if (operation == "target-suspend" && argc == 3) {
        const Json spec = Json::parse(argv[2], nullptr, false);
        if (spec.is_discarded() || !spec.is_object()) {
            return Usage();
        }

        auto toPids = [](const Json& value) {
            std::vector<unsigned int> pids;
            if (!value.is_array()) {
                return pids;
            }
            pids.reserve(value.size());
            for (const auto& item : value) {
                pids.push_back(item.get<unsigned int>());
            }
            return pids;
        };

        std::vector<unsigned int> targetPids = toPids(spec.value("target_pids", Json::array()));
        std::vector<unsigned int> frozenPids = toPids(spec.value("frozen_pids", Json::array()));
        bool freezeSuspended = spec.value("freeze_suspended", false);
        const bool requestSuspended = spec.value("request_suspended", false);
        const Json refreshResults = spec.value("refresh_results", Json::array());
        const Json attemptResults = spec.value("attempt_results", Json::array());
        std::size_t refreshIndex = 0;
        std::size_t attemptIndex = 0;
        Json events = Json::array();

        const auto outcome = ExecuteTargetSuspendRequest(
            requestSuspended,
            targetPids,
            frozenPids,
            freezeSuspended,
            [&] {
                std::vector<unsigned int> refreshed;
                if (refreshIndex < refreshResults.size()) {
                    refreshed = toPids(refreshResults[refreshIndex]);
                }
                ++refreshIndex;
                targetPids = refreshed;
                events.push_back(Json{{"action", "refresh_targets"}, {"pids", targetPids}});
            },
            [&](unsigned int pid, bool shouldSuspend) {
                bool success = false;
                if (attemptIndex < attemptResults.size() && attemptResults[attemptIndex].is_boolean()) {
                    success = attemptResults[attemptIndex].get<bool>();
                }
                ++attemptIndex;
                events.push_back(Json{
                    {"action", "set_pid_suspended"},
                    {"pid", pid},
                    {"suspended", shouldSuspend},
                    {"success", success},
                });
                return success;
            });

        result = Json{
            {"operation", "target-suspend"},
            {"any_success", outcome.anySuccess},
            {"warning", outcome.warning},
            {"reset_freeze_start", outcome.resetFreezeStart},
            {"target_pids", targetPids},
            {"frozen_pids", frozenPids},
            {"freeze_suspended", freezeSuspended},
            {"events", std::move(events)},
        };
    } else if (operation == "item-desync" && argc == 6) {
        const bool foregroundAllowed = ParseBool(argv[2]);
        const bool sectionEnabled = ParseBool(argv[3]);
        const bool hotkeyPressed = ParseBool(argv[4]);
        const int slot = std::atoi(argv[5]);
        const bool pressed = foregroundAllowed && sectionEnabled && hotkeyPressed;
        Json events = Json::array();
        Json slotKey = nullptr;
        if (pressed) {
            const unsigned int key = InventorySlotKey(slot);
            slotKey = key;
            ExecuteItemDesyncPulse(
                key,
                [&](unsigned int heldKey) {
                    events.push_back(Json{{"action", "key_down"}, {"key", heldKey}, {"path", "direct"}});
                },
                [&](unsigned int releasedKey) {
                    events.push_back(Json{{"action", "key_up"}, {"key", releasedKey}, {"path", "direct"}});
                });
        }
        result = Json{
            {"operation", "item-desync"},
            {"pressed", pressed},
            {"loop_state", pressed},
            {"slot_key", std::move(slotKey)},
            {"events", std::move(events)},
        };
    } else if (operation == "wallhop-worker" && argc == 11) {
        const auto plan = BuildWallhopWorkerPlan(
            std::atoi(argv[2]),
            std::atoi(argv[3]),
            std::atoi(argv[4]),
            std::atoi(argv[5]),
            std::atoi(argv[6]),
            ParseBool(argv[7]),
            ParseBool(argv[8]),
            ParseBool(argv[9]),
            static_cast<unsigned int>(std::strtoul(argv[10], nullptr, 10)));
        Json events = Json::array();
        ExecuteWallhopWorker(
            plan,
            [&](int dx, int dy) {
                events.push_back(Json{{"action", "mouse_move"}, {"dx", dx}, {"dy", dy}});
            },
            [&](int delayMs) {
                events.push_back(Json{{"action", "wait"}, {"duration_ms", delayMs}, {"kind", "coarse"}});
            },
            [&](unsigned int key) {
                events.push_back(Json{{"action", "key_down"}, {"key", key}, {"path", "bound"}});
            },
            [&](unsigned int key) {
                events.push_back(Json{{"action", "key_up"}, {"key", key}, {"path", "bound"}});
            });
        result = Json{
            {"operation", "wallhop-worker"},
            {"plan", Json{
                {"dx", plan.dx},
                {"dy", plan.dy},
                {"vertical", plan.vertical},
                {"delay_ms", plan.delayMs},
                {"bonus_delay_ms", plan.bonusDelayMs},
                {"left", plan.left},
                {"jump", plan.jump},
                {"flick_back", plan.flickBack},
                {"jump_key", plan.jumpKey},
            }},
            {"events", std::move(events)},
        };
    } else if (operation == "walless-worker" && argc == 3) {
        const Json spec = Json::parse(argv[2], nullptr, false);
        if (spec.is_discarded() || !spec.is_object()) {
            return Usage();
        }
        std::vector<unsigned int> pids;
        if (const auto& pidValues = spec.value("pids", Json::array()); pidValues.is_array()) {
            pids.reserve(pidValues.size());
            for (const auto& value : pidValues) {
                pids.push_back(value.get<unsigned int>());
            }
        }
        const bool sideLeft = spec.value("side_left", false);
        const bool globalZoomAction = spec.value("global_zoom_action", false);
        const auto shiftKeyAction = static_cast<unsigned int>(spec.value("shift_key_action", 0u));
        const bool zoomReverse = spec.value("zoom_reverse", false);
        const bool globalZoomRelease = spec.value("global_zoom_release", false);
        const auto shiftKeyRelease = static_cast<unsigned int>(spec.value("shift_key_release", 0u));
        Json events = Json::array();

        ExecuteWallessLhjWorker(
            pids,
            [&] {
                events.push_back(Json{{"action", "setting_read"}, {"name", "side_left"}, {"value", sideLeft}});
                return sideLeft;
            },
            [&] {
                events.push_back(Json{{"action", "setting_read"}, {"name", "global_zoom_action"}, {"value", globalZoomAction}});
                return globalZoomAction;
            },
            [&] {
                events.push_back(Json{{"action", "setting_read"}, {"name", "shift_key_action"}, {"value", shiftKeyAction}});
                return shiftKeyAction;
            },
            [&] {
                events.push_back(Json{{"action", "setting_read"}, {"name", "zoom_reverse"}, {"value", zoomReverse}});
                return zoomReverse;
            },
            [&] {
                events.push_back(Json{{"action", "setting_read"}, {"name", "global_zoom_release"}, {"value", globalZoomRelease}});
                return globalZoomRelease;
            },
            [&] {
                events.push_back(Json{{"action", "setting_read"}, {"name", "shift_key_release"}, {"value", shiftKeyRelease}});
                return shiftKeyRelease;
            },
            [&](unsigned int key) {
                events.push_back(Json{{"action", "key_down"}, {"key", key}, {"path", "direct"}});
            },
            [&](unsigned int key) {
                events.push_back(Json{{"action", "key_up"}, {"key", key}, {"path", "direct"}});
            },
            [&](unsigned int key) {
                events.push_back(Json{{"action", "key_down"}, {"key", key}, {"path", "bound"}});
            },
            [&](unsigned int key) {
                events.push_back(Json{{"action", "key_up"}, {"key", key}, {"path", "bound"}});
            },
            [&](const std::vector<unsigned int>& workerPids, bool suspended) {
                events.push_back(Json{{"action", "process_suspended"}, {"pids", workerPids}, {"suspended", suspended}});
            },
            [&](int delayMs) {
                events.push_back(Json{{"action", "wait"}, {"duration_ms", delayMs}, {"kind", "coarse"}});
            });

        result = Json{
            {"operation", "walless-worker"},
            {"events", std::move(events)},
        };
    } else if (operation == "hhj-worker" && argc == 3) {
        const Json spec = Json::parse(argv[2], nullptr, false);
        if (spec.is_discarded() || !spec.is_object()) {
            return Usage();
        }
        std::vector<unsigned int> pids;
        if (const auto& pidValues = spec.value("pids", Json::array()); pidValues.is_array()) {
            pids.reserve(pidValues.size());
            for (const auto& value : pidValues) {
                pids.push_back(value.get<unsigned int>());
            }
        }
        Json events = Json::array();
        OracleHhjSettings settings{spec, events};
        ExecuteHhjWorker(
            pids,
            settings,
            [&](unsigned int key) {
                events.push_back(Json{{"action", "key_down"}, {"key", key}, {"path", "bound"}});
            },
            [&](unsigned int key) {
                events.push_back(Json{{"action", "key_up"}, {"key", key}, {"path", "bound"}});
            },
            [&](const std::vector<unsigned int>& workerPids, bool suspended) {
                events.push_back(Json{{"action", "process_suspended"}, {"pids", workerPids}, {"suspended", suspended}});
            },
            [&](int delayMs) {
                events.push_back(Json{{"action", "wait"}, {"duration_ms", delayMs}, {"kind", "coarse"}});
            },
            [&](bool active) {
                events.push_back(Json{{"action", "hhj_active"}, {"active", active}});
            });
        result = Json{
            {"operation", "hhj-worker"},
            {"events", std::move(events)},
        };
    } else if (operation == "itemclip-step" && argc == 11) {
        const bool foreground = ParseBool(argv[2]);
        const bool sectionPress = ParseBool(argv[3]);
        const bool sectionExecute = ParseBool(argv[4]);
        const bool hotkey = ParseBool(argv[5]);
        const bool toggleSwitch = ParseBool(argv[6]);
        const bool wasPressed = ParseBool(argv[7]);
        const bool isLoop = ParseBool(argv[8]);
        const int clipDelay = std::atoi(argv[9]);
        const int clipSlot = std::atoi(argv[10]);

        const auto edge = EvaluateToggleLoopEdge(
            foreground, sectionPress, hotkey, toggleSwitch, wasPressed);
        const bool nextLoop = ApplyToggleLoopStateAction(edge.stateAction, isLoop);
        const bool executeCycle = foreground && sectionExecute && nextLoop;
        Json events = Json::array();
        Json planJson = nullptr;
        if (executeCycle) {
            const int halfDelayMs = ItemClipHalfDelayMs(clipDelay);
            const unsigned int slotKey = InventorySlotKey(clipSlot);
            planJson = Json{
                {"half_delay_ms", halfDelayMs},
                {"slot_key", slotKey},
            };
            ExecuteItemClipCycle(
                slotKey,
                halfDelayMs,
                [&](unsigned int key) {
                    events.push_back(Json{{"action", "key_down"}, {"key", key}, {"path", "direct"}});
                },
                [&](int delayMs) {
                    events.push_back(Json{{"action", "wait"}, {"duration_ms", delayMs}, {"kind", "coarse"}});
                },
                [&](unsigned int key) {
                    events.push_back(Json{{"action", "key_up"}, {"key", key}, {"path", "direct"}});
                });
        }

        const char* actionName = "none";
        if (edge.stateAction == ToggleLoopStateAction::Toggle) {
            actionName = "toggle";
        } else if (edge.stateAction == ToggleLoopStateAction::Clear) {
            actionName = "clear";
        }
        result = Json{
            {"operation", "itemclip-step"},
            {"pressed", edge.pressed},
            {"next_was_pressed", edge.nextWasPressed},
            {"state_action", actionName},
            {"next_is_loop", nextLoop},
            {"execute_cycle", executeCycle},
            {"plan", std::move(planJson)},
            {"events", std::move(events)},
        };
    } else if (operation == "wallwalk-step" && argc == 15) {
        const bool foreground = ParseBool(argv[2]);
        const bool sectionPress = ParseBool(argv[3]);
        const bool sectionExecute = ParseBool(argv[4]);
        const bool hotkey = ParseBool(argv[5]);
        const bool toggleSwitch = ParseBool(argv[6]);
        const bool wasPressed = ParseBool(argv[7]);
        const bool isLoop = ParseBool(argv[8]);
        const auto fps = static_cast<unsigned int>(std::strtoul(argv[9], nullptr, 10));
        const bool sideFirst = ParseBool(argv[10]);
        const int strengthX = std::atoi(argv[11]);
        const bool sideSecond = ParseBool(argv[12]);
        const int strengthY = std::atoi(argv[13]);
        const int tailDelayUs = std::atoi(argv[14]);

        const auto edge = EvaluateToggleLoopEdge(
            foreground, sectionPress, hotkey, toggleSwitch, wasPressed);
        const bool nextLoop = ApplyToggleLoopStateAction(edge.stateAction, isLoop);
        const bool executeCycle = foreground && sectionExecute && nextLoop;
        Json events = Json::array();
        Json planJson = nullptr;
        if (executeCycle) {
            const int frameDelayMs = WallWalkFrameDelayMs(fps);
            const int firstMove = sideFirst ? -strengthX : strengthX;
            const int secondMove = sideSecond ? -strengthY : strengthY;
            planJson = Json{
                {"frame_delay_ms", frameDelayMs},
                {"first_move", firstMove},
                {"second_move", secondMove},
            };
            ExecuteWallWalkCycle(
                firstMove,
                secondMove,
                frameDelayMs,
                [&](int dx, int dy) {
                    events.push_back(Json{{"action", "mouse_move"}, {"dx", dx}, {"dy", dy}});
                },
                [&](int delayMs) {
                    events.push_back(Json{{"action", "wait"}, {"duration_ms", delayMs}, {"kind", "coarse"}});
                },
                [&] {
                    events.push_back(Json{{"action", "wallwalk_tail_delay_read"}, {"delay_us", tailDelayUs}});
                    return tailDelayUs;
                },
                [&](int delayUs) {
                    if (delayUs % 1000 == 0) {
                        events.push_back(Json{{"action", "wait"}, {"duration_ms", delayUs / 1000}, {"kind", "coarse"}});
                    } else {
                        events.push_back(Json{{"action", "wait"}, {"duration_us", delayUs}, {"kind", "coarse"}});
                    }
                });
        }

        const char* actionName = "none";
        if (edge.stateAction == ToggleLoopStateAction::Toggle) {
            actionName = "toggle";
        } else if (edge.stateAction == ToggleLoopStateAction::Clear) {
            actionName = "clear";
        }
        result = Json{
            {"operation", "wallwalk-step"},
            {"pressed", edge.pressed},
            {"next_was_pressed", edge.nextWasPressed},
            {"state_action", actionName},
            {"next_is_loop", nextLoop},
            {"execute_cycle", executeCycle},
            {"plan", std::move(planJson)},
            {"events", std::move(events)},
        };
    } else if (operation == "presskey-worker" && argc == 7) {
        const auto plan = BuildPressKeyWorkerPlan(
            static_cast<unsigned int>(std::strtoul(argv[2], nullptr, 10)),
            static_cast<unsigned int>(std::strtoul(argv[3], nullptr, 10)),
            std::atoi(argv[4]),
            std::atoi(argv[5]));
        const bool runningAfterBonus = ParseBool(argv[6]);
        Json events = Json::array();
        ExecutePressKeyWorker(
            plan,
            [&](unsigned int key) {
                events.push_back(Json{{"action", "key_up"}, {"key", key}, {"path", "bound"}});
            },
            [&](int delayMs) {
                events.push_back(Json{{"action", "wait"}, {"duration_ms", delayMs}, {"kind", "coarse"}});
            },
            [&] {
                events.push_back(Json{{"action", "running_check"}, {"running", runningAfterBonus}});
                return runningAfterBonus;
            },
            [&](unsigned int key) {
                events.push_back(Json{{"action", "key_down"}, {"key", key}, {"path", "bound"}});
            });
        result = Json{
            {"operation", "presskey-worker"},
            {"plan", Json{
                {"trigger", plan.trigger},
                {"output", plan.output},
                {"bonus_delay_ms", plan.bonusDelayMs},
                {"press_delay_ms", plan.pressDelayMs},
                {"release_trigger_first", plan.releaseTriggerFirst},
            }},
            {"events", std::move(events)},
        };
    } else if (operation == "speedglitch-step" && argc == 11) {
        const bool foreground = ParseBool(argv[2]);
        const bool section = ParseBool(argv[3]);
        const bool hotkey = ParseBool(argv[4]);
        const bool toggleSwitch = ParseBool(argv[5]);
        const bool wasPressed = ParseBool(argv[6]);
        const bool isSpeed = ParseBool(argv[7]);
        const auto fps = static_cast<unsigned int>(std::strtoul(argv[8], nullptr, 10));
        const int strengthX = std::atoi(argv[9]);
        const int strengthY = std::atoi(argv[10]);

        const auto edge = EvaluateSpeedglitchEdge(
            foreground,
            section,
            hotkey,
            toggleSwitch,
            wasPressed);
        const bool nextIsSpeed = ApplySpeedglitchStateAction(edge.stateAction, isSpeed);
        const bool executeCycle = foreground && section && nextIsSpeed;
        Json events = Json::array();
        Json planJson = nullptr;
        if (executeCycle) {
            const auto plan = BuildSpeedglitchCyclePlan(fps, strengthX, strengthY);
            planJson = Json{
                {"strength_x", plan.strengthX},
                {"strength_y", plan.strengthY},
                {"first_ms", plan.delays.firstMs},
                {"second_ms", plan.delays.secondMs},
            };
            ExecuteSpeedglitchCycle(
                plan.delays,
                [&] {
                    events.push_back(Json{{"action", "mouse_move"}, {"dx", plan.strengthX}, {"dy", 0}});
                },
                [&](int delayMs) {
                    events.push_back(Json{{"action", "wait"}, {"duration_ms", delayMs}, {"kind", "coarse"}});
                },
                [&] {
                    events.push_back(Json{{"action", "mouse_move"}, {"dx", plan.strengthY}, {"dy", 0}});
                });
        }

        const char* actionName = "none";
        if (edge.stateAction == SpeedglitchStateAction::Toggle) {
            actionName = "toggle";
        } else if (edge.stateAction == SpeedglitchStateAction::Clear) {
            actionName = "clear";
        }
        result = Json{
            {"operation", "speedglitch-step"},
            {"pressed", edge.pressed},
            {"next_was_pressed", edge.nextWasPressed},
            {"state_action", actionName},
            {"next_is_speed", nextIsSpeed},
            {"execute_cycle", executeCycle},
            {"plan", std::move(planJson)},
            {"events", std::move(events)},
        };
    } else {
        return Usage();
    }

    std::cout << result.dump() << '\n';
    return 0;
}
