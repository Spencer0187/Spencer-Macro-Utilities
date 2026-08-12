#include "profile_manager.h"
#include "json.hpp"

#include <cassert>
#include <chrono>
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

    fs::remove_all(directory);
    return 0;
}
