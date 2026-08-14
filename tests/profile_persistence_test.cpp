#include "profile_manager.h"
#include "json.hpp"
#include "legacy_globals.h"

#include <cassert>
#include <chrono>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <string>

namespace fs = std::filesystem;
using json = nlohmann::json;

namespace {

void WriteText(const fs::path& path, const std::string& text)
{
    std::ofstream file(path, std::ios::binary | std::ios::trunc);
    assert(file.is_open());
    file << text;
    file.close();
    assert(file.good());
}

json ReadJson(const fs::path& path)
{
    std::ifstream file(path, std::ios::binary);
    assert(file.is_open());
    json value;
    file >> value;
    return value;
}

} // namespace

int main()
{
    const auto suffix = std::chrono::steady_clock::now().time_since_epoch().count();
    const fs::path directory = fs::temp_directory_path() / ("smu-profile-test-" + std::to_string(suffix));
    const fs::path settings = directory / "SMCSettings.json";
    const fs::path backup = fs::path(settings.string() + ".bak");
    fs::create_directories(directory);

    const json initial = {
        {"Profile A", {{"marker", "A"}}},
        {"Profile B", {{"marker", "B"}}},
        {"_metadata", json::object()}
    };
    WriteText(settings, initial.dump(2));

    assert(DuplicateProfileInFile(settings.string(), "Profile A", "Profile C"));
    assert(fs::exists(backup));
    const json afterFirstWrite = ReadJson(settings);
    assert(afterFirstWrite.contains("Profile A"));
    assert(afterFirstWrite.contains("Profile B"));
    assert(afterFirstWrite.contains("Profile C"));

    // A truncated primary must recover from .bak and must never be treated as
    // an empty file. The recovered write should remain valid JSON.
    WriteText(settings, "{");
    assert(DuplicateProfileInFile(settings.string(), "Profile A", "Profile D"));
    const json recovered = ReadJson(settings);
    assert(recovered.contains("Profile A"));
    assert(recovered.contains("Profile B"));
    assert(recovered.contains("Profile D"));

    // With neither a readable primary nor a valid backup, a mutation must
    // fail without replacing the unreadable primary.
    fs::remove(backup);
    WriteText(settings, "{");
    assert(!DuplicateProfileInFile(settings.string(), "Profile A", "Profile Z"));
    std::ifstream unchanged(settings, std::ios::binary);
    std::string contents((std::istreambuf_iterator<char>(unchanged)), std::istreambuf_iterator<char>());
    assert(contents == "{");

    // Persisted/UI buffers must synchronize with the runtime values consumed
    // by the macro timing code.
    std::strcpy(Globals::PasteDelayChar, "7");
    std::strcpy(Globals::BunnyHopDelayChar, "13");
    std::strcpy(Globals::HHJLengthChar, "321");
    std::strcpy(Globals::FloorBounceDelay3Char, "77");
    std::strcpy(Globals::WallhopPixels, "345");
    std::strcpy(Globals::WallhopVerticalChar, "12");
    std::strcpy(Globals::WallhopDelayChar, "23");
    std::strcpy(Globals::WallhopBonusDelayChar, "4");
    std::strcpy(Globals::PressKeyDelayChar, "18");
    std::strcpy(Globals::PressKeyBonusDelayChar, "3");
    std::strcpy(Globals::SpamDelay, "7.5");
    std::strcpy(Globals::RobloxWallWalkValueChar, "-88");
    std::strcpy(Globals::RobloxPixelValueChar, "812");
    std::strcpy(Globals::ItemClipDelay, "42");
    std::strcpy(Globals::AntiAFKTimeChar, "9");
    std::strcpy(Globals::AutoHHJKey1TimeChar, "551");
    std::strcpy(Globals::RobloxFPSChar, "240");
    SyncRuntimeSettingsFromBuffers();
    assert(Globals::PasteDelay == 7);
    assert(Globals::BunnyHopDelay == 13);
    assert(Globals::HHJLength == 321);
    assert(Globals::FloorBounceDelay3 == 77);
    assert(Globals::wallhop_dx == 345);
    assert(Globals::wallhop_dy == -345);
    assert(Globals::wallhop_vertical == 12);
    assert(Globals::WallhopDelay == 23);
    assert(Globals::WallhopBonusDelay == 4);
    assert(Globals::PressKeyDelay == 18);
    assert(Globals::PressKeyBonusDelay == 3);
    assert(Globals::spam_delay == 7.5f);
    assert(Globals::real_delay == 4);
    assert(Globals::wallwalk_strengthx == -88);
    assert(Globals::wallwalk_strengthy == 88);
    assert(Globals::RobloxPixelValue == 812);
    assert(Globals::speed_strengthx == 812);
    assert(Globals::speed_strengthy == -812);
    assert(Globals::clip_delay == 42);
    assert(Globals::AntiAFKTime == 9);
    assert(Globals::AutoHHJKey1Time == 551);
    assert(Globals::RobloxFPS.load(std::memory_order_relaxed) == 240);

    // The same synchronization must update the first live instance when the
    // application has already initialized its macro containers.
    Globals::wallhop_instances.emplace_back();
    Globals::presskey_instances.emplace_back();
    Globals::spamkey_instances.emplace_back();
    SyncRuntimeSettingsFromBuffers();
    assert(Globals::wallhop_instances.front().WallhopDelay == 23);
    assert(Globals::wallhop_instances.front().WallhopBonusDelay == 4);
    assert(Globals::presskey_instances.front().PressKeyDelay == 18);
    assert(Globals::presskey_instances.front().PressKeyBonusDelay == 3);
    assert(Globals::spamkey_instances.front().spam_delay == 7.5f);
    assert(Globals::spamkey_instances.front().real_delay == 4);

    fs::remove_all(directory);
    return 0;
}
