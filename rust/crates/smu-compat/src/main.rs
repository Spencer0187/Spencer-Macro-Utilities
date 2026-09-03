//! Small compatibility-harness CLI used by local and CI parity checks.

use std::{env, path::PathBuf, process::ExitCode};

use smu_compat::{DiffPolicy, diff_images, difference_image, load_ui_case_manifest};

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("smu-compat: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, Box<dyn std::error::Error>> {
    let mut args = env::args_os();
    let _program = args.next();
    let Some(command) = args.next() else {
        print_usage();
        return Ok(ExitCode::FAILURE);
    };

    let remaining: Vec<_> = args.collect();
    match command.to_string_lossy().as_ref() {
        "image-diff" => image_diff(&remaining),
        "manifest-check" => manifest_check(&remaining),
        _ => {
            print_usage();
            Ok(ExitCode::FAILURE)
        }
    }
}

fn image_diff(args: &[std::ffi::OsString]) -> Result<ExitCode, Box<dyn std::error::Error>> {
    if !(2..=3).contains(&args.len()) {
        print_usage();
        return Ok(ExitCode::FAILURE);
    }
    let expected_path = PathBuf::from(&args[0]);
    let actual_path = PathBuf::from(&args[1]);
    let expected = image::open(expected_path)?;
    let actual = image::open(actual_path)?;
    let policy = DiffPolicy::default();
    let stats = diff_images(&expected, &actual, policy)?;

    println!("compared_pixels={}", stats.compared_pixels);
    println!("ignored_pixels={}", stats.ignored_pixels);
    println!("different_pixels={}", stats.different_pixels);
    println!("max_channel_delta={}", stats.max_channel_delta);

    if let Some(output_path) = args.get(2) {
        difference_image(&expected, &actual, policy)?.save(PathBuf::from(output_path))?;
    }

    Ok(if stats.exact_match() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

fn manifest_check(args: &[std::ffi::OsString]) -> Result<ExitCode, Box<dyn std::error::Error>> {
    if args.len() != 1 {
        print_usage();
        return Ok(ExitCode::FAILURE);
    }
    let manifest = load_ui_case_manifest(&PathBuf::from(&args[0]))?;
    println!("references={}", manifest.references.len());
    println!("automated_cases={}", manifest.cases.len());
    Ok(ExitCode::SUCCESS)
}

fn print_usage() {
    eprintln!("usage:");
    eprintln!("  smu-compat image-diff <expected.png> <actual.png> [difference.png]");
    eprintln!("  smu-compat manifest-check <cases.toml>");
}
