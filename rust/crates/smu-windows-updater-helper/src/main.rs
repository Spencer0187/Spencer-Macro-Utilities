#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    windows::run().map_err(std::io::Error::other)?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("SMU update helper is Windows-only.");
}
