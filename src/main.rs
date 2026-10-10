// Hides the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tapedeck_engine::{podcast, Engine, Status};
use tauri::State;

struct AppState {
    engine: Engine,
}

#[tauri::command]
fn play_file(path: String, state: State<AppState>) {
    state.engine.play_file(path);
}

#[tauri::command]
fn play_url(url: String, state: State<AppState>) {
    state.engine.play_url(url);
}

#[tauri::command]
fn pause(state: State<AppState>) {
    state.engine.pause();
}

#[tauri::command]
fn resume(state: State<AppState>) {
    state.engine.resume();
}

#[tauri::command]
fn stop(state: State<AppState>) {
    state.engine.stop();
}

#[tauri::command]
fn set_volume(volume: f32, state: State<AppState>) {
    state.engine.set_volume(volume);
}

#[tauri::command]
fn status(state: State<AppState>) -> Status {
    state.engine.status()
}

// Blocking network call. Tauri runs sync commands off the main thread.
#[tauri::command]
fn fetch_podcast(feed_url: String) -> Result<podcast::Podcast, String> {
    podcast::fetch(&feed_url)
}

fn main() {
    tauri::Builder::default()
        .manage(AppState { engine: Engine::spawn() })
        .invoke_handler(tauri::generate_handler![
            play_file,
            play_url,
            pause,
            resume,
            stop,
            set_volume,
            status,
            fetch_podcast,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Tapedeck");
}
