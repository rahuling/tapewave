//! Debug-only native renderer regression harness. Does not play audio, fetch
//! feeds, install anything, or write user preferences. Captures its own wgpu
//! surface, not the desktop. Run: tapewave --ui-smoke-test <output-directory>.
use super::*;

pub(super) struct Harness {
    output: PathBuf,
    scene: usize,
    frames: usize,
    waiting: bool,
}

impl Harness {
    pub fn from_args() -> Option<Self> {
        if std::env::args_os().nth(1).as_deref() != Some(std::ffi::OsStr::new("--ui-smoke-test")) {
            return None;
        }
        let output = std::env::args_os().nth(2).map(PathBuf::from)?;
        Some(Self {
            output,
            scene: 0,
            frames: 0,
            waiting: false,
        })
    }
    pub fn update(&mut self, app: &mut Tapewave, ctx: &egui::Context) {
        const NAMES: [&str; 9] = [
            "music",
            "radio",
            "podcasts",
            "appearance",
            "paper",
            "compact-queue",
            "add-station",
            "empty",
            "compact-appearance",
        ];
        if self.scene >= NAMES.len() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.waiting {
            let image = ctx.input(|input| {
                input.events.iter().find_map(|event| match event {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(image) = image {
                let result = (|| -> Result<(), String> {
                    std::fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
                    let pixels: Vec<u8> = image
                        .pixels
                        .iter()
                        .flat_map(|pixel| pixel.to_array())
                        .collect();
                    image::save_buffer(
                        self.output.join(format!("{}.png", NAMES[self.scene])),
                        &pixels,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    )
                    .map_err(|e| e.to_string())
                })();
                if let Err(error) = result {
                    eprintln!("Screenshot failed: {error}");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
                self.scene += 1;
                self.frames = 0;
                self.waiting = false;
            }
        }
        if self.scene >= NAMES.len() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.frames == 0 {
            app.settings = settings();
            app.dialog = None;
            app.queue_open = false;
            app.settings.palette = Palette::default();
            app.queue.replace(app.settings.library.clone(), 0);
            app.status = Status {
                state: "playing".into(),
                position: 84.0,
                duration: Some(278.0),
                seekable: true,
                speed: 1.0,
                ..Default::default()
            };
            app.active = Some(app.settings.library[0].clone());
            app.tab = Tab::Music;
            app.selected_track = Some(1);
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([880.0, 640.0].into()));
            match self.scene {
                1 => {
                    app.tab = Tab::Radio;
                    app.active = Some(Item {
                        kind: Kind::Radio,
                        title: "Night Signal".into(),
                        url: "https://example.invalid/radio".into(),
                        ..Default::default()
                    });
                    app.status.seekable = false;
                    app.status.duration = None;
                    app.status.position = 728.0;
                    app.station_errors.insert(
                        "https://example.invalid/talk".into(),
                        "Station unavailable. Check the stream address or retry.".into(),
                    );
                }
                2 => {
                    app.tab = Tab::Podcasts;
                    app.selected_show = Some(app.settings.shows[0].url.clone());
                    app.active = Some(app.settings.shows[0].episodes[0].clone());
                    app.status.position = 1122.0;
                    app.status.duration = Some(2436.0);
                }
                3 => app.dialog = Some(Dialog::Appearance(Palette::default())),
                4 => app.settings.palette = Palette::light(),
                5 => {
                    app.queue_open = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([620.0, 480.0].into()));
                }
                6 => {
                    app.dialog = Some(Dialog::Station {
                        index: None,
                        name: "Night Signal".into(),
                        url: "https://example.invalid/stream".into(),
                        error: String::new(),
                    })
                }
                7 => {
                    app.settings.library.clear();
                    app.active = None;
                    app.status = Status::default();
                }
                8 => {
                    app.dialog = Some(Dialog::Appearance(Palette::default()));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([620.0, 480.0].into()));
                }
                _ => {}
            }
            app.poll_theme(ctx);
        }
        self.frames += 1;
        if self.frames == 5 && !self.waiting {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            self.waiting = true;
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }
}

pub(super) fn settings() -> Settings {
    let names = [
        ("A quieter place", "North Coast", "Soft Geometry", 278.0),
        (
            "Everything in its own time",
            "North Coast",
            "Soft Geometry",
            312.0,
        ),
        ("Sunday, slowly", "Low Season", "Open Windows", 226.0),
        ("Half light", "The Still Hours", "After the Rain", 247.0),
        ("Another way home", "Low Season", "Open Windows", 381.0),
        ("Between the lines", "North Coast", "Soft Geometry", 235.0),
        (
            "Nothing to hurry",
            "The Still Hours",
            "After the Rain",
            292.0,
        ),
    ];
    let library = names
        .into_iter()
        .map(|(title, subtitle, album, duration)| Item {
            title: title.into(),
            subtitle: subtitle.into(),
            album: album.into(),
            duration: Some(duration),
            url: format!("fixture:{title}"),
            ..Default::default()
        })
        .collect();
    let stations = [
        ("Night Signal", "radio"),
        ("Low Frequency", "ambient"),
        ("Room for Jazz", "jazz"),
        ("Open Dialogue", "talk"),
    ]
    .into_iter()
    .map(|(name, path)| Station {
        name: name.into(),
        url: format!("https://example.invalid/{path}"),
    })
    .collect();
    let episodes = [
        ("42. Making room for less", "2026-10-12", 2436.0),
        ("41. The things we keep", "2026-10-05", 2160.0),
        ("40. A practice of paying attention", "2026-09-28", 3120.0),
        ("39. Enough is a useful word", "2026-09-21", 2640.0),
    ]
    .into_iter()
    .enumerate()
    .map(|(i, (title, date, duration))| Item {
        kind: Kind::Podcast,
        title: title.into(),
        subtitle: "Field Notes".into(),
        url: format!("https://example.invalid/episode-{i}"),
        published: date.into(),
        duration: Some(duration),
        ..Default::default()
    })
    .collect::<Vec<_>>();
    let shows = vec![
        Show {
            title: "Field Notes".into(),
            url: "https://example.invalid/field-notes".into(),
            episodes: episodes.clone(),
        },
        Show {
            title: "Ordinary Things".into(),
            url: "https://example.invalid/ordinary".into(),
            episodes: Vec::new(),
        },
        Show {
            title: "Long Form".into(),
            url: "https://example.invalid/long-form".into(),
            episodes: Vec::new(),
        },
    ];
    let mut progress = std::collections::BTreeMap::new();
    progress.insert(
        episodes[1].key(),
        Progress {
            position: 496.0,
            duration: Some(2160.0),
            completed: false,
        },
    );
    progress.insert(
        episodes[3].key(),
        Progress {
            position: 2640.0,
            duration: Some(2640.0),
            completed: true,
        },
    );
    Settings {
        library,
        stations,
        shows,
        progress,
        volume: 0.65,
        ..Default::default()
    }
}
