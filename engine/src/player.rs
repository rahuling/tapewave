//! Playback engine. Owns the audio output on a dedicated thread and takes
//! commands over a channel, so the UI never blocks on decoding or I/O.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rodio::{Decoder, MixerDeviceSink, Player};

use crate::stream::LiveStream;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Status {
    /// "stopped", "playing" or "paused".
    pub state: String,
    pub source: Option<String>,
    pub volume: f32,
    pub error: Option<String>,
}

enum Command {
    PlayFile(PathBuf),
    PlayUrl(String),
    Pause,
    Resume,
    Stop,
    SetVolume(f32),
}

#[derive(Clone)]
pub struct Engine {
    tx: Sender<Command>,
    status: Arc<Mutex<Status>>,
}

impl Engine {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        let status = Arc::new(Mutex::new(Status {
            state: "stopped".into(),
            volume: 1.0,
            ..Default::default()
        }));
        let shared = Arc::clone(&status);

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
            let mut worker = Worker { out: &mut out, player: None, shared: &shared, volume: 1.0 };

            loop {
                match rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(cmd) => worker.handle(cmd),
                    Err(RecvTimeoutError::Timeout) => worker.check_finished(),
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
        });

        Self { tx, status }
    }

    pub fn play_file(&self, path: impl Into<PathBuf>) {
        let _ = self.tx.send(Command::PlayFile(path.into()));
    }

    pub fn play_url(&self, url: impl Into<String>) {
        let _ = self.tx.send(Command::PlayUrl(url.into()));
    }

    pub fn pause(&self) {
        let _ = self.tx.send(Command::Pause);
    }

    pub fn resume(&self) {
        let _ = self.tx.send(Command::Resume);
    }

    pub fn stop(&self) {
        let _ = self.tx.send(Command::Stop);
    }

    pub fn set_volume(&self, volume: f32) {
        let _ = self.tx.send(Command::SetVolume(volume.clamp(0.0, 1.0)));
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
}

impl Worker<'_> {
    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::PlayFile(path) => {
                let source = path.display().to_string();
                match File::open(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|f| Decoder::new(BufReader::new(f)).map_err(|e| e.to_string()))
                {
                    Ok(decoder) => self.start(decoder, source),
                    Err(e) => self.fail(format!("could not open {source}: {e}")),
                }
            }
            Command::PlayUrl(url) => match open_stream(&url) {
                Ok(decoder) => self.start(decoder, url),
                Err(e) => self.fail(format!("could not play {url}: {e}")),
            },
            Command::Pause => {
                if let Some(p) = &self.player {
                    p.pause();
                }
            }
            Command::Resume => {
                if let Some(p) = &self.player {
                    p.play();
                }
            }
            Command::Stop => {
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
        }
        self.publish();
    }

    fn start<D>(&mut self, decoder: D, source: String)
    where
        D: rodio::Source + Send + 'static,
    {
        // Replace the current track. Dropping the old player stops it.
        let player = Player::connect_new(self.out.mixer());
        player.set_volume(self.volume);
        player.append(decoder);
        self.player = Some(player);
        if let Ok(mut s) = self.shared.lock() {
            s.source = Some(source);
            s.error = None;
        }
    }

    fn fail(&mut self, message: String) {
        self.player = None;
        set_error(self.shared, message);
        self.publish();
    }

    /// Called on idle ticks. Marks the state stopped when a finite track ends.
    fn check_finished(&mut self) {
        if let Some(p) = &self.player {
            if p.empty() {
                self.player = None;
                self.publish();
            }
        }
    }

    fn publish(&self) {
        if let Ok(mut s) = self.shared.lock() {
            s.volume = self.volume;
            s.state = match &self.player {
                None => "stopped",
                Some(p) if p.is_paused() => "paused",
                Some(_) => "playing",
            }
            .into();
            if self.player.is_none() {
                s.source = None;
            }
        }
    }
}

fn open_stream(url: &str) -> Result<Decoder<LiveStream>, String> {
    let response = ureq::get(url).call().map_err(|e| e.to_string())?;
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
