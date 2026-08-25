#include "profile_manager.h"
#include "json.hpp"
#include "legacy_globals.h"

// This test intentionally uses assert() for both execution and verification.
// Keep those expressions active in Release/CI builds where NDEBUG is normally set.
#ifdef NDEBUG
#undef NDEBUG
#endif
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

std::string ReadText(const fs::path& path)
{
    std::ifstream file(path, std::ios::binary);
    assert(file.is_open());
    return std::string(std::istreambuf_iterator<char>(file), std::istreambuf_iterator<char>());
}

json Marker(const std::string& value)
{
    return {{"marker", value}};
}

void AssertMarker(const fs::path& path, const std::string& expected)
{
    const json value = ReadJson(path);
    assert(value.at("marker").get<std::string>() == expected);
}

struct ResolutionLayout {
    fs::path root;
    fs::path canonical;
    fs::path executable;
    fs::path current;
};

ResolutionLayout MakeResolutionLayout(const fs::path& root, const std::string& name)
{
    ResolutionLayout layout{
        root / name,
        root / name / "config",
        root / name / "app" / "bin",
        root / name / "cwd"
    };
    fs::create_directories(layout.canonical);
    fs::create_directories(layout.executable);
    fs::create_directories(layout.current);
    return layout;
}

fs::path ResolveForTest(const ResolutionLayout& layout)
{
    return fs::path(ResolveSettingsFilePathForTesting(
        layout.canonical.string(), layout.executable.string(), layout.current.string()));
}

void RunSettingsResolutionMigrationTests(const fs::path& root)
{
    const fs::path expectedWindows = fs::path("/test/localappdata") / "Spencer Macro Utilities";
    assert(fs::path(BuildWindowsSettingsDirectoryForTesting("/test/localappdata")) == expectedWindows);

    // RMC-only canonical storage migrates to SMC without removing the legacy original.
    {
        const auto layout = MakeResolutionLayout(root, "canonical-rmc");
        const fs::path legacy = layout.canonical / "RMCSettings.json";
        WriteText(legacy, Marker("canonical-rmc").dump());
        const fs::path resolved = ResolveForTest(layout);
        assert(resolved == layout.canonical / "SMCSettings.json");
        AssertMarker(resolved, "canonical-rmc");
        AssertMarker(legacy, "canonical-rmc");
    }

    // A backup can be the only surviving copy; recovery must still create canonical SMC.
    {
        const auto layout = MakeResolutionLayout(root, "backup-only-rmc");
        const fs::path legacyBackup = layout.canonical / "RMCSettings.json.bak";
        WriteText(legacyBackup, Marker("rmc-backup").dump());
        const fs::path resolved = ResolveForTest(layout);
        assert(resolved == layout.canonical / "SMCSettings.json");
        AssertMarker(resolved, "rmc-backup");
        AssertMarker(legacyBackup, "rmc-backup");
        assert(!fs::exists(layout.canonical / "RMCSettings.json"));
    }

    // SMC wins over RMC globally, even when the RMC copy is in the canonical directory.
    {
        const auto layout = MakeResolutionLayout(root, "smc-wins");
        const fs::path rmc = layout.canonical / "RMCSettings.json";
        const fs::path smc = layout.executable / "SMCSettings.json";
        WriteText(rmc, Marker("rmc").dump());
        WriteText(smc, Marker("smc").dump());
        const fs::path resolved = ResolveForTest(layout);
        assert(resolved == layout.canonical / "SMCSettings.json");
        AssertMarker(resolved, "smc");
        AssertMarker(rmc, "rmc");
        AssertMarker(smc, "smc");
    }

    // A corrupt canonical SMC must not mask valid RMC data. Preserve the corrupt bytes.
    {
        const auto layout = MakeResolutionLayout(root, "corrupt-smc-valid-rmc");
        const fs::path canonicalSmc = layout.canonical / "SMCSettings.json";
        const fs::path rmc = layout.canonical / "RMCSettings.json";
        WriteText(canonicalSmc, "{");
        WriteText(rmc, Marker("recovered-rmc").dump());
        const fs::path resolved = ResolveForTest(layout);
        assert(resolved == canonicalSmc);
        AssertMarker(resolved, "recovered-rmc");
        AssertMarker(rmc, "recovered-rmc");

        bool preservedCorruptPrimary = false;
        for (const auto& entry : fs::directory_iterator(layout.canonical)) {
            const std::string filename = entry.path().filename().string();
            if (filename.rfind("SMCSettings.json.corrupt-", 0) == 0) {
                preservedCorruptPrimary = ReadText(entry.path()) == "{";
            }
        }
        assert(preservedCorruptPrimary);
    }

    // Executable-directory SMC discovery migrates into canonical storage.
    {
        const auto layout = MakeResolutionLayout(root, "exe-smc");
        const fs::path source = layout.executable / "SMCSettings.json";
        WriteText(source, Marker("exe-smc").dump());
        const fs::path resolved = ResolveForTest(layout);
        assert(resolved == layout.canonical / "SMCSettings.json");
        AssertMarker(resolved, "exe-smc");
        AssertMarker(source, "exe-smc");
    }

    // Executable-directory RMC backup discovery uses normal backup recovery semantics.
    {
        const auto layout = MakeResolutionLayout(root, "exe-rmc-backup");
        const fs::path sourceBackup = layout.executable / "RMCSettings.json.bak";
        WriteText(sourceBackup, Marker("exe-rmc-backup").dump());
        const fs::path resolved = ResolveForTest(layout);
        assert(resolved == layout.canonical / "SMCSettings.json");
        AssertMarker(resolved, "exe-rmc-backup");
        AssertMarker(sourceBackup, "exe-rmc-backup");
    }

    // One directory above the executable remains a supported historical location.
    {
        const auto layout = MakeResolutionLayout(root, "parent-smc");
        const fs::path source = layout.executable.parent_path() / "SMCSettings.json";
        WriteText(source, Marker("parent-smc").dump());
        const fs::path resolved = ResolveForTest(layout);
        assert(resolved == layout.canonical / "SMCSettings.json");
        AssertMarker(resolved, "parent-smc");
        AssertMarker(source, "parent-smc");
    }

    // The historical/current working directory is searched when distinct.
    {
        const auto layout = MakeResolutionLayout(root, "cwd-rmc");
        const fs::path source = layout.current / "RMCSettings.json";
        WriteText(source, Marker("cwd-rmc").dump());
        const fs::path resolved = ResolveForTest(layout);
        assert(resolved == layout.canonical / "SMCSettings.json");
        AssertMarker(resolved, "cwd-rmc");
        AssertMarker(source, "cwd-rmc");
    }

    // Canonical SMC backup recovery is self-healing and retains the backup.
    {
        const auto layout = MakeResolutionLayout(root, "canonical-smc-backup");
        const fs::path backupOnly = layout.canonical / "SMCSettings.json.bak";
        WriteText(backupOnly, Marker("canonical-smc-backup").dump());
        const fs::path resolved = ResolveForTest(layout);
        assert(resolved == layout.canonical / "SMCSettings.json");
        AssertMarker(resolved, "canonical-smc-backup");
        AssertMarker(backupOnly, "canonical-smc-backup");
    }

    // Migration is idempotent: once canonical SMC exists, changed legacy data cannot overwrite it.
    {
        const auto layout = MakeResolutionLayout(root, "idempotent");
        const fs::path legacy = layout.executable / "RMCSettings.json";
        WriteText(legacy, Marker("first").dump());
        const fs::path firstResolved = ResolveForTest(layout);
        AssertMarker(firstResolved, "first");
        WriteText(legacy, Marker("changed-legacy").dump());
        const fs::path secondResolved = ResolveForTest(layout);
        assert(secondResolved == firstResolved);
        AssertMarker(secondResolved, "first");
        AssertMarker(legacy, "changed-legacy");
    }
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

    RunSettingsResolutionMigrationTests(directory / "resolution-cases");

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
