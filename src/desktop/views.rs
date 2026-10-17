//! Quiet Hi-Fi view layer. Controls use the same semantic palette, including
//! dialogs, focus, selection and custom vector transport icons.
use super::*;
use crate::library::time;
use crate::theme::color;
use egui::{Align, Align2, Color32, FontId, Layout, Sense, Stroke, StrokeKind, Vec2};

#[derive(Clone, Copy)]
enum Icon {
    Previous,
    Play,
    Pause,
    Next,
    Stop,
}

impl Tapewave {
    pub(super) fn render(&mut self, ui: &mut egui::Ui) {
        let p = self.effective_palette.clone();
        egui::Panel::top("menu")
            .exact_size(29.0)
            .frame(
                egui::Frame::new()
                    .fill(color(p.panel))
                    .inner_margin(egui::Margin::symmetric(16, 0)),
            )
            .show(ui, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.menu_button("File", |ui| {
                        if ui.button("Add music files…    Ctrl+O").clicked() {
                            self.request_files(false, ui.ctx().clone());
                            ui.close();
                        }
                        if ui.button("Add music folder…").clicked() {
                            self.request_files(true, ui.ctx().clone());
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Exit").clicked() {
                            self.exit_requested = true;
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                    ui.menu_button("Playback", |ui| {
                        if ui.button("Play / Pause    Ctrl+P").clicked() {
                            self.toggle();
                            ui.close();
                        }
                        if ui.button("Stop    Ctrl+S").clicked() {
                            self.stop();
                            ui.close();
                        }
                        if ui.button("Previous").clicked() {
                            self.previous();
                            ui.close();
                        }
                        if ui.button("Next").clicked() {
                            self.next();
                            ui.close();
                        }
                    });
                    ui.menu_button("View", |ui| {
                        ui.checkbox(&mut self.queue_open, "Playback queue");
                        if ui.button("Appearance…").clicked() {
                            self.dialog = Some(Dialog::Appearance(self.settings.palette.clone()));
                            ui.close();
                        }
                    });
                    ui.menu_button("Help", |ui| {
                        if ui.button("About Tapewave").clicked() {
                            self.dialog = Some(Dialog::About);
                            ui.close();
                        }
                    });
                });
            });
        egui::Panel::top("player")
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(color(p.panel))
                    .inner_margin(egui::Margin::symmetric(24, 16)),
            )
            .show(ui, |ui| self.player(ui));
        egui::Panel::top("sources")
            .exact_size(48.0)
            .frame(
                egui::Frame::new()
                    .fill(color(p.background))
                    .inner_margin(egui::Margin::symmetric(24, 4)),
            )
            .show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    for (tab, label) in [
                        (Tab::Music, "Music"),
                        (Tab::Radio, "Radio"),
                        (Tab::Podcasts, "Podcasts"),
                    ] {
                        let selected = self.tab == tab;
                        let response = ui.add(
                            egui::Button::new(RichText::new(label).color(color(if selected {
                                p.text
                            } else {
                                p.secondary
                            })))
                            .frame(false)
                            .min_size(egui::vec2(74.0, 36.0)),
                        );
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::SelectableLabel,
                                true,
                                selected,
                                label,
                            )
                        });
                        if selected {
                            let r = response.rect;
                            ui.painter().line_segment(
                                [
                                    egui::pos2(r.left(), r.bottom()),
                                    egui::pos2(r.right(), r.bottom()),
                                ],
                                Stroke::new(2.0, color(p.accent)),
                            );
                        }
                        if response.clicked() {
                            self.tab = tab;
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .selectable_label(
                                self.queue_open,
                                format!("Queue  {}", self.queue.items.len()),
                            )
                            .clicked()
                        {
                            self.queue_open = !self.queue_open;
                        }
                    });
                });
            });
        egui::Panel::bottom("status")
            .exact_size(29.0)
            .frame(
                egui::Frame::new()
                    .fill(color(p.panel))
                    .inner_margin(egui::Margin::symmetric(24, 4)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let label = if self.scan_busy {
                        "Reading your music…".into()
                    } else if let Some(error) = &self.settings_error {
                        error.clone()
                    } else if self.status.state == "loading"
                        && self
                            .active
                            .as_ref()
                            .is_some_and(|item| item.kind == Kind::Podcast)
                    {
                        "Preparing episode for seeking and resume…".into()
                    } else if self.status.state == "loading" {
                        "Connecting / opening audio…".into()
                    } else {
                        match self.status.state.as_str() {
                            "playing" => "Playing",
                            "paused" => "Paused",
                            _ => "Ready",
                        }
                        .into()
                    };
                    ui.add(
                        egui::Label::new(RichText::new(label).size(12.0).color(color(p.secondary)))
                            .truncate(),
                    );
                    if self.high_contrast {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new("Windows high contrast").small());
                        });
                    }
                });
            });
        if self.queue_open {
            egui::Panel::right("queue")
                .default_size(250.0)
                .min_size(200.0)
                .max_size(320.0)
                .frame(egui::Frame::new().fill(color(p.panel)).inner_margin(16))
                .show(ui, |ui| self.queue_view(ui));
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(color(p.background))
                    .inner_margin(egui::Margin::symmetric(24, 16)),
            )
            .show(ui, |ui| match self.tab {
                Tab::Music => self.music(ui),
                Tab::Radio => self.radio(ui),
                Tab::Podcasts => self.podcasts(ui),
            });
    }

    fn player(&mut self, ui: &mut egui::Ui) {
        let p = self.effective_palette.clone();
        let radio = self
            .active
            .as_ref()
            .is_some_and(|item| item.kind == Kind::Radio);
        let podcast = self
            .active
            .as_ref()
            .is_some_and(|item| item.kind == Kind::Podcast);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(52.0, 52.0), Sense::hover());
            ui.painter().rect_filled(rect, 3.0, color(p.selection));
            let centre = rect.center();
            ui.painter()
                .circle_stroke(centre, 17.0, Stroke::new(1.3, color(p.secondary)));
            ui.painter()
                .circle_stroke(centre, 9.0, Stroke::new(1.0, color(p.secondary)));
            ui.painter().circle_filled(centre, 3.0, color(p.accent));
            ui.add_space(7.0);
            ui.allocate_ui_with_layout(
                egui::vec2((ui.available_width() - 90.0).max(100.0), 52.0),
                Layout::top_down(Align::Min),
                |ui| {
                    let title = self
                        .active
                        .as_ref()
                        .map(|item| item.title.as_str())
                        .unwrap_or("Ready when you are");
                    ui.add(egui::Label::new(RichText::new(title).size(19.0).strong()).truncate())
                        .on_hover_text(title);
                    let subtitle = self
                        .active
                        .as_ref()
                        .map(|item| {
                            if item.subtitle.is_empty() {
                                if item.kind == Kind::Radio {
                                    "Live audio stream".into()
                                } else {
                                    "Local audio".into()
                                }
                            } else if item.album.is_empty() {
                                item.subtitle.clone()
                            } else {
                                format!("{} · {}", item.subtitle, item.album)
                            }
                        })
                        .unwrap_or_else(|| {
                            "Your music, radio and podcasts. All in one place.".into()
                        });
                    ui.add(
                        egui::Label::new(
                            RichText::new(subtitle).size(13.0).color(color(p.secondary)),
                        )
                        .truncate(),
                    );
                },
            );
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                ui.label(
                    RichText::new(if radio {
                        "LIVE RADIO"
                    } else if podcast {
                        "PODCAST"
                    } else {
                        "MUSIC"
                    })
                    .size(11.0)
                    .color(color(if radio { p.accent } else { p.secondary })),
                );
            });
        });
        ui.add_space(9.0);
        if radio {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(match self.status.state.as_str() {
                        "playing" => "Listening live · No timeshift",
                        "loading" => "Connecting to station…",
                        _ => "Disconnected",
                    })
                    .size(12.0)
                    .color(color(p.secondary)),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.monospace(if self.status.state == "playing" {
                        time(self.status.position)
                    } else {
                        "--:--".into()
                    });
                });
            });
        } else {
            let duration = self
                .status
                .duration
                .or_else(|| self.active.as_ref().and_then(|item| item.duration));
            let enabled = self.status.seekable && duration.is_some_and(|d| d > 0.0);
            ui.horizontal(|ui|{
                ui.label(RichText::new(time(self.seek_preview.unwrap_or(self.status.position))).monospace().size(12.0));
                let mut position=self.seek_preview.unwrap_or(self.status.position);
                ui.spacing_mut().slider_width=(ui.available_width()-65.0).max(60.0);
                let response=ui.add_enabled(enabled,egui::Slider::new(&mut position,0.0..=duration.unwrap_or(1.0).max(1.0)).show_value(false).handle_shape(egui::style::HandleShape::Circle));
                response.widget_info(||egui::WidgetInfo::slider(enabled,position,"Playback position"));
                paint_slider(ui,&response,position,duration.unwrap_or(1.0).max(1.0),enabled,&p);
                if response.dragged(){self.seek_preview=Some(position);}
                if response.drag_stopped() || (response.changed() && !response.dragged()) {self.engine.seek(position);self.seek_preview=None;}
                if !enabled {response.on_hover_text("Seeking becomes available when a finite audio file has been prepared and its duration is known.");}
                ui.label(RichText::new(duration.map(time).unwrap_or_else(||"--:--".into())).monospace().size(12.0));
            });
        }
        ui.add_space(5.0);
        let compact = ui.available_width() < 760.0;
        ui.horizontal(|ui| {
            let finite = self.active.is_some() && !radio;
            if icon_button(
                ui,
                Icon::Previous,
                "Previous",
                finite
                    && (self.queue.has_previous()
                        || (self.status.seekable && self.status.position > 3.0)),
                &p,
            )
            .clicked()
            {
                self.previous();
            }
            let playing = self.status.state == "playing";
            let loading = self.status.state == "loading";
            let label = if radio {
                if playing || loading {
                    "Disconnect"
                } else {
                    "Connect"
                }
            } else if loading {
                "Cancel"
            } else if playing {
                "Pause"
            } else {
                "Play"
            };
            let icon = if (radio && (playing || loading)) || (!radio && loading) {
                Icon::Stop
            } else if playing {
                Icon::Pause
            } else {
                Icon::Play
            };
            if primary_button(
                ui,
                icon,
                label,
                self.active.is_some() || self.selected_track.is_some(),
                &p,
            )
            .clicked()
            {
                self.toggle();
            }
            if icon_button(ui, Icon::Next, "Next", finite && self.queue.has_next(), &p).clicked() {
                self.next();
            }
            if ui
                .add_enabled(
                    self.active.is_some() && !radio,
                    egui::Button::new("Stop").frame(false),
                )
                .clicked()
            {
                self.stop();
            }
            if podcast && !compact {
                self.podcast_controls(ui);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| self.volume(ui));
        });
        if podcast && compact {
            ui.horizontal(|ui| self.podcast_controls(ui));
        }
        if let Some(error) = self.status.error.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(color(p.warning), error);
                if self.active.is_some()
                    && ui.small_button("Retry").clicked()
                    && let Some(item) = self.active.clone()
                {
                    self.start(item);
                }
            });
        }
    }

    fn volume(&mut self, ui: &mut egui::Ui) {
        let mut volume = self.settings.volume;
        ui.spacing_mut().slider_width = 78.0;
        ui.label(
            RichText::new(format!("{}%", (volume * 100.0).round()))
                .small()
                .monospace(),
        );
        let response = ui.add(
            egui::Slider::new(&mut volume, 0.0..=1.0)
                .show_value(false)
                .handle_shape(egui::style::HandleShape::Circle),
        );
        response.widget_info(|| egui::WidgetInfo::slider(true, volume as f64, "Volume"));
        paint_slider(
            ui,
            &response,
            volume as f64,
            1.0,
            true,
            &self.effective_palette,
        );
        if response.on_hover_text("Volume").changed() {
            self.settings.volume = volume;
            self.engine.set_volume(volume);
            self.dirty = true;
        }
        if ui
            .small_button(if self.settings.volume == 0.0 {
                "Unmute"
            } else {
                "Mute"
            })
            .clicked()
        {
            if self.settings.volume > 0.0 {
                self.volume_before_mute = self.settings.volume;
                self.settings.volume = 0.0;
            } else {
                self.settings.volume = self.volume_before_mute.max(0.05);
            }
            self.engine.set_volume(self.settings.volume);
            self.dirty = true;
        }
    }

    fn podcast_controls(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        if ui
            .add_enabled(self.status.seekable, egui::Button::new("−15s").frame(false))
            .clicked()
        {
            self.engine.seek((self.status.position - 15.0).max(0.0));
        }
        if ui
            .add_enabled(self.status.seekable, egui::Button::new("+30s").frame(false))
            .clicked()
        {
            self.engine.seek(self.status.position + 30.0);
        }
        let mut speed = if self.status.speed > 0.0 {
            self.status.speed
        } else {
            1.0
        };
        egui::ComboBox::from_id_salt("speed")
            .width(54.0)
            .selected_text(format!("{speed:.2}×"))
            .show_ui(ui, |ui| {
                for option in [0.75, 1.0, 1.25, 1.5, 1.75, 2.0] {
                    if ui
                        .selectable_value(&mut speed, option, format!("{option:.2}×"))
                        .changed()
                    {
                        self.engine.set_speed(speed);
                    }
                }
            })
            .response
            .on_hover_text("Playback rate. This version also changes pitch.");
    }

    fn music(&mut self, ui: &mut egui::Ui) {
        self.section_header(
            ui,
            "Your music",
            format!("{} tracks", self.settings.library.len()),
            "Search music",
        );
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(!self.scan_busy, egui::Button::new("+ Add files"))
                .clicked()
            {
                self.request_files(false, ui.ctx().clone());
            }
            if ui
                .add_enabled(!self.scan_busy, egui::Button::new("Add folder…"))
                .clicked()
            {
                self.request_files(true, ui.ctx().clone());
            }
            ui.separator();
            let shuffle = if self.queue.shuffle {
                "Shuffle on"
            } else {
                "Shuffle off"
            };
            ui.toggle_value(&mut self.queue.shuffle, shuffle)
                .on_hover_text("Shuffle the finite playback queue");
            let repeat = if self.queue.repeat {
                "Repeat on"
            } else {
                "Repeat off"
            };
            ui.toggle_value(&mut self.queue.repeat, repeat)
                .on_hover_text("Repeat the finite playback queue");
        });
        ui.add_space(12.0);
        if let Some(error) = &self.library_error {
            ui.colored_label(color(self.effective_palette.warning), error);
        }
        if self.settings.library.is_empty() {
            empty(
                ui,
                "Your music belongs here.",
                "Add audio files or a folder, or drop them into this window.",
            );
            return;
        }
        let visible: Vec<_> = self
            .settings
            .library
            .iter()
            .enumerate()
            .filter(|(_, item)| item.matches(&self.search[0]))
            .map(|(i, item)| (i, item.clone()))
            .collect();
        if visible.is_empty() {
            empty(
                ui,
                "No matching tracks.",
                "Try another title, artist or album.",
            );
            return;
        }
        table_header(ui, &self.effective_palette);
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt("music_list")
            .auto_shrink([false, false])
            .show_rows(ui, 37.0, visible.len(), |ui, range| {
                for row in range {
                    let (index, item) = visible[row].clone();
                    let playing = self
                        .active
                        .as_ref()
                        .is_some_and(|active| active.key() == item.key())
                        && self.status.state == "playing";
                    let response = track_row(
                        ui,
                        &item,
                        self.selected_track == Some(index),
                        playing,
                        &self.effective_palette,
                    );
                    if response.clicked() {
                        self.selected_track = Some(index);
                    }
                    if response.double_clicked()
                        || (response.has_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                    {
                        self.play_music(index);
                    }
                    response.context_menu(|ui| {
                        if ui.button("Play").clicked() {
                            self.play_music(index);
                            ui.close();
                        }
                        if ui.button("Play next").clicked() {
                            self.queue.insert_next(item.clone());
                            ui.close();
                        }
                        if ui.button("Add to queue").clicked() {
                            self.queue.items.push(item.clone());
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Remove from collection (keep file)").clicked() {
                            remove = Some(index);
                            ui.close();
                        }
                    });
                }
            });
        if let Some(index) = remove {
            self.settings.library.remove(index);
            self.selected_track = None;
            self.dirty = true;
        }
    }

    fn radio(&mut self, ui: &mut egui::Ui) {
        self.section_header(
            ui,
            "Your stations",
            format!("{} saved", self.settings.stations.len()),
            "Search stations",
        );
        if ui.button("+ Add station").clicked() {
            self.dialog = Some(Dialog::Station {
                index: None,
                name: String::new(),
                url: String::new(),
                error: String::new(),
            });
        }
        ui.add_space(10.0);
        if self.settings.stations.is_empty() {
            empty(
                ui,
                "A small dial to the world.",
                "Save a station name and its direct HTTP/HTTPS audio stream.",
            );
            return;
        }
        let stations: Vec<_> = self
            .settings
            .stations
            .iter()
            .enumerate()
            .filter(|(_, station)| {
                format!("{} {}", station.name, station.url)
                    .to_lowercase()
                    .contains(&self.search[1].to_lowercase())
            })
            .map(|(i, station)| (i, station.clone()))
            .collect();
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt("stations")
            .show(ui, |ui| {
                for (index, station) in stations {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2((ui.available_width() - 165.0).max(100.0), 58.0),
                            Layout::top_down(Align::Min),
                            |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&station.name).strong().size(16.0),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&station.name);
                                let connected = self.active.as_ref().is_some_and(|item| {
                                    item.kind == Kind::Radio && item.url == station.url
                                }) && ["playing", "loading"]
                                    .contains(&self.status.state.as_str());
                                let state = if connected && self.status.state == "loading" {
                                    "Connecting…"
                                } else if connected {
                                    "Connected · Live"
                                } else {
                                    "Saved station"
                                };
                                ui.colored_label(
                                    color(if connected {
                                        self.effective_palette.accent
                                    } else {
                                        self.effective_palette.secondary
                                    }),
                                    state,
                                );
                            },
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.menu_button("…", |ui| {
                                if ui.button("Edit station").clicked() {
                                    self.dialog = Some(Dialog::Station {
                                        index: Some(index),
                                        name: station.name.clone(),
                                        url: station.url.clone(),
                                        error: String::new(),
                                    });
                                    ui.close();
                                }
                                if ui.button("Remove station").clicked() {
                                    remove = Some(index);
                                    ui.close();
                                }
                            });
                            let connected = self.active.as_ref().is_some_and(|item| {
                                item.kind == Kind::Radio && item.url == station.url
                            }) && ["playing", "loading"]
                                .contains(&self.status.state.as_str());
                            if ui
                                .button(if connected { "Disconnect" } else { "Connect" })
                                .clicked()
                            {
                                if connected {
                                    self.stop();
                                } else {
                                    self.start(Item {
                                        kind: Kind::Radio,
                                        title: station.name.clone(),
                                        url: station.url.clone(),
                                        ..Default::default()
                                    });
                                }
                            }
                        });
                    });
                    if let Some(error) = self.station_errors.get(&station.url).cloned() {
                        ui.horizontal_wrapped(|ui| {
                            ui.colored_label(color(self.effective_palette.warning), error);
                            if ui.small_button("Retry").clicked() {
                                self.start(Item {
                                    kind: Kind::Radio,
                                    title: station.name.clone(),
                                    url: station.url.clone(),
                                    ..Default::default()
                                });
                            }
                        });
                    }
                    ui.add_space(10.0);
                }
            });
        if let Some(index) = remove {
            self.settings.stations.remove(index);
            self.dirty = true;
        }
    }

    fn podcasts(&mut self, ui: &mut egui::Ui) {
        self.section_header(
            ui,
            "Your podcasts",
            format!("{} shows", self.settings.shows.len()),
            "Search episodes",
        );
        if ui.button("+ Add podcast").clicked() {
            self.dialog = Some(Dialog::Podcast {
                url: String::new(),
                error: String::new(),
            });
        }
        ui.add_space(10.0);
        if self.settings.shows.is_empty() && self.feed_busy.is_empty() {
            empty(
                ui,
                "Pick up the conversation.",
                "Add an RSS or Atom feed. Your listening position is remembered.",
            );
        }
        // Failed first subscriptions still have a recoverable, visible error.
        let orphan_errors: Vec<_> = self
            .feed_errors
            .iter()
            .filter(|(url, _)| !self.settings.shows.iter().any(|show| show.url == **url))
            .map(|(u, e)| (u.clone(), e.clone()))
            .collect();
        for (url, error) in orphan_errors {
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(color(self.effective_palette.warning), error);
                if ui.small_button("Retry feed").clicked() {
                    self.load_feed(url, ui.ctx().clone());
                }
            });
        }
        if !self.feed_busy.is_empty() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading podcast feed…");
            });
        }
        if ui.available_width() < 660.0 {
            let title = self
                .settings
                .shows
                .iter()
                .find(|show| Some(&show.url) == self.selected_show.as_ref())
                .map(|show| show.title.as_str())
                .unwrap_or("Choose a show");
            egui::ComboBox::from_id_salt("show_picker")
                .selected_text(title)
                .show_ui(ui, |ui| {
                    for show in &self.settings.shows {
                        ui.selectable_value(
                            &mut self.selected_show,
                            Some(show.url.clone()),
                            &show.title,
                        );
                    }
                });
            ui.add_space(8.0);
            self.episode_list(ui);
        } else {
            ui.horizontal_top(|ui| {
                let height = ui.available_height();
                let shows = self.settings.shows.clone();
                ui.allocate_ui_with_layout(
                    egui::vec2(180.0, height),
                    Layout::top_down(Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("shows")
                            .show(ui, |ui| {
                                for show in shows {
                                    if ui
                                        .selectable_label(
                                            self.selected_show.as_deref() == Some(&show.url),
                                            format!(
                                                "{}\n{} episodes",
                                                show.title,
                                                show.episodes.len()
                                            ),
                                        )
                                        .clicked()
                                    {
                                        self.selected_show = Some(show.url.clone());
                                        self.selected_episode = None;
                                    }
                                }
                            });
                    },
                );
                ui.separator();
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), height),
                    Layout::top_down(Align::Min),
                    |ui| {
                        self.episode_list(ui);
                    },
                );
            });
        }
    }

    fn episode_list(&mut self, ui: &mut egui::Ui) {
        let Some(show) = self
            .settings
            .shows
            .iter()
            .find(|show| Some(&show.url) == self.selected_show.as_ref())
            .cloned()
        else {
            return;
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&show.title).size(17.0).strong());
            if ui
                .add_enabled(
                    !self.feed_busy.contains(&show.url),
                    egui::Button::new("Refresh").small(),
                )
                .clicked()
            {
                self.load_feed(show.url.clone(), ui.ctx().clone());
            }
            ui.menu_button("…", |ui| {
                if ui.button("Remove show (keep listening history)").clicked() {
                    self.settings
                        .shows
                        .retain(|existing| existing.url != show.url);
                    self.selected_show = self.settings.shows.first().map(|show| show.url.clone());
                    self.dirty = true;
                    ui.close();
                }
            });
        });
        if let Some(error) = self.feed_errors.get(&show.url) {
            ui.colored_label(color(self.effective_palette.warning), error);
        }
        if show.episodes.is_empty() {
            ui.label("No episodes loaded. Refresh to read the feed.");
        }
        let search = self.search[2].clone();
        egui::ScrollArea::vertical()
            .id_salt("episodes")
            .show(ui, |ui| {
                for (index, item) in show
                    .episodes
                    .iter()
                    .enumerate()
                    .filter(|(_, item)| item.matches(&search))
                {
                    ui.separator();
                    let mark = self
                        .settings
                        .progress
                        .get(&item.key())
                        .cloned()
                        .unwrap_or_default();
                    let current = self.active.as_ref().is_some_and(|active| {
                        active.kind == Kind::Podcast && active.url == item.url
                    });
                    let playing = current && self.status.state == "playing";
                    ui.vertical(|ui| {
                        let response = ui.add(
                            egui::Label::new(RichText::new(&item.title).strong())
                                .sense(Sense::click()),
                        );
                        if response.double_clicked()
                            || (response.has_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                        {
                            self.play_episode(&show.url, index);
                        }
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(item.published.get(..10).unwrap_or(""))
                                    .size(12.0)
                                    .color(color(self.effective_palette.secondary)),
                            );
                            if let Some(duration) = mark.duration.or(item.duration) {
                                ui.label(RichText::new(time(duration)).size(12.0));
                            }
                            if mark.completed {
                                ui.small("Played");
                            } else if mark.position > 0.0 {
                                ui.colored_label(
                                    color(self.effective_palette.accent),
                                    format!("Continue from {}", time(mark.position)),
                                );
                            }
                            let label = if playing {
                                "Pause"
                            } else if current && self.status.state == "paused" {
                                "Resume"
                            } else if mark.completed {
                                "Replay"
                            } else if mark.position > 0.0 {
                                "Resume"
                            } else {
                                "Play"
                            };
                            if ui.button(label).clicked() {
                                if current
                                    && ["playing", "paused"].contains(&self.status.state.as_str())
                                {
                                    self.toggle();
                                } else {
                                    self.play_episode(&show.url, index);
                                }
                            }
                        });
                        if !item.description.is_empty() {
                            egui::CollapsingHeader::new("Episode details")
                                .id_salt(&item.url)
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(&item.description)
                                            .small()
                                            .color(color(self.effective_palette.secondary)),
                                    );
                                });
                        }
                    });
                    ui.add_space(7.0);
                }
            });
    }

    fn section_header(&mut self, ui: &mut egui::Ui, title: &str, count: String, hint: &str) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(title).size(18.0).strong());
            ui.label(
                RichText::new(count)
                    .small()
                    .color(color(self.effective_palette.secondary)),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.search[self.tab.index()])
                        .hint_text(hint)
                        .desired_width(170.0),
                );
                if response.changed()
                    && self.tab == Tab::Music
                    && self.selected_track.is_some_and(|index| {
                        self.settings
                            .library
                            .get(index)
                            .is_none_or(|item| !item.matches(&self.search[0]))
                    })
                {
                    self.selected_track = None;
                }
            });
        });
        ui.add_space(8.0);
    }

    fn queue_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Playback queue").strong());
            if ui.small_button("Close").clicked() {
                self.queue_open = false;
            }
        });
        ui.label(
            RichText::new("Live radio does not replace this queue.")
                .small()
                .color(color(self.effective_palette.secondary)),
        );
        ui.add_space(8.0);
        if self.queue.items.is_empty() {
            ui.label("Play a track or episode, or add music with its context menu.");
            return;
        }
        let items = self.queue.items.clone();
        let mut remove = None;
        egui::ScrollArea::vertical()
            .id_salt("queue_items")
            .show(ui, |ui| {
                for (index, item) in items.iter().enumerate() {
                    let response = ui.selectable_label(
                        self.queue.current == Some(index),
                        format!("{}. {}", index + 1, item.title),
                    );
                    if response.double_clicked()
                        || (response.has_focus()
                            && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                    {
                        self.queue.current = Some(index);
                        self.start(item.clone());
                    }
                    response.context_menu(|ui| {
                        if ui.button("Play now").clicked() {
                            self.queue.current = Some(index);
                            self.start(item.clone());
                            ui.close();
                        }
                        if ui.button("Remove from queue").clicked() {
                            remove = Some(index);
                            ui.close();
                        }
                    });
                }
            });
        if let Some(index) = remove {
            self.queue.remove(index);
        }
    }

    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let mut open = true;
        let mut close = false;
        let title = match dialog {
            Dialog::Station { .. } => "Station",
            Dialog::Podcast { .. } => "Add podcast",
            Dialog::Appearance(_) => "Appearance",
            Dialog::About => "About Tapewave",
        };
        let modal=egui::Modal::new(egui::Id::new("application_dialog")).show(ctx,|ui|{
            ui.set_width(410.0);
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height()-64.0).max(180.0)).show(ui,|ui|{
            ui.horizontal(|ui|{
                ui.heading(title);
                ui.with_layout(Layout::right_to_left(Align::Center),|ui|{if ui.button("Close").clicked(){close=true;}});
            });
            ui.separator();
            match &mut dialog {
                Dialog::Station{index,name,url,error}=>{
                    ui.label("Station name");ui.add(egui::TextEdit::singleline(name).desired_width(390.0));
                    ui.label("Direct audio stream URL");ui.add(egui::TextEdit::singleline(url).desired_width(390.0).hint_text("https://…/stream"));
                    ui.small("HTTP/HTTPS audio streams, not station website URLs.");
                    if !error.is_empty(){ui.colored_label(color(self.effective_palette.warning),error.as_str());}
                    ui.horizontal(|ui|{
                        if ui.button("Save station").clicked(){
                            if name.trim().is_empty(){*error="Give this station a name.".into();}
                            else if !library::valid_url(url.trim()){*error="Enter a valid HTTP/HTTPS URL without embedded credentials.".into();}
                            else {
                                let station=Station{name:name.trim().to_string(),url:url.trim().to_string()};
                                if let Some(index)=index{if let Some(old)=self.settings.stations.get_mut(*index){*old=station;}}
                                else if self.settings.stations.iter().any(|old|old.url==station.url){*error="This station is already saved.".into();return;}
                                else{self.settings.stations.push(station);}
                                self.dirty=true;close=true;
                            }
                        }
                        if ui.button("Cancel").clicked(){close=true;}
                    });
                },
                Dialog::Podcast{url,error}=>{
                    ui.label("RSS or Atom feed URL");ui.add(egui::TextEdit::singleline(url).desired_width(390.0).hint_text("https://…/feed.xml"));
                    ui.label("Feeds are saved locally. Episode audio is fetched only when you choose Play.");
                    if !error.is_empty(){ui.colored_label(color(self.effective_palette.warning),error.as_str());}
                    ui.horizontal(|ui|{
                        if ui.button("Add podcast").clicked(){if !library::valid_url(url.trim()){*error="Enter a valid HTTP/HTTPS feed URL.".into();}
                            else{let url=url.trim().to_string();self.selected_show=Some(url.clone());self.load_feed(url,ctx.clone());self.tab=Tab::Podcasts;close=true;}}
                        if ui.button("Cancel").clicked(){close=true;}
                    });
                },
                Dialog::Appearance(draft)=>{
                    let compact=ctx.content_rect().height()<560.0;
                    if compact{ui.small("Scroll for custom colours and Save. Layout and playback stay the same.");}
                    else{ui.label("Colours are yours. Layout and playback stay the same.");}
                    if self.high_contrast{ui.label("Windows high-contrast colours take precedence. Your custom palette is retained.");}
                    ui.horizontal(|ui|{
                        if ui.button("Quiet Hi-Fi").clicked(){*draft=Palette::default();}
                        if ui.button("Paper").clicked(){*draft=Palette::light();}
                        if ui.button("Blue Hour").clicked(){*draft=Palette{accent:[140,185,230],..Palette::default()};}
                    });
                    ui.separator();
                    egui::Grid::new("palette_fields").num_columns(if compact{4}else{2}).spacing([if compact{6.0}else{36.0},6.0]).show(ui,|ui|{
                        for (i,(label,value)) in [("Background",&mut draft.background),("Panels",&mut draft.panel),("Primary text",&mut draft.text),("Secondary text",&mut draft.secondary),("Accent / focus",&mut draft.accent),("Borders",&mut draft.border),("Selection",&mut draft.selection),("Warning",&mut draft.warning)].into_iter().enumerate(){
                            ui.label(label);ui.color_edit_button_srgb(value);
                            if !compact || i%2==1 {ui.end_row();}
                        }
                    });
                    let readable=draft.readable();
                    if !readable{ui.label("Increase contrast before saving: text needs 4.5:1 on every surface; accent and warning need 3:1 on the background.");}
                    ui.horizontal(|ui|{
                        if ui.add_enabled(readable,egui::Button::new("Save appearance")).clicked(){self.settings.palette=draft.clone();self.dirty=true;close=true;}
                        if ui.button("Reset to default").clicked(){*draft=Palette::default();}
                        if ui.button("Cancel").clicked(){close=true;}
                    });
                },
                Dialog::About=>{
                    ui.heading("Tapewave");ui.label(format!("Version {} · Native Windows x64",env!("CARGO_PKG_VERSION")));
                    ui.label("Quiet Hi-Fi. Your music, radio and podcasts.");
                    ui.label("Rust/egui native rendering · No WebView or local server.");
                    ui.small("Minimize to tray. Close the window to exit.\nCtrl+O: add music · Ctrl+P: play/pause · Ctrl+S: stop\nPodcast speed changes pitch in this version.\nMIT License. Not affiliated with Winamp.");
                },
            }
            });
        });
        if modal.should_close() {
            open = false;
        }
        if open && !close {
            self.dialog = Some(dialog);
        }
        self.poll_theme(ctx);
        if close {
            self.persist();
        }
    }
}

fn empty(ui: &mut egui::Ui, title: &str, description: &str) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(title).size(21.0));
        ui.add_space(8.0);
        ui.label(description);
    });
}

/// Keep egui's native keyboard/accessibility and generous hit area, but draw
/// the quiet four-pixel rail and small handle from the approved visual design.
fn paint_slider(
    ui: &egui::Ui,
    response: &egui::Response,
    value: f64,
    maximum: f64,
    enabled: bool,
    p: &Palette,
) {
    let rect = response.rect;
    ui.painter().rect_filled(rect, 0.0, color(p.panel));
    let inset = rect.height() / 2.5;
    let left = rect.left() + inset;
    let right = rect.right() - inset;
    let y = rect.center().y;
    let fraction = (value / maximum).clamp(0.0, 1.0) as f32;
    let x = left + (right - left) * fraction;
    let rail = egui::Rect::from_min_max(egui::pos2(left, y - 2.0), egui::pos2(right, y + 2.0));
    ui.painter().rect_filled(rail, 2.0, color(p.border));
    if enabled {
        ui.painter().rect_filled(
            egui::Rect::from_min_max(rail.min, egui::pos2(x, y + 2.0)),
            2.0,
            color(p.accent),
        );
        ui.painter().circle_filled(
            egui::pos2(x, y),
            if response.dragged() { 6.0 } else { 5.0 },
            color(p.accent),
        );
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            4.0,
            Stroke::new(1.0, color(p.accent)),
            StrokeKind::Inside,
        );
    }
}

fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    enabled: bool,
    p: &Palette,
) -> egui::Response {
    ui.add_enabled_ui(enabled, |ui| {
        let response = ui.add_sized([36.0, 36.0], egui::Button::new(""));
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
        paint_icon(
            ui.painter(),
            icon,
            response.rect.center(),
            color(if enabled { p.text } else { p.secondary }),
        );
        response.on_hover_text(label)
    })
    .inner
}
fn primary_button(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    enabled: bool,
    p: &Palette,
) -> egui::Response {
    let foreground =
        if theme::contrast([0, 0, 0], p.accent) > theme::contrast([255, 255, 255], p.accent) {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
    let response = ui.add_enabled(
        enabled,
        egui::Button::new(
            RichText::new(format!("     {label}"))
                .strong()
                .color(foreground),
        )
        .fill(color(p.accent))
        .min_size(egui::vec2(111.0, 36.0)),
    );
    paint_icon(
        ui.painter(),
        icon,
        egui::pos2(response.rect.left() + 20.0, response.rect.center().y),
        foreground,
    );
    response
}
fn paint_icon(painter: &egui::Painter, icon: Icon, centre: egui::Pos2, colour: Color32) {
    let x = centre.x;
    let y = centre.y;
    let triangle = |points: Vec<egui::Pos2>| {
        painter.add(egui::Shape::convex_polygon(points, colour, Stroke::NONE));
    };
    match icon {
        Icon::Play => triangle(vec![
            egui::pos2(x - 4.0, y - 7.0),
            egui::pos2(x + 6.0, y),
            egui::pos2(x - 4.0, y + 7.0),
        ]),
        Icon::Pause => {
            for dx in [-4.0, 3.0] {
                painter.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(x + dx, y - 6.0), egui::vec2(3.0, 12.0)),
                    0.0,
                    colour,
                );
            }
        }
        Icon::Stop => {
            painter.rect_filled(
                egui::Rect::from_center_size(centre, egui::vec2(11.0, 11.0)),
                1.0,
                colour,
            );
        }
        Icon::Previous => {
            triangle(vec![
                egui::pos2(x + 5.0, y - 6.0),
                egui::pos2(x - 4.0, y),
                egui::pos2(x + 5.0, y + 6.0),
            ]);
            painter.line_segment(
                [egui::pos2(x - 6.0, y - 6.0), egui::pos2(x - 6.0, y + 6.0)],
                Stroke::new(2.0, colour),
            );
        }
        Icon::Next => {
            triangle(vec![
                egui::pos2(x - 5.0, y - 6.0),
                egui::pos2(x + 4.0, y),
                egui::pos2(x - 5.0, y + 6.0),
            ]);
            painter.line_segment(
                [egui::pos2(x + 6.0, y - 6.0), egui::pos2(x + 6.0, y + 6.0)],
                Stroke::new(2.0, colour),
            );
        }
    }
}

fn columns(width: f32) -> [f32; 5] {
    if width > 620.0 {
        [
            28.0,
            width * 0.36,
            width * 0.23,
            (width - width * 0.59 - 88.0).max(10.0),
            60.0,
        ]
    } else {
        [28.0, (width - 88.0).max(20.0), 0.0, 0.0, 60.0]
    }
}
fn table_header(ui: &mut egui::Ui, p: &Palette) {
    let width = ui.available_width();
    let sizes = columns(width);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 26.0), Sense::hover());
    let mut x = rect.left();
    for (label, size) in ["", "Title", "Artist", "Album", "Time"]
        .into_iter()
        .zip(sizes)
    {
        if size > 0.0 {
            ui.painter().text(
                egui::pos2(x + 4.0, rect.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(12.0),
                color(p.secondary),
            );
        }
        x += size;
    }
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_bottom()],
        Stroke::new(1.0, color(p.border)),
    );
}
fn track_row(
    ui: &mut egui::Ui,
    item: &Item,
    selected: bool,
    playing: bool,
    p: &Palette,
) -> egui::Response {
    let width = ui.available_width();
    let sizes = columns(width);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 37.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            true,
            selected,
            &item.title,
        )
    });
    if selected || response.hovered() {
        ui.painter().rect_filled(rect, 0.0, color(p.selection));
    }
    if selected || response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            0.0,
            Stroke::new(1.0, color(p.accent)),
            StrokeKind::Inside,
        );
    }
    if playing {
        paint_icon(
            ui.painter(),
            Icon::Play,
            egui::pos2(rect.left() + 11.0, rect.center().y),
            color(p.accent),
        );
    }
    let duration = item.duration.map(time).unwrap_or_else(|| "--:--".into());
    let mut x = rect.left();
    for (i, (label, size)) in [
        "",
        item.title.as_str(),
        item.subtitle.as_str(),
        item.album.as_str(),
        duration.as_str(),
    ]
    .into_iter()
    .zip(sizes)
    .enumerate()
    {
        if size > 0.0 {
            let clip = egui::Rect::from_min_size(
                egui::pos2(x + 4.0, rect.top()),
                Vec2::new((size - 8.0).max(1.0), rect.height()),
            );
            ui.painter().with_clip_rect(clip).text(
                egui::pos2(x + 4.0, rect.center().y),
                Align2::LEFT_CENTER,
                label,
                if i == 4 {
                    FontId::monospace(12.0)
                } else {
                    FontId::proportional(14.0)
                },
                color(if playing && i == 1 {
                    p.accent
                } else if i == 1 {
                    p.text
                } else {
                    p.secondary
                }),
            );
        }
        x += size;
    }
    response.on_hover_text(format!(
        "{}\n{}\nDouble-click to play",
        item.title, item.subtitle
    ))
}
