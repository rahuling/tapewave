//! Audio engine for Tapedeck. No UI or Tauri dependencies, so the same crate
//! can back the desktop app now and the iOS/Android/iPadOS apps later.

pub mod player;
pub mod podcast;
pub mod stream;

pub use player::{Engine, Status};
