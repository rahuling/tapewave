//! Audio engine for Tapewave. No UI dependencies; the native Windows shell
//! communicates with the playback worker through in-process channels.

mod loader;

pub mod player;
pub mod podcast;
pub mod stream;

pub use player::{Engine, Status};
