//! Linux updater transport matching the legacy C++ curl dependency.

use crate::{MAXIMUM_UPDATE_BYTES, UpdateTransport, UpdateTransportError};
use std::ffi::{OsStr, OsString};
use std::process::Command;

const CURL_TIMEOUT_SECONDS: &str = "60";
const CURL_MAXIMUM_BYTES: &str = "536870912";
const USER_AGENT_HEADER: &str = "User-Agent: Spencer-Macro-Utilities-Updater";
const GITHUB_ACCEPT_HEADER: &str = "Accept: application/vnd.github+json";

#[derive(Clone, Debug)]
pub struct CurlUpdateTransport {
    program: OsString,
}

impl Default for CurlUpdateTransport {
    fn default() -> Self {
        Self { program: OsString::from("curl") }
    }
}

impl CurlUpdateTransport {
    #[cfg(test)]
    fn with_program(program: impl Into<OsString>) -> Self {
        Self { program: program.into() }
    }
}

impl UpdateTransport for CurlUpdateTransport {
    fn get(&mut self, url: &str) -> Result<Vec<u8>, UpdateTransportError> {
        if !url.starts_with("https://") {
            return Err(UpdateTransportError::new("Updater refused a non-HTTPS URL."));
        }

        let output =
            Command::new(&self.program).args(curl_arguments(url)).output().map_err(|error| {
                UpdateTransportError::new(format!("Failed to launch curl: {error}"))
            })?;
        if !output.status.success() {
            return Err(UpdateTransportError::new(format!(
                "Update HTTP request failed; {}",
                exit_status_message(output.status)
            )));
        }
        if output.stdout.is_empty() {
            return Err(UpdateTransportError::new("Update check failed."));
        }
        let downloaded_size = u64::try_from(output.stdout.len()).unwrap_or(u64::MAX);
        if downloaded_size > MAXIMUM_UPDATE_BYTES {
            return Err(UpdateTransportError::new(
                "Updater download exceeded the 512 MiB safety limit.",
            ));
        }
        Ok(output.stdout)
    }
}

fn curl_arguments(url: &str) -> [&OsStr; 15] {
    [
        OsStr::new("-fsSL"),
        OsStr::new("--max-time"),
        OsStr::new(CURL_TIMEOUT_SECONDS),
        OsStr::new("--max-filesize"),
        OsStr::new(CURL_MAXIMUM_BYTES),
        OsStr::new("--proto"),
        OsStr::new("=https"),
        OsStr::new("--proto-redir"),
        OsStr::new("=https"),
        OsStr::new("-H"),
        OsStr::new(USER_AGENT_HEADER),
        OsStr::new("-H"),
        OsStr::new(GITHUB_ACCEPT_HEADER),
        OsStr::new("--"),
        OsStr::new(url),
    ]
}

fn exit_status_message(status: std::process::ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("curl exited with code {code}.");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt as _;
        if let Some(signal) = status.signal() {
            return format!("curl was terminated by signal {signal}.");
        }
    }
    "curl did not complete successfully.".to_owned()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::*;

    #[test]
    fn curl_arguments_match_legacy_https_limits_and_headers() {
        let args = curl_arguments("https://example.test/path")
            .into_iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(args.windows(2).any(|pair| pair == ["--max-time", "60"]));
        assert!(args.windows(2).any(|pair| pair == ["--max-filesize", "536870912"]));
        assert!(args.windows(2).any(|pair| pair == ["--proto", "=https"]));
        assert!(args.windows(2).any(|pair| pair == ["--proto-redir", "=https"]));
        assert!(args.contains(&USER_AGENT_HEADER.to_owned()));
        assert!(args.contains(&GITHUB_ACCEPT_HEADER.to_owned()));
        assert!(args.contains(&"https://example.test/path".to_owned()));
    }

    #[test]
    fn transport_rejects_non_https_before_launching_curl() {
        let mut transport = CurlUpdateTransport::with_program("definitely-not-a-real-curl");
        let error = transport.get("http://example.test/").expect_err("HTTP must be rejected");
        assert_eq!(error.to_string(), "Updater refused a non-HTTPS URL.");
    }

    #[test]
    fn missing_curl_is_reported_as_transport_error() {
        let mut transport = CurlUpdateTransport::with_program("smu-missing-curl-for-test");
        let error = transport.get("https://example.test/").expect_err("missing curl should fail");
        assert!(error.to_string().starts_with("Failed to launch curl:"));
    }
}
