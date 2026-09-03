#include "profile_manager.h"
#include "json.hpp"

#include <filesystem>
#include <fstream>
#include <iostream>
#include <string>
#include <string_view>

namespace {

namespace fs = std::filesystem;
using Json = nlohmann::json;

Json ReadJsonOrNull(const fs::path& path)
{
    std::ifstream input(path, std::ios::binary);
    if (!input.is_open()) {
        return nullptr;
    }
    Json value = Json::parse(input, nullptr, false);
    return value.is_discarded() ? Json(nullptr) : value;
}

Json MutationResult(const fs::path& path, bool success)
{
    return Json{
        {"success", success},
        {"document", ReadJsonOrNull(path)},
        {"backup_exists", fs::exists(fs::path(path.string() + ".bak"))},
    };
}

int Usage()
{
    std::cerr << "usage:\n"
              << "  smu_profile_compat_oracle legacy-keys\n"
              << "  smu_profile_compat_oracle detect <json>\n"
              << "  smu_profile_compat_oracle best-profile <json> <requested>\n"
              << "  smu_profile_compat_oracle profile-names <path>\n"
              << "  smu_profile_compat_oracle duplicate <path> <source> <new>\n"
              << "  smu_profile_compat_oracle rename <path> <old> <new>\n"
              << "  smu_profile_compat_oracle delete <path> <name>\n"
              << "  smu_profile_compat_oracle resolve <canonical-dir> <exe-dir> <cwd>\n";
    return 2;
}

} // namespace

int main(int argc, char** argv)
{
    if (argc < 2) {
        return Usage();
    }

    const std::string_view operation(argv[1]);
    Json result;

    if (operation == "legacy-keys" && argc == 2) {
        result = Json{{"operation", "legacy-keys"}, {"keys", LegacyFormatKeysForTesting()}};
    } else if (operation == "detect" && argc == 3) {
        result = Json{
            {"operation", "detect"},
            {"format", DetectSettingsFormatForTesting(argv[2])},
        };
    } else if (operation == "best-profile" && argc == 4) {
        result = Json{
            {"operation", "best-profile"},
            {"best", FindBestProfileForTesting(argv[2], argv[3])},
        };
    } else if (operation == "profile-names" && argc == 3) {
        result = Json{
            {"operation", "profile-names"},
            {"names", GetProfileNames(argv[2])},
        };
    } else if (operation == "duplicate" && argc == 5) {
        result = MutationResult(argv[2], DuplicateProfileInFile(argv[2], argv[3], argv[4]));
        result["operation"] = "duplicate";
    } else if (operation == "rename" && argc == 5) {
        result = MutationResult(argv[2], RenameProfileInFile(argv[2], argv[3], argv[4]));
        result["operation"] = "rename";
    } else if (operation == "delete" && argc == 4) {
        result = MutationResult(argv[2], DeleteProfileFromFile(argv[2], argv[3]));
        result["operation"] = "delete";
    } else if (operation == "resolve" && argc == 5) {
        result = Json{
            {"operation", "resolve"},
            {"resolved", ResolveSettingsFilePathForTesting(argv[2], argv[3], argv[4])},
        };
    } else {
        return Usage();
    }

    std::cout << result.dump() << '\n';
    return 0;
}
