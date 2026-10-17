#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(windows)]
mod native;

#[cfg(windows)]
mod desktop;
mod library;
mod settings;
mod theme;

#[cfg(windows)]
fn main() {
    if let Err(error) = native::run() {
        #[cfg(debug_assertions)]
        if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--ui-smoke-test")) {
            eprintln!("UI smoke test failed: {error}");
            std::process::exit(1);
        }
        native::show_startup_error(&error);
    }
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Tapewave's native desktop shell requires Windows 10/11 x64.");
    std::process::exit(1);
}
