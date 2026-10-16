# Tapewave

A Winamp-inspired, **native Windows x64** music player written in Rust.
Runs on Windows 10 (1809+) and Windows 11 on Intel/AMD processors.
No browser frontend, WebView2, local server, account or telemetry.

## Current features

- Local MP3, FLAC, OGG/Vorbis, WAV and AAC/MP4 playback.
- Direct HTTP/HTTPS internet radio streams.
- RSS/Atom podcast feeds and an episode list (double-click or select and Play).
- Play/pause, stop and volume, with visible loading/error states.
- Native file picker, Unicode file paths and file drag-and-drop.
- Native menus, About dialog, keyboard navigation and a DPI-aware interface.
- Tray icon: minimize hides the window; click the icon to restore, right-click
  for playback controls or Exit. Closing the main window exits the application.
- `Ctrl+O` opens a file; `Ctrl+P` toggles play/pause; `Ctrl+S` stops.
- Handles Windows media play/pause and stop commands when Windows routes them
  to the application. Global/background media-session integration is not yet implemented.
- Volume and entered radio/feed URLs are saved on normal exit in
  `%LOCALAPPDATA%\Tapewave\settings.json`. Nothing starts playing automatically.
- Single-instance behaviour: launching again restores the existing window.
  Command-line files are accepted on the first launch, not forwarded to an existing instance.

## Install / uninstall

The packaging pipeline produces:

- `Tapewave-<version>-windows-x64-setup.exe`: per-user installer, no admin rights
  needed. Adds Start Menu launch/uninstall shortcuts and an optional desktop
  shortcut, and registers with Windows **Settings > Apps > Installed apps**.
- `Tapewave-<version>-windows-x64-portable.zip`: executable and licence, with
  nothing to install. Preferences still use the per-user location above.

Use Windows Settings or the Start Menu shortcut to uninstall. Interactive
uninstall offers to remove preferences; it never deletes music or podcasts.
Silent uninstall preserves preferences. No startup entry, file association,
firewall rule or default-player setting is added.

Builds are **not code-signed yet**. Windows may display an unknown-publisher
warning. Authenticode signing requires a publisher certificate; it is not
replaced by the application's version metadata.

## Build from source

Install stable Rust with the MSVC toolchain and Visual Studio Build Tools
(Desktop development with C++ and a Windows SDK). WebView2 is not required.

```powershell
cargo run --target x86_64-pc-windows-msvc
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

Create a release executable and portable ZIP:

```powershell
./scripts/build-windows.ps1
```

To also compile the installer, install **Inno Setup 6** from
https://jrsoftware.org/isinfo.php and run:

```powershell
./scripts/build-windows.ps1 -Installer
# Or provide an explicit compiler path:
./scripts/build-windows.ps1 -Installer -IsccPath 'C:\Tools\Inno Setup 6\ISCC.exe'
```

Outputs go to `dist/`. The stable installer AppId supports upgrades in place.
Close Tapewave before installing an upgrade. The C runtime is statically linked;
there is no separate VC++ runtime installer to run.

## Architecture

```text
Native Win32 UI thread (windows-sys)
  -> in-process commands -> playback worker -> rodio / Windows audio
                              -> latest-wins source loader
                                   -> bounded HTTP prefetch buffer
  -> background podcast fetch -> result channel -> UI timer
```

- `src/native.rs`: Win32 window, controls, file picker, menus, tray and message loop.
- `src/settings.rs`: per-user preferences.
- `engine/`: UI-independent playback, loading, radio and podcast code.
- `packaging/windows/`: native icon/version/manifest resources and Inno Setup script.
- `scripts/build-windows.ps1`: test, build and package.
- `.github/workflows/windows.yml`: Windows tests, lint and packaging artifacts.
- `docs/`: public project website, not an application frontend.

Networking/decoder probing does not block the command thread. Stop and newer
sources invalidate stale loader results. The loader has a latest-wins mailbox;
network prefetch is bounded to 256 KiB per stream. A decoder times out after ten
seconds without a chunk. This is **logical cancellation**, not forced termination
of an OS socket read: an abandoned network worker can finish later. A new source
can wait for the active loader's network/probing timeout. Automatic reconnection,
ICY metadata and detailed playback-time network error reporting remain future work.
Podcast fetches have a 20-second deadline and an 8 MiB feed limit.

## Planned

- Playlists, seeking, track duration and podcast subscriptions.
- 10-band EQ and a real spectrum visualizer.
- Classic Winamp `.wsz` skins.
- Full Windows media-session integration and accessibility/high-contrast testing.
- Authenticode signing and update delivery.

The current desktop shell is Windows-specific. The engine remains independent
so future native platforms need not reuse a web interface.

MIT License. Not affiliated with Winamp or Llama Group.
