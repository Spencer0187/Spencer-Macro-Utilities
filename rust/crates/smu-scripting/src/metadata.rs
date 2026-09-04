//! Imported-script metadata parsing from `app/script_metadata.cpp`.

use crate::parse_script_hotkey;
use smu_core::Hotkey;
use std::fs;
use std::path::Path;

const MAX_METADATA_NAME_BYTES: usize = 128;
const MAX_METADATA_DESCRIPTION_BYTES: usize = 1024;
const MAX_METADATA_AUTHOR_BYTES: usize = 128;
const MAX_METADATA_VERSION_BYTES: usize = 64;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportedScriptMetadata {
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub memory_limit_mb: Option<usize>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportedScriptHeader {
    pub metadata: ImportedScriptMetadata,
    pub hotkey: Option<Hotkey>,
}

fn trim_ascii_whitespace(value: &str) -> &str {
    value.trim_matches(|character: char| character.is_ascii_whitespace())
}

fn lower_no_separators(value: &str) -> String {
    value
        .bytes()
        .filter(|byte| !matches!(byte, b' ' | b'_' | b'-'))
        .map(|byte| char::from(byte.to_ascii_lowercase()))
        .collect()
}

fn clamp_utf8_bytes(value: &str, maximum_bytes: usize) -> String {
    if value.len() <= maximum_bytes {
        return value.to_owned();
    }
    let mut boundary = maximum_bytes;
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_owned()
}

fn fallback_name(path: &Path) -> String {
    clamp_utf8_bytes(
        &path.file_stem().map_or_else(String::new, |stem| stem.to_string_lossy().into_owned()),
        MAX_METADATA_NAME_BYTES,
    )
}

fn parse_cpp_stoull_decimal(value: &str) -> Option<usize> {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut index = 0;
    let negative = match bytes.first() {
        Some(b'+') => {
            index = 1;
            false
        }
        Some(b'-') => {
            index = 1;
            true
        }
        _ => false,
    };
    if index == bytes.len() || !bytes[index..].iter().all(u8::is_ascii_digit) {
        return None;
    }

    let mut magnitude = 0_u128;
    for &byte in &bytes[index..] {
        magnitude = magnitude.checked_mul(10)?.checked_add(u128::from(byte - b'0'))?;
    }
    let unsigned_max = u128::from(u64::MAX);
    if magnitude > unsigned_max {
        return None;
    }
    let parsed_u64 = if negative {
        0_u64.wrapping_sub(u64::try_from(magnitude).ok()?)
    } else {
        u64::try_from(magnitude).ok()?
    };
    // C++ assigns `unsigned long long` into `size_t`, so narrower targets truncate.
    #[allow(clippy::cast_possible_truncation)]
    Some(parsed_u64 as usize)
}

/// Parse the comment-only metadata prefix of one imported script.
///
/// Non-comment source stops metadata scanning exactly like the C++ implementation. I/O failure is
/// not an error; the filename stem remains the script name and all optional fields stay empty.
#[must_use]
pub fn parse_script_header(path: &Path) -> ImportedScriptHeader {
    let fallback = fallback_name(path);
    let mut header = ImportedScriptHeader {
        metadata: ImportedScriptMetadata {
            name: fallback.clone(),
            ..ImportedScriptMetadata::default()
        },
        hotkey: None,
    };
    let Ok(contents) = fs::read_to_string(path) else {
        return header;
    };

    let mut raw_hotkey = None;
    for line in contents.lines() {
        let mut trimmed = trim_ascii_whitespace(line);
        if trimmed.is_empty() {
            continue;
        }
        let Some(comment_body) = trimmed.strip_prefix("--") else {
            break;
        };
        trimmed = trim_ascii_whitespace(comment_body);
        if trimmed.is_empty() || !trimmed.starts_with('@') {
            continue;
        }
        let Some(colon) = trimmed.find(':') else {
            continue;
        };
        let key = lower_no_separators(&trimmed[1..colon]);
        let value = trim_ascii_whitespace(&trimmed[colon + 1..]);
        if value.is_empty() {
            continue;
        }

        match key.as_str() {
            "name" => {
                header.metadata.name = clamp_utf8_bytes(value, MAX_METADATA_NAME_BYTES);
            }
            "desc" if header.metadata.description.is_empty() => {
                header.metadata.description =
                    clamp_utf8_bytes(value, MAX_METADATA_DESCRIPTION_BYTES);
            }
            "description" => {
                header.metadata.description =
                    clamp_utf8_bytes(value, MAX_METADATA_DESCRIPTION_BYTES);
            }
            "author" => {
                header.metadata.author = clamp_utf8_bytes(value, MAX_METADATA_AUTHOR_BYTES);
            }
            "version" => {
                header.metadata.version = clamp_utf8_bytes(value, MAX_METADATA_VERSION_BYTES);
            }
            "memorylimitmb" => {
                if let Some(parsed) = parse_cpp_stoull_decimal(value) {
                    header.metadata.memory_limit_mb = Some(parsed);
                }
            }
            "keybind" => raw_hotkey = Some(value.to_owned()),
            _ => {}
        }
    }

    if header.metadata.name.is_empty() {
        header.metadata.name = fallback;
    }
    header.hotkey = raw_hotkey.and_then(|value| parse_script_hotkey(&value));
    header
}

#[must_use]
pub fn parse_script_metadata(path: &Path) -> ImportedScriptMetadata {
    parse_script_header(path).metadata
}

#[must_use]
pub fn parse_script_hotkey_metadata(path: &Path) -> Option<Hotkey> {
    parse_script_header(path).hotkey
}

#[must_use]
pub fn is_supported_script_extension(path: &Path) -> bool {
    path.extension().and_then(|extension| extension.to_str()).is_some_and(|extension| {
        matches!(extension.to_ascii_lowercase().as_str(), "smus" | "hss" | "lua" | "txt")
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{
        ImportedScriptMetadata, is_supported_script_extension, parse_script_header,
        parse_script_metadata,
    };
    use serde::Deserialize;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Debug, Deserialize)]
    struct MetadataFixture {
        cases: Vec<MetadataCase>,
    }

    #[derive(Debug, Deserialize)]
    struct MetadataCase {
        name: String,
        filename: String,
        content: String,
        observed: ObservedMetadata,
    }

    #[derive(Debug, Deserialize)]
    struct ObservedMetadata {
        name: String,
        description: String,
        author: String,
        version: String,
        memory_limit_mb: Option<usize>,
        hotkey: Option<u32>,
        supported_extension: bool,
    }

    fn temp_script(name: &str, contents: &str) -> std::path::PathBuf {
        let unique =
            SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_nanos());
        let directory = std::env::temp_dir().join(format!("smu-script-metadata-{unique}"));
        fs::create_dir_all(&directory).expect("metadata temp directory should be creatable");
        let path = directory.join(name);
        fs::write(&path, contents).expect("metadata fixture should be writable");
        path
    }

    #[test]
    fn metadata_cases_match_cpp_oracle_fixture() {
        let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let fixture_text =
            fs::read_to_string(repository.join("compat/scripting/metadata_cases.json"))
                .expect("checked-in C++ metadata fixture should exist");
        let fixture: MetadataFixture = serde_json::from_str(&fixture_text)
            .expect("checked-in C++ metadata fixture should parse");

        let directory = std::env::temp_dir().join(format!(
            "smu-script-metadata-cpp-fixture-{}",
            SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_nanos())
        ));
        fs::create_dir_all(&directory).expect("metadata fixture directory should be creatable");

        for (index, case) in fixture.cases.into_iter().enumerate() {
            let case_directory = directory.join(index.to_string());
            fs::create_dir_all(&case_directory)
                .expect("metadata case directory should be creatable");
            let path = case_directory.join(&case.filename);
            fs::write(&path, &case.content).expect("metadata case should be writable");

            let parsed = parse_script_header(&path);
            assert_eq!(parsed.metadata.name, case.observed.name, "case {} name", case.name);
            assert_eq!(
                parsed.metadata.description, case.observed.description,
                "case {} description",
                case.name
            );
            assert_eq!(parsed.metadata.author, case.observed.author, "case {} author", case.name);
            assert_eq!(
                parsed.metadata.version, case.observed.version,
                "case {} version",
                case.name
            );
            assert_eq!(
                parsed.metadata.memory_limit_mb, case.observed.memory_limit_mb,
                "case {} memory limit",
                case.name
            );
            assert_eq!(
                parsed.hotkey.map(smu_core::Hotkey::raw),
                case.observed.hotkey,
                "case {} hotkey",
                case.name
            );
            assert_eq!(
                is_supported_script_extension(&path),
                case.observed.supported_extension,
                "case {} extension",
                case.name
            );
        }

        fs::remove_dir_all(directory).expect("metadata fixture directory should be removable");
    }

    #[test]
    fn missing_file_uses_stem_fallback() {
        assert_eq!(
            parse_script_metadata(std::path::Path::new("/not/real/Fallback.lua")),
            ImportedScriptMetadata {
                name: "Fallback".to_owned(),
                ..ImportedScriptMetadata::default()
            }
        );
    }

    #[test]
    fn description_beats_desc_and_code_stops_scanning() {
        let path = temp_script(
            "sample.lua",
            concat!(
                "-- @desc: short\n",
                "-- @description: long\n",
                "print('stop')\n",
                "-- @author: ignored\n"
            ),
        );
        let metadata = parse_script_metadata(&path);
        assert_eq!(metadata.description, "long");
        assert!(metadata.author.is_empty());
        fs::remove_dir_all(path.parent().expect("fixture should have parent"))
            .expect("metadata fixture should be removable");
    }

    #[test]
    fn memory_parser_preserves_cpp_stoull_sign_behavior() {
        let plus = temp_script("plus.lua", "-- @memoryLimitMB: +42\n");
        assert_eq!(parse_script_metadata(&plus).memory_limit_mb, Some(42));
        fs::remove_dir_all(plus.parent().expect("fixture should have parent"))
            .expect("metadata fixture should be removable");

        let negative = temp_script("negative.lua", "-- @memoryLimitMB: -1\n");
        if usize::BITS == 64 {
            assert_eq!(parse_script_metadata(&negative).memory_limit_mb, Some(usize::MAX));
        }
        fs::remove_dir_all(negative.parent().expect("fixture should have parent"))
            .expect("metadata fixture should be removable");
    }

    #[test]
    fn hotkey_uses_last_metadata_value() {
        let path = temp_script("hotkey.smus", "-- @keybind: Ctrl+A\n-- @key_bind: Alt+F2\n");
        assert_eq!(parse_script_header(&path).hotkey.map(smu_core::Hotkey::raw), Some(0x4_0071));
        fs::remove_dir_all(path.parent().expect("fixture should have parent"))
            .expect("metadata fixture should be removable");
    }

    #[test]
    fn supported_extensions_are_ascii_case_insensitive() {
        for name in ["x.smus", "x.HSS", "x.Lua", "x.TXT"] {
            assert!(is_supported_script_extension(std::path::Path::new(name)), "{name}");
        }
        for name in ["x", "x.json", "x.lua.bak"] {
            assert!(!is_supported_script_extension(std::path::Path::new(name)), "{name}");
        }
    }
}
