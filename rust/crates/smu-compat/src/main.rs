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
        "case-diff" => case_diff(&remaining),
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

fn case_diff(args: &[std::ffi::OsString]) -> Result<ExitCode, Box<dyn std::error::Error>> {
    if !(4..=5).contains(&args.len()) {
        print_usage();
        return Ok(ExitCode::FAILURE);
    }

    let manifest_path = PathBuf::from(&args[0]);
    let case_name = args[1].to_string_lossy();
    let expected_path = PathBuf::from(&args[2]);
    let actual_path = PathBuf::from(&args[3]);
    let manifest = load_ui_case_manifest(&manifest_path)?;
    let Some(case) = manifest.cases.iter().find(|candidate| candidate.name == case_name) else {
        return Err(format!("UI case not found in manifest: {case_name}").into());
    };

    let expected = image::open(expected_path)?;
    let actual = image::open(actual_path)?;
    let strict_policy = DiffPolicy { channel_tolerance: 0, ignore: &case.ignore };
    let gate_policy = DiffPolicy { channel_tolerance: case.gate_tolerance, ignore: &case.ignore };
    let strict = diff_images(&expected, &actual, strict_policy)?;
    let gate = diff_images(&expected, &actual, gate_policy)?;

    println!("case={}", case.name);
    println!("ignored_pixels={}", gate.ignored_pixels);
    println!("strict_different_pixels={}", strict.different_pixels);
    println!("strict_max_channel_delta={}", strict.max_channel_delta);
    println!("gate_tolerance={}", case.gate_tolerance);
    println!("gate_different_pixels={}", gate.different_pixels);
    println!("gate_max_channel_delta={}", gate.max_channel_delta);

    if let Some(output_path) = args.get(4) {
        difference_image(&expected, &actual, gate_policy)?.save(PathBuf::from(output_path))?;
    }

    Ok(if gate.exact_match() { ExitCode::SUCCESS } else { ExitCode::FAILURE })
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
    eprintln!("  smu-compat case-diff <cases.toml> <case> <expected> <actual> [difference.png]");
    eprintln!("  smu-compat manifest-check <cases.toml>");
}
