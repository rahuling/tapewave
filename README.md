# Tapedeck

An open-source, Winamp-style music player for the internet age. Written in Rust.

## Status: Milestone 1

Desktop (Windows, macOS, Linux) with:

- Local audio files (MP3, FLAC, OGG/Vorbis, WAV, AAC/MP4)
- Internet radio streams (direct HTTP audio streams)
- Podcasts from RSS/Atom feeds, playing episode audio enclosures
- Play, pause, stop, and volume

## Planned

- Playlists (M3U/PLS import and export), podcast subscriptions
- 10-band EQ and spectrum visualizer
- Classic Winamp skin support (`.wsz`)
- iOS, iPadOS, and Android apps. The engine is a separate crate with no UI dependencies, so the same playback code can back the mobile apps.
- YouTube Music and Spotify as embedded sources, later. These services don't expose audio to third-party apps, so Tapedeck can control their players but can't analyze their audio (no EQ or visualizer for them).

## Layout

```
engine/      playback engine, radio streams, podcast feeds (no UI deps)
src/         Tauri desktop shell and commands
ui/          frontend (plain HTML/JS, no bundler)
```

## Running

Requires Rust (stable) and, on Windows, WebView2 (included in Windows 11).

```
cargo run
```

## Name

"Tapedeck" is a working name. Check trademark and crate/package-name availability before a public release.
