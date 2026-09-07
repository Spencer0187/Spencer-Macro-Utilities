#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
fn main() {
    if let Err(error) = windows::run() {
        windows::show_error(&error);
        std::process::exit(1);
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("SMU update helper is Windows-only.");
}
