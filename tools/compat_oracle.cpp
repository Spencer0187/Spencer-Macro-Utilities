#include "../app/input_actions_compat_logic.h"
#include "../app/script_api_compat_logic.h"
#include "../app/script_metadata.h"
#include "../core/key_codes.h"
#include "../core/keymapping.h"
#include "../platform/roblox_log_reader.h"

#include "json.hpp"

#include <filesystem>
#include <iostream>
#include <optional>
#include <regex>
#include <set>
#include <string>
#include <string_view>

namespace {

using Json = nlohmann::json;

Json KeyJson(smu::core::KeyCode key)
{
    return Json{
        {"raw", key},
        {"name", std::string(smu::core::KeyCodeName(key))},
        {"label", smu::core::FormatKeyCodeFallback(key)},
        {"modifier", smu::core::IsModifierKey(key)},
    };
}

Json HotkeyJson(const std::string& input)
{
    const auto parsed = smu::app::ParseScriptHotkeyString(input);
    Json result{
        {"operation", "hotkey"},
        {"input", input},
        {"parsed", parsed.has_value()},
    };
    if (!parsed) {
        return result;
    }

    const unsigned int raw = *parsed;
    const auto key = static_cast<smu::core::KeyCode>(raw & smu::core::HOTKEY_KEY_MASK);
    result["raw"] = raw;
    result["key"] = KeyJson(key);
    result["shift"] = (raw & smu::core::HOTKEY_MASK_SHIFT) != 0;
    result["ctrl"] = (raw & smu::core::HOTKEY_MASK_CTRL) != 0;
    result["alt"] = (raw & smu::core::HOTKEY_MASK_ALT) != 0;
    result["win"] = (raw & smu::core::HOTKEY_MASK_WIN) != 0;
    return result;
}

Json MoveDegreesJson(const std::string& input)
{
    const Json payload = Json::parse(input);
    std::optional<bool> camfix;
    if (const auto it = payload.find("camfix"); it != payload.end() && it->is_boolean()) {
        camfix = it->get<bool>();
    }
    std::optional<std::string> sensitivity;
    if (const auto it = payload.find("sensitivity"); it != payload.end() && it->is_string()) {
        sensitivity = it->get<std::string>();
    }

    const auto calibration = smu::app::compat::ComputeMoveDegreesCalibration(camfix, sensitivity);
    Json result{
        {"operation", "move-degrees"},
        {"valid", calibration.valid},
        {"pixels_per_degree", calibration.pixelsPerDegree},
        {"error", calibration.error},
    };
    if (!calibration.valid) {
        return result;
    }

    const double dx = payload.value("dx", 0.0);
    const double dy = payload.value("dy", 0.0);
    const auto roundedX = smu::app::compat::RoundMouseDelta(dx * calibration.pixelsPerDegree, "dx");
    if (!roundedX.valid) {
        result["valid"] = false;
        result["error"] = roundedX.error;
        return result;
    }
    const auto roundedY = smu::app::compat::RoundMouseDelta(-dy * calibration.pixelsPerDegree, "dy");
    if (!roundedY.valid) {
        result["valid"] = false;
        result["error"] = roundedY.error;
        return result;
    }
    result["dx_pixels"] = roundedX.value;
    result["dy_pixels"] = roundedY.value;
    return result;
}

Json RobloxRegexJson(const std::string& input)
{
    const Json payload = Json::parse(input);
    const std::string pattern = payload.at("pattern").get<std::string>();
    const std::string text = payload.at("text").get<std::string>();
    Json result{
        {"operation", "roblox-regex"},
        {"pattern", pattern},
        {"text", text},
        {"valid", false},
        {"matches", false},
    };
    try {
        const std::regex expression(pattern, std::regex::ECMAScript | std::regex::optimize);
        result["valid"] = true;
        result["matches"] = std::regex_search(text, expression);
    } catch (const std::regex_error&) {
    }
    return result;
}

Json InputHotkeyJson(const std::string& input)
{
    const Json payload = Json::parse(input);
    const unsigned int hotkey = payload.at("hotkey").get<unsigned int>();
    const bool strict = payload.value("strict", false);
    std::set<unsigned int> pressed;
    for (const auto& value : payload.value("pressed", Json::array())) {
        pressed.insert(value.get<unsigned int>());
    }

    const bool result = smu::app::compat::IsHotkeyPressedWithPredicate(
        [&pressed](smu::core::KeyCode key) {
            return pressed.contains(static_cast<unsigned int>(key));
        },
        hotkey,
        strict);
    return Json{
        {"operation", "input-hotkey"},
        {"hotkey", hotkey},
        {"strict", strict},
        {"pressed", pressed},
        {"result", result},
    };
}

std::optional<unsigned int> LinuxVkToEvdev(unsigned int vk)
{
    switch (vk) {
    case 0xA0: return 42;
    case 0xA1: return 54;
    case 0xA2: return 29;
    case 0xA3: return 97;
    case 0xA4: return 56;
    case 0xA5: return 100;
    default: break;
    }
    if (vk > 0xFFu) {
        return std::nullopt;
    }
    const auto evdev = win_vkey_to_evdev_key(static_cast<std::uint8_t>(vk));
    if (evdev == EVDEV_UNASSIGNED) {
        return std::nullopt;
    }
    return evdev;
}

unsigned int LinuxEvdevToVk(unsigned int evdev)
{
    switch (evdev) {
    case 42:
    case 54: return 0x10;
    case 29:
    case 97: return 0x11;
    case 56:
    case 100: return 0x12;
    default:
        if (evdev > 0xFFFFu) {
            return VK_UNASSIGNED;
        }
        return evdev_to_win_vkey(static_cast<std::uint16_t>(evdev));
    }
}

Json LinuxKeymapJson(const std::string&)
{
    Json winToEvdev = Json::array();
    for (unsigned int vk = 0; vk <= 257; ++vk) {
        const auto evdev = LinuxVkToEvdev(vk);
        winToEvdev.push_back(Json{
            {"vk", vk},
            {"evdev", evdev ? Json(*evdev) : Json(nullptr)},
        });
    }

    Json evdevToWin = Json::array();
    for (unsigned int evdev = 0; evdev < 0x2FFu; ++evdev) {
        evdevToWin.push_back(Json{
            {"evdev", evdev},
            {"vk", LinuxEvdevToVk(evdev)},
        });
    }
    return Json{
        {"operation", "linux-keymap"},
        {"win_to_evdev", std::move(winToEvdev)},
        {"evdev_to_win", std::move(evdevToWin)},
    };
}

Json KeyNameJson(const std::string& input)
{
    const auto parsed = smu::app::ParseScriptKeyName(input);
    Json result{
        {"operation", "key-name"},
        {"input", input},
        {"parsed", parsed.has_value()},
    };
    if (parsed) {
        result["key"] = KeyJson(*parsed);
    }
    return result;
}

Json ScriptMetadataJson(const std::filesystem::path& path)
{
    const auto metadata = smu::app::ParseScriptMetadata(path);
    const auto hotkey = smu::app::ParseScriptHotkeyMetadata(path);
    Json result{
        {"operation", "script-metadata"},
        {"path", path.string()},
        {"supported_extension", smu::app::IsSupportedScriptExtension(path)},
        {"name", metadata.name},
        {"description", metadata.description},
        {"author", metadata.author},
        {"version", metadata.version},
        {"memory_limit_mb", metadata.memoryLimitMB ? Json(*metadata.memoryLimitMB) : Json(nullptr)},
        {"hotkey", hotkey ? Json(*hotkey) : Json(nullptr)},
    };
    return result;
}

Json RobloxSnapshotJson(const smu::platform::RobloxLogSnapshot& snapshot)
{
    return Json{
        {"available", snapshot.available},
        {"started_new_log", snapshot.startedNewLog},
        {"path", snapshot.path.string()},
        {"lines", snapshot.lines},
        {"state", snapshot.state},
        {"place_id", snapshot.placeId},
        {"user_id", snapshot.userId},
        {"universe_id", snapshot.universeId},
        {"job_id", snapshot.jobId},
        {"client_channel", snapshot.clientChannel},
        {"client_roblox_git_hash", snapshot.clientRobloxGitHash},
        {"server_roblox_git_hash", snapshot.serverRobloxGitHash},
        {"server_prefix", snapshot.serverPrefix},
        {"server_luau_version", snapshot.serverLuauVersion},
        {"resolution_width", snapshot.resolutionWidth},
        {"resolution_height", snapshot.resolutionHeight},
        {"server",
         {
             {"type", snapshot.server.type},
             {"rcc_address", snapshot.server.rccAddress},
             {"rcc_port", snapshot.server.rccPort},
             {"udmux_address", snapshot.server.udmuxAddress},
             {"udmux_port", snapshot.server.udmuxPort},
         }},
    };
}

Json RobloxFileJson(const std::filesystem::path& path)
{
    smu::platform::RobloxLogReader reader({path.parent_path()});
    Json result{
        {"operation", "roblox-file"},
        {"requested_path", path.string()},
    };
    result["snapshot"] = RobloxSnapshotJson(reader.poll(true));
    return result;
}

int Usage()
{
    std::cerr << "usage:\n"
              << "  smu_compat_oracle hotkey <text>\n"
              << "  smu_compat_oracle input-hotkey <json>\n"
              << "  smu_compat_oracle move-degrees <json>\n"
              << "  smu_compat_oracle roblox-regex <json>\n"
              << "  smu_compat_oracle key-name <text>\n"
              << "  smu_compat_oracle script-metadata <path>\n"
              << "  smu_compat_oracle roblox-file <path>\n";
    return 2;
}

} // namespace

int main(int argc, char** argv)
{
    if (argc != 3) {
        return Usage();
    }

    const std::string_view operation(argv[1]);
    Json result;
    if (operation == "hotkey") {
        result = HotkeyJson(argv[2]);
    } else if (operation == "input-hotkey") {
        result = InputHotkeyJson(argv[2]);
    } else if (operation == "linux-keymap") {
        result = LinuxKeymapJson(argv[2]);
    } else if (operation == "move-degrees") {
        result = MoveDegreesJson(argv[2]);
    } else if (operation == "roblox-regex") {
        result = RobloxRegexJson(argv[2]);
    } else if (operation == "key-name") {
        result = KeyNameJson(argv[2]);
    } else if (operation == "script-metadata") {
        result = ScriptMetadataJson(argv[2]);
    } else if (operation == "roblox-file") {
        result = RobloxFileJson(argv[2]);
    } else {
        return Usage();
    }

    std::cout << result.dump() << '\n';
    return 0;
}
