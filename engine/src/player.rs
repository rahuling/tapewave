//! Playback engine. Owns the audio output on a dedicated thread and takes
//! commands over a channel, so the UI never blocks on decoding or I/O.

use std::fs::File;
use std::io::{Read, Seek, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rodio::{Decoder, MixerDeviceSink, Player, Source};

use crate::loader::{Latest, LoadState};
use crate::stream::LiveStream;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Status {
    /// "stopped", "loading", "playing" or "paused".
    pub state: String,
    pub source: Option<String>,
    pub volume: f32,
    pub error: Option<String>,
    pub request_id: u64,
    pub position: f64,
    pub duration: Option<f64>,
    pub seekable: bool,
    pub finished: bool,
    pub speed: f32,
}

enum Command {
    Play(u64, Input),
    Pause,
    Resume,
    Stop,
    SetVolume(f32),
    Seek(f64),
    Speed(f32),
    Loaded(u64, String, Result<Audio, String>),
    Shutdown,
}

struct Audio {
    source: Box<dyn Source<Item = f32> + Send>,
    duration: Option<Duration>,
    seekable: bool,
    start: f64,
}

enum Input {
    File(PathBuf),
    Url(String),
    Episode(String, f64),
}

impl Input {
    fn name(&self) -> String {
        match self {
            Self::File(path) => path.display().to_string(),
            Self::Url(url) | Self::Episode(url, _) => url.clone(),
        }
    }
}

struct Client {
    tx: Sender<Command>,
    next_id: AtomicU64,
    active: Arc<AtomicU64>,
}

impl Drop for Client {
    fn drop(&mut self) {
        self.active.store(0, Ordering::Relaxed);
        let _ = self.tx.send(Command::Shutdown);
    }
}

#[derive(Clone)]
pub struct Engine {
    client: Arc<Client>,
    status: Arc<Mutex<Status>>,
}

impl Engine {
    /// A deterministic, hardware-free client used only by UI regression tests.
    /// Commands fail cleanly instead of initializing audio drivers in test processes.
    #[cfg(debug_assertions)]
    pub fn silent() -> Self {
        let (tx, rx) = mpsc::channel();
        drop(rx);
        Self {
            client: Arc::new(Client {
                tx,
                next_id: AtomicU64::new(0),
                active: Arc::new(AtomicU64::new(0)),
            }),
            status: Arc::new(Mutex::new(Status {
                state: "stopped".into(),
                volume: 1.0,
                ..Default::default()
            })),
        }
    }

    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status {
            state: "stopped".into(),
            volume: 1.0,
            ..Default::default()
        }));
        let shared = Arc::clone(&status);
        let completed = tx.clone();
        let active = Arc::new(AtomicU64::new(0));
        let cancellation = Arc::clone(&active);

        thread::spawn(move || {
            let mut out = match rodio::DeviceSinkBuilder::open_default_sink() {
                Ok(sink) => sink,
                Err(e) => {
                    set_error(&shared, format!("no audio output: {e}"));
                    return;
                }
            };
            // Stopping the output drops it, so the sink lives for the whole thread.
            out.log_on_drop(false);
            // A single loader and a one-item, latest-wins mailbox prevent
            // unbounded threads/queues when the user changes sources rapidly.
            let loads: Arc<Latest<(u64, Input)>> = Arc::new(Latest::default());
            let episodes: Arc<Latest<(u64, Input)>> = Arc::new(Latest::default());
            for pending in [Arc::clone(&loads), Arc::clone(&episodes)] {
                let completed = completed.clone();
                let cancellation = Arc::clone(&cancellation);
                thread::spawn(move || {
                    while let Some((id, input)) = pending.take() {
                        if cancellation.load(Ordering::Relaxed) != id {
                            continue;
                        }
                        let name = input.name();
                        let audio = load(input, id, &cancellation);
                        if completed.send(Command::Loaded(id, name, audio)).is_err() {
                            break;
                        }
                    }
                });
            }
            let mut worker = Worker {
                out: &mut out,
                player: None,
                shared: &shared,
                volume: 1.0,
                loads,
                loading: LoadState::default(),
                episodes,
                request_id: 0,
                duration: None,
                seekable: false,
                finished: false,
                speed: 1.0,
                load_generation: 0,
            };

            loop {
                match rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(Command::Shutdown) => break,
                    Ok(cmd) => worker.handle(cmd),
                    Err(RecvTimeoutError::Timeout) => worker.check_finished(),
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            worker.loads.close();
            worker.episodes.close();
        });

        Self {
            client: Arc::new(Client {
                tx,
                next_id: AtomicU64::new(0),
                active,
            }),
            status,
        }
    }

    fn play(&self, input: Input) -> u64 {
        let id = self.client.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        self.client.active.store(id, Ordering::Relaxed);
        if self.client.tx.send(Command::Play(id, input)).is_err()
            && let Ok(mut status) = self.status.lock()
        {
            status.request_id = id;
            status.state = "stopped".into();
            status.source = None;
            status.finished = false;
            status.error = Some(
                "The audio engine is unavailable. Check your output device and restart Tapewave."
                    .into(),
            );
        }
        id
    }

    pub fn play_file(&self, path: impl Into<PathBuf>) -> u64 {
        self.play(Input::File(path.into()))
    }

    pub fn play_url(&self, url: impl Into<String>) -> u64 {
        self.play(Input::Url(url.into()))
    }

    /// Prepare a finite episode in a temporary file so seek/resume is real,
    /// rather than pretending a non-seekable radio stream supports it.
    pub fn play_episode(&self, url: impl Into<String>, resume_at: f64) -> u64 {
        self.play(Input::Episode(
            url.into(),
            if resume_at.is_finite() {
                resume_at.clamp(0.0, 1_000_000_000.0)
            } else {
                0.0
            },
        ))
    }

    pub fn seek(&self, seconds: f64) {
        if seconds.is_finite() {
            let _ = self
                .client
                .tx
                .send(Command::Seek(seconds.clamp(0.0, 1_000_000_000.0)));
        }
    }

    pub fn set_speed(&self, speed: f32) {
        if speed.is_finite() {
            let _ = self.client.tx.send(Command::Speed(speed.clamp(0.5, 2.0)));
        }
    }

    pub fn pause(&self) {
        let _ = self.client.tx.send(Command::Pause);
    }

    pub fn resume(&self) {
        let _ = self.client.tx.send(Command::Resume);
    }

    pub fn stop(&self) {
        self.client.active.store(0, Ordering::Relaxed);
        let _ = self.client.tx.send(Command::Stop);
    }

    pub fn set_volume(&self, volume: f32) {
        let volume = if volume.is_finite() {
            volume.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let _ = self.client.tx.send(Command::SetVolume(volume));
    }

    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

fn set_error(shared: &Mutex<Status>, message: String) {
    if let Ok(mut s) = shared.lock() {
        s.state = "stopped".into();
        s.error = Some(message);
    }
}

struct Worker<'a> {
    out: &'a mut MixerDeviceSink,
    player: Option<Player>,
    shared: &'a Mutex<Status>,
    volume: f32,
    loads: Arc<Latest<(u64, Input)>>,
    loading: LoadState,
    episodes: Arc<Latest<(u64, Input)>>,
    request_id: u64,
    duration: Option<Duration>,
    seekable: bool,
    finished: bool,
    speed: f32,
    load_generation: u64,
}

impl Worker<'_> {
    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Play(id, input) => self.request(id, input),
            Command::Loaded(id, name, result) => {
                // Stop/new-source invalidates older results, including errors.
                if self.request_id != id || !self.loading.complete(self.load_generation) {
                    return;
                }
                match result {
                    Ok(audio) => {
                        self.start(audio, name);
                        if let Some(player) = self.player.as_ref().filter(|_| self.loading.paused) {
                            player.pause();
                        }
                    }
                    Err(error) => self.fail(format!("Could not play {name}: {error}")),
                }
            }
            Command::Pause => {
                self.loading.paused = true;
                if let Some(p) = &self.player {
                    p.pause();
                }
            }
            Command::Resume => {
                self.loading.paused = false;
                if let Some(p) = &self.player {
                    p.play();
                }
            }
            Command::Stop => {
                self.loading.cancel();
                self.loads.clear();
                self.episodes.clear();
                self.finished = false;
                self.seekable = false;
                self.duration = None;
                if let Some(p) = self.player.take() {
                    p.stop();
                }
            }
            Command::SetVolume(v) => {
                self.volume = v;
                if let Some(p) = &self.player {
                    p.set_volume(v);
                }
            }
            Command::Seek(seconds) => {
                if self.seekable {
                    let target = self
                        .duration
                        .map_or(seconds, |d| seconds.min((d.as_secs_f64() - 0.1).max(0.0)));
                    if let Some(player) = &self.player
                        && let Err(error) = player.try_seek(Duration::from_secs_f64(target))
                    {
                        self.seekable = false;
                        if let Ok(mut status) = self.shared.lock() {
                            status.error =
                                Some(format!("Seeking is unavailable for this file: {error}"));
                        }
                    }
                }
            }
            Command::Speed(speed) => {
                self.speed = speed;
                if let Some(player) = &self.player {
                    player.set_speed(speed);
                }
            }
            Command::Shutdown => unreachable!("handled by the command loop"),
        }
        self.publish();
    }

    fn request(&mut self, id: u64, input: Input) {
        // Drop previous playback and publish loading immediately. File opening,
        // HTTP and decoder probing happen entirely on the loader thread.
        self.player = None;
        self.load_generation = self.loading.begin();
        self.request_id = id;
        self.finished = false;
        self.seekable = false;
        self.duration = None;
        self.loads.clear();
        self.episodes.clear();
        if let Ok(mut status) = self.shared.lock() {
            status.source = Some(input.name());
            status.error = None;
        }
        if matches!(input, Input::Episode(..)) {
            self.episodes.replace((id, input));
        } else {
            self.loads.replace((id, input));
        }
    }

    fn start(&mut self, audio: Audio, source: String) {
        // Replace the current track. Dropping the old player stops it.
        let player = Player::connect_new(self.out.mixer());
        player.pause();
        player.set_volume(self.volume);
        player.set_speed(self.speed);
        self.duration = audio.duration;
        self.seekable = audio.seekable;
        player.append(audio.source);
        let mut warning = None;
        if audio.seekable && audio.start > 0.0 {
            let target = audio.duration.map_or(audio.start, |d| {
                audio.start.min((d.as_secs_f64() - 1.0).max(0.0))
            });
            if let Err(error) = player.try_seek(Duration::from_secs_f64(target)) {
                warning = Some(format!("Could not restore listening position: {error}"));
            }
        }
        if !self.loading.paused {
            player.play();
        }
        self.player = Some(player);
        if let Ok(mut s) = self.shared.lock() {
            s.source = Some(source);
            s.error = warning;
        }
    }

    fn fail(&mut self, message: String) {
        self.player = None;
        set_error(self.shared, message);
        self.publish();
    }

    /// Called on idle ticks. Marks the state stopped when a finite track ends.
    fn check_finished(&mut self) {
        if self.player.as_ref().is_some_and(|player| player.empty()) {
            self.player = None;
            self.finished = true;
        }
        self.publish();
    }

    fn publish(&self) {
        if let Ok(mut s) = self.shared.lock() {
            s.volume = self.volume;
            s.request_id = self.request_id;
            s.duration = self.duration.map(|d| d.as_secs_f64());
            s.seekable = self.seekable && self.player.is_some();
            s.finished = self.finished;
            s.speed = self.speed;
            s.position = self.player.as_ref().map_or_else(
                || {
                    if self.finished {
                        s.duration.unwrap_or(s.position)
                    } else {
                        0.0
                    }
                },
                |p| p.get_pos().as_secs_f64(),
            );
            s.state = match &self.player {
                None if self.loading.pending && self.loading.paused => "paused",
                None if self.loading.pending => "loading",
                None => "stopped",
                Some(p) if p.is_paused() => "paused",
                Some(_) => "playing",
            }
            .into();
            if self.player.is_none() && !self.loading.pending && !self.finished {
                s.source = None;
            }
        }
    }
}

fn load(input: Input, id: u64, active: &AtomicU64) -> Result<Audio, String> {
    match input {
        Input::File(path) => {
            let file = File::open(path).map_err(|e| e.to_string())?;
            disk_audio(file, 0.0)
        }
        Input::Url(url) => open_stream(&url).map(|decoder| Audio {
            source: Box::new(decoder),
            duration: None,
            seekable: false,
            start: 0.0,
        }),
        Input::Episode(url, start) => {
            let agent: ureq::Agent = ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(180)))
                .build()
                .into();
            let response = agent.get(&url).call().map_err(|e| e.to_string())?;
            let mut reader = response.into_body().into_reader();
            // Anonymous/delete-on-close storage is also reclaimed by Windows
            // if the process exits while a network read is being cancelled.
            let mut temp = tempfile::tempfile().map_err(|e| e.to_string())?;
            let mut bytes = [0u8; 64 * 1024];
            let mut total = 0usize;
            loop {
                if active.load(Ordering::Relaxed) != id {
                    return Err("Episode preparation cancelled".into());
                }
                let count = reader.read(&mut bytes).map_err(|e| e.to_string())?;
                if count == 0 {
                    break;
                }
                total += count;
                if total > 256 * 1024 * 1024 {
                    return Err("Episode exceeds the 256 MiB preparation limit".into());
                }
                temp.write_all(&bytes[..count]).map_err(|e| e.to_string())?;
            }
            temp.flush().map_err(|e| e.to_string())?;
            temp.rewind().map_err(|e| e.to_string())?;
            disk_audio(temp, start)
        }
    }
}

fn disk_audio(file: File, start: f64) -> Result<Audio, String> {
    let decoder = Decoder::try_from(file).map_err(|e| e.to_string())?;
    let duration = decoder.total_duration();
    Ok(Audio {
        source: Box::new(decoder),
        duration,
        seekable: true,
        start,
    })
}

fn open_stream(url: &str) -> Result<Decoder<LiveStream>, String> {
    // Do not set a total body deadline: live radio can run indefinitely.
    // LiveStream bounds how long the decoder waits for the next chunk.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(8)))
        .timeout_recv_response(Some(Duration::from_secs(10)))
        .timeout_recv_body(None)
        .timeout_global(None)
        .timeout_resolve(Some(Duration::from_secs(8)))
        .build()
        .into();
    let response = agent.get(url).call().map_err(|e| e.to_string())?;
    let mime = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let reader = response.into_body().into_reader();

    let mut builder = Decoder::builder()
        .with_data(LiveStream::new(Box::new(reader)))
        .with_seekable(false);
    if let Some(mime) = mime {
        builder = builder.with_mime_type(&mime);
    }
    builder.build().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav() -> Vec<u8> {
        let samples = 8000u32;
        let data_len = samples * 2;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&8000u32.to_le_bytes());
        bytes.extend_from_slice(&16000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        bytes.resize(44 + data_len as usize, 0);
        bytes
    }

    #[test]
    fn local_decoder_reports_real_duration_and_supports_seeking() {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&wav()).unwrap();
        file.flush().unwrap();
        let mut audio = load(
            Input::File(file.path().to_path_buf()),
            1,
            &AtomicU64::new(1),
        )
        .unwrap();
        assert!(audio.seekable);
        assert!((audio.duration.unwrap().as_secs_f64() - 1.0).abs() < 0.02);
        audio.source.try_seek(Duration::from_millis(500)).unwrap();
        assert!(audio.source.next().is_some());
    }

    #[test]
    fn failed_engine_reports_an_error_instead_of_loading_forever() {
        let (tx, rx) = mpsc::channel();
        drop(rx);
        let engine = Engine {
            client: Arc::new(Client {
                tx,
                next_id: AtomicU64::new(0),
                active: Arc::new(AtomicU64::new(0)),
            }),
            status: Arc::new(Mutex::new(Status::default())),
        };
        let id = engine.play_file("unopened.wav");
        let status = engine.status();
        assert_eq!(status.request_id, id);
        assert_eq!(status.state, "stopped");
        assert!(status.error.unwrap().contains("restart Tapewave"));
    }
}
