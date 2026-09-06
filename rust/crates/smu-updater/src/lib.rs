//! Update-contract parsing and platform package selection for SMU.
//!
//! Network transport and platform-specific replacement are deliberately kept out of this crate's
//! core contract API. That lets the updater validate untrusted release metadata before any install
//! operation is attempted and keeps HTTP transport replaceable.

mod check;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod linux_install;
#[cfg(target_os = "windows")]
mod windows;

pub use check::{
    LATEST_RELEASE_API_URL, UpdateCheckError, UpdateChecker, UpdateTransport, UpdateTransportError,
    UpdaterStatus,
};
#[cfg(target_os = "linux")]
pub use linux::CurlUpdateTransport;
#[cfg(target_os = "linux")]
pub use linux_install::{
    LinuxUpdateError, LinuxUpdateLaunch, apply_linux_update, linux_auto_apply_supported,
};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fmt;
#[cfg(target_os = "windows")]
pub use windows::WinInetUpdateTransport;

pub const LATEST_RELEASE_PAGE_URL: &str =
    "https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/latest";
const TRUSTED_RELEASE_ASSET_PREFIX: &str =
    "https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/download/";
const UPDATE_MANIFEST_SCHEMA_VERSION: i32 = 1;
pub const MAXIMUM_MANIFEST_BYTES: u64 = 1024 * 1024;
pub const MAXIMUM_UPDATE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReleaseAsset {
    pub name: String,
    pub download_url: String,
    pub size_bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ReleaseInfo {
    pub tag_name: String,
    pub version: String,
    pub html_url: String,
    pub manifest_schema_version: i32,
    pub minimum_updater_version: String,
    pub manifest_driven: bool,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LinuxArchitecture {
    #[default]
    Unknown,
    X86_64,
    AArch64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UpdatePlatform {
    Windows,
    MacOs,
    Linux(LinuxArchitecture),
    Unsupported,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdaterContractError(String);

impl UpdaterContractError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for UpdaterContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for UpdaterContractError {}

#[must_use]
pub fn normalize_version(version: &str) -> String {
    let mut normalized = version.trim_matches(|character: char| character.is_ascii_whitespace());
    if normalized.starts_with(['v', 'V']) {
        normalized = normalized.get(1..).unwrap_or_default();
    }
    normalized.trim_matches(|character: char| character.is_ascii_whitespace()).to_owned()
}

fn version_parts(version: &str) -> Vec<i32> {
    let mut parts = Vec::new();
    let mut current = String::new();
    for character in version.chars() {
        if character.is_ascii_digit() {
            current.push(character);
        } else if !current.is_empty() {
            parts.push(parse_cpp_atoi_component(&current));
            current.clear();
        }
    }
    if !current.is_empty() {
        parts.push(parse_cpp_atoi_component(&current));
    }
    parts
}

fn parse_cpp_atoi_component(value: &str) -> i32 {
    let mut number = 0_i32;
    for byte in value.bytes() {
        number = number.wrapping_mul(10).wrapping_add(i32::from(byte - b'0'));
    }
    number
}

#[must_use]
pub fn compare_versions(lhs: &str, rhs: &str) -> Ordering {
    let left_normalized = normalize_version(lhs);
    let right_normalized = normalize_version(rhs);
    let left = version_parts(&left_normalized);
    let right = version_parts(&right_normalized);
    let count = left.len().max(right.len());
    for index in 0..count {
        let left_part = left.get(index).copied().unwrap_or_default();
        let right_part = right.get(index).copied().unwrap_or_default();
        match left_part.cmp(&right_part) {
            Ordering::Equal => {}
            ordering => return ordering,
        }
    }
    left_normalized.cmp(&right_normalized)
}

#[must_use]
pub fn is_canonical_manifest_version(version: &str) -> bool {
    let mut components = version.split('.');
    for _ in 0..3 {
        let Some(component) = components.next() else {
            return false;
        };
        if component.is_empty()
            || component.len() > 9
            || (component.len() > 1 && component.starts_with('0'))
            || !component.bytes().all(|byte| byte.is_ascii_digit())
        {
            return false;
        }
    }
    components.next().is_none()
}

fn json_string(object: &Value, key: &str) -> Result<String, UpdaterContractError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(value)) => Ok(value.clone()),
        Some(_) => Err(UpdaterContractError::new(format!(
            "Updater JSON field `{key}` had the wrong type."
        ))),
    }
}

fn json_u64_or_zero(object: &Value, key: &str) -> Result<u64, UpdaterContractError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(0),
        Some(Value::Number(value)) => value.as_u64().ok_or_else(|| {
            UpdaterContractError::new(format!("Updater JSON field `{key}` had the wrong type."))
        }),
        Some(_) => Err(UpdaterContractError::new(format!(
            "Updater JSON field `{key}` had the wrong type."
        ))),
    }
}

fn parse_json(source: &str, context: &str) -> Result<Value, UpdaterContractError> {
    serde_json::from_str(source)
        .map_err(|error| UpdaterContractError::new(format!("Failed to parse {context}: {error}")))
}

/// Parse the public GitHub release fields needed to decide whether an update exists.
///
/// # Errors
///
/// Returns an error when the JSON shape or release tag violates the frozen updater contract.
pub fn parse_release_metadata_summary(
    release_json: &str,
) -> Result<ReleaseInfo, UpdaterContractError> {
    let root = parse_json(release_json, "latest GitHub release JSON")?;
    if !root.is_object() {
        return Err(UpdaterContractError::new(
            "Latest GitHub release metadata was not a JSON object.",
        ));
    }

    let tag_name = json_string(&root, "tag_name")?;
    let version = normalize_version(&tag_name);
    let html_url = json_string(&root, "html_url")?;
    if !is_canonical_manifest_version(&version) {
        return Err(UpdaterContractError::new(
            "Latest GitHub release tag must use canonical MAJOR.MINOR.PATCH versioning.",
        ));
    }
    if !root.get("assets").is_some_and(Value::is_array) {
        return Err(UpdaterContractError::new("Latest GitHub release metadata is incomplete."));
    }

    Ok(ReleaseInfo { tag_name, version, html_url, ..ReleaseInfo::default() })
}

fn is_valid_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn manifest_artifact_size(artifact: &Value) -> Result<u64, UpdaterContractError> {
    let Some(size) = artifact.get("size") else {
        return Err(UpdaterContractError::new(
            "Update manifest artifact size must be a positive integer.",
        ));
    };
    let Value::Number(size) = size else {
        return Err(UpdaterContractError::new(
            "Update manifest artifact size must be a positive integer.",
        ));
    };
    let parsed = size.as_u64().unwrap_or_default();
    if parsed == 0 || parsed > MAXIMUM_UPDATE_BYTES {
        return Err(UpdaterContractError::new(
            "Update manifest artifact size is outside the updater safety limits.",
        ));
    }
    Ok(parsed)
}

fn parse_manifest_assets(
    root: &Value,
    artifacts: &serde_json::Map<String, Value>,
    required_url_prefix: &str,
) -> Result<Vec<ReleaseAsset>, UpdaterContractError> {
    let Some(release_assets) = root.get("assets").and_then(Value::as_array) else {
        return Err(UpdaterContractError::new(
            "Latest GitHub release did not include an assets array.",
        ));
    };
    let mut declared_asset_names = BTreeSet::new();
    let mut parsed_assets = Vec::new();

    for artifact in artifacts.values() {
        if !artifact.is_object() {
            return Err(UpdaterContractError::new(
                "Update manifest artifact entries must be JSON objects.",
            ));
        }
        let asset_name = json_string(artifact, "asset")?;
        let expected_sha256 = json_string(artifact, "sha256")?.to_ascii_lowercase();
        if asset_name.is_empty()
            || asset_name.contains(['/', '\\'])
            || matches!(asset_name.as_str(), "." | "..")
            || !is_valid_sha256(&expected_sha256)
        {
            return Err(UpdaterContractError::new(
                "Update manifest contains an invalid asset name or SHA-256 digest.",
            ));
        }
        if !declared_asset_names.insert(asset_name.clone()) {
            return Err(UpdaterContractError::new(
                "Update manifest declares the same release asset more than once.",
            ));
        }

        let manifest_size = manifest_artifact_size(artifact)?;
        let mut matching_asset: Option<&Value> = None;
        for release_asset in release_assets {
            if !release_asset.is_object() || json_string(release_asset, "name")? != asset_name {
                continue;
            }
            if matching_asset.is_some() {
                return Err(UpdaterContractError::new(format!(
                    "GitHub release metadata contains duplicate entries for manifest asset: {asset_name}"
                )));
            }
            matching_asset = Some(release_asset);
        }
        let Some(matching_asset) = matching_asset else {
            return Err(UpdaterContractError::new(format!(
                "Update manifest references a release asset that is missing from GitHub: {asset_name}"
            )));
        };

        let download_url = json_string(matching_asset, "browser_download_url")?;
        let github_size = json_u64_or_zero(matching_asset, "size")?;
        if !download_url.starts_with(required_url_prefix) || github_size != manifest_size {
            return Err(UpdaterContractError::new(format!(
                "Update manifest does not match GitHub metadata for asset: {asset_name}"
            )));
        }

        let explicit_url = json_string(artifact, "url")?;
        if !explicit_url.is_empty() && explicit_url != download_url {
            return Err(UpdaterContractError::new(format!(
                "Update manifest URL does not match the official GitHub release asset URL: {asset_name}"
            )));
        }
        parsed_assets.push(ReleaseAsset {
            name: asset_name,
            download_url,
            size_bytes: manifest_size,
            sha256: expected_sha256,
        });
    }
    Ok(parsed_assets)
}

/// Validate a GitHub release together with its V3.4 update manifest.
///
/// # Errors
///
/// Returns an error when either JSON document is malformed, inconsistent, unsafe, or outside the
/// size/version constraints frozen by the C++ updater contract.
pub fn parse_release_metadata_with_manifest(
    release_json: &str,
    manifest_json: &str,
) -> Result<ReleaseInfo, UpdaterContractError> {
    let root = parse_json(release_json, "updater release contract")?;
    let manifest = parse_json(manifest_json, "updater release contract")?;
    if !root.is_object() || !manifest.is_object() {
        return Err(UpdaterContractError::new(
            "Updater release metadata or update manifest was not a JSON object.",
        ));
    }

    let tag_name = json_string(&root, "tag_name")?;
    let version = normalize_version(&tag_name);
    let html_url = json_string(&root, "html_url")?;
    if version.is_empty() {
        return Err(UpdaterContractError::new("Latest GitHub release did not include a tag_name."));
    }

    let schema_version = manifest.get("schema_version").and_then(Value::as_i64);
    if schema_version != Some(i64::from(UPDATE_MANIFEST_SCHEMA_VERSION)) {
        return Err(UpdaterContractError::new(
            "Update manifest uses an unsupported schema_version.",
        ));
    }

    let manifest_release_version = json_string(&manifest, "release_version")?;
    if !is_canonical_manifest_version(&manifest_release_version)
        || manifest_release_version != version
    {
        return Err(UpdaterContractError::new(
            "Update manifest release_version must be canonical MAJOR.MINOR.PATCH and match the GitHub release tag.",
        ));
    }
    let minimum_updater_version = json_string(&manifest, "minimum_updater_version")?;
    if !is_canonical_manifest_version(&minimum_updater_version) {
        return Err(UpdaterContractError::new(
            "Update manifest minimum_updater_version must use canonical MAJOR.MINOR.PATCH.",
        ));
    }

    let Some(artifacts) = manifest.get("artifacts").and_then(Value::as_object) else {
        return Err(UpdaterContractError::new(
            "Update manifest did not include an artifacts mapping.",
        ));
    };
    if artifacts.is_empty() {
        return Err(UpdaterContractError::new(
            "Update manifest did not include an artifacts mapping.",
        ));
    }

    let required_url_prefix = format!("{TRUSTED_RELEASE_ASSET_PREFIX}{tag_name}/");
    let assets = parse_manifest_assets(&root, artifacts, &required_url_prefix)?;
    Ok(ReleaseInfo {
        tag_name,
        version,
        html_url,
        manifest_schema_version: UPDATE_MANIFEST_SCHEMA_VERSION,
        minimum_updater_version,
        manifest_driven: true,
        assets,
    })
}

#[must_use]
pub fn is_updater_version_allowed(release: &ReleaseInfo, updater_version: &str) -> bool {
    release.manifest_driven
        && release.manifest_schema_version == UPDATE_MANIFEST_SCHEMA_VERSION
        && !release.minimum_updater_version.is_empty()
        && compare_versions(&normalize_version(updater_version), &release.minimum_updater_version)
            != Ordering::Less
}

fn lower_asset_name(value: &str) -> String {
    value.to_ascii_lowercase()
}

fn asset_name_has_extension(value: &str, extension: &str) -> bool {
    std::path::Path::new(value)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|actual| actual.eq_ignore_ascii_case(extension))
}

#[must_use]
pub fn score_windows_asset_name(asset_name: &str) -> i32 {
    let name = lower_asset_name(asset_name);
    if !asset_name_has_extension(&name, "zip") {
        return 0;
    }
    if ["linux", "appimage", "macos", "mac-os", "darwin", "osx", "universal"]
        .iter()
        .any(|needle| name.contains(needle))
    {
        return 0;
    }

    let mut score = 10;
    if ["windows", "win64", "win32", "win-x64", "win_x64"]
        .iter()
        .any(|needle| name.contains(needle))
    {
        score += 50;
    }
    if name.contains("x64") || name.contains("amd64") {
        score += 10;
    }
    if name.contains("spencer") || name.contains("macro") {
        score += 8;
    }
    score
}

#[must_use]
pub fn score_macos_asset_name(asset_name: &str) -> i32 {
    let name = lower_asset_name(asset_name);
    if !asset_name_has_extension(&name, "zip") {
        return 0;
    }
    if ["windows", "win64", "win32", "win-x64", "win_x64", "linux", "appimage"]
        .iter()
        .any(|needle| name.contains(needle))
    {
        return 0;
    }
    let names_macos = ["macos", "mac-os", "darwin", "osx", "universal"]
        .iter()
        .any(|needle| name.contains(needle));
    if !names_macos {
        return 0;
    }

    let mut score = 80;
    if name.contains("universal") {
        score += 20;
    }
    if name.contains("spencer") || name.contains("macro") {
        score += 8;
    }
    score
}

fn asset_targets_different_linux_architecture(
    lower_name: &str,
    architecture: LinuxArchitecture,
) -> bool {
    match architecture {
        LinuxArchitecture::X86_64 => lower_name.contains("aarch64") || lower_name.contains("arm64"),
        LinuxArchitecture::AArch64 => {
            ["x86_64", "x86-64", "amd64", "x64"].iter().any(|needle| lower_name.contains(needle))
        }
        LinuxArchitecture::Unknown => false,
    }
}

#[must_use]
pub fn score_linux_asset_name(asset_name: &str, architecture: LinuxArchitecture) -> i32 {
    let name = lower_asset_name(asset_name);
    let app_image = asset_name_has_extension(&name, "appimage");
    let names_different_platform =
        ["windows", "win64", "win32", "win-x64", "win_x64", "macos", "mac-os", "darwin", "osx"]
            .iter()
            .any(|needle| name.contains(needle));
    let distribution_bundle = asset_name_has_extension(&name, "zip")
        && name.contains("linux")
        && !names_different_platform;
    if (!app_image && !distribution_bundle)
        || names_different_platform
        || asset_targets_different_linux_architecture(&name, architecture)
    {
        return 0;
    }

    let mut score = if app_image { 300 } else { 200 };
    if name.contains("spencer") || name.contains("macro") {
        score += 10;
    }
    score
}

#[must_use]
pub const fn current_platform() -> UpdatePlatform {
    #[cfg(target_os = "windows")]
    {
        return UpdatePlatform::Windows;
    }
    #[cfg(target_os = "macos")]
    {
        return UpdatePlatform::MacOs;
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        return UpdatePlatform::Linux(LinuxArchitecture::X86_64);
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        return UpdatePlatform::Linux(LinuxArchitecture::AArch64);
    }
    #[cfg(all(target_os = "linux", not(any(target_arch = "x86_64", target_arch = "aarch64"))))]
    {
        return UpdatePlatform::Linux(LinuxArchitecture::Unknown);
    }
    #[allow(unreachable_code)]
    UpdatePlatform::Unsupported
}

#[must_use]
pub fn score_asset_for_platform(asset_name: &str, platform: UpdatePlatform) -> i32 {
    match platform {
        UpdatePlatform::Windows => score_windows_asset_name(asset_name),
        UpdatePlatform::MacOs => score_macos_asset_name(asset_name),
        UpdatePlatform::Linux(architecture) => score_linux_asset_name(asset_name, architecture),
        UpdatePlatform::Unsupported => 0,
    }
}

#[must_use]
pub fn select_update_asset(
    release: &ReleaseInfo,
    platform: UpdatePlatform,
) -> Option<ReleaseAsset> {
    let mut best_asset = None;
    let mut best_score = 0;
    for asset in &release.assets {
        let score = score_asset_for_platform(&asset.name, platform);
        if score > best_score {
            best_score = score;
            best_asset = Some(asset.clone());
        }
    }
    best_asset
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    const SHA256: &str = "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD";

    fn linux_architecture() -> LinuxArchitecture {
        if cfg!(target_arch = "x86_64") {
            LinuxArchitecture::X86_64
        } else if cfg!(target_arch = "aarch64") {
            LinuxArchitecture::AArch64
        } else {
            LinuxArchitecture::Unknown
        }
    }

    fn current_bundle_name() -> &'static str {
        if cfg!(target_arch = "x86_64") {
            "Spencer-Macro-Utilities-V3.3.0-Linux-x86_64.zip"
        } else {
            "Spencer-Macro-Utilities-V3.3.0-Linux-aarch64.zip"
        }
    }

    fn current_appimage_name() -> &'static str {
        if cfg!(target_arch = "x86_64") {
            "Spencer-Macro-Utilities-x86_64.AppImage"
        } else {
            "Spencer-Macro-Utilities-aarch64.AppImage"
        }
    }

    fn versioned_appimage_name() -> &'static str {
        if cfg!(target_arch = "x86_64") {
            "Spencer-Macro-Utilities-V3.3.0-Linux-x86_64.AppImage"
        } else {
            "Spencer-Macro-Utilities-V3.3.0-Linux-aarch64.AppImage"
        }
    }

    fn other_architecture_name() -> &'static str {
        if cfg!(target_arch = "x86_64") {
            "Spencer-Macro-Utilities-Linux-arm64.zip"
        } else {
            "Spencer-Macro-Utilities-Linux-x86_64.zip"
        }
    }

    fn release_contract() -> (String, String, String, String) {
        let official_asset = current_bundle_name();
        let extra_asset = current_appimage_name();
        let release_base =
            "https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/download/V3.4.0/";
        let release_json = format!(
            concat!(
                "{{\"tag_name\":\"V3.4.0\",",
                "\"html_url\":\"https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/tag/V3.4.0\",",
                "\"assets\":[",
                "{{\"name\":\"update-manifest.json\",\"browser_download_url\":\"{0}update-manifest.json\",\"size\":321}},",
                "{{\"name\":\"{1}\",\"browser_download_url\":\"{0}{1}\",\"size\":12345}},",
                "{{\"name\":\"{2}\",\"browser_download_url\":\"{0}{2}\",\"size\":54321}}]}}"
            ),
            release_base, official_asset, extra_asset
        );
        let manifest_json = format!(
            concat!(
                "{{\"schema_version\":1,\"release_version\":\"3.4.0\",",
                "\"minimum_updater_version\":\"3.4.0\",\"artifacts\":{{",
                "\"linux-current\":{{\"asset\":\"{0}\",\"size\":12345,",
                "\"sha256\":\"{1}\",\"url\":\"{2}{0}\"}}}}}}"
            ),
            official_asset, SHA256, release_base
        );
        (release_json, manifest_json, official_asset.to_owned(), extra_asset.to_owned())
    }

    fn replace_once(value: &str, from: &str, to: &str) -> String {
        value.replacen(from, to, 1)
    }

    #[test]
    fn version_normalization_and_comparison_preserve_cpp_rules() {
        assert_eq!(normalize_version(" \tV3.4.0 \n"), "3.4.0");
        assert_eq!(compare_versions("v3.4.0", "3.4"), Ordering::Greater);
        assert_eq!(compare_versions("3.4.0", "3.4.0"), Ordering::Equal);
        assert_eq!(compare_versions("3.4.1", "3.4.0"), Ordering::Greater);
        assert_eq!(compare_versions("3.4.0-beta1", "3.4.0-beta2"), Ordering::Less);
        assert_eq!(compare_versions("release-3.4", "3.4.0"), Ordering::Greater);
    }

    #[test]
    fn canonical_version_rules_match_update_manifest_contract() {
        for valid in ["0.0.0", "3.4.0", "999999999.1.2"] {
            assert!(is_canonical_manifest_version(valid), "{valid}");
        }
        for invalid in ["V3.4.0", "3.4", "03.4.0", "3.4.0-beta", "3.4.0.1", "1000000000.1.1"] {
            assert!(!is_canonical_manifest_version(invalid), "{invalid}");
        }
    }

    #[test]
    fn release_summary_matches_cpp_contract() {
        let legacy = concat!(
            "{\"tag_name\":\"V3.3.1\",",
            "\"html_url\":\"https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/tag/V3.3.1\",",
            "\"assets\":[]}"
        );
        let summary = parse_release_metadata_summary(legacy).expect("valid summary should parse");
        assert_eq!(summary.version, "3.3.1");
        assert!(!summary.manifest_driven);
        assert!(summary.assets.is_empty());
        assert!(
            parse_release_metadata_summary(&replace_once(legacy, "V3.3.1", "V3.3.1-beta")).is_err()
        );
    }

    #[test]
    fn valid_manifest_contract_matches_cpp_release_shape() {
        let (release_json, manifest_json, official_asset, _) = release_contract();
        let release = parse_release_metadata_with_manifest(&release_json, &manifest_json)
            .expect("valid manifest contract should parse");
        assert!(release.manifest_driven);
        assert_eq!(release.manifest_schema_version, 1);
        assert_eq!(release.minimum_updater_version, "3.4.0");
        assert_eq!(release.assets.len(), 1);
        assert_eq!(release.assets[0].name, official_asset);
        assert_eq!(release.assets[0].size_bytes, 12_345);
        assert_eq!(
            release.assets[0].sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            select_update_asset(&release, UpdatePlatform::Linux(linux_architecture()))
                .map(|asset| asset.name),
            Some(official_asset)
        );
        assert!(is_updater_version_allowed(&release, "3.4.0"));
        assert!(!is_updater_version_allowed(&release, "3.3.9"));
    }

    #[test]
    fn manifest_rejection_matrix_matches_cpp_test_contract() {
        let (release_json, manifest_json, official_asset, extra_asset) = release_contract();
        let release_base =
            "https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/download/V3.4.0/";

        let manifest_without_url = replace_once(
            &manifest_json,
            &format!(",\"url\":\"{release_base}{official_asset}\""),
            "",
        );
        assert!(parse_release_metadata_with_manifest(&release_json, &manifest_without_url).is_ok());

        let invalid_manifests = [
            replace_once(&manifest_json, "\"schema_version\":1", "\"schema_version\":2"),
            replace_once(
                &manifest_json,
                "\"release_version\":\"3.4.0\"",
                "\"release_version\":\"3.4.1\"",
            ),
            replace_once(&manifest_json, "\"minimum_updater_version\":\"3.4.0\",", ""),
            replace_once(
                &manifest_json,
                "\"minimum_updater_version\":\"3.4.0\"",
                "\"minimum_updater_version\":\"3.4\"",
            ),
            replace_once(&manifest_json, "\"size\":12345", "\"size\":12346"),
            replace_once(&manifest_json, SHA256, "abc"),
            replace_once(
                &manifest_json,
                &format!("{release_base}{official_asset}"),
                "https://example.invalid/not-official.zip",
            ),
            replace_once(
                &manifest_json,
                &format!("\"asset\":\"{official_asset}\""),
                "\"asset\":\"missing-update.zip\"",
            ),
        ];
        for invalid in invalid_manifests {
            assert!(
                parse_release_metadata_with_manifest(&release_json, &invalid).is_err(),
                "invalid manifest must be rejected: {invalid}"
            );
        }

        let duplicate_release = replace_once(
            &release_json,
            &format!("\"name\":\"{extra_asset}\""),
            &format!("\"name\":\"{official_asset}\""),
        );
        assert!(parse_release_metadata_with_manifest(&duplicate_release, &manifest_json).is_err());

        let duplicate_manifest = replace_once(
            &manifest_json,
            "}}}",
            &format!(
                "}},\"duplicate\":{{\"asset\":\"{official_asset}\",\"size\":12345,\"sha256\":\"{SHA256}\"}}}}}}"
            ),
        );
        assert!(parse_release_metadata_with_manifest(&release_json, &duplicate_manifest).is_err());
        assert_eq!(
            LATEST_RELEASE_PAGE_URL,
            "https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/latest"
        );
    }

    #[test]
    fn cross_platform_asset_scores_match_cpp_test_contract() {
        let legacy_windows = score_windows_asset_name("Spencer-Macro-Utilities-V3.2.1.zip");
        assert!(legacy_windows > 0);
        assert!(score_windows_asset_name("Spencer-Macro-Utilities-3.3.0.zip") > 0);
        assert!(
            score_windows_asset_name("Spencer-Macro-Utilities-V3.3.0-Windows-x64.zip")
                > legacy_windows
        );
        assert!(score_windows_asset_name("Spencer-Macro-Utilities-Windows.zip") > legacy_windows);
        assert_eq!(
            score_windows_asset_name("Spencer-Macro-Utilities-V3.3.0-macOS-universal.zip"),
            0
        );
        assert_eq!(score_windows_asset_name(current_bundle_name()), 0);

        assert!(
            score_macos_asset_name("Spencer-Macro-Utilities-V3.3.0-macOS-universal.zip")
                > score_macos_asset_name("Spencer-Macro-Utilities-V3.2.1.zip")
        );
        assert!(score_macos_asset_name("Spencer-Macro-Utilities-V3.4.0-macOS-universal.ZIP") > 0);
        assert_eq!(score_macos_asset_name("Spencer-Macro-Utilities-V3.2.1.zip"), 0);
        assert_eq!(score_macos_asset_name("Spencer-Macro-Utilities-V3.3.0-macOS-universal.dmg"), 0);
        assert_eq!(score_macos_asset_name("Spencer-Macro-Utilities-V3.3.0-Windows-x64.zip"), 0);
        assert_eq!(score_macos_asset_name(current_bundle_name()), 0);

        let architecture = linux_architecture();
        let bundle_score = score_linux_asset_name(current_bundle_name(), architecture);
        let appimage_score = score_linux_asset_name(versioned_appimage_name(), architecture);
        assert!(bundle_score > 0);
        let uppercase_bundle = current_bundle_name().replace(".zip", ".ZIP");
        assert_eq!(score_linux_asset_name(&uppercase_bundle, architecture), bundle_score);
        assert!(appimage_score > bundle_score);
        assert_eq!(score_linux_asset_name(other_architecture_name(), architecture), 0);
        assert_eq!(
            score_linux_asset_name(
                "Spencer-Macro-Utilities-V3.3.0-macOS-universal.zip",
                architecture
            ),
            0
        );
        assert_eq!(
            score_linux_asset_name("Spencer-Macro-Utilities-V3.3.0-Windows-x64.zip", architecture),
            0
        );
    }

    #[test]
    fn selection_uses_strictly_greater_score_and_keeps_first_tie() {
        let release = ReleaseInfo {
            assets: vec![
                ReleaseAsset { name: "first-linux.zip".to_owned(), ..ReleaseAsset::default() },
                ReleaseAsset { name: "second-linux.zip".to_owned(), ..ReleaseAsset::default() },
                ReleaseAsset { name: "best-linux.AppImage".to_owned(), ..ReleaseAsset::default() },
                ReleaseAsset { name: "equal-linux.AppImage".to_owned(), ..ReleaseAsset::default() },
            ],
            ..ReleaseInfo::default()
        };
        assert_eq!(
            select_update_asset(&release, UpdatePlatform::Linux(LinuxArchitecture::Unknown))
                .map(|asset| asset.name),
            Some("best-linux.AppImage".to_owned())
        );
    }
}
