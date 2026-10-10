//! Manual smoke test: `cargo run -p tapewave-engine --example smoke -- <stream-url> <feed-url> [local-file]`.
//! Parses the podcast feed, plays the first episode briefly, then the stream, and prints status.

use std::thread::sleep;
use std::time::Duration;

use tapewave_engine::{podcast, Engine};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let stream_url = args.first().expect("stream url");
    let feed_url = args.get(1).expect("feed url");

    let feed = podcast::fetch(feed_url).expect("fetch feed");
    println!("podcast: {} ({} episodes)", feed.title, feed.episodes.len());
    if let Some(ep) = feed.episodes.first() {
        println!("first episode: {} -> {}", ep.title, ep.audio_url);
    }

    let engine = Engine::spawn();

    if let Some(file) = args.get(2) {
        engine.play_file(file);
        sleep(Duration::from_secs(3));
        println!("file status: {:?}", engine.status());
    }

    engine.play_url(stream_url);
    for i in 0..8 {
        sleep(Duration::from_secs(1));
        println!("t={}s stream status: {:?}", i + 1, engine.status());
    }

    engine.pause();
    sleep(Duration::from_millis(500));
    println!("after pause: {:?}", engine.status());

    engine.stop();
    sleep(Duration::from_millis(500));
    println!("after stop: {:?}", engine.status());
}
