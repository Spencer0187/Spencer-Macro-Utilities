use crate::MAXIMUM_UPDATE_BYTES;
use std::collections::BTreeSet;
use std::fmt;
use std::io::Cursor;
use std::path::{Component, Path};
use zip::ZipArchive;

const MAXIMUM_MACOS_BUNDLE_ENTRIES: usize = 20_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MacosBundleError(String);

impl MacosBundleError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for MacosBundleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for MacosBundleError {}

fn normalized_zip_name(name: &str) -> Result<Vec<&str>, MacosBundleError> {
    if name.is_empty() || name.contains('\\') || name.contains('\0') {
        return Err(MacosBundleError::new("Downloaded macOS update contains an invalid ZIP path."));
    }
    let path = Path::new(name);
    if path.is_absolute() {
        return Err(MacosBundleError::new(
            "Downloaded macOS update contains an absolute ZIP path.",
        ));
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => {
                let value = value.to_str().ok_or_else(|| {
                    MacosBundleError::new("Downloaded macOS update contains a non-UTF-8 ZIP path.")
                })?;
                if value.is_empty() || value == "." || value == ".." || value.contains(':') {
                    return Err(MacosBundleError::new(
                        "Downloaded macOS update contains an unsafe ZIP path.",
                    ));
                }
                parts.push(value);
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(MacosBundleError::new(
                    "Downloaded macOS update contains an unsafe ZIP path.",
                ));
            }
        }
    }
    if parts.is_empty() {
        return Err(MacosBundleError::new("Downloaded macOS update contains an empty ZIP path."));
    }
    Ok(parts)
}

const fn entry_is_supported_file(unix_mode: Option<u32>) -> bool {
    let Some(mode) = unix_mode else {
        return true;
    };
    let file_type = mode & 0o170_000;
    file_type == 0 || file_type == 0o100_000
}

pub fn validate_macos_bundle_archive(bytes: &[u8]) -> Result<String, MacosBundleError> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| {
        MacosBundleError::new("Downloaded macOS update was not a valid ZIP archive.")
    })?;
    if archive.is_empty() || archive.len() > MAXIMUM_MACOS_BUNDLE_ENTRIES {
        return Err(MacosBundleError::new(
            "Downloaded macOS update contains an invalid number of ZIP entries.",
        ));
    }

    let mut root_app: Option<String> = None;
    let mut files = BTreeSet::new();
    let mut expanded_size = 0_u64;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|_| {
            MacosBundleError::new("Downloaded macOS update contains an unreadable ZIP entry.")
        })?;
        if entry.encrypted() {
            return Err(MacosBundleError::new(
                "Downloaded macOS update contains an encrypted ZIP entry.",
            ));
        }
        let parts = normalized_zip_name(entry.name())?;
        if entry.is_dir() && parts.as_slice() == ["__MACOSX"] {
            continue;
        }
        let apple_double = parts.first() == Some(&"__MACOSX");
        let Some(raw_app_name) = parts.get(usize::from(apple_double)) else {
            return Err(MacosBundleError::new(
                "Downloaded macOS update contains metadata outside its app bundle.",
            ));
        };
        let app_name = if apple_double && parts.len() == 2 {
            raw_app_name.strip_prefix("._").unwrap_or(raw_app_name)
        } else {
            raw_app_name
        };
        if !app_name.to_ascii_lowercase().ends_with(".app") {
            return Err(MacosBundleError::new(format!(
                "Downloaded macOS update contains an unexpected top-level entry: {}",
                parts.join("/")
            )));
        }
        match &root_app {
            Some(existing) if existing != app_name => {
                return Err(MacosBundleError::new(
                    "Downloaded macOS update contains more than one app bundle.",
                ));
            }
            None => root_app = Some(app_name.to_owned()),
            _ => {}
        }

        if entry.is_dir() {
            continue;
        }
        if !entry_is_supported_file(entry.unix_mode()) {
            return Err(MacosBundleError::new(
                "Downloaded macOS update contains an unsupported ZIP entry type.",
            ));
        }
        let normalized = parts.join("/");
        if !files.insert(normalized) {
            return Err(MacosBundleError::new(
                "Downloaded macOS update contains a duplicate ZIP file path.",
            ));
        }
        expanded_size = expanded_size.checked_add(entry.size()).ok_or_else(|| {
            MacosBundleError::new("Downloaded macOS update expanded size overflowed.")
        })?;
        if expanded_size > MAXIMUM_UPDATE_BYTES {
            return Err(MacosBundleError::new(
                "Downloaded macOS update exceeds the 512 MiB expanded-size limit.",
            ));
        }
    }

    let root_app = root_app.ok_or_else(|| {
        MacosBundleError::new("Downloaded macOS update did not contain an app bundle.")
    })?;
    let executable_prefix = format!("{root_app}/Contents/MacOS/");
    let plist_path = format!("{root_app}/Contents/Info.plist");
    if !files.contains(&plist_path)
        || !files.iter().any(|relative| relative.starts_with(&executable_prefix))
    {
        return Err(MacosBundleError::new(
            "Downloaded macOS update app bundle is missing its Info.plist or executable payload.",
        ));
    }
    Ok(root_app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;
    use zip::write::SimpleFileOptions;
    use zip::{CompressionMethod, ZipWriter};

    fn fixture(entries: &[(&str, Option<u32>)]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, mode) in entries {
            let mut options =
                SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
            if let Some(mode) = mode {
                options = options.unix_permissions(*mode);
            }
            if name.ends_with('/') {
                assert!(writer.add_directory(*name, options).is_ok());
            } else {
                assert!(writer.start_file(*name, options).is_ok());
                assert!(writer.write_all(b"x").is_ok());
            }
        }
        writer.finish().map_or_else(|_| Vec::new(), Cursor::into_inner)
    }

    #[test]
    fn accepts_one_root_app_and_matching_appledouble_metadata() {
        let bytes = fixture(&[
            ("Spencer Macro Utilities.app/", None),
            ("Spencer Macro Utilities.app/Contents/Info.plist", None),
            ("Spencer Macro Utilities.app/Contents/MacOS/Spencer-Macro-Utilities", Some(0o100_755)),
            ("__MACOSX/", None),
            ("__MACOSX/._Spencer Macro Utilities.app", None),
            ("__MACOSX/Spencer Macro Utilities.app/Contents/._Info.plist", None),
        ]);
        assert_eq!(
            validate_macos_bundle_archive(&bytes),
            Ok("Spencer Macro Utilities.app".to_owned())
        );
    }

    #[test]
    fn rejects_traversal_backslash_and_ambiguous_app_roots() {
        for bytes in [
            fixture(&[
                ("Spencer Macro Utilities.app/Contents/Info.plist", None),
                ("../outside", None),
            ]),
            fixture(&[
                ("Spencer Macro Utilities.app/Contents/Info.plist", None),
                ("Spencer Macro Utilities.app\\Contents\\MacOS\\SMU", None),
            ]),
            fixture(&[
                ("First.app/Contents/Info.plist", None),
                ("First.app/Contents/MacOS/SMU", None),
                ("Second.app/Contents/Info.plist", None),
            ]),
        ] {
            assert!(validate_macos_bundle_archive(&bytes).is_err());
        }
    }

    #[test]
    fn rejects_symlink_and_special_unix_entry_modes() {
        assert!(entry_is_supported_file(None));
        assert!(entry_is_supported_file(Some(0o100_755)));
        assert!(entry_is_supported_file(Some(0o755)));
        assert!(!entry_is_supported_file(Some(0o120_777)));
        assert!(!entry_is_supported_file(Some(0o060_600)));
        assert!(!entry_is_supported_file(Some(0o010_600)));
    }

    #[test]
    fn rejects_files_outside_app_and_incomplete_app_payloads() {
        assert!(
            validate_macos_bundle_archive(&fixture(&[
                ("Spencer Macro Utilities.app/Contents/Info.plist", None),
                ("README.md", None),
            ]))
            .is_err()
        );
        assert!(
            validate_macos_bundle_archive(&fixture(&[(
                "Spencer Macro Utilities.app/Contents/Info.plist",
                None,
            )]))
            .is_err()
        );
    }
}
