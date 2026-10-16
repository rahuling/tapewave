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

use crate::loader::{Latest, LoadState};
use crate::stream::LiveStream;

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize)]
pub struct Status {
    /// "stopped", "loading", "playing" or "paused".
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
    Loaded(u64, String, Result<Audio, String>),
    Shutdown,
}

type Audio = Box<dyn rodio::Source<Item = f32> + Send>;

enum Input {
    File(PathBuf),
    Url(String),
}

impl Input {
    fn name(&self) -> String {
        match self {
            Self::File(path) => path.display().to_string(),
            Self::Url(url) => url.clone(),
        }
    }
}

struct Client {
    tx: Sender<Command>,
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
    }
}

#[derive(Clone)]
pub struct Engine {
    client: Arc<Client>,
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
        let completed = tx.clone();

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
            let pending = Arc::clone(&loads);
            thread::spawn(move || {
                while let Some((generation, input)) = pending.take() {
                    let name = input.name();
                    let audio = load(input);
                    if completed
                        .send(Command::Loaded(generation, name, audio))
                        .is_err()
                    {
                        break;
                    }
                }
            });
            let mut worker = Worker {
                out: &mut out,
                player: None,
                shared: &shared,
                volume: 1.0,
                loads,
                loading: LoadState::default(),
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
        });

        Self {
            client: Arc::new(Client { tx }),
            status,
        }
    }

    pub fn play_file(&self, path: impl Into<PathBuf>) {
        let _ = self.client.tx.send(Command::PlayFile(path.into()));
    }

    pub fn play_url(&self, url: impl Into<String>) {
        let _ = self.client.tx.send(Command::PlayUrl(url.into()));
    }

    pub fn pause(&self) {
        let _ = self.client.tx.send(Command::Pause);
    }

    pub fn resume(&self) {
        let _ = self.client.tx.send(Command::Resume);
    }

    pub fn stop(&self) {
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
}

impl Worker<'_> {
    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::PlayFile(path) => self.request(Input::File(path)),
            Command::PlayUrl(url) => self.request(Input::Url(url)),
            Command::Loaded(generation, name, result) => {
                // Stop/new-source invalidates older results, including errors.
                if !self.loading.complete(generation) {
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
            Command::Shutdown => unreachable!("handled by the command loop"),
        }
        self.publish();
    }

    fn request(&mut self, input: Input) {
        // Drop previous playback and publish loading immediately. File opening,
        // HTTP and decoder probing happen entirely on the loader thread.
        self.player = None;
        let generation = self.loading.begin();
        if let Ok(mut status) = self.shared.lock() {
            status.source = Some(input.name());
            status.error = None;
        }
        self.loads.replace((generation, input));
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
        if self.player.as_ref().is_some_and(|player| player.empty()) {
            self.player = None;
            self.publish();
        }
    }

    fn publish(&self) {
        if let Ok(mut s) = self.shared.lock() {
            s.volume = self.volume;
            s.state = match &self.player {
                None if self.loading.pending && self.loading.paused => "paused",
                None if self.loading.pending => "loading",
                None => "stopped",
                Some(p) if p.is_paused() => "paused",
                Some(_) => "playing",
            }
            .into();
            if self.player.is_none() && !self.loading.pending {
                s.source = None;
            }
        }
    }
}

fn load(input: Input) -> Result<Audio, String> {
    match input {
        Input::File(path) => {
            let file = File::open(path).map_err(|e| e.to_string())?;
            let decoder = Decoder::new(BufReader::new(file)).map_err(|e| e.to_string())?;
            Ok(Box::new(decoder))
        }
        Input::Url(url) => open_stream(&url).map(|decoder| Box::new(decoder) as Audio),
    }
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
