//! Application state and actions. Browsing never changes playback; only an
//! explicit play action replaces a source or the finite queue.
#[cfg(debug_assertions)]
mod smoke;
mod views;

use crate::{
    library::{self, Item, Kind, Progress, Queue, Show, Station},
    native,
    settings::Settings,
    theme::{self, Palette},
};
use eframe::egui::{self, RichText};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};
use tapewave_engine::{Engine, Status, podcast};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Music,
    Radio,
    Podcasts,
}
impl Tab {
    fn index(self) -> usize {
        match self {
            Self::Music => 0,
            Self::Radio => 1,
            Self::Podcasts => 2,
        }
    }
}

enum Dialog {
    Station {
        index: Option<usize>,
        name: String,
        url: String,
        error: String,
    },
    Podcast {
        url: String,
        error: String,
    },
    Appearance(Palette),
    About,
}

enum Job {
    Music(Vec<Item>, Option<String>, Option<String>),
    Feed(String, Result<podcast::Podcast, String>),
}

pub struct Tapewave {
    engine: Engine,
    settings: Settings,
    status: Status,
    integration: Option<native::Integration>,
    tx: Sender<Job>,
    rx: Receiver<Job>,
    tab: Tab,
    search: [String; 3],
    queue: Queue,
    active: Option<Item>,
    active_id: u64,
    last_completed: u64,
    selected_track: Option<usize>,
    selected_show: Option<String>,
    selected_episode: Option<usize>,
    queue_open: bool,
    dialog: Option<Dialog>,
    effective_palette: Palette,
    high_contrast: bool,
    feed_busy: HashSet<String>,
    feed_errors: HashMap<String, String>,
    station_errors: HashMap<String, String>,
    scan_busy: bool,
    library_error: Option<String>,
    settings_error: Option<String>,
    seek_preview: Option<f64>,
    volume_before_mute: f32,
    last_save: Instant,
    dirty: bool,
    exit_requested: bool,
    last_theme_poll: Instant,
    #[cfg(debug_assertions)]
    smoke: Option<smoke::Harness>,
}

impl Tapewave {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Use the local Windows system font when available, with egui's bundled
        // fonts retained as fallback. No fonts or artwork are fetched online.
        let mut fonts = egui::FontDefinitions::default();
        if let Ok(bytes) = std::fs::read("C:\\Windows\\Fonts\\segoeui.ttf") {
            fonts
                .font_data
                .insert("segoe".into(), egui::FontData::from_owned(bytes).into());
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "segoe".into());
        }
        cc.egui_ctx.set_fonts(fonts);
        cc.egui_ctx.all_styles_mut(|style| {
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.button_padding = egui::vec2(12.0, 7.0);
            style.spacing.interact_size.y = 32.0;
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
            style
                .text_styles
                .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        });
        let mut app = Self::from_settings(Settings::load());
        #[cfg(debug_assertions)]
        if let Some(harness) = smoke::Harness::from_args() {
            app.settings = smoke::settings();
            app.smoke = Some(harness);
            app.poll_theme(&cc.egui_ctx);
            return app;
        }
        app.integration = Some(native::Integration::new(cc));
        app.poll_theme(&cc.egui_ctx);
        if let Some(path) = std::env::args_os().nth(1)
            && library::audio_file(std::path::Path::new(&path))
        {
            app.add_paths(vec![path.into()], true, cc.egui_ctx.clone());
        }
        app
    }

    fn from_settings(settings: Settings) -> Self {
        let (tx, rx) = mpsc::channel();
        let engine = Engine::spawn();
        engine.set_volume(settings.volume);
        let selected_show = settings.shows.first().map(|show| show.url.clone());
        Self {
            engine,
            status: Status::default(),
            integration: None,
            effective_palette: settings.palette.clone(),
            settings,
            tx,
            rx,
            tab: Tab::Music,
            search: Default::default(),
            queue: Queue::default(),
            active: None,
            active_id: 0,
            last_completed: 0,
            selected_track: None,
            selected_show,
            selected_episode: None,
            queue_open: false,
            dialog: None,
            high_contrast: false,
            feed_busy: HashSet::new(),
            feed_errors: HashMap::new(),
            station_errors: HashMap::new(),
            scan_busy: false,
            library_error: None,
            settings_error: None,
            seek_preview: None,
            volume_before_mute: 1.0,
            last_save: Instant::now(),
            dirty: false,
            exit_requested: false,
            last_theme_poll: Instant::now() - Duration::from_secs(2),
            #[cfg(debug_assertions)]
            smoke: None,
        }
    }

    fn poll_theme(&mut self, ctx: &egui::Context) {
        let windows = native::high_contrast();
        self.high_contrast = windows.is_some();
        let requested = match &self.dialog {
            Some(Dialog::Appearance(draft)) if draft.readable() => draft.clone(),
            _ => self.settings.palette.clone(),
        };
        self.effective_palette = windows.unwrap_or(requested);
        self.effective_palette.apply(ctx);
        self.last_theme_poll = Instant::now();
    }

    fn persist(&mut self) {
        #[cfg(debug_assertions)]
        if self.smoke.is_some() {
            return;
        }
        self.capture_progress();
        match self.settings.save() {
            Ok(()) => {
                self.dirty = false;
                self.settings_error = None;
            }
            Err(error) => {
                self.settings_error = Some(format!("Could not save preferences: {error}"))
            }
        }
        self.last_save = Instant::now();
    }

    fn capture_progress(&mut self) {
        let Some(item) = self
            .active
            .as_ref()
            .filter(|item| item.kind == Kind::Podcast)
        else {
            return;
        };
        if self.status.request_id != self.active_id
            || self.status.state == "loading"
            || self.status.source.is_none()
        {
            return;
        }
        let old = self
            .settings
            .progress
            .get(&item.key())
            .cloned()
            .unwrap_or_default();
        // A failed prepare must never overwrite a previously saved position.
        if self.status.error.is_some() && self.status.position == 0.0 {
            return;
        }
        self.settings.progress.insert(
            item.key(),
            Progress {
                position: self.status.position.max(0.0),
                duration: self.status.duration,
                completed: self.status.finished || (old.completed && self.status.position == 0.0),
            },
        );
        self.dirty = true;
    }

    fn start(&mut self, item: Item) {
        self.capture_progress();
        if item.kind != Kind::Music && !library::valid_url(&item.url) {
            return;
        }
        self.engine.set_speed(1.0);
        self.active_id = match item.kind {
            Kind::Music => match &item.path {
                Some(path) => self.engine.play_file(path.clone()),
                None => return,
            },
            Kind::Radio => {
                self.station_errors.remove(&item.url);
                self.engine.play_url(item.url.clone())
            }
            Kind::Podcast => {
                let at = self
                    .settings
                    .progress
                    .get(&item.key())
                    .map_or(0.0, Progress::resume_at);
                self.engine.play_episode(item.url.clone(), at)
            }
        };
        self.status = Status {
            state: "loading".into(),
            request_id: self.active_id,
            ..Default::default()
        };
        self.active = Some(item);
        self.seek_preview = None;
    }

    fn play_music(&mut self, index: usize) {
        if index >= self.settings.library.len() {
            return;
        }
        // Queue the current filtered collection, not hidden tracks.
        let items: Vec<_> = self
            .settings
            .library
            .iter()
            .filter(|item| item.matches(&self.search[0]))
            .cloned()
            .collect();
        let key = self.settings.library[index].key();
        if let Some(at) = items.iter().position(|item| item.key() == key) {
            self.queue.replace(items, at);
            self.start(self.queue.items[at].clone());
        }
    }

    fn play_episode(&mut self, url: &str, index: usize) {
        if let Some(show) = self.settings.shows.iter().find(|show| show.url == url) {
            if index >= show.episodes.len() {
                return;
            }
            self.queue.replace(show.episodes.clone(), index);
            self.queue.shuffle = false;
            self.queue.repeat = false;
            self.start(self.queue.items[index].clone());
        }
    }

    fn toggle(&mut self) {
        if self
            .active
            .as_ref()
            .is_some_and(|item| item.kind == Kind::Radio)
        {
            if self.status.state == "playing" || self.status.state == "loading" {
                self.stop();
            } else if let Some(item) = self.active.clone() {
                self.start(item);
            }
            return;
        }
        match self.status.state.as_str() {
            "playing" => self.engine.pause(),
            "paused" => self.engine.resume(),
            "loading" => self.stop(),
            _ => {
                if let Some(item) = self.active.clone() {
                    self.start(item);
                } else if let Some(index) = self.selected_track {
                    self.play_music(index);
                }
            }
        }
    }

    fn stop(&mut self) {
        self.capture_progress();
        self.engine.stop();
        self.status.state = "stopped".into();
        self.status.position = 0.0;
        self.status.source = None;
        self.status.seekable = false;
        self.status.finished = false;
        // Prevent a stale, pre-stop worker snapshot from auto-advancing.
        self.active_id = 0;
        self.persist();
    }

    fn next(&mut self) {
        if self
            .active
            .as_ref()
            .is_some_and(|item| item.kind == Kind::Radio)
        {
            return;
        }
        if let Some(index) = self.queue.next_index() {
            self.start(self.queue.items[index].clone());
        }
    }
    fn previous(&mut self) {
        if self
            .active
            .as_ref()
            .is_some_and(|item| item.kind == Kind::Radio)
        {
            return;
        }
        if self.status.position > 3.0 && self.status.seekable {
            self.engine.seek(0.0);
        } else if let Some(index) = self.queue.previous_index() {
            self.start(self.queue.items[index].clone());
        }
    }

    fn request_files(&mut self, folder: bool, ctx: egui::Context) {
        if self.scan_busy {
            return;
        }
        self.scan_busy = true;
        self.library_error = None;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let picker = rfd::FileDialog::new();
            let paths = if folder {
                picker.pick_folder().into_iter().collect()
            } else {
                picker
                    .add_filter("Audio", &["mp3", "flac", "ogg", "wav", "aac", "m4a", "mp4"])
                    .pick_files()
                    .unwrap_or_default()
            };
            let (items, error) = scan(paths);
            let _ = tx.send(Job::Music(items, error, None));
            ctx.request_repaint();
        });
    }
    fn add_paths(&mut self, paths: Vec<PathBuf>, play: bool, ctx: egui::Context) {
        if self.scan_busy {
            return;
        }
        self.scan_busy = true;
        // An explicitly-opened file starts after the background metadata scan;
        // a dropped collection is added without interrupting playback.
        let play_key = play
            .then(|| {
                paths
                    .first()
                    .map(|path| format!("file:{}", path.to_string_lossy()))
            })
            .flatten();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let (items, error) = scan(paths);
            let _ = tx.send(Job::Music(items, error, play_key));
            ctx.request_repaint();
        });
    }

    fn load_feed(&mut self, url: String, ctx: egui::Context) {
        if !library::valid_url(&url) || !self.feed_busy.insert(url.clone()) {
            return;
        }
        self.feed_errors.remove(&url);
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let result = podcast::fetch(&url);
            let _ = tx.send(Job::Feed(url, result));
            ctx.request_repaint();
        });
    }

    fn jobs(&mut self) {
        while let Ok(job) = self.rx.try_recv() {
            match job {
                Job::Music(items, error, play_key) => {
                    self.scan_busy = false;
                    self.library_error = error;
                    for item in items {
                        if !self
                            .settings
                            .library
                            .iter()
                            .any(|existing| existing.path == item.path)
                        {
                            self.settings.library.push(item);
                        }
                    }
                    if let Some(key) = play_key
                        && let Some(index) = self
                            .settings
                            .library
                            .iter()
                            .position(|item| item.key() == key)
                    {
                        self.selected_track = Some(index);
                        self.play_music(index);
                    }
                    self.dirty = true;
                }
                Job::Feed(url, result) => {
                    self.feed_busy.remove(&url);
                    match result {
                        Ok(feed) => {
                            let episodes = feed
                                .episodes
                                .into_iter()
                                .filter(|ep| library::valid_url(&ep.audio_url))
                                .map(|ep| Item {
                                    kind: Kind::Podcast,
                                    title: ep.title,
                                    subtitle: feed.title.clone(),
                                    url: ep.audio_url,
                                    published: ep.published.unwrap_or_default(),
                                    description: ep.description,
                                    duration: ep.duration,
                                    ..Default::default()
                                })
                                .collect();
                            if let Some(show) =
                                self.settings.shows.iter_mut().find(|show| show.url == url)
                            {
                                show.title = feed.title;
                                show.episodes = episodes;
                            } else {
                                self.settings.shows.push(Show {
                                    title: feed.title,
                                    url: url.clone(),
                                    episodes,
                                });
                                self.selected_show = Some(url);
                            }
                            self.dirty = true;
                        }
                        Err(error) => {
                            self.feed_errors.insert(url, error);
                        }
                    }
                }
            }
        }
    }

    fn keyboard(&mut self, ctx: &egui::Context) {
        if self.dialog.is_some() {
            return;
        }
        let (open, toggle, stop) = ctx.input(|input| {
            (
                input.modifiers.ctrl && input.key_pressed(egui::Key::O),
                input.modifiers.ctrl && input.key_pressed(egui::Key::P),
                input.modifiers.ctrl && input.key_pressed(egui::Key::S),
            )
        });
        if open {
            self.request_files(false, ctx.clone());
        }
        if toggle {
            self.toggle();
        }
        if stop {
            self.stop();
        }
        // Text input and focused buttons keep their own Space behaviour.
        if ctx.memory(|memory| memory.focused().is_none())
            && ctx.input(|input| input.key_pressed(egui::Key::Space))
        {
            self.toggle();
        }
    }
}

impl eframe::App for Tapewave {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(debug_assertions)]
        if let Some(mut harness) = self.smoke.take() {
            harness.update(self, ctx);
            self.smoke = Some(harness);
            return;
        }
        let mut restoring = false;
        if let Some(integration) = &self.integration {
            let events: Vec<_> = integration.events.try_iter().collect();
            for event in events {
                match event {
                    native::Event::Restore => {
                        restoring = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    native::Event::Toggle => self.toggle(),
                    native::Event::Stop => self.stop(),
                    native::Event::Next => self.next(),
                    native::Event::Previous => self.previous(),
                    native::Event::Exit => {
                        self.exit_requested = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
            }
        }
        self.jobs();
        let status = self.engine.status();
        if self.active_id != 0 && status.request_id == self.active_id {
            self.status = status;
            if let Some(item) = self.active.as_mut() {
                if self.status.duration.is_some() {
                    item.duration = self.status.duration;
                }
                if item.kind == Kind::Radio
                    && let Some(error) = &self.status.error
                {
                    self.station_errors.insert(item.url.clone(), error.clone());
                }
            }
            if self.status.finished && self.last_completed != self.active_id {
                self.last_completed = self.active_id;
                self.capture_progress();
                if self
                    .active
                    .as_ref()
                    .is_some_and(|item| item.kind != Kind::Radio)
                {
                    self.next();
                }
            }
        } else if self.active.is_none() && status.error.is_some() {
            self.status = status;
        }
        if self.last_save.elapsed() > Duration::from_secs(5) {
            self.capture_progress();
            if self.dirty {
                self.persist();
            } else {
                self.last_save = Instant::now();
            }
        }
        if self.last_theme_poll.elapsed() > Duration::from_secs(1) {
            self.poll_theme(ctx);
        }
        if ctx.input(|input| input.viewport().close_requested()) || self.exit_requested {
            self.persist();
        }
        if !restoring
            && ctx.input(|input| input.viewport().minimized == Some(true))
            && self.integration.as_ref().is_some_and(|integration| {
                integration
                    .tray_ready
                    .load(std::sync::atomic::Ordering::Relaxed)
            })
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
        ctx.request_repaint_after(Duration::from_millis(200));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        #[cfg(debug_assertions)]
        if self.smoke.is_some() {
            ctx.input_mut(|input| input.pointer = Default::default());
        }
        self.keyboard(&ctx);
        let drops = ctx.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect::<Vec<_>>()
        });
        if !drops.is_empty() {
            self.add_paths(drops, false, ctx.clone());
        }
        self.render(ui);
        self.dialogs(&ctx);
    }

    fn on_exit(&mut self) {
        self.persist();
        self.engine.stop();
    }
}

fn scan(paths: Vec<PathBuf>) -> (Vec<Item>, Option<String>) {
    let mut files = Vec::new();
    let mut skipped = 0usize;
    for path in paths {
        if path.is_dir() {
            for entry in walkdir::WalkDir::new(&path)
                .follow_links(false)
                .into_iter()
                .take(50_000)
            {
                match entry {
                    Ok(entry)
                        if entry.file_type().is_file() && library::audio_file(entry.path()) =>
                    {
                        files.push(entry.path().to_path_buf())
                    }
                    Err(_) => skipped += 1,
                    _ => {}
                }
                if files.len() >= 10_000 {
                    break;
                }
            }
        } else if library::audio_file(&path) {
            files.push(path);
        }
        if files.len() >= 10_000 {
            break;
        }
    }
    files.sort();
    files.dedup();
    files.truncate(10_000);
    let count = files.len();
    let error = if count >= 10_000 {
        Some("Import limited to 10,000 tracks per operation. Add other folders separately.".into())
    } else if skipped > 0 {
        Some(format!("Skipped {skipped} inaccessible files or folders."))
    } else {
        None
    };
    (files.into_iter().map(library::read_track).collect(), error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_sections_render_at_normal_and_compact_sizes() {
        for size in [[880.0, 640.0], [620.0, 480.0]] {
            for tab in [Tab::Music, Tab::Radio, Tab::Podcasts] {
                let ctx = egui::Context::default();
                let mut app = Tapewave::from_settings(smoke::settings());
                app.tab = tab;
                app.effective_palette.apply(&ctx);
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
                        ..Default::default()
                    },
                    |ui| app.render(ui),
                );
                // This is a layout-only test, not a GPU integration. Explicitly
                // acknowledge font texture uploads rather than leaking deltas.
                output.textures_delta.clear();
                assert!(!output.shapes.is_empty());
                assert!(
                    ctx.globally_used_rect().max.x <= size[0] + 1.0,
                    "layout overflow at {size:?}: {:?}",
                    ctx.globally_used_rect()
                );
            }
        }
    }

    #[test]
    fn browsing_sources_preserves_queue_and_playback_selection() {
        let mut app = Tapewave::from_settings(smoke::settings());
        app.queue.replace(app.settings.library.clone(), 0);
        app.active = Some(app.queue.items[0].clone());
        let key = app.active.as_ref().unwrap().key();
        app.tab = Tab::Radio;
        app.tab = Tab::Podcasts;
        assert_eq!(app.active.as_ref().unwrap().key(), key);
        assert_eq!(app.queue.current, Some(0));
    }
}
