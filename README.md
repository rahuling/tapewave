# Tapewave

**Quiet Hi-Fi.** A native Windows x64 audio player for your music, radio and podcasts.
Rust/egui renders directly to a native Windows window through Direct3D 12.
No HTML application frontend, WebView, local server, account or telemetry.

## Version 0.2 — the player comes first

- A persistent player: Previous, Play/Pause, Next, Stop, volume and mute.
- Separate **Music**, **Radio** and **Podcasts** sections. Browsing never changes playback.
- Music collection with native file/folder pickers, drag-and-drop, title/artist/album
  metadata, search, a finite playback queue, shuffle and repeat.
- Saved radio stations: add/edit/remove, Connect/Disconnect and source-local errors.
  Radio is live; there is no pretend seek bar or timeshift pause.
- Saved podcast shows and episode lists, feed refresh, played state and resume positions.
- Duration/position and seeking for supported finite files; podcast skip −15s / +30s.
- Podcast playback rates from 0.75× to 2×. **Rate currently changes pitch as well.**
- **View > Appearance**: Quiet Hi-Fi, Paper and Blue Hour presets, plus editable
  background, panels, text, accent, borders, selection and warning colours.
  Readable palettes preview live; low-contrast edits must be corrected before Save.
  Cancel restores the saved palette, Reset restores the default, and Windows high
  contrast takes precedence without overwriting your colours.
- Standard Windows title bar, resize/Snap behaviour, saved window geometry, native
  file dialogs and a tray icon. Minimize hides to the tray only if its icon was added
  successfully. Close exits. The tray's right-click menu provides playback and Exit.
- Ctrl+O: add files; Ctrl+P: play/pause; Ctrl+S: stop. Space toggles playback when no
  input or button has keyboard focus. Tab/Shift+Tab navigate controls.
- Handles Windows media commands when Windows routes them to the application.
  Full global/background media-session integration remains future work.

The layout adapts at compact sizes: music hides secondary columns and podcasts
use a show dropdown instead of squeezing the two-column show/episode view.
Colour and layout are independent; there is no need to edit source to change colours.

## Use

**Music:** add files/folders, single-click to select and double-click to play.
Right-click a track for Play next / Add to queue / Remove from collection. Removing
an item from the app never deletes its file. Dropping files adds to Music without
interrupting the current source. A folder import is capped at 10,000 audio files
and 50,000 directory entries per operation; symbolic links are not followed.

**Radio:** save a name and a direct HTTP/HTTPS audio URL, not the station's website.
Disconnect ends the stream. Switching to radio preserves the finite queue.

**Podcasts:** add an RSS/Atom feed, choose a show, then Play/Resume an episode.
Finite episode audio is prepared in an automatically removed temporary file before
playback, enabling real seek/resume without depending on HTTP range support.
This is not a download manager: there is no offline episode library or persistent
audio cache. Preparation has a 256 MiB limit and a 180-second network deadline.
Slow or large episodes may take time to start. Progress is saved approximately
every five seconds, on source changes, on Stop and on normal exit. A completed
episode starts at the beginning when replayed.

**Queue:** a music play action queues the currently filtered collection; an episode
play action queues that show's episode order. Previous/Next operate on that finite
queue. Podcast skip buttons always mean seconds, not queued episodes.

## Install / uninstall

Target: **Intel/AMD Windows 10 (1809+) or Windows 11**, with a Direct3D 12-capable
graphics environment. No separate WebView or VC++ runtime installer is required.

Packages in `dist/`:

- `Tapewave-<version>-windows-x64-setup.exe`: per-user install, no administrator
  rights required; Start Menu launch/uninstall shortcuts, optional desktop shortcut,
  and uninstall via Windows **Settings > Apps > Installed apps**.
- `Tapewave-<version>-windows-x64-portable.zip`: executable and licence; no installation.
  Preferences still use the per-user locations below.

Both packages are **unsigned** until a publisher supplies an Authenticode certificate.
Windows may display an unknown-publisher/SmartScreen warning.

Install upgrades in place using the stable installer AppId; close Tapewave first.
Interactive uninstall can remove saved preferences and window geometry. Silent
uninstall preserves them. Music files are never deleted. No startup entry, file
association, default-player setting or firewall rule is installed.

## Local data and privacy

- `%LOCALAPPDATA%\Tapewave\settings.json`: volume, colours, file references and tags,
  saved stations/shows, cached episode metadata and listening positions.
- `%LOCALAPPDATA%\Tapewave\window.ron`: window geometry / UI memory.
- Temporary episode files: system temporary folder, deleted when their source is dropped.

Network requests only occur for a station/feed/episode you request. No artwork,
fonts, station directories or analytics are fetched. Stream/feed URLs with embedded
username/password credentials are rejected; signed URL query parameters are saved
as entered, so treat the settings file as private.

The original single radio/feed URL settings are migrated additively on first load.
There is no automatic playback at launch. Single-instance launch restores the
existing window; file arguments are not forwarded to an existing instance.

## Build and package

Requires stable Rust MSVC, Visual Studio C++ Build Tools and a Windows SDK.

```powershell
cargo run --target x86_64-pc-windows-msvc
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -j 2 -- -D warnings
./scripts/build-windows.ps1
```

For the setup EXE, install Inno Setup 6 from https://jrsoftware.org/isinfo.php:

```powershell
./scripts/build-windows.ps1 -Installer
# Optional explicit compiler path:
./scripts/build-windows.ps1 -Installer -IsccPath 'C:\Tools\Inno Setup 6\ISCC.exe'
```

On machines with restrictive PowerShell script policies, run the trusted build
script in a child PowerShell process without changing the machine-wide policy:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File ./scripts/build-windows.ps1 -Installer
```

Debug and test symbols/incremental compilation are disabled to reduce build disk use.
The GPU backend is Windows Direct3D 12 only, rather than bundling unrelated renderers.

### UI regression captures

Debug builds include an isolated fixture harness that captures their **own rendered
surface**, not the desktop. It plays no audio, contacts no services, creates no tray
icon and saves no user preferences:

```powershell
cargo run -- --ui-smoke-test C:\Temp\tapewave-ui
```

It writes Music, Radio, Podcasts, Appearance, light palette, compact queue, station
dialog, empty-state and compact-Appearance PNGs, then exits. The harness is excluded
from release builds. `scripts/check-ui.ps1` runs the capture and checks all nine files.

## Architecture

```text
Native Windows title bar + egui/wgpu UI
  -> in-process commands -> playback worker -> rodio / Windows audio
                              -> latest-wins local/radio loader
                              -> separate latest-wins episode preparation worker
  -> background file import / feed parsing -> result channel
  -> Win32 tray/media bridge -> application events
```

- `src/desktop.rs`: state, background jobs and playback actions.
- `src/desktop/views.rs`: the Quiet Hi-Fi views, dialogs and transport drawing.
- `src/native.rs`: Windows tray, media commands, single instance and high contrast.
- `src/library.rs`, `src/settings.rs`, `src/theme.rs`: queue/data, persistence and semantic colours.
- `engine/`: UI-independent audio, timing, seek/rate, bounded radio prefetch and podcasts.
- `design/quiet-hifi-mockup.html`: approved design reference, not the application frontend.
- `packaging/windows/`, `scripts/`, `.github/workflows/`: resources, packaging and CI.

Loading never performs network I/O on the command/UI threads. Requests carry IDs;
Stop/new-source actions invalidate stale results. Radio prefetch is bounded to
256 KiB. Episode preparation has a separate worker so it cannot hold up local/radio
loading. Cancellation is logical: an abandoned OS network read may complete later.
Automatic radio reconnection, ICY metadata and detailed playback-time stream errors
remain future work. Feed fetches have a 20-second deadline and an 8 MiB input limit.

## Still planned

Real spectrum/EQ, `.wsz` skins, playlist file import/export, pitch-preserving speed,
offline podcast downloads, full Windows media sessions, Authenticode signing and
automatic updates. No placeholder EQ/visualizer controls are shown.

MIT License. Not affiliated with Winamp or Llama Group.
