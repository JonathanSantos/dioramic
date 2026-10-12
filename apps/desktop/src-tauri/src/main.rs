//! Dioramic desktop app entry point.

// No console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = dioramic_desktop::run() {
        eprintln!("dioramic desktop failed: {error}");
        std::process::exit(1);
    }
}
