#include "updater.h"

#include "json.hpp"
#include "miniz.h"

#include <algorithm>
#include <cctype>
#include <cstdint>
#include <cstdlib>
#include <exception>
#include <sstream>
#include <utility>

namespace smu::updater {
namespace detail {

bool HttpGetString(const std::string& url, std::string& output, std::string* errorMessage);
bool DownloadUrlToFile(const std::string& url, const std::filesystem::path& destination, std::string* errorMessage);
bool DownloadUrlToMemory(const std::string& url, std::vector<char>& data, std::string* errorMessage);
int ScoreAssetForCurrentPlatform(const ReleaseAsset& asset);
bool ApplyUpdateFromAsset(const ReleaseAsset& asset, const std::string& newVersion, const std::string& localVersion, std::string* errorMessage);
bool PlatformAutoApplySupported();

} // namespace detail
namespace {

constexpr const char* kLatestReleaseUrl =
    "https://api.github.com/repos/Spencer0187/Spencer-Macro-Utilities/releases/latest";
constexpr const char* kTrustedReleaseAssetPrefix =
    "https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/download/";
constexpr std::size_t kMaximumUpdateBytes = 512ULL * 1024ULL * 1024ULL;

bool IsTrustedReleaseAsset(const ReleaseAsset& asset)
{
    return asset.sizeBytes > 0 &&
        asset.sizeBytes <= kMaximumUpdateBytes &&
        asset.downloadUrl.rfind(kTrustedReleaseAssetPrefix, 0) == 0;
}

std::string Trim(std::string value)
{
    const auto first = std::find_if_not(value.begin(), value.end(), [](unsigned char ch) {
        return std::isspace(ch) != 0;
    });
    const auto last = std::find_if_not(value.rbegin(), value.rend(), [](unsigned char ch) {
        return std::isspace(ch) != 0;
    }).base();
    if (first >= last) {
        return {};
    }
    return std::string(first, last);
}

std::vector<int> VersionParts(const std::string& version)
{
    std::vector<int> parts;
    std::string current;
    for (char ch : version) {
        if (std::isdigit(static_cast<unsigned char>(ch))) {
            current += ch;
        } else if (!current.empty()) {
            parts.push_back(std::atoi(current.c_str()));
            current.clear();
        }
    }
    if (!current.empty()) {
        parts.push_back(std::atoi(current.c_str()));
    }
    return parts;
}

} // namespace

std::string NormalizeVersion(std::string version)
{
    version = Trim(std::move(version));
    if (!version.empty() && (version.front() == 'v' || version.front() == 'V')) {
        version.erase(version.begin());
    }
    return Trim(std::move(version));
}

int CompareVersions(const std::string& lhs, const std::string& rhs)
{
    const std::vector<int> left = VersionParts(NormalizeVersion(lhs));
    const std::vector<int> right = VersionParts(NormalizeVersion(rhs));
    const std::size_t count = std::max(left.size(), right.size());
    for (std::size_t i = 0; i < count; ++i) {
        const int l = i < left.size() ? left[i] : 0;
        const int r = i < right.size() ? right[i] : 0;
        if (l != r) {
            return l < r ? -1 : 1;
        }
    }
    return NormalizeVersion(lhs) == NormalizeVersion(rhs) ? 0 : NormalizeVersion(lhs).compare(NormalizeVersion(rhs));
}

std::optional<ReleaseInfo> FetchLatestRelease(std::string* errorMessage)
{
    std::string response;
    if (!detail::HttpGetString(kLatestReleaseUrl, response, errorMessage)) {
        return std::nullopt;
    }

    try {
        const auto root = nlohmann::json::parse(response);
        ReleaseInfo release;
        release.tagName = root.value("tag_name", "");
        release.version = NormalizeVersion(release.tagName);
        release.htmlUrl = root.value("html_url", "");

        if (release.version.empty()) {
            if (errorMessage) {
                *errorMessage = "Latest GitHub release did not include a tag_name.";
            }
            return std::nullopt;
        }

        if (root.contains("assets") && root["assets"].is_array()) {
            for (const auto& assetJson : root["assets"]) {
                ReleaseAsset asset;
                asset.name = assetJson.value("name", "");
                asset.downloadUrl = assetJson.value("browser_download_url", "");
                const std::uint64_t sizeBytes = assetJson.value("size", std::uint64_t {0});
                if (sizeBytes <= kMaximumUpdateBytes) {
                    asset.sizeBytes = static_cast<std::size_t>(sizeBytes);
                }
                if (!asset.name.empty() &&
                    !asset.downloadUrl.empty() &&
                    IsTrustedReleaseAsset(asset)) {
                    release.assets.push_back(std::move(asset));
                }
            }
        }

        return release;
    } catch (const std::exception& ex) {
        if (errorMessage) {
            *errorMessage = std::string("Failed to parse latest GitHub release JSON: ") + ex.what();
        }
        return std::nullopt;
    }
}

std::optional<ReleaseAsset> SelectUpdateAsset(const ReleaseInfo& release)
{
    const ReleaseAsset* bestAsset = nullptr;
    int bestScore = 0;
    for (const ReleaseAsset& asset : release.assets) {
        const int score = detail::ScoreAssetForCurrentPlatform(asset);
        if (score > bestScore) {
            bestScore = score;
            bestAsset = &asset;
        }
    }

    if (!bestAsset) {
        return std::nullopt;
    }
    return *bestAsset;
}

UpdaterStatus CheckForUpdate(const std::string& localVersion)
{
    UpdaterStatus status;
    status.localVersion = NormalizeVersion(localVersion);
    status.autoApplySupported = IsAutoApplySupported();

    std::string error;
    auto latest = FetchLatestRelease(&error);
    if (!latest) {
        status.message = error.empty() ? "Update check failed." : error;
        return status;
    }

    status.checkSucceeded = true;
    status.latestVersion = latest->version;
    status.updateAvailable = CompareVersions(status.localVersion, status.latestVersion) < 0;
    status.selectedAsset = SelectUpdateAsset(*latest);
    status.latestRelease = std::move(latest);

    if (!status.updateAvailable) {
        status.message = "You are running the latest version.";
    } else if (!status.selectedAsset) {
        status.message = "An update is available, but no matching package asset was found for this platform.";
    } else if (!status.autoApplySupported) {
        status.message = "An update is available, but automatic installation is not available in this launch mode.";
    } else {
        status.message = "An update is available.";
    }

    return status;
}

bool DownloadAssetToFile(const ReleaseAsset& asset, const std::filesystem::path& destination, std::string* errorMessage)
{
    if (!IsTrustedReleaseAsset(asset)) {
        if (errorMessage) {
            *errorMessage = "Updater refused an untrusted or oversized release asset.";
        }
        return false;
    }
    if (!detail::DownloadUrlToFile(asset.downloadUrl, destination, errorMessage)) {
        return false;
    }

    std::error_code ec;
    const std::uintmax_t downloadedSize = std::filesystem::file_size(destination, ec);
    if (ec || downloadedSize != asset.sizeBytes) {
        std::error_code removeError;
        std::filesystem::remove(destination, removeError);
        if (errorMessage) {
            *errorMessage = "Downloaded update size did not match the GitHub release metadata.";
        }
        return false;
    }
    return true;
}

bool DownloadAssetToMemory(const ReleaseAsset& asset, std::vector<char>& data, std::string* errorMessage)
{
    if (!IsTrustedReleaseAsset(asset)) {
        if (errorMessage) {
            *errorMessage = "Updater refused an untrusted or oversized release asset.";
        }
        return false;
    }
    if (!detail::DownloadUrlToMemory(asset.downloadUrl, data, errorMessage)) {
        return false;
    }
    if (data.size() != asset.sizeBytes) {
        data.clear();
        if (errorMessage) {
            *errorMessage = "Downloaded update size did not match the GitHub release metadata.";
        }
        return false;
    }
    return true;
}

bool ExtractUpdatePackage(
    const std::vector<char>& packageBytes,
    const std::string& fileNameToExtract,
    std::vector<char>& extractedData,
    std::string* errorMessage)
{
    extractedData.clear();
    if (packageBytes.empty() || packageBytes.size() > kMaximumUpdateBytes) {
        if (errorMessage) {
            *errorMessage = "Downloaded update package was empty or exceeded the 512 MiB safety limit.";
        }
        return false;
    }
    if (fileNameToExtract.empty() ||
        fileNameToExtract.find('/') != std::string::npos ||
        fileNameToExtract.find('\\') != std::string::npos ||
        fileNameToExtract == "." ||
        fileNameToExtract == "..") {
        if (errorMessage) {
            *errorMessage = "Updater refused an unsafe package file name.";
        }
        return false;
    }

    mz_zip_archive zipArchive;
    mz_zip_zero_struct(&zipArchive);

    if (!mz_zip_reader_init_mem(&zipArchive, packageBytes.data(), packageBytes.size(), 0)) {
        if (errorMessage) {
            *errorMessage = "Failed to open downloaded update package as a ZIP archive.";
        }
        return false;
    }

    const int fileIndex = mz_zip_reader_locate_file(&zipArchive, fileNameToExtract.c_str(), nullptr, 0);
    if (fileIndex < 0) {
        mz_zip_reader_end(&zipArchive);
        if (errorMessage) {
            *errorMessage = "The update package did not contain " + fileNameToExtract + ".";
        }
        return false;
    }

    mz_zip_archive_file_stat fileStat {};
    if (!mz_zip_reader_file_stat(
            &zipArchive,
            static_cast<mz_uint>(fileIndex),
            &fileStat) ||
        fileStat.m_is_directory ||
        fileStat.m_is_encrypted ||
        fileStat.m_uncomp_size == 0 ||
        fileStat.m_uncomp_size > kMaximumUpdateBytes) {
        mz_zip_reader_end(&zipArchive);
        if (errorMessage) {
            *errorMessage = "The requested update file was empty, encrypted, or too large.";
        }
        return false;
    }

    size_t uncompressedSize = 0;
    void* buffer = mz_zip_reader_extract_to_heap(&zipArchive, fileIndex, &uncompressedSize, 0);
    if (!buffer) {
        mz_zip_reader_end(&zipArchive);
        if (errorMessage) {
            *errorMessage = "Failed to extract " + fileNameToExtract + " from the update package.";
        }
        return false;
    }

    extractedData.assign(
        static_cast<const char*>(buffer),
        static_cast<const char*>(buffer) + uncompressedSize);
    mz_free(buffer);
    mz_zip_reader_end(&zipArchive);
    return true;
}

bool ApplyUpdate(const ReleaseInfo& release, const std::string& localVersion, std::string* errorMessage)
{
    const auto asset = SelectUpdateAsset(release);
    if (!asset) {
        if (errorMessage) {
            *errorMessage = "No matching update package asset was found for this platform.";
        }
        return false;
    }
    if (!IsTrustedReleaseAsset(*asset)) {
        if (errorMessage) {
            *errorMessage = "Updater refused an untrusted or oversized release asset.";
        }
        return false;
    }

    return detail::ApplyUpdateFromAsset(*asset, release.version, NormalizeVersion(localVersion), errorMessage);
}

bool IsAutoApplySupported()
{
    return detail::PlatformAutoApplySupported();
}

} // namespace smu::updater
