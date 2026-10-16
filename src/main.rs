#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod native;

mod settings;

#[cfg(windows)]
fn main() {
    if let Err(error) = native::run() {
        native::show_startup_error(&error);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Tapewave's native desktop shell requires Windows 10/11 x64.");
    std::process::exit(1);
}
