#include "../app/script_metadata.h"
#include "../core/key_codes.h"
#include "../platform/roblox_log_reader.h"

#include "json.hpp"

#include <filesystem>
#include <iostream>
#include <optional>
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
