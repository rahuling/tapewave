//! Per-user preferences. No account, registry writes or automatic playback.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub volume: f32,
    pub radio_url: String,
    pub feed_url: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: 1.0,
            radio_url: String::new(),
            feed_url: String::new(),
        }
    }
}

impl Settings {
    fn path() -> Option<PathBuf> {
        std::env::var_os("LOCALAPPDATA")
            .map(|dir| PathBuf::from(dir).join("Tapewave/settings.json"))
    }

    pub fn load() -> Self {
        let mut settings: Self = Self::path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        settings.volume = if settings.volume.is_finite() {
            settings.volume.clamp(0.0, 1.0)
        } else {
            1.0
        };
        settings
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("LOCALAPPDATA is unavailable")?;
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(
            path,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_or_partial_preferences_get_defaults() {
        let settings: Settings =
            serde_json::from_str(r#"{"radio_url":"https://example.com"}"#).unwrap();
        assert_eq!(settings.volume, 1.0);
        assert_eq!(settings.feed_url, "");
    }

    #[test]
    fn round_trip_preferences() {
        let settings = Settings {
            volume: 0.4,
            radio_url: "https://example.com/radio".into(),
            feed_url: String::new(),
        };
        let decoded: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(decoded.volume, settings.volume);
        assert_eq!(decoded.radio_url, settings.radio_url);
    }
}
