use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

fn repository_root(manifest_dir: &Path) -> PathBuf {
    manifest_dir.join("../../..")
}

fn parse_version(version: &str) -> Result<(u16, u16, u16), Box<dyn Error>> {
    let parts = version.trim().split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err(format!("expected MAJOR.MINOR.PATCH version, got {version:?}").into());
    }
    Ok((parts[0].parse()?, parts[1].parse()?, parts[2].parse()?))
}

fn packed_windows_version(major: u16, minor: u16, patch: u16) -> u64 {
    (u64::from(major) << 48) | (u64::from(minor) << 32) | (u64::from(patch) << 16)
}

fn main() -> Result<(), Box<dyn Error>> {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let root = repository_root(&manifest_dir);
    let version_path = root.join("version");
    let icon_path = root.join("resources/windows/smu.ico");
    println!("cargo:rerun-if-changed={}", version_path.display());
    println!("cargo:rerun-if-changed={}", icon_path.display());

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return Ok(());
    }

    let version = fs::read_to_string(&version_path)?.trim().to_owned();
    let (major, minor, patch) = parse_version(&version)?;
    let packed = packed_windows_version(major, minor, patch);
    let original_filename = format!("Spencer-Macro-Utilities-V{version}-Windows.exe");
    let icon_path = icon_path.to_str().ok_or("Windows icon path is not valid UTF-8")?;

    let mut resource = winres::WindowsResource::new();
    resource
        .set_language(0x0409)
        .set_icon_with_id(icon_path, "102")
        .set("CompanyName", "Spencer's Open Source Macro Industries")
        .set("FileDescription", "Spencer Macro Utilities")
        .set("FileVersion", &version)
        .set("InternalName", "Spencer-Macro-Utilities.exe")
        .set("LegalCopyright", "Copyright (C) 2025")
        .set("OriginalFilename", &original_filename)
        .set("ProductName", "Spencer Macro Utilities")
        .set("ProductVersion", &version)
        .set_version_info(winres::VersionInfo::FILEVERSION, packed)
        .set_version_info(winres::VersionInfo::PRODUCTVERSION, packed);
    resource.compile()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_version_packs_like_windows_version_resource() {
        let (major, minor, patch) = parse_version("3.4.0").expect("version parses");
        assert_eq!(packed_windows_version(major, minor, patch), 0x0003_0004_0000_0000);
    }

    #[test]
    fn version_parser_rejects_non_triplets() {
        assert!(parse_version("3.4").is_err());
        assert!(parse_version("3.4.0.1").is_err());
    }
}
