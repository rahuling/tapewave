//! Persistable collection and queue state; independent of windows and audio hardware.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    #[default]
    Music,
    Radio,
    Podcast,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    pub kind: Kind,
    pub title: String,
    pub subtitle: String,
    pub album: String,
    // Keep Windows paths as paths, not lossy UTF-8 strings.
    pub path: Option<PathBuf>,
    pub url: String,
    pub duration: Option<f64>,
    pub published: String,
    pub description: String,
}

impl Item {
    pub fn key(&self) -> String {
        if let Some(path) = &self.path {
            format!("file:{}", path.to_string_lossy())
        } else {
            self.url.clone()
        }
    }
    pub fn matches(&self, search: &str) -> bool {
        let search = search.to_lowercase();
        format!("{} {} {}", self.title, self.subtitle, self.album)
            .to_lowercase()
            .contains(&search)
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Station {
    pub name: String,
    pub url: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Show {
    pub title: String,
    pub url: String,
    pub episodes: Vec<Item>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Progress {
    pub position: f64,
    pub duration: Option<f64>,
    pub completed: bool,
}

impl Progress {
    pub fn resume_at(&self) -> f64 {
        if self.completed || !self.position.is_finite() {
            0.0
        } else {
            self.position.max(0.0)
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Queue {
    pub items: Vec<Item>,
    pub current: Option<usize>,
    pub repeat: bool,
    pub shuffle: bool,
    history: Vec<usize>,
    shuffle_seen: Vec<usize>,
    forced_next: Vec<usize>,
    rng: u64,
}

impl Queue {
    pub fn has_next(&self) -> bool {
        let Some(current) = self.current.filter(|index| *index < self.items.len()) else {
            return false;
        };
        if self.repeat {
            return true;
        }
        if self
            .forced_next
            .iter()
            .any(|index| *index != current && *index < self.items.len())
        {
            return true;
        }
        if self.shuffle && self.items.len() > 1 {
            return (0..self.items.len())
                .any(|index| index != current && !self.shuffle_seen.contains(&index));
        }
        current + 1 < self.items.len()
    }
    pub fn has_previous(&self) -> bool {
        !self.history.is_empty()
            || self
                .current
                .is_some_and(|index| index > 0 && index < self.items.len())
    }
    pub fn replace(&mut self, items: Vec<Item>, index: usize) {
        self.items = items;
        self.current = (index < self.items.len()).then_some(index);
        self.history.clear();
        self.shuffle_seen.clear();
        self.forced_next.clear();
    }
    pub fn next_index(&mut self) -> Option<usize> {
        let current = self.current?;
        let len = self.items.len();
        if len == 0 {
            return None;
        }
        let forced = std::iter::from_fn(|| self.forced_next.pop())
            .find(|index| *index < len && *index != current);
        let next = if let Some(index) = forced {
            if self.shuffle {
                if !self.shuffle_seen.contains(&current) {
                    self.shuffle_seen.push(current);
                }
                if !self.shuffle_seen.contains(&index) {
                    self.shuffle_seen.push(index);
                }
            }
            index
        } else if self.shuffle && len > 1 {
            if !self.shuffle_seen.contains(&current) {
                self.shuffle_seen.push(current);
            }
            let mut remaining: Vec<_> = (0..len)
                .filter(|index| !self.shuffle_seen.contains(index))
                .collect();
            if remaining.is_empty() {
                if !self.repeat {
                    return None;
                }
                self.shuffle_seen.clear();
                self.shuffle_seen.push(current);
                remaining = (0..len).filter(|index| *index != current).collect();
            }
            // A small non-cryptographic generator, seeded once per queue.
            if self.rng == 0 {
                self.rng = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(1)
                    .max(1);
            }
            self.rng ^= self.rng << 13;
            self.rng ^= self.rng >> 7;
            self.rng ^= self.rng << 17;
            let index = remaining[self.rng as usize % remaining.len()];
            self.shuffle_seen.push(index);
            index
        } else if current + 1 < len {
            current + 1
        } else if self.repeat {
            0
        } else {
            return None;
        };
        self.history.push(current);
        if self.history.len() > 512 {
            self.history.remove(0);
        }
        self.current = Some(next);
        Some(next)
    }
    pub fn previous_index(&mut self) -> Option<usize> {
        let previous = self
            .history
            .pop()
            .or_else(|| self.current?.checked_sub(1))?;
        self.current = Some(previous);
        Some(previous)
    }
    pub fn insert_next(&mut self, item: Item) {
        let index = self
            .current
            .map_or(self.items.len(), |i| (i + 1).min(self.items.len()));
        self.items.insert(index, item);
        for old in &mut self.history {
            if *old >= index {
                *old += 1;
            }
        }
        for old in &mut self.shuffle_seen {
            if *old >= index {
                *old += 1;
            }
        }
        for old in &mut self.forced_next {
            if *old >= index {
                *old += 1;
            }
        }
        self.forced_next.push(index);
    }
    pub fn remove(&mut self, index: usize) {
        if index >= self.items.len() {
            return;
        }
        self.items.remove(index);
        self.current = self.current.and_then(|current| {
            if current == index {
                None
            } else if current > index {
                Some(current - 1)
            } else {
                Some(current)
            }
        });
        self.history = self
            .history
            .iter()
            .copied()
            .filter(|old| *old != index)
            .map(|old| if old > index { old - 1 } else { old })
            .collect();
        self.shuffle_seen = self
            .shuffle_seen
            .iter()
            .copied()
            .filter(|old| *old != index)
            .map(|old| if old > index { old - 1 } else { old })
            .collect();
        self.forced_next = self
            .forced_next
            .iter()
            .copied()
            .filter(|old| *old != index)
            .map(|old| if old > index { old - 1 } else { old })
            .collect();
    }
}

pub fn valid_url(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://"))
        && !value.chars().any(|c| c.is_whitespace() || c.is_control())
        && value.split_once("://").is_some_and(|(_, rest)| {
            let host = rest.split(['/', '?', '#']).next().unwrap_or("");
            !host.is_empty() && !host.contains('@')
        })
}

pub fn audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|x| x.to_str())
        .is_some_and(|ext| {
            ["mp3", "flac", "ogg", "wav", "aac", "m4a", "mp4"]
                .iter()
                .any(|x| ext.eq_ignore_ascii_case(x))
        })
}

/// Called on a background thread. Ignore unreadable tags; let playback report
/// decoding failures rather than hiding a user's file from the collection.
pub fn read_track(path: PathBuf) -> Item {
    use lofty::{
        file::{AudioFile, TaggedFileExt},
        probe::Probe,
        tag::Accessor,
    };
    let title = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let mut item = Item {
        title,
        path: Some(path.clone()),
        ..Default::default()
    };
    if let Ok(tagged) = Probe::open(&path).and_then(|probe| probe.read()) {
        let duration = tagged.properties().duration().as_secs_f64();
        item.duration = (duration > 0.0).then_some(duration);
        if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
            if let Some(title) = tag.title() {
                item.title = title.into_owned();
            }
            if let Some(artist) = tag.artist() {
                item.subtitle = artist.into_owned();
            }
            if let Some(album) = tag.album() {
                item.album = album.into_owned();
            }
        }
    }
    item
}

pub fn time(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "--:--".into();
    }
    let seconds = seconds as u64;
    if seconds >= 3600 {
        format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        )
    } else {
        format!("{}:{:02}", seconds / 60, seconds % 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item(title: &str) -> Item {
        Item {
            title: title.into(),
            ..Default::default()
        }
    }
    #[test]
    fn queue_bounds_repeat_and_history() {
        let mut q = Queue::default();
        assert_eq!(q.next_index(), None);
        q.replace(vec![item("A"), item("B")], 0);
        assert!(q.has_next());
        assert!(!q.has_previous());
        assert_eq!(q.next_index(), Some(1));
        assert!(!q.has_next());
        assert!(q.has_previous());
        assert_eq!(q.next_index(), None);
        q.repeat = true;
        assert_eq!(q.next_index(), Some(0));
        assert_eq!(q.previous_index(), Some(1));
    }
    #[test]
    fn radio_does_not_need_to_mutate_the_finite_queue() {
        let mut q = Queue::default();
        q.replace(vec![item("A"), item("B")], 0);
        q.insert_next(item("C"));
        assert_eq!(q.items[1].title, "C");
        assert_eq!(q.current, Some(0));
    }
    #[test]
    fn deleting_queue_items_keeps_history_indices_valid() {
        let mut q = Queue::default();
        q.replace(vec![item("A"), item("B"), item("C")], 0);
        q.next_index();
        q.next_index();
        q.remove(0);
        assert_eq!(q.current, Some(1));
        assert_eq!(q.previous_index(), Some(0));
        q.remove(0);
        assert_eq!(q.previous_index(), None);
    }
    #[test]
    fn shuffle_does_not_immediately_repeat_a_track() {
        let mut q = Queue {
            shuffle: true,
            repeat: true,
            rng: 123,
            ..Default::default()
        };
        q.replace(vec![item("A"), item("B"), item("C")], 0);
        for _ in 0..30 {
            let old = q.current;
            assert_ne!(q.next_index(), old);
        }
    }

    #[test]
    fn shuffle_without_repeat_finishes_after_each_track_once() {
        let mut q = Queue {
            shuffle: true,
            rng: 123,
            ..Default::default()
        };
        q.replace(vec![item("A"), item("B"), item("C")], 0);
        let a = q.next_index().unwrap();
        let b = q.next_index().unwrap();
        assert_ne!(a, b);
        assert_ne!(a, 0);
        assert_ne!(b, 0);
        assert_eq!(q.next_index(), None);
    }
    #[test]
    fn rejects_bad_sources_and_completed_resume() {
        for url in [
            "https://",
            "https:///file",
            "file:///x",
            "https://bad host",
            "https://user:pass@host",
        ] {
            assert!(!valid_url(url));
        }
        assert!(valid_url("HTTPS://example.com/audio?x=1"));
        assert_eq!(
            Progress {
                position: 40.0,
                completed: true,
                ..Default::default()
            }
            .resume_at(),
            0.0
        );
        assert_eq!(time(3661.0), "1:01:01");
        assert_eq!(time(f64::NAN), "--:--");
    }

    #[test]
    fn play_next_takes_priority_even_when_shuffle_is_enabled() {
        let mut q = Queue {
            shuffle: true,
            rng: 123,
            ..Default::default()
        };
        q.replace(vec![item("A"), item("B"), item("C")], 0);
        q.insert_next(item("Requested next"));
        let index = q.next_index().unwrap();
        assert_eq!(q.items[index].title, "Requested next");
    }
}
