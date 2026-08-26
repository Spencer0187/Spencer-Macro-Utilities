#include "asset_selection.h"
#include "miniz.h"
#include "updater.h"

#include <cerrno>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <iterator>
#include <optional>
#include <string>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>
#include <vector>

namespace smu::updater::detail {
bool ExtractLinuxAppImageFromPackage(
    const std::vector<char>& packageBytes,
    const std::string& bundleAssetName,
    const std::string& newVersion,
    std::vector<char>& appImageBytes,
    std::string* errorMessage);
std::string BuildLinuxUpdaterScript();
bool PlatformAutoApplySupported();
std::string Sha256Hex(const std::vector<char>& data);
bool IsUpdaterVersionAllowed(const ReleaseInfo& release, const std::string& updaterVersion);
std::optional<ReleaseInfo> ParseReleaseMetadataSummary(
    const std::string& releaseJson,
    std::string* errorMessage);
std::optional<ReleaseInfo> ParseReleaseMetadataWithManifest(
    const std::string& releaseJson,
    const std::string& manifestJson,
    std::string* errorMessage);
}

namespace {

void Expect(bool condition, const char* message)
{
    if (!condition) {
        std::cerr << "FAILED: " << message << '\n';
        std::exit(1);
    }
}

std::string CurrentAppImageName()
{
#if defined(__x86_64__) || defined(__amd64__)
    return "Spencer-Macro-Utilities-x86_64.AppImage";
#elif defined(__aarch64__)
    return "Spencer-Macro-Utilities-aarch64.AppImage";
#else
#error "This updater test supports x86_64 and aarch64 Linux."
#endif
}

std::string CurrentVersionedAppImageName()
{
#if defined(__x86_64__) || defined(__amd64__)
    return "Spencer-Macro-Utilities-V3.3.0-Linux-x86_64.AppImage";
#else
    return "Spencer-Macro-Utilities-V3.3.0-Linux-aarch64.AppImage";
#endif
}

std::string CurrentBundleName()
{
#if defined(__x86_64__) || defined(__amd64__)
    return "Spencer-Macro-Utilities-V3.3.0-Linux-x86_64.zip";
#else
    return "Spencer-Macro-Utilities-V3.3.0-Linux-aarch64.zip";
#endif
}

std::string OtherArchitectureName()
{
#if defined(__x86_64__) || defined(__amd64__)
    return "Spencer-Macro-Utilities-Linux-arm64.zip";
#else
    return "Spencer-Macro-Utilities-Linux-x86_64.zip";
#endif
}

smu::updater::detail::LinuxArchitecture CurrentArchitecture()
{
#if defined(__x86_64__) || defined(__amd64__)
    return smu::updater::detail::LinuxArchitecture::X86_64;
#else
    return smu::updater::detail::LinuxArchitecture::AArch64;
#endif
}

std::vector<char> FakeAppImage(bool currentArchitecture)
{
    std::vector<char> bytes(64, '\0');
    bytes[0] = 0x7f;
    bytes[1] = 'E';
    bytes[2] = 'L';
    bytes[3] = 'F';
    bytes[4] = 2; // ELFCLASS64
    bytes[5] = 1; // ELFDATA2LSB

#if defined(__x86_64__) || defined(__amd64__)
    const unsigned int machine = currentArchitecture ? 62 : 183;
#else
    const unsigned int machine = currentArchitecture ? 183 : 62;
#endif
    bytes[18] = static_cast<char>(machine & 0xff);
    bytes[19] = static_cast<char>((machine >> 8) & 0xff);
    return bytes;
}

std::vector<char> MakeZip(const std::string& entryName, const std::vector<char>& contents)
{
    mz_zip_archive archive;
    mz_zip_zero_struct(&archive);
    Expect(mz_zip_writer_init_heap(&archive, 0, 0) == MZ_TRUE, "initialize in-memory ZIP writer");
    Expect(
        mz_zip_writer_add_mem(
            &archive,
            entryName.c_str(),
            contents.data(),
            contents.size(),
            MZ_BEST_COMPRESSION) == MZ_TRUE,
        "add in-memory ZIP entry");

    void* data = nullptr;
    std::size_t size = 0;
    Expect(
        mz_zip_writer_finalize_heap_archive(&archive, &data, &size) == MZ_TRUE,
        "finalize in-memory ZIP");
    std::vector<char> result(static_cast<char*>(data), static_cast<char*>(data) + size);
    mz_free(data);
    Expect(mz_zip_writer_end(&archive) == MZ_TRUE, "close in-memory ZIP writer");
    return result;
}

void WriteExecutable(
    const std::filesystem::path& path,
    const std::string& contents)
{
    std::ofstream file(path, std::ios::binary | std::ios::trunc);
    file << contents;
    file.close();
    Expect(file.good(), "write updater shell test executable");
    Expect(chmod(path.c_str(), 0755) == 0, "make updater shell test executable");
}

std::string ReadFile(const std::filesystem::path& path)
{
    std::ifstream file(path, std::ios::binary);
    Expect(file.good(), "open updater shell test file");
    return std::string(
        std::istreambuf_iterator<char>(file),
        std::istreambuf_iterator<char>());
}

int RunLinuxUpdaterScript(
    const std::filesystem::path& directory,
    const std::filesystem::path& currentAppImage,
    const std::filesystem::path& newAppImage,
    const std::string& pathOverride = {})
{
    const std::filesystem::path updaterScript =
        directory / "apply update.sh";
    WriteExecutable(
        updaterScript,
        smu::updater::detail::BuildLinuxUpdaterScript());

    const pid_t child = fork();
    Expect(child >= 0, "fork Linux updater shell test");
    if (child == 0) {
        setenv("TMPDIR", directory.c_str(), 1);
        if (!pathOverride.empty()) {
            setenv("PATH", pathOverride.c_str(), 1);
        }
        execl(
            "/bin/sh",
            "sh",
            updaterScript.c_str(),
            currentAppImage.c_str(),
            newAppImage.c_str(),
            "2147483647",
            static_cast<char*>(nullptr));
        _exit(errno == ENOENT ? 127 : 126);
    }

    int status = 0;
    pid_t waited = 0;
    do {
        waited = waitpid(child, &status, 0);
    } while (waited < 0 && errno == EINTR);
    Expect(waited == child, "wait for Linux updater shell test");
    Expect(WIFEXITED(status), "Linux updater shell test exits normally");
    return WEXITSTATUS(status);
}

bool WaitForPath(const std::filesystem::path& path)
{
    for (int attempt = 0; attempt < 100; ++attempt) {
        if (std::filesystem::exists(path)) {
            return true;
        }
        usleep(10'000);
    }
    return false;
}

bool HasUpdaterBackup(const std::filesystem::path& directory)
{
    for (const auto& entry : std::filesystem::directory_iterator(directory)) {
        if (entry.path().filename().string().find(".old.") != std::string::npos) {
            return true;
        }
    }
    return false;
}

void TestAssetSelection()
{
    smu::updater::ReleaseInfo release;
    release.assets = {
        {"Spencer-Macro-Utilities-Windows-v3.3.0.zip", "unused", 1},
        {"Spencer-Macro-Utilities-macOS-v3.3.0.zip", "unused", 1},
        {CurrentBundleName(), "unused", 1},
        {OtherArchitectureName(), "unused", 1},
    };

    auto selected = smu::updater::SelectUpdateAsset(release);
    Expect(selected.has_value(), "select all-in-one Linux distribution ZIP");
    Expect(
        selected->name == CurrentBundleName(),
        "ignore other platforms and architectures");

    release.assets.push_back({CurrentAppImageName(), "unused", 1});
    selected = smu::updater::SelectUpdateAsset(release);
    Expect(selected.has_value(), "select standalone AppImage");
    Expect(selected->name == CurrentAppImageName(), "prefer standalone AppImage over Linux ZIP");
}

std::string ReplaceOnce(std::string value, const std::string& from, const std::string& to)
{
    const std::size_t position = value.find(from);
    Expect(position != std::string::npos, "find updater contract test token");
    value.replace(position, from.size(), to);
    return value;
}

void TestReleaseManifestContract()
{
    const std::string officialAsset = CurrentBundleName();
    const std::string extraAsset = CurrentAppImageName();
    const std::string releaseBase =
        "https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/download/V3.4.0/";
    const std::string sha256 =
        "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD";

    const std::string releaseJson =
        "{\"tag_name\":\"V3.4.0\","
        "\"html_url\":\"https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/tag/V3.4.0\","
        "\"assets\":["
        "{\"name\":\"update-manifest.json\",\"browser_download_url\":\"" + releaseBase +
            "update-manifest.json\",\"size\":321},"
        "{\"name\":\"" + officialAsset + "\",\"browser_download_url\":\"" + releaseBase + officialAsset +
            "\",\"size\":12345},"
        "{\"name\":\"" + extraAsset + "\",\"browser_download_url\":\"" + releaseBase + extraAsset +
            "\",\"size\":54321}]}";

    const std::string legacyReleaseJson =
        "{\"tag_name\":\"V3.3.1\","
        "\"html_url\":\"https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/tag/V3.3.1\","
        "\"assets\":[]}";
    std::string error;
    auto legacySummary = smu::updater::detail::ParseReleaseMetadataSummary(
        legacyReleaseJson,
        &error);
    Expect(legacySummary.has_value(),
        "parse an older GitHub release summary without requiring update-manifest.json");
    Expect(legacySummary->version == "3.3.1",
        "normalize the older public release version before manifest lookup");
    Expect(!legacySummary->manifestDriven && legacySummary->assets.empty(),
        "keep release-summary parsing separate from the manifest install contract");
    Expect(
        !smu::updater::detail::ParseReleaseMetadataSummary(
            ReplaceOnce(legacyReleaseJson, "V3.3.1", "V3.3.1-beta"),
            &error),
        "reject non-canonical latest-release tags before deciding that a manifest is unnecessary");

    const std::string manifestJson =
        "{\"schema_version\":1,\"release_version\":\"3.4.0\","
        "\"minimum_updater_version\":\"3.4.0\",\"artifacts\":{"
        "\"linux-current\":{\"asset\":\"" + officialAsset + "\",\"size\":12345,"
        "\"sha256\":\"" + sha256 + "\",\"url\":\"" + releaseBase + officialAsset + "\"}}}";

    auto release = smu::updater::detail::ParseReleaseMetadataWithManifest(
        releaseJson,
        manifestJson,
        &error);
    Expect(release.has_value(), "accept a valid V3.4 update manifest");
    Expect(release->manifestDriven, "mark releases parsed from the update manifest");
    Expect(release->manifestSchemaVersion == 1, "record update manifest schema version");
    Expect(release->minimumUpdaterVersion == "3.4.0", "record minimum updater version");
    Expect(release->assets.size() == 1, "ignore GitHub release assets omitted from the manifest");
    Expect(release->assets.front().name == officialAsset, "retain only the manifest-declared artifact");
    Expect(release->assets.front().sizeBytes == 12345, "use manifest artifact byte size");
    Expect(
        release->assets.front().sha256 ==
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "normalize manifest SHA-256 to lowercase");
    auto selected = smu::updater::SelectUpdateAsset(*release);
    Expect(selected.has_value() && selected->name == officialAsset,
        "do not let a higher-scoring undeclared GitHub asset influence selection");
    Expect(
        smu::updater::detail::IsUpdaterVersionAllowed(*release, "3.4.0"),
        "allow the manifest minimum updater version");
    Expect(
        !smu::updater::detail::IsUpdaterVersionAllowed(*release, "3.3.9"),
        "reject an updater older than minimum_updater_version");

    const std::string manifestWithoutUrl = ReplaceOnce(
        manifestJson,
        ",\"url\":\"" + releaseBase + officialAsset + "\"",
        "");
    Expect(
        smu::updater::detail::ParseReleaseMetadataWithManifest(
            releaseJson,
            manifestWithoutUrl,
            &error).has_value(),
        "allow release-relative asset selection without an explicit manifest URL");
    Expect(
        !smu::updater::detail::ParseReleaseMetadataWithManifest(
            releaseJson,
            ReplaceOnce(manifestJson, "\"schema_version\":1", "\"schema_version\":2"),
            &error),
        "reject unsupported update manifest schema versions");
    Expect(
        !smu::updater::detail::ParseReleaseMetadataWithManifest(
            releaseJson,
            ReplaceOnce(manifestJson, "\"release_version\":\"3.4.0\"", "\"release_version\":\"3.4.1\""),
            &error),
        "reject a manifest for a different release tag");
    Expect(
        !smu::updater::detail::ParseReleaseMetadataWithManifest(
            releaseJson,
            ReplaceOnce(manifestJson, "\"minimum_updater_version\":\"3.4.0\",", ""),
            &error),
        "reject a manifest without minimum_updater_version");
    Expect(
        !smu::updater::detail::ParseReleaseMetadataWithManifest(
            releaseJson,
            ReplaceOnce(manifestJson, "\"minimum_updater_version\":\"3.4.0\"", "\"minimum_updater_version\":\"3.4\""),
            &error),
        "reject a non-canonical minimum_updater_version");
    Expect(
        !smu::updater::detail::ParseReleaseMetadataWithManifest(
            ReplaceOnce(releaseJson, "\"name\":\"" + extraAsset + "\"", "\"name\":\"" + officialAsset + "\""),
            manifestJson,
            &error),
        "reject duplicate GitHub metadata entries for one manifest asset");
    Expect(
        !smu::updater::detail::ParseReleaseMetadataWithManifest(
            releaseJson,
            ReplaceOnce(manifestJson, "\"size\":12345", "\"size\":12346"),
            &error),
        "reject manifest and GitHub artifact size disagreement");
    Expect(
        !smu::updater::detail::ParseReleaseMetadataWithManifest(
            releaseJson,
            ReplaceOnce(manifestJson, sha256, "abc"),
            &error),
        "reject malformed manifest SHA-256 digests");
    Expect(
        !smu::updater::detail::ParseReleaseMetadataWithManifest(
            releaseJson,
            ReplaceOnce(manifestJson, releaseBase + officialAsset,
                "https://example.invalid/not-official.zip"),
            &error),
        "reject an explicit manifest URL that differs from the official release asset URL");
    Expect(
        !smu::updater::detail::ParseReleaseMetadataWithManifest(
            releaseJson,
            ReplaceOnce(manifestJson, "\"asset\":\"" + officialAsset + "\"",
                "\"asset\":\"missing-update.zip\""),
            &error),
        "reject a manifest reference to a missing GitHub release asset");

    const std::string duplicateManifest = ReplaceOnce(
        manifestJson,
        "}}}",
        "},\"duplicate\":{\"asset\":\"" + officialAsset + "\",\"size\":12345,\"sha256\":\"" + sha256 + "\"}}}");
    Expect(
        !smu::updater::detail::ParseReleaseMetadataWithManifest(
            releaseJson,
            duplicateManifest,
            &error),
        "reject duplicate manifest declarations of one release asset");

    Expect(
        std::string(smu::updater::LatestReleasePageUrl()) ==
            "https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/latest",
        "manual updater escape hatch is the stable latest-release URL");
}

void TestSha256()
{
    const std::vector<char> empty;
    Expect(
        smu::updater::detail::Sha256Hex(empty) ==
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "SHA-256 empty-string known vector");
    const std::vector<char> abc {'a', 'b', 'c'};
    Expect(
        smu::updater::detail::Sha256Hex(abc) ==
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "SHA-256 abc known vector");
}

void TestCrossPlatformAssetNames()
{
    using smu::updater::detail::ScoreLinuxAssetName;
    using smu::updater::detail::ScoreMacOSAssetName;
    using smu::updater::detail::ScoreWindowsAssetName;

    const int legacyWindowsScore =
        ScoreWindowsAssetName("Spencer-Macro-Utilities-V3.2.1.zip");
    Expect(legacyWindowsScore > 0, "retain V3.2 generic Windows ZIP compatibility");
    Expect(
        ScoreWindowsAssetName("Spencer-Macro-Utilities-3.3.0.zip") > 0,
        "accept the V3.3 generic signed Windows ZIP");
    Expect(
        ScoreWindowsAssetName(
            "Spencer-Macro-Utilities-V3.3.0-Windows-x64.zip") >
            legacyWindowsScore,
        "prefer an explicitly named Windows ZIP over a legacy generic ZIP");
    Expect(
        ScoreWindowsAssetName("Spencer-Macro-Utilities-Windows.zip") > legacyWindowsScore,
        "make the stable V3.4.0 Windows ZIP the preferred pre-manifest Windows package");
    Expect(
        ScoreWindowsAssetName(
            "Spencer-Macro-Utilities-V3.3.0-macOS-universal.zip") == 0,
        "never treat the macOS ZIP as a Windows update");
    Expect(
        ScoreWindowsAssetName(CurrentBundleName()) == 0,
        "never treat the Linux bundle as a Windows update");

    Expect(
        ScoreMacOSAssetName(
            "Spencer-Macro-Utilities-V3.3.0-macOS-universal.zip") >
            ScoreMacOSAssetName("Spencer-Macro-Utilities-V3.2.1.zip"),
        "prefer the explicit universal macOS ZIP");
    Expect(
        ScoreMacOSAssetName("Spencer-Macro-Utilities-V3.2.1.zip") == 0,
        "do not fall back to the generic Windows ZIP on macOS");
    Expect(
        ScoreMacOSAssetName(
            "Spencer-Macro-Utilities-V3.3.0-macOS-universal.dmg") == 0,
        "do not select a mounted-install DMG for in-place macOS updating");
    Expect(
        ScoreMacOSAssetName(
            "Spencer-Macro-Utilities-V3.3.0-Windows-x64.zip") == 0,
        "never treat the Windows ZIP as a macOS update");
    Expect(
        ScoreMacOSAssetName(CurrentBundleName()) == 0,
        "never treat the Linux bundle as a macOS update");

    const int linuxBundleScore =
        ScoreLinuxAssetName(CurrentBundleName(), CurrentArchitecture());
    const int appImageScore =
        ScoreLinuxAssetName(CurrentVersionedAppImageName(), CurrentArchitecture());
    Expect(linuxBundleScore > 0, "accept the all-in-one Linux distribution ZIP");
    Expect(appImageScore > linuxBundleScore, "prefer a direct matching AppImage");
    Expect(
        ScoreLinuxAssetName(OtherArchitectureName(), CurrentArchitecture()) == 0,
        "reject a Linux update for another CPU architecture");
    Expect(
        ScoreLinuxAssetName(
            "Spencer-Macro-Utilities-V3.3.0-macOS-universal.zip",
            CurrentArchitecture()) == 0,
        "never treat the macOS ZIP as a Linux update");
    Expect(
        ScoreLinuxAssetName(
            "Spencer-Macro-Utilities-V3.3.0-Windows-x64.zip",
            CurrentArchitecture()) == 0,
        "never treat the Windows ZIP as a Linux update");
}

void TestBundleExtraction()
{
    const std::vector<char> expected = FakeAppImage(true);
    std::vector<char> extracted;
    std::string error;

    const std::string bundleName = CurrentBundleName();
    const std::vector<char> package = MakeZip(CurrentVersionedAppImageName(), expected);
    Expect(
        smu::updater::detail::ExtractLinuxAppImageFromPackage(
            package,
            bundleName,
            "3.3.0",
            extracted,
            &error),
        "extract current-architecture root-level AppImage");
    Expect(extracted == expected, "preserve extracted AppImage bytes");

    const std::vector<char> nestedPackage =
        MakeZip("nested/" + CurrentAppImageName(), expected);
    Expect(
        !smu::updater::detail::ExtractLinuxAppImageFromPackage(
            nestedPackage,
            bundleName,
            "3.3.0",
            extracted,
            &error),
        "reject unexpected nested AppImage path");

    const std::vector<char> wrongArchitecturePackage =
        MakeZip(CurrentAppImageName(), FakeAppImage(false));
    Expect(
        !smu::updater::detail::ExtractLinuxAppImageFromPackage(
            wrongArchitecturePackage,
            bundleName,
            "3.3.0",
            extracted,
            &error),
        "reject wrong-architecture AppImage payload");

    const std::vector<char> rootPackage =
        MakeZip(CurrentAppImageName(), expected);
    Expect(
        !smu::updater::ExtractUpdatePackage(
            rootPackage,
            "../" + CurrentAppImageName(),
            extracted,
            &error),
        "reject a traversal path requested from an update ZIP");
    Expect(
        !smu::updater::ExtractUpdatePackage(
            rootPackage,
            "nested\\" + CurrentAppImageName(),
            extracted,
            &error),
        "reject a Windows-style nested path requested from an update ZIP");

    const std::string windowsEntry =
        "Spencer-Macro-Utilities/Spencer-Macro-Utilities-V3.4.0-Windows.exe";
    const std::vector<char> nestedWindowsPackage = MakeZip(windowsEntry, expected);
    Expect(
        smu::updater::ExtractUpdatePackageEntry(
            nestedWindowsPackage,
            windowsEntry,
            extracted,
            &error),
        "extract an exact safe nested updater entry");
    Expect(extracted == expected, "preserve nested updater entry bytes");
    Expect(
        !smu::updater::ExtractUpdatePackageEntry(
            nestedWindowsPackage,
            "Spencer-Macro-Utilities/../Spencer-Macro-Utilities-V3.4.0-Windows.exe",
            extracted,
            &error),
        "reject traversal segments in nested updater entries");
}


void TestAppImageAutoApplySupport()
{
    const char* originalAppImage = std::getenv("APPIMAGE");
    const std::optional<std::string> savedAppImage =
        originalAppImage
        ? std::optional<std::string>(originalAppImage)
        : std::nullopt;
    const char* originalAppDir = std::getenv("APPDIR");
    const std::optional<std::string> savedAppDir =
        originalAppDir
        ? std::optional<std::string>(originalAppDir)
        : std::nullopt;

    unsetenv("APPIMAGE");
    unsetenv("APPDIR");
    Expect(
        !smu::updater::detail::PlatformAutoApplySupported(),
        "disable automatic apply outside an AppImage launch");

    char directoryTemplate[] = "/tmp/smu-updater-test-XXXXXX";
    char* createdDirectory = mkdtemp(directoryTemplate);
    Expect(createdDirectory != nullptr, "create updater writeability test directory");
    const std::filesystem::path testDirectory(createdDirectory);
    const std::filesystem::path appImagePath =
        testDirectory / "Spencer Macro Utilities.AppImage";
    std::error_code ec;
    const std::filesystem::path testExecutable =
        std::filesystem::canonical("/proc/self/exe", ec);
    Expect(!ec, "resolve updater test executable");
    const std::filesystem::path declaredAppDir =
        testExecutable.parent_path();
    setenv("APPDIR", declaredAppDir.c_str(), 1);

    setenv("APPIMAGE", appImagePath.c_str(), 1);
    Expect(
        !smu::updater::detail::PlatformAutoApplySupported(),
        "reject a missing APPIMAGE path");

    setenv("APPIMAGE", testDirectory.c_str(), 1);
    Expect(
        !smu::updater::detail::PlatformAutoApplySupported(),
        "reject an APPIMAGE path that names a directory");

    {
        std::ofstream appImage(appImagePath, std::ios::binary);
        appImage << "test";
        Expect(appImage.good(), "create a pretend current AppImage");
    }
    Expect(chmod(appImagePath.c_str(), 0555) == 0, "make pretend AppImage read-only");
    setenv("APPIMAGE", appImagePath.c_str(), 1);
    setenv("APPDIR", testDirectory.c_str(), 1);
    Expect(
        !smu::updater::detail::PlatformAutoApplySupported(),
        "reject an APPIMAGE variable inherited by a non-AppImage SMU process");
    setenv("APPDIR", declaredAppDir.c_str(), 1);
    Expect(
        smu::updater::detail::PlatformAutoApplySupported(),
        "allow atomic replacement of a read-only AppImage in a writable folder");

    Expect(chmod(testDirectory.c_str(), 0555) == 0, "make AppImage folder read-only");
    if (geteuid() != 0) {
        Expect(
            !smu::updater::detail::PlatformAutoApplySupported(),
            "disable automatic apply when the AppImage folder is not writable");
    }
    Expect(chmod(testDirectory.c_str(), 0700) == 0, "restore updater test directory permissions");

    std::filesystem::remove_all(testDirectory, ec);
    Expect(!ec, "remove updater writeability test directory");
    if (savedAppImage) {
        setenv("APPIMAGE", savedAppImage->c_str(), 1);
    } else {
        unsetenv("APPIMAGE");
    }
    if (savedAppDir) {
        setenv("APPDIR", savedAppDir->c_str(), 1);
    } else {
        unsetenv("APPDIR");
    }
}

void TestLinuxReplacementAndRollback()
{
    char directoryTemplate[] = "/tmp/smu-updater-script-test-XXXXXX";
    char* createdDirectory = mkdtemp(directoryTemplate);
    Expect(createdDirectory != nullptr, "create updater shell test directory");
    const std::filesystem::path testDirectory(createdDirectory);
    const std::filesystem::path currentAppImage =
        testDirectory / "Current SMU.AppImage";
    const std::filesystem::path newAppImage =
        testDirectory / "New SMU.AppImage";
    const std::filesystem::path relaunchMarker =
        testDirectory / "previous-version-relaunched";

    const std::string previousVersion =
        "#!/bin/sh\n"
        "# previous version\n"
        "touch \"" + relaunchMarker.string() + "\"\n"
        "sleep 4\n";
    const std::string healthyUpdate =
        "#!/bin/sh\n"
        "# healthy update\n"
        "sleep 4\n";
    WriteExecutable(currentAppImage, previousVersion);
    WriteExecutable(newAppImage, healthyUpdate);
    Expect(
        RunLinuxUpdaterScript(
            testDirectory,
            currentAppImage,
            newAppImage) == 0,
        "replace and launch a healthy AppImage");
    Expect(
        ReadFile(currentAppImage) == healthyUpdate,
        "keep a successfully launched AppImage");
    Expect(
        !std::filesystem::exists(newAppImage),
        "move the staged AppImage into place");
    Expect(!HasUpdaterBackup(testDirectory), "remove backup after successful launch");

    const std::string brokenUpdate =
        "#!/bin/sh\n"
        "# broken update\n"
        "exit 7\n";
    WriteExecutable(currentAppImage, previousVersion);
    WriteExecutable(newAppImage, brokenUpdate);
    Expect(
        RunLinuxUpdaterScript(
            testDirectory,
            currentAppImage,
            newAppImage) == 1,
        "detect an AppImage that exits during startup");
    Expect(
        ReadFile(currentAppImage) == previousVersion,
        "restore the previous AppImage after a failed launch");
    Expect(!HasUpdaterBackup(testDirectory), "consume backup during rollback");
    Expect(
        WaitForPath(relaunchMarker),
        "relaunch the previous AppImage after a failed updated-app launch");

    std::error_code ec;
    std::filesystem::remove_all(testDirectory, ec);
    Expect(!ec, "remove updater shell test directory");
}

void TestLinuxPreSwapFailureRelaunchesPreviousVersion()
{
    char directoryTemplate[] = "/tmp/smu-updater-preswap-test-XXXXXX";
    char* createdDirectory = mkdtemp(directoryTemplate);
    Expect(createdDirectory != nullptr, "create pre-swap updater test directory");
    const std::filesystem::path testDirectory(createdDirectory);
    const std::filesystem::path currentAppImage =
        testDirectory / "Current SMU.AppImage";
    const std::filesystem::path missingUpdate =
        testDirectory / "Missing update.AppImage";
    const std::filesystem::path relaunchMarker =
        testDirectory / "previous-version-relaunched";

    const std::string previousVersion =
        "#!/bin/sh\n"
        "touch \"" + relaunchMarker.string() + "\"\n";
    WriteExecutable(currentAppImage, previousVersion);

    Expect(
        RunLinuxUpdaterScript(
            testDirectory,
            currentAppImage,
            missingUpdate) == 1,
        "fail before swapping when the staged AppImage disappeared");
    Expect(
        ReadFile(currentAppImage) == previousVersion,
        "leave the previous AppImage intact after a pre-swap failure");
    Expect(
        WaitForPath(relaunchMarker),
        "relaunch the previous AppImage after a pre-swap failure");
    Expect(!HasUpdaterBackup(testDirectory), "avoid a backup before the swap");

    std::error_code ec;
    std::filesystem::remove_all(testDirectory, ec);
    Expect(!ec, "remove pre-swap updater test directory");
}

void TestLinuxInstallFailureRestoresAndRelaunchesPreviousVersion()
{
    char directoryTemplate[] = "/tmp/smu-updater-install-test-XXXXXX";
    char* createdDirectory = mkdtemp(directoryTemplate);
    Expect(createdDirectory != nullptr, "create install-failure updater test directory");
    const std::filesystem::path testDirectory(createdDirectory);
    const std::filesystem::path currentAppImage =
        testDirectory / "Current SMU.AppImage";
    const std::filesystem::path newAppImage =
        testDirectory / "New SMU.AppImage";
    const std::filesystem::path relaunchMarker =
        testDirectory / "previous-version-relaunched";
    const std::filesystem::path commandDirectory =
        testDirectory / "commands";
    const std::filesystem::path moveCount =
        testDirectory / "move-count";

    const std::string previousVersion =
        "#!/bin/sh\n"
        "touch \"" + relaunchMarker.string() + "\"\n";
    const std::string healthyUpdate =
        "#!/bin/sh\n"
        "sleep 4\n";
    WriteExecutable(currentAppImage, previousVersion);
    WriteExecutable(newAppImage, healthyUpdate);
    std::filesystem::create_directory(commandDirectory);

    const std::filesystem::path moveWrapper = commandDirectory / "mv";
    WriteExecutable(
        moveWrapper,
        "#!/bin/sh\n"
        "count=0\n"
        "if [ -f \"" + moveCount.string() + "\" ]; then\n"
        "    count=$(cat \"" + moveCount.string() + "\")\n"
        "fi\n"
        "count=$((count + 1))\n"
        "printf '%s\\n' \"$count\" > \"" + moveCount.string() + "\"\n"
        "if [ \"$count\" -eq 2 ]; then\n"
        "    exit 1\n"
        "fi\n"
        "exec /bin/mv \"$@\"\n");

    const char* inheritedPath = std::getenv("PATH");
    const std::string testPath =
        commandDirectory.string() + ":" +
        (inheritedPath ? inheritedPath : "/usr/bin:/bin");
    Expect(
        RunLinuxUpdaterScript(
            testDirectory,
            currentAppImage,
            newAppImage,
            testPath) == 1,
        "detect failure while moving the staged AppImage into place");
    Expect(
        ReadFile(currentAppImage) == previousVersion,
        "restore the previous AppImage after install move failure");
    Expect(
        WaitForPath(relaunchMarker),
        "relaunch the previous AppImage after install move failure");
    Expect(!HasUpdaterBackup(testDirectory), "consume backup after install move failure");

    std::error_code ec;
    std::filesystem::remove_all(testDirectory, ec);
    Expect(!ec, "remove install-failure updater test directory");
}

} // namespace

int main()
{
    TestAssetSelection();
    TestReleaseManifestContract();
    TestSha256();
    TestCrossPlatformAssetNames();
    TestBundleExtraction();
    TestAppImageAutoApplySupport();
    TestLinuxReplacementAndRollback();
    TestLinuxPreSwapFailureRelaunchesPreviousVersion();
    TestLinuxInstallFailureRestoresAndRelaunchesPreviousVersion();
    std::cout << "Linux updater tests passed.\n";
    return 0;
}
