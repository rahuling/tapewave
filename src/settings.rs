//! Per-user preferences. No account, registry writes or automatic playback.

use crate::{
    library::{Item, Progress, Show, Station},
    theme::Palette,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Settings {
    pub volume: f32,
    pub radio_url: String,
    pub feed_url: String,
    pub palette: Palette,
    pub library: Vec<Item>,
    pub stations: Vec<Station>,
    pub shows: Vec<Show>,
    pub progress: BTreeMap<String, Progress>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            volume: 1.0,
            radio_url: String::new(),
            feed_url: String::new(),
            palette: Palette::default(),
            library: Vec::new(),
            stations: Vec::new(),
            shows: Vec::new(),
            progress: BTreeMap::new(),
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
        if !settings.palette.readable() {
            settings.palette = Palette::default();
        }
        // Additive migration of the original single-source preferences.
        if settings.stations.is_empty() && crate::library::valid_url(&settings.radio_url) {
            settings.stations.push(Station {
                name: "Saved station".into(),
                url: settings.radio_url.clone(),
            });
        }
        if settings.shows.is_empty() && crate::library::valid_url(&settings.feed_url) {
            settings.shows.push(Show {
                title: "Saved podcast".into(),
                url: settings.feed_url.clone(),
                episodes: Vec::new(),
            });
        }
        settings.radio_url.clear();
        settings.feed_url.clear();
        settings
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("LOCALAPPDATA is unavailable")?;
        self.save_to(&path)
    }

    fn save_to(&self, path: &std::path::Path) -> Result<(), String> {
        use std::io::Write;
        let directory = path
            .parent()
            .ok_or("Preferences directory is unavailable")?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        // Same-directory atomic replacement prevents a failed write from
        // truncating the user's music collection and listening history.
        let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        file.as_file().sync_all().map_err(|e| e.to_string())?;
        file.persist(path).map_err(|e| e.to_string())?;
        Ok(())
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
            ..Default::default()
        };
        let decoded: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(decoded.volume, settings.volume);
        assert_eq!(decoded.radio_url, settings.radio_url);
    }

    #[test]
    fn preferences_replace_atomically_and_keep_theme_and_collection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        Settings::default().save_to(&path).unwrap();
        let mut settings = Settings {
            palette: Palette::light(),
            ..Default::default()
        };
        settings.stations.push(Station {
            name: "Jazz".into(),
            url: "https://example.com/live".into(),
        });
        settings.save_to(&path).unwrap();
        let loaded: Settings = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(loaded.palette, Palette::light());
        assert_eq!(loaded.stations[0].name, "Jazz");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
