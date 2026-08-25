#include "updater.h"

#include "json.hpp"
#include "miniz.h"

#include <algorithm>
#include <array>
#include <cctype>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <fstream>
#include <set>
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
constexpr const char* kLatestReleasePageUrl =
    "https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/latest";
constexpr const char* kTrustedReleaseAssetPrefix =
    "https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/download/";
constexpr const char* kUpdateManifestName = "update-manifest.json";
constexpr int kUpdateManifestSchemaVersion = 1;
constexpr std::size_t kMaximumManifestBytes = 1024ULL * 1024ULL;
constexpr std::size_t kMaximumUpdateBytes = 512ULL * 1024ULL * 1024ULL;

bool IsValidSha256(const std::string& value)
{
    if (value.size() != 64) {
        return false;
    }
    return std::all_of(value.begin(), value.end(), [](unsigned char ch) {
        return std::isxdigit(ch) != 0;
    });
}

std::string LowerHex(std::string value)
{
    std::transform(value.begin(), value.end(), value.begin(), [](unsigned char ch) {
        return static_cast<char>(std::tolower(ch));
    });
    return value;
}

bool IsTrustedReleaseAsset(const ReleaseAsset& asset)
{
    return asset.sizeBytes > 0 &&
        asset.sizeBytes <= kMaximumUpdateBytes &&
        IsValidSha256(asset.sha256) &&
        asset.downloadUrl.rfind(kTrustedReleaseAssetPrefix, 0) == 0;
}

class Sha256 {
public:
    Sha256()
        : state_ {0x6a09e667U, 0xbb67ae85U, 0x3c6ef372U, 0xa54ff53aU,
            0x510e527fU, 0x9b05688cU, 0x1f83d9abU, 0x5be0cd19U}
    {
    }

    void Update(const void* bytes, std::size_t size)
    {
        const auto* input = static_cast<const unsigned char*>(bytes);
        totalBytes_ += static_cast<std::uint64_t>(size);
        while (size > 0) {
            const std::size_t copySize = std::min(size, buffer_.size() - bufferSize_);
            std::memcpy(buffer_.data() + bufferSize_, input, copySize);
            bufferSize_ += copySize;
            input += copySize;
            size -= copySize;
            if (bufferSize_ == buffer_.size()) {
                Transform(buffer_.data());
                bufferSize_ = 0;
            }
        }
    }

    std::array<unsigned char, 32> Final()
    {
        const std::uint64_t totalBits = totalBytes_ * 8U;
        unsigned char padding[64] {0x80};
        const std::size_t paddingSize = bufferSize_ < 56
            ? 56 - bufferSize_
            : 64 + 56 - bufferSize_;
        Update(padding, paddingSize);

        unsigned char lengthBytes[8] {};
        for (int i = 0; i < 8; ++i) {
            lengthBytes[7 - i] = static_cast<unsigned char>((totalBits >> (i * 8)) & 0xffU);
        }
        Update(lengthBytes, sizeof(lengthBytes));

        std::array<unsigned char, 32> digest {};
        for (std::size_t i = 0; i < state_.size(); ++i) {
            digest[i * 4] = static_cast<unsigned char>((state_[i] >> 24) & 0xffU);
            digest[i * 4 + 1] = static_cast<unsigned char>((state_[i] >> 16) & 0xffU);
            digest[i * 4 + 2] = static_cast<unsigned char>((state_[i] >> 8) & 0xffU);
            digest[i * 4 + 3] = static_cast<unsigned char>(state_[i] & 0xffU);
        }
        return digest;
    }

private:
    static std::uint32_t RotateRight(std::uint32_t value, unsigned int count)
    {
        return (value >> count) | (value << (32U - count));
    }

    void Transform(const unsigned char* block)
    {
        static constexpr std::array<std::uint32_t, 64> k {
            0x428a2f98U, 0x71374491U, 0xb5c0fbcfU, 0xe9b5dba5U, 0x3956c25bU, 0x59f111f1U, 0x923f82a4U, 0xab1c5ed5U,
            0xd807aa98U, 0x12835b01U, 0x243185beU, 0x550c7dc3U, 0x72be5d74U, 0x80deb1feU, 0x9bdc06a7U, 0xc19bf174U,
            0xe49b69c1U, 0xefbe4786U, 0x0fc19dc6U, 0x240ca1ccU, 0x2de92c6fU, 0x4a7484aaU, 0x5cb0a9dcU, 0x76f988daU,
            0x983e5152U, 0xa831c66dU, 0xb00327c8U, 0xbf597fc7U, 0xc6e00bf3U, 0xd5a79147U, 0x06ca6351U, 0x14292967U,
            0x27b70a85U, 0x2e1b2138U, 0x4d2c6dfcU, 0x53380d13U, 0x650a7354U, 0x766a0abbU, 0x81c2c92eU, 0x92722c85U,
            0xa2bfe8a1U, 0xa81a664bU, 0xc24b8b70U, 0xc76c51a3U, 0xd192e819U, 0xd6990624U, 0xf40e3585U, 0x106aa070U,
            0x19a4c116U, 0x1e376c08U, 0x2748774cU, 0x34b0bcb5U, 0x391c0cb3U, 0x4ed8aa4aU, 0x5b9cca4fU, 0x682e6ff3U,
            0x748f82eeU, 0x78a5636fU, 0x84c87814U, 0x8cc70208U, 0x90befffaU, 0xa4506cebU, 0xbef9a3f7U, 0xc67178f2U};

        std::uint32_t words[64] {};
        for (std::size_t i = 0; i < 16; ++i) {
            words[i] =
                (static_cast<std::uint32_t>(block[i * 4]) << 24) |
                (static_cast<std::uint32_t>(block[i * 4 + 1]) << 16) |
                (static_cast<std::uint32_t>(block[i * 4 + 2]) << 8) |
                static_cast<std::uint32_t>(block[i * 4 + 3]);
        }
        for (std::size_t i = 16; i < 64; ++i) {
            const std::uint32_t s0 = RotateRight(words[i - 15], 7) ^ RotateRight(words[i - 15], 18) ^ (words[i - 15] >> 3);
            const std::uint32_t s1 = RotateRight(words[i - 2], 17) ^ RotateRight(words[i - 2], 19) ^ (words[i - 2] >> 10);
            words[i] = words[i - 16] + s0 + words[i - 7] + s1;
        }

        std::uint32_t a = state_[0];
        std::uint32_t b = state_[1];
        std::uint32_t c = state_[2];
        std::uint32_t d = state_[3];
        std::uint32_t e = state_[4];
        std::uint32_t f = state_[5];
        std::uint32_t g = state_[6];
        std::uint32_t h = state_[7];

        for (std::size_t i = 0; i < 64; ++i) {
            const std::uint32_t sigma1 = RotateRight(e, 6) ^ RotateRight(e, 11) ^ RotateRight(e, 25);
            const std::uint32_t choice = (e & f) ^ ((~e) & g);
            const std::uint32_t temp1 = h + sigma1 + choice + k[i] + words[i];
            const std::uint32_t sigma0 = RotateRight(a, 2) ^ RotateRight(a, 13) ^ RotateRight(a, 22);
            const std::uint32_t majority = (a & b) ^ (a & c) ^ (b & c);
            const std::uint32_t temp2 = sigma0 + majority;
            h = g;
            g = f;
            f = e;
            e = d + temp1;
            d = c;
            c = b;
            b = a;
            a = temp1 + temp2;
        }

        state_[0] += a;
        state_[1] += b;
        state_[2] += c;
        state_[3] += d;
        state_[4] += e;
        state_[5] += f;
        state_[6] += g;
        state_[7] += h;
    }

    std::array<std::uint32_t, 8> state_;
    std::array<unsigned char, 64> buffer_ {};
    std::size_t bufferSize_ = 0;
    std::uint64_t totalBytes_ = 0;
};

std::string HexDigest(const std::array<unsigned char, 32>& digest)
{
    static constexpr char kHex[] = "0123456789abcdef";
    std::string result;
    result.resize(digest.size() * 2);
    for (std::size_t i = 0; i < digest.size(); ++i) {
        result[i * 2] = kHex[digest[i] >> 4];
        result[i * 2 + 1] = kHex[digest[i] & 0x0f];
    }
    return result;
}

std::string Sha256HexBytes(const void* bytes, std::size_t size)
{
    Sha256 sha;
    if (size > 0) {
        sha.Update(bytes, size);
    }
    return HexDigest(sha.Final());
}

bool FileSha256Hex(const std::filesystem::path& path, std::string& digest, std::string* errorMessage)
{
    std::ifstream file(path, std::ios::binary);
    if (!file) {
        if (errorMessage) {
            *errorMessage = "Could not open downloaded update for SHA-256 verification.";
        }
        return false;
    }

    Sha256 sha;
    std::array<char, 64 * 1024> buffer {};
    while (file) {
        file.read(buffer.data(), static_cast<std::streamsize>(buffer.size()));
        const std::streamsize count = file.gcount();
        if (count > 0) {
            sha.Update(buffer.data(), static_cast<std::size_t>(count));
        }
    }
    if (!file.eof()) {
        if (errorMessage) {
            *errorMessage = "Failed while reading the downloaded update for SHA-256 verification.";
        }
        return false;
    }
    digest = HexDigest(sha.Final());
    return true;
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

bool IsCanonicalManifestVersion(const std::string& version)
{
    std::size_t start = 0;
    for (int component = 0; component < 3; ++component) {
        const std::size_t end = version.find('.', start);
        if ((component < 2 && end == std::string::npos) ||
            (component == 2 && end != std::string::npos)) {
            return false;
        }
        const std::size_t componentEnd = end == std::string::npos ? version.size() : end;
        if (componentEnd <= start || componentEnd - start > 9 ||
            (componentEnd - start > 1 && version[start] == '0')) {
            return false;
        }
        for (std::size_t i = start; i < componentEnd; ++i) {
            if (!std::isdigit(static_cast<unsigned char>(version[i]))) {
                return false;
            }
        }
        start = componentEnd + 1;
    }
    return start == version.size() + 1;
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

namespace detail {

std::string Sha256Hex(const std::vector<char>& data)
{
    return Sha256HexBytes(data.data(), data.size());
}

bool IsUpdaterVersionAllowed(const ReleaseInfo& release, const std::string& updaterVersion)
{
    return release.manifestDriven &&
        release.manifestSchemaVersion == kUpdateManifestSchemaVersion &&
        !release.minimumUpdaterVersion.empty() &&
        CompareVersions(NormalizeVersion(updaterVersion), release.minimumUpdaterVersion) >= 0;
}

std::optional<ReleaseInfo> ParseReleaseMetadataWithManifest(
    const std::string& releaseJson,
    const std::string& manifestJson,
    std::string* errorMessage)
{
    try {
        const auto root = nlohmann::json::parse(releaseJson);
        const auto manifest = nlohmann::json::parse(manifestJson);
        if (!root.is_object() || !manifest.is_object()) {
            if (errorMessage) {
                *errorMessage = "Updater release metadata or update manifest was not a JSON object.";
            }
            return std::nullopt;
        }

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

        if (!manifest.contains("schema_version") || !manifest["schema_version"].is_number_integer() ||
            manifest["schema_version"].get<int>() != kUpdateManifestSchemaVersion) {
            if (errorMessage) {
                *errorMessage = "Update manifest uses an unsupported schema_version.";
            }
            return std::nullopt;
        }
        release.manifestSchemaVersion = kUpdateManifestSchemaVersion;

        const std::string manifestReleaseVersion = manifest.value("release_version", "");
        if (!IsCanonicalManifestVersion(manifestReleaseVersion) || manifestReleaseVersion != release.version) {
            if (errorMessage) {
                *errorMessage = "Update manifest release_version must be canonical MAJOR.MINOR.PATCH and match the GitHub release tag.";
            }
            return std::nullopt;
        }

        const std::string minimumUpdaterVersion = manifest.value("minimum_updater_version", "");
        if (!IsCanonicalManifestVersion(minimumUpdaterVersion)) {
            if (errorMessage) {
                *errorMessage = "Update manifest minimum_updater_version must use canonical MAJOR.MINOR.PATCH.";
            }
            return std::nullopt;
        }
        release.minimumUpdaterVersion = minimumUpdaterVersion;

        if (!manifest.contains("artifacts") || !manifest["artifacts"].is_object() || manifest["artifacts"].empty()) {
            if (errorMessage) {
                *errorMessage = "Update manifest did not include an artifacts mapping.";
            }
            return std::nullopt;
        }
        if (!root.contains("assets") || !root["assets"].is_array()) {
            if (errorMessage) {
                *errorMessage = "Latest GitHub release did not include an assets array.";
            }
            return std::nullopt;
        }

        const std::string requiredUrlPrefix = std::string(kTrustedReleaseAssetPrefix) + release.tagName + "/";
        std::set<std::string> declaredAssetNames;
        for (auto it = manifest["artifacts"].begin(); it != manifest["artifacts"].end(); ++it) {
            const auto& artifactJson = it.value();
            if (!artifactJson.is_object()) {
                if (errorMessage) {
                    *errorMessage = "Update manifest artifact entries must be JSON objects.";
                }
                return std::nullopt;
            }

            const std::string assetName = artifactJson.value("asset", "");
            const std::string expectedSha256 = LowerHex(artifactJson.value("sha256", ""));
            if (assetName.empty() || assetName.find('/') != std::string::npos || assetName.find('\\') != std::string::npos ||
                assetName == "." || assetName == ".." || !IsValidSha256(expectedSha256)) {
                if (errorMessage) {
                    *errorMessage = "Update manifest contains an invalid asset name or SHA-256 digest.";
                }
                return std::nullopt;
            }
            if (!declaredAssetNames.insert(assetName).second) {
                if (errorMessage) {
                    *errorMessage = "Update manifest declares the same release asset more than once.";
                }
                return std::nullopt;
            }

            if (!artifactJson.contains("size") ||
                (!artifactJson["size"].is_number_unsigned() && !artifactJson["size"].is_number_integer())) {
                if (errorMessage) {
                    *errorMessage = "Update manifest artifact size must be a positive integer.";
                }
                return std::nullopt;
            }
            std::uint64_t manifestSize64 = 0;
            if (artifactJson["size"].is_number_unsigned()) {
                manifestSize64 = artifactJson["size"].get<std::uint64_t>();
            } else {
                const std::int64_t signedSize = artifactJson["size"].get<std::int64_t>();
                if (signedSize > 0) {
                    manifestSize64 = static_cast<std::uint64_t>(signedSize);
                }
            }
            if (manifestSize64 == 0 || manifestSize64 > kMaximumUpdateBytes) {
                if (errorMessage) {
                    *errorMessage = "Update manifest artifact size is outside the updater safety limits.";
                }
                return std::nullopt;
            }
            const std::size_t manifestSize = static_cast<std::size_t>(manifestSize64);

            const nlohmann::json* matchingAsset = nullptr;
            for (const auto& releaseAssetJson : root["assets"]) {
                if (!releaseAssetJson.is_object() || releaseAssetJson.value("name", "") != assetName) {
                    continue;
                }
                if (matchingAsset) {
                    if (errorMessage) {
                        *errorMessage = "GitHub release metadata contains duplicate entries for manifest asset: " + assetName;
                    }
                    return std::nullopt;
                }
                matchingAsset = &releaseAssetJson;
            }
            if (!matchingAsset) {
                if (errorMessage) {
                    *errorMessage = "Update manifest references a release asset that is missing from GitHub: " + assetName;
                }
                return std::nullopt;
            }

            const std::string downloadUrl = matchingAsset->value("browser_download_url", "");
            const std::uint64_t githubSize = matchingAsset->value("size", std::uint64_t {0});
            if (downloadUrl.rfind(requiredUrlPrefix, 0) != 0 || githubSize != manifestSize) {
                if (errorMessage) {
                    *errorMessage = "Update manifest does not match GitHub metadata for asset: " + assetName;
                }
                return std::nullopt;
            }

            const std::string explicitUrl = artifactJson.value("url", "");
            if (!explicitUrl.empty() && explicitUrl != downloadUrl) {
                if (errorMessage) {
                    *errorMessage = "Update manifest URL does not match the official GitHub release asset URL: " + assetName;
                }
                return std::nullopt;
            }

            release.assets.push_back({assetName, downloadUrl, manifestSize, expectedSha256});
        }

        release.manifestDriven = true;
        return release;
    } catch (const std::exception& ex) {
        if (errorMessage) {
            *errorMessage = std::string("Failed to parse updater release contract: ") + ex.what();
        }
        return std::nullopt;
    }
}

} // namespace detail

const char* LatestReleasePageUrl()
{
    return kLatestReleasePageUrl;
}

std::optional<ReleaseInfo> FetchLatestRelease(std::string* errorMessage)
{
    std::string response;
    if (!detail::HttpGetString(kLatestReleaseUrl, response, errorMessage)) {
        return std::nullopt;
    }

    try {
        const auto root = nlohmann::json::parse(response);
        if (!root.is_object()) {
            if (errorMessage) {
                *errorMessage = "Latest GitHub release metadata was not a JSON object.";
            }
            return std::nullopt;
        }
        const std::string tagName = root.value("tag_name", "");
        if (tagName.empty() || !root.contains("assets") || !root["assets"].is_array()) {
            if (errorMessage) {
                *errorMessage = "Latest GitHub release metadata is incomplete.";
            }
            return std::nullopt;
        }

        const std::string requiredUrlPrefix = std::string(kTrustedReleaseAssetPrefix) + tagName + "/";
        const nlohmann::json* manifestAsset = nullptr;
        for (const auto& assetJson : root["assets"]) {
            if (!assetJson.is_object() || assetJson.value("name", "") != kUpdateManifestName) {
                continue;
            }
            if (manifestAsset) {
                if (errorMessage) {
                    *errorMessage = "Latest release contains more than one update-manifest.json asset.";
                }
                return std::nullopt;
            }
            manifestAsset = &assetJson;
        }
        if (!manifestAsset) {
            if (errorMessage) {
                *errorMessage = "Latest release does not include update-manifest.json; this updater requires the V3.4 release contract.";
            }
            return std::nullopt;
        }

        const std::string manifestUrl = manifestAsset->value("browser_download_url", "");
        const std::uint64_t manifestSize = manifestAsset->value("size", std::uint64_t {0});
        if (manifestUrl.rfind(requiredUrlPrefix, 0) != 0 || manifestSize == 0 || manifestSize > kMaximumManifestBytes) {
            if (errorMessage) {
                *errorMessage = "Updater refused an invalid or oversized update-manifest.json asset.";
            }
            return std::nullopt;
        }

        std::string manifestResponse;
        if (!detail::HttpGetString(manifestUrl, manifestResponse, errorMessage)) {
            return std::nullopt;
        }
        if (manifestResponse.size() != manifestSize || manifestResponse.size() > kMaximumManifestBytes) {
            if (errorMessage) {
                *errorMessage = "Downloaded update-manifest.json size did not match GitHub release metadata.";
            }
            return std::nullopt;
        }
        return detail::ParseReleaseMetadataWithManifest(response, manifestResponse, errorMessage);
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
    const bool updaterVersionAllowed = detail::IsUpdaterVersionAllowed(*latest, status.localVersion);
    if (updaterVersionAllowed) {
        status.selectedAsset = SelectUpdateAsset(*latest);
    }
    status.latestRelease = std::move(latest);

    if (!status.updateAvailable) {
        status.message = "You are running the latest version.";
    } else if (!updaterVersionAllowed) {
        status.autoApplySupported = false;
        status.message = "An update is available, but it requires a newer updater. Install it manually from the official latest release page.";
    } else if (!status.selectedAsset) {
        status.message = "An update is available, but the manifest does not declare a matching package for this platform.";
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
            *errorMessage = "Downloaded update size did not match update-manifest.json.";
        }
        return false;
    }

    std::string actualSha256;
    if (!FileSha256Hex(destination, actualSha256, errorMessage) ||
        actualSha256 != LowerHex(asset.sha256)) {
        std::error_code removeError;
        std::filesystem::remove(destination, removeError);
        if (errorMessage && !actualSha256.empty()) {
            *errorMessage = "Downloaded update SHA-256 did not match update-manifest.json.";
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
            *errorMessage = "Downloaded update size did not match update-manifest.json.";
        }
        return false;
    }
    if (detail::Sha256Hex(data) != LowerHex(asset.sha256)) {
        data.clear();
        if (errorMessage) {
            *errorMessage = "Downloaded update SHA-256 did not match update-manifest.json.";
        }
        return false;
    }
    return true;
}

bool ExtractUpdatePackageEntry(
    const std::vector<char>& packageBytes,
    const std::string& entryPathToExtract,
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

    bool safePath = !entryPathToExtract.empty() &&
        entryPathToExtract.size() <= 1024 &&
        entryPathToExtract.front() != '/' &&
        entryPathToExtract.back() != '/' &&
        entryPathToExtract.find('\\') == std::string::npos;
    if (safePath) {
        std::size_t segmentStart = 0;
        while (segmentStart < entryPathToExtract.size()) {
            const std::size_t slash = entryPathToExtract.find('/', segmentStart);
            const std::size_t segmentEnd = slash == std::string::npos
                ? entryPathToExtract.size()
                : slash;
            const std::string segment = entryPathToExtract.substr(segmentStart, segmentEnd - segmentStart);
            if (segment.empty() || segment == "." || segment == "..") {
                safePath = false;
                break;
            }
            if (slash == std::string::npos) {
                break;
            }
            segmentStart = slash + 1;
        }
    }
    if (!safePath) {
        if (errorMessage) {
            *errorMessage = "Updater refused an unsafe package entry path.";
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

    const int fileIndex = mz_zip_reader_locate_file(&zipArchive, entryPathToExtract.c_str(), nullptr, 0);
    if (fileIndex < 0) {
        mz_zip_reader_end(&zipArchive);
        if (errorMessage) {
            *errorMessage = "The update package did not contain " + entryPathToExtract + ".";
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
            *errorMessage = "Failed to extract " + entryPathToExtract + " from the update package.";
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

bool ExtractUpdatePackage(
    const std::vector<char>& packageBytes,
    const std::string& fileNameToExtract,
    std::vector<char>& extractedData,
    std::string* errorMessage)
{
    if (fileNameToExtract.empty() ||
        fileNameToExtract.find('/') != std::string::npos ||
        fileNameToExtract.find('\\') != std::string::npos ||
        fileNameToExtract == "." ||
        fileNameToExtract == "..") {
        extractedData.clear();
        if (errorMessage) {
            *errorMessage = "Updater refused an unsafe package file name.";
        }
        return false;
    }
    return ExtractUpdatePackageEntry(packageBytes, fileNameToExtract, extractedData, errorMessage);
}

bool ApplyUpdate(const ReleaseInfo& release, const std::string& localVersion, std::string* errorMessage)
{
    if (!detail::IsUpdaterVersionAllowed(release, localVersion)) {
        if (errorMessage) {
            *errorMessage = "Updater refused to apply a release without a compatible V3.4 update manifest.";
        }
        return false;
    }

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
