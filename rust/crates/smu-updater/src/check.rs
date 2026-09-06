//! Update-check orchestration separated from HTTP and install-side effects.

use super::{
    MAXIMUM_MANIFEST_BYTES, ReleaseAsset, ReleaseInfo, UpdatePlatform, compare_versions,
    is_canonical_manifest_version, is_updater_version_allowed, normalize_version,
    parse_release_metadata_summary, parse_release_metadata_with_manifest, select_update_asset,
};
use serde_json::Value;
use std::cmp::Ordering;
use std::fmt;

pub const LATEST_RELEASE_API_URL: &str =
    "https://api.github.com/repos/Spencer0187/Spencer-Macro-Utilities/releases/latest";
const RELEASE_BY_TAG_API_PREFIX: &str =
    "https://api.github.com/repos/Spencer0187/Spencer-Macro-Utilities/releases/tags/V";
const TRUSTED_RELEASE_ASSET_PREFIX: &str =
    "https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/download/";
const UPDATE_MANIFEST_NAME: &str = "update-manifest.json";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct UpdaterStatus {
    pub check_succeeded: bool,
    pub update_available: bool,
    pub auto_apply_supported: bool,
    pub local_version: String,
    pub latest_version: String,
    pub message: String,
    pub latest_release: Option<ReleaseInfo>,
    pub selected_asset: Option<ReleaseAsset>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateTransportError(String);

impl UpdateTransportError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for UpdateTransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for UpdateTransportError {}

/// Minimal fetch boundary. Concrete HTTP belongs in the network/platform composition layer.
pub trait UpdateTransport {
    /// Fetch a URL as raw bytes.
    ///
    /// # Errors
    ///
    /// Returns the transport's user-facing failure text.
    fn get(&mut self, url: &str) -> Result<Vec<u8>, UpdateTransportError>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdateCheckError(String);

impl UpdateCheckError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for UpdateCheckError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for UpdateCheckError {}

pub struct UpdateChecker<T> {
    transport: T,
    platform: UpdatePlatform,
    auto_apply_supported: bool,
}

impl<T> UpdateChecker<T> {
    #[must_use]
    pub const fn new(transport: T, platform: UpdatePlatform, auto_apply_supported: bool) -> Self {
        Self { transport, platform, auto_apply_supported }
    }

    #[must_use]
    pub const fn transport(&self) -> &T {
        &self.transport
    }

    #[must_use]
    pub fn into_transport(self) -> T {
        self.transport
    }
}

impl<T: UpdateTransport> UpdateChecker<T> {
    fn get_text(&mut self, url: &str) -> Result<String, UpdateCheckError> {
        let bytes =
            self.transport.get(url).map_err(|error| UpdateCheckError::new(error.to_string()))?;
        String::from_utf8(bytes).map_err(|error| {
            UpdateCheckError::new(format!("Failed to parse latest GitHub release JSON: {error}"))
        })
    }

    fn parse_release_contract_from_response(
        &mut self,
        response: &str,
    ) -> Result<ReleaseInfo, UpdateCheckError> {
        let root: Value = serde_json::from_str(response).map_err(|error| {
            UpdateCheckError::new(format!("Failed to parse latest GitHub release JSON: {error}"))
        })?;
        let summary = parse_release_metadata_summary(response)
            .map_err(|error| UpdateCheckError::new(error.to_string()))?;
        let assets = root.get("assets").and_then(Value::as_array).ok_or_else(|| {
            UpdateCheckError::new("Latest GitHub release metadata is incomplete.")
        })?;

        let mut manifest_asset = None;
        for asset in assets {
            if !asset.is_object()
                || asset.get("name").and_then(Value::as_str) != Some(UPDATE_MANIFEST_NAME)
            {
                continue;
            }
            if manifest_asset.is_some() {
                return Err(UpdateCheckError::new(
                    "Latest release contains more than one update-manifest.json asset.",
                ));
            }
            manifest_asset = Some(asset);
        }
        let Some(manifest_asset) = manifest_asset else {
            return Err(UpdateCheckError::new(
                "Latest release does not include update-manifest.json; this updater requires the V3.4 release contract.",
            ));
        };

        let manifest_url =
            manifest_asset.get("browser_download_url").and_then(Value::as_str).unwrap_or_default();
        let manifest_size = manifest_asset.get("size").and_then(Value::as_u64).unwrap_or_default();
        let required_url_prefix = format!("{TRUSTED_RELEASE_ASSET_PREFIX}{}/", summary.tag_name);
        if !manifest_url.starts_with(&required_url_prefix)
            || manifest_size == 0
            || manifest_size > MAXIMUM_MANIFEST_BYTES
        {
            return Err(UpdateCheckError::new(
                "Updater refused an invalid or oversized update-manifest.json asset.",
            ));
        }

        let manifest_bytes = self
            .transport
            .get(manifest_url)
            .map_err(|error| UpdateCheckError::new(error.to_string()))?;
        let downloaded_size = u64::try_from(manifest_bytes.len()).unwrap_or(u64::MAX);
        if downloaded_size != manifest_size || downloaded_size > MAXIMUM_MANIFEST_BYTES {
            return Err(UpdateCheckError::new(
                "Downloaded update-manifest.json size did not match GitHub release metadata.",
            ));
        }
        let manifest = String::from_utf8(manifest_bytes).map_err(|error| {
            UpdateCheckError::new(format!("Failed to parse updater release contract: {error}"))
        })?;
        parse_release_metadata_with_manifest(response, &manifest)
            .map_err(|error| UpdateCheckError::new(error.to_string()))
    }

    /// Fetch the latest release and validate its update manifest.
    ///
    /// # Errors
    ///
    /// Returns a transport or release-contract validation error.
    pub fn fetch_latest_release(&mut self) -> Result<ReleaseInfo, UpdateCheckError> {
        let response = self.get_text(LATEST_RELEASE_API_URL)?;
        self.parse_release_contract_from_response(&response)
    }

    /// Fetch one exact canonical SMU release tag and validate its update manifest.
    ///
    /// # Errors
    ///
    /// Returns an error for non-canonical versions, transport failures, invalid release contracts, or a
    /// GitHub response whose canonical release version does not match the requested version.
    pub fn fetch_release_version(
        &mut self,
        version: &str,
    ) -> Result<ReleaseInfo, UpdateCheckError> {
        if !is_canonical_manifest_version(version) {
            return Err(UpdateCheckError::new("Updater refused a non-canonical release version."));
        }
        let url = format!("{RELEASE_BY_TAG_API_PREFIX}{version}");
        let response = self.get_text(&url)?;
        let release = self.parse_release_contract_from_response(&response)?;
        if release.version != version {
            return Err(UpdateCheckError::new(
                "GitHub release tag did not match the requested update version.",
            ));
        }
        Ok(release)
    }

    /// Match C++ `CheckForUpdate`: a failed check is represented in the returned status.
    #[must_use]
    pub fn check_for_update(&mut self, local_version: &str) -> UpdaterStatus {
        let mut status = UpdaterStatus {
            local_version: normalize_version(local_version),
            auto_apply_supported: self.auto_apply_supported,
            ..UpdaterStatus::default()
        };

        let release_response = match self.get_text(LATEST_RELEASE_API_URL) {
            Ok(response) => response,
            Err(error) => {
                status.message = fallback_update_error(error.to_string());
                return status;
            }
        };
        let summary = match parse_release_metadata_summary(&release_response) {
            Ok(summary) => summary,
            Err(error) => {
                status.message = fallback_update_error(error.to_string());
                return status;
            }
        };

        status.latest_version.clone_from(&summary.version);
        let comparison = compare_versions(&status.local_version, &status.latest_version);
        status.update_available = comparison == Ordering::Less;
        if !status.update_available {
            status.check_succeeded = true;
            status.latest_release = Some(summary);
            status.message = if comparison == Ordering::Greater {
                "This build is newer than the latest public release.".to_owned()
            } else {
                "You are running the latest version.".to_owned()
            };
            return status;
        }

        let latest = match self.parse_release_contract_from_response(&release_response) {
            Ok(latest) => latest,
            Err(error) => {
                status.message = fallback_update_error(error.to_string());
                return status;
            }
        };
        status.check_succeeded = true;
        let updater_allowed = is_updater_version_allowed(&latest, &status.local_version);
        if updater_allowed {
            status.selected_asset = select_update_asset(&latest, self.platform);
        }
        status.latest_release = Some(latest);

        if !updater_allowed {
            status.auto_apply_supported = false;
            "An update is available, but it requires a newer updater. Install it manually from the official latest release page."
                .clone_into(&mut status.message);
        } else if status.selected_asset.is_none() {
            "An update is available, but the manifest does not declare a matching package for this platform."
                .clone_into(&mut status.message);
        } else if !status.auto_apply_supported {
            "An update is available, but automatic installation is not available in this launch mode."
                .clone_into(&mut status.message);
        } else {
            "An update is available.".clone_into(&mut status.message);
        }
        status
    }
}

fn fallback_update_error(error: String) -> String {
    if error.is_empty() { "Update check failed.".to_owned() } else { error }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;
    use crate::LinuxArchitecture;
    use std::collections::VecDeque;

    #[derive(Debug)]
    struct ScriptedResponse {
        url: String,
        response: Result<Vec<u8>, UpdateTransportError>,
    }

    #[derive(Debug, Default)]
    struct FakeTransport {
        scripted: VecDeque<ScriptedResponse>,
        requests: Vec<String>,
    }

    impl FakeTransport {
        fn ok(mut self, url: &str, body: impl Into<Vec<u8>>) -> Self {
            self.scripted
                .push_back(ScriptedResponse { url: url.to_owned(), response: Ok(body.into()) });
            self
        }

        fn error(mut self, url: &str, message: &str) -> Self {
            self.scripted.push_back(ScriptedResponse {
                url: url.to_owned(),
                response: Err(UpdateTransportError::new(message)),
            });
            self
        }
    }

    impl UpdateTransport for FakeTransport {
        fn get(&mut self, url: &str) -> Result<Vec<u8>, UpdateTransportError> {
            self.requests.push(url.to_owned());
            let Some(scripted) = self.scripted.pop_front() else {
                return Err(UpdateTransportError::new("unexpected updater request"));
            };
            if scripted.url != url {
                return Err(UpdateTransportError::new(format!(
                    "unexpected updater URL: expected {}, got {url}",
                    scripted.url
                )));
            }
            scripted.response
        }
    }

    struct ContractFixture {
        release: String,
        manifest: String,
        manifest_url: String,
    }

    fn contract_fixture(
        release_version: &str,
        minimum_updater_version: &str,
        update_asset: &str,
    ) -> ContractFixture {
        let tag = format!("V{release_version}");
        let release_base = format!(
            "https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/download/{tag}/"
        );
        let update_url = format!("{release_base}{update_asset}");
        let manifest_url = format!("{release_base}{UPDATE_MANIFEST_NAME}");
        let manifest = format!(
            concat!(
                "{{\"schema_version\":1,\"release_version\":\"{release_version}\",",
                "\"minimum_updater_version\":\"{minimum_updater_version}\",\"artifacts\":{{",
                "\"test\":{{\"asset\":\"{update_asset}\",\"size\":5,",
                "\"sha256\":\"ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad\",",
                "\"url\":\"{update_url}\"}}}}}}"
            ),
            release_version = release_version,
            minimum_updater_version = minimum_updater_version,
            update_asset = update_asset,
            update_url = update_url,
        );
        let release = format!(
            concat!(
                "{{\"tag_name\":\"{tag}\",\"html_url\":\"{page}\",\"assets\":[",
                "{{\"name\":\"update-manifest.json\",\"browser_download_url\":\"{manifest_url}\",\"size\":{manifest_size}}},",
                "{{\"name\":\"{update_asset}\",\"browser_download_url\":\"{update_url}\",\"size\":5}}]}}"
            ),
            tag = tag,
            page = crate::LATEST_RELEASE_PAGE_URL,
            manifest_url = manifest_url,
            manifest_size = manifest.len(),
            update_asset = update_asset,
            update_url = update_url,
        );
        ContractFixture { release, manifest, manifest_url }
    }

    #[test]
    fn equal_and_ahead_versions_skip_manifest_fetch() {
        let fixture = contract_fixture("3.4.0", "3.4.0", "Spencer-Macro-Utilities-Windows.zip");
        for (local, message) in [
            ("3.4.0", "You are running the latest version."),
            ("9.0.0", "This build is newer than the latest public release."),
        ] {
            let transport =
                FakeTransport::default().ok(LATEST_RELEASE_API_URL, fixture.release.clone());
            let mut checker = UpdateChecker::new(transport, UpdatePlatform::Windows, true);
            let status = checker.check_for_update(local);
            assert!(status.check_succeeded);
            assert!(!status.update_available);
            assert_eq!(status.message, message);
            assert_eq!(checker.transport().requests, [LATEST_RELEASE_API_URL]);
        }
    }

    #[test]
    fn exact_release_version_fetches_tag_specific_contract_and_rejects_noncanonical_input() {
        let fixture = contract_fixture("3.4.1", "3.4.0", "Spencer-Macro-Utilities-Windows.zip");
        let release_url = format!("{RELEASE_BY_TAG_API_PREFIX}3.4.1");
        let transport = FakeTransport::default()
            .ok(&release_url, fixture.release)
            .ok(&fixture.manifest_url, fixture.manifest);
        let mut checker = UpdateChecker::new(transport, UpdatePlatform::Windows, false);
        let release =
            checker.fetch_release_version("3.4.1").expect("tagged release should validate");
        assert_eq!(release.version, "3.4.1");
        assert_eq!(checker.transport().requests, [release_url, fixture.manifest_url]);

        let mut checker =
            UpdateChecker::new(FakeTransport::default(), UpdatePlatform::Windows, false);
        let error = checker
            .fetch_release_version("V3.4.1")
            .expect_err("non-canonical version must be rejected before transport");
        assert_eq!(error.to_string(), "Updater refused a non-canonical release version.");
        assert!(checker.transport().requests.is_empty());
    }

    #[test]
    fn exact_release_version_rejects_mismatched_returned_tag() {
        let fixture = contract_fixture("3.4.2", "3.4.0", "Spencer-Macro-Utilities-Windows.zip");
        let release_url = format!("{RELEASE_BY_TAG_API_PREFIX}3.4.1");
        let transport = FakeTransport::default()
            .ok(&release_url, fixture.release)
            .ok(&fixture.manifest_url, fixture.manifest);
        let mut checker = UpdateChecker::new(transport, UpdatePlatform::Windows, false);
        let error = checker
            .fetch_release_version("3.4.1")
            .expect_err("returned tag must equal requested version");
        assert_eq!(
            error.to_string(),
            "GitHub release tag did not match the requested update version."
        );
    }

    #[test]
    fn available_update_fetches_manifest_and_selects_package() {
        let fixture = contract_fixture("3.4.1", "3.4.0", "Spencer-Macro-Utilities-Windows.zip");
        let transport = FakeTransport::default()
            .ok(LATEST_RELEASE_API_URL, fixture.release)
            .ok(&fixture.manifest_url, fixture.manifest);
        let mut checker = UpdateChecker::new(transport, UpdatePlatform::Windows, true);
        let status = checker.check_for_update("3.4.0");
        assert!(status.check_succeeded);
        assert!(status.update_available);
        assert!(status.auto_apply_supported);
        assert_eq!(status.message, "An update is available.");
        assert_eq!(
            status.selected_asset.as_ref().map(|asset| asset.name.as_str()),
            Some("Spencer-Macro-Utilities-Windows.zip")
        );
        assert_eq!(checker.transport().requests.len(), 2);
    }

    #[test]
    fn old_updater_requires_manual_install() {
        let fixture = contract_fixture("3.5.0", "3.4.0", "Spencer-Macro-Utilities-Windows.zip");
        let transport = FakeTransport::default()
            .ok(LATEST_RELEASE_API_URL, fixture.release)
            .ok(&fixture.manifest_url, fixture.manifest);
        let mut checker = UpdateChecker::new(transport, UpdatePlatform::Windows, true);
        let status = checker.check_for_update("3.3.9");
        assert!(status.check_succeeded);
        assert!(status.update_available);
        assert!(!status.auto_apply_supported);
        assert!(status.selected_asset.is_none());
        assert_eq!(
            status.message,
            "An update is available, but it requires a newer updater. Install it manually from the official latest release page."
        );
    }

    #[test]
    fn platform_package_and_launch_mode_messages_remain_distinct() {
        let fixture = contract_fixture("3.4.1", "3.4.0", "Spencer-Macro-Utilities-Windows.zip");
        let transport = FakeTransport::default()
            .ok(LATEST_RELEASE_API_URL, fixture.release.clone())
            .ok(&fixture.manifest_url, fixture.manifest.clone());
        let mut checker =
            UpdateChecker::new(transport, UpdatePlatform::Linux(LinuxArchitecture::X86_64), true);
        let no_package = checker.check_for_update("3.4.0");
        assert_eq!(
            no_package.message,
            "An update is available, but the manifest does not declare a matching package for this platform."
        );

        let transport = FakeTransport::default()
            .ok(LATEST_RELEASE_API_URL, fixture.release)
            .ok(&fixture.manifest_url, fixture.manifest);
        let mut checker = UpdateChecker::new(transport, UpdatePlatform::Windows, false);
        let no_auto_apply = checker.check_for_update("3.4.0");
        assert!(no_auto_apply.selected_asset.is_some());
        assert_eq!(
            no_auto_apply.message,
            "An update is available, but automatic installation is not available in this launch mode."
        );
    }

    #[test]
    fn missing_manifest_keeps_update_available_but_check_unsuccessful() {
        let release = concat!(
            "{\"tag_name\":\"V3.4.1\",",
            "\"html_url\":\"https://github.com/Spencer0187/Spencer-Macro-Utilities/releases/tag/V3.4.1\",",
            "\"assets\":[]}"
        );
        let transport = FakeTransport::default().ok(LATEST_RELEASE_API_URL, release);
        let mut checker = UpdateChecker::new(transport, UpdatePlatform::Windows, true);
        let status = checker.check_for_update("3.4.0");
        assert!(!status.check_succeeded);
        assert!(status.update_available);
        assert_eq!(status.latest_version, "3.4.1");
        assert_eq!(
            status.message,
            "Latest release does not include update-manifest.json; this updater requires the V3.4 release contract."
        );
    }

    #[test]
    fn release_and_manifest_transport_failures_are_preserved() {
        let transport = FakeTransport::default().error(LATEST_RELEASE_API_URL, "offline");
        let mut checker = UpdateChecker::new(transport, UpdatePlatform::Windows, true);
        let status = checker.check_for_update("3.4.0");
        assert!(!status.check_succeeded);
        assert_eq!(status.message, "offline");

        let fixture = contract_fixture("3.4.1", "3.4.0", "Spencer-Macro-Utilities-Windows.zip");
        let transport = FakeTransport::default()
            .ok(LATEST_RELEASE_API_URL, fixture.release)
            .error(&fixture.manifest_url, "manifest offline");
        let mut checker = UpdateChecker::new(transport, UpdatePlatform::Windows, true);
        let status = checker.check_for_update("3.4.0");
        assert!(!status.check_succeeded);
        assert!(status.update_available);
        assert_eq!(status.message, "manifest offline");
    }

    #[test]
    fn manifest_size_must_match_release_metadata() {
        let fixture = contract_fixture("3.4.1", "3.4.0", "Spencer-Macro-Utilities-Windows.zip");
        let transport = FakeTransport::default()
            .ok(LATEST_RELEASE_API_URL, fixture.release)
            .ok(&fixture.manifest_url, b"{}".to_vec());
        let mut checker = UpdateChecker::new(transport, UpdatePlatform::Windows, true);
        assert_eq!(
            checker.check_for_update("3.4.0").message,
            "Downloaded update-manifest.json size did not match GitHub release metadata."
        );
    }
}
