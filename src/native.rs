//! Native Win32 shell. All HWNDs and GDI resources stay on the UI thread.
//! Worker results are polled by a window timer; workers never touch HWNDs.

use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr::{null, null_mut};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use crate::settings::Settings;
use tapewave_engine::{Engine, Status, podcast};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::SystemServices::{
    APPCOMMAND_MEDIA_PLAY_PAUSE, APPCOMMAND_MEDIA_STOP, SS_ENDELLIPSIS, SS_PATHELLIPSIS,
};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::Controls::Dialogs::*;
use windows_sys::Win32::UI::Controls::*;
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const OPEN: u16 = 101;
const PLAY: u16 = 102;
const STOP: u16 = 103;
const RADIO: u16 = 104;
const FEED: u16 = 105;
const EPISODE: u16 = 106;
const VOLUME: u16 = 107;
const RADIO_URL: u16 = 108;
const FEED_URL: u16 = 109;
const EPISODES: u16 = 110;
const EXIT: u16 = 111;
const ABOUT: u16 = 112;
// The Win32 trackbar's get-position message is WM_USER (not exported by windows-sys).
const TBM_GETPOS: u32 = WM_USER;
const TRAY_MESSAGE: u32 = WM_APP + 1;
const TIMER: usize = 1;
const BG: u32 = 0x00221d1b;
const PANEL: u32 = 0x00322926;
const TEXT: u32 = 0x00e6ddd8;
const ACCENT: u32 = 0x009ad36f;

fn wide(value: &str) -> Vec<u16> {
    // Text from feeds and file paths must not inject an early terminator.
    value
        .replace('\0', "")
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

struct Controls {
    display: HWND,
    source: HWND,
    error: HWND,
    play: HWND,
    volume: HWND,
    radio: HWND,
    feed: HWND,
    load: HWND,
    feed_title: HWND,
    episodes: HWND,
}

struct App {
    engine: Engine,
    controls: Option<Controls>,
    font: HFONT,
    display_font: HFONT,
    background: HBRUSH,
    panel: HBRUSH,
    dpi: u32,
    children: Vec<(HWND, i32, i32, i32, i32)>,
    episodes: Vec<podcast::Episode>,
    feed_result: Option<Receiver<Result<podcast::Podcast, String>>>,
    last_status: Option<Status>,
    last_source: Option<Source>,
    tray_added: bool,
    settings: Settings,
}

#[derive(Clone)]
enum Source {
    File(std::path::PathBuf),
    Url(String),
}

impl Drop for App {
    fn drop(&mut self) {
        // SAFETY: these GDI objects belong to App and are no longer in use
        // after the window and its child controls have been destroyed.
        unsafe {
            DeleteObject(self.font);
            DeleteObject(self.display_font);
            DeleteObject(self.background);
            DeleteObject(self.panel);
        }
    }
}

impl App {
    fn new() -> Self {
        let settings = Settings::load();
        let engine = Engine::spawn();
        engine.set_volume(settings.volume);
        Self {
            engine,
            controls: None,
            font: null_mut(),
            display_font: null_mut(),
            // SAFETY: CreateSolidBrush accepts any COLORREF.
            background: unsafe { CreateSolidBrush(BG) },
            panel: unsafe { CreateSolidBrush(PANEL) },
            dpi: 96,
            children: Vec::new(),
            episodes: Vec::new(),
            feed_result: None,
            last_status: None,
            last_source: None,
            tray_added: false,
            settings,
        }
    }

    fn px(&self, value: i32) -> i32 {
        (value as i64 * self.dpi as i64 / 96) as i32
    }

    unsafe fn control(
        &mut self,
        parent: HWND,
        class: &str,
        text: &str,
        style: u32,
        id: u16,
        rect: (i32, i32, i32, i32),
    ) -> HWND {
        let (x, y, width, height) = rect;
        // SAFETY: all strings outlive the call; parent is the live main window.
        let control = unsafe {
            CreateWindowExW(
                0,
                wide(class).as_ptr(),
                wide(text).as_ptr(),
                WS_CHILD | WS_VISIBLE | style,
                self.px(x),
                self.px(y),
                self.px(width),
                self.px(height),
                parent,
                id as usize as HMENU,
                GetModuleHandleW(null()),
                null(),
            )
        };
        unsafe { SendMessageW(control, WM_SETFONT, self.font as usize, 1) };
        self.children.push((control, x, y, width, height));
        control
    }

    unsafe fn create_controls(&mut self, hwnd: HWND) -> Result<(), String> {
        self.dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        unsafe { self.make_fonts() };
        let button = WS_TABSTOP | BS_PUSHBUTTON as u32;
        let edit = WS_TABSTOP | WS_BORDER | ES_AUTOHSCROLL as u32;
        unsafe {
            self.control(
                hwnd,
                "STATIC",
                "T A P E W A V E   /   NATIVE X64",
                0,
                0,
                (20, 16, 472, 22),
            );
            let display = self.control(hwnd, "STATIC", "STOPPED", 0, 0, (20, 48, 472, 32));
            SendMessageW(display, WM_SETFONT, self.display_font as usize, 1);
            let source = self.control(
                hwnd,
                "STATIC",
                "Nothing playing",
                SS_PATHELLIPSIS,
                0,
                (20, 86, 472, 24),
            );
            let error = self.control(hwnd, "STATIC", "", 0, 0, (20, 116, 472, 44));
            self.control(
                hwnd,
                "BUTTON",
                "Open file...",
                button,
                OPEN,
                (20, 170, 146, 36),
            );
            let play = self.control(hwnd, "BUTTON", "Play", button, PLAY, (178, 170, 146, 36));
            self.control(hwnd, "BUTTON", "Stop", button, STOP, (336, 170, 156, 36));
            self.control(hwnd, "STATIC", "VOLUME", 0, 0, (20, 228, 90, 22));
            let volume = self.control(
                hwnd,
                "msctls_trackbar32",
                "",
                WS_TABSTOP | TBS_AUTOTICKS,
                VOLUME,
                (112, 218, 380, 40),
            );
            SendMessageW(volume, TBM_SETRANGE, 1, (100 << 16) as isize);
            SendMessageW(
                volume,
                TBM_SETPOS,
                1,
                (self.settings.volume * 100.0).round() as isize,
            );
            self.control(
                hwnd,
                "STATIC",
                "INTERNET RADIO  /  AUDIO URL",
                0,
                0,
                (20, 278, 472, 22),
            );
            let radio = self.control(hwnd, "EDIT", "", edit, RADIO_URL, (20, 308, 366, 30));
            SendMessageW(radio, EM_SETLIMITTEXT, 8192, 0);
            self.control(hwnd, "BUTTON", "Tune in", button, RADIO, (398, 306, 94, 34));
            self.control(
                hwnd,
                "STATIC",
                "PODCASTS  /  RSS OR ATOM FEED",
                0,
                0,
                (20, 366, 472, 22),
            );
            let feed = self.control(hwnd, "EDIT", "", edit, FEED_URL, (20, 396, 366, 30));
            SendMessageW(feed, EM_SETLIMITTEXT, 8192, 0);
            let load = self.control(
                hwnd,
                "BUTTON",
                "Load feed",
                button,
                FEED,
                (398, 394, 94, 34),
            );
            let feed_title = self.control(
                hwnd,
                "STATIC",
                "No podcast loaded",
                SS_ENDELLIPSIS,
                0,
                (20, 444, 472, 24),
            );
            let episodes = self.control(
                hwnd,
                "LISTBOX",
                "",
                WS_TABSTOP
                    | WS_BORDER
                    | WS_VSCROLL
                    | LBS_NOTIFY as u32
                    | LBS_NOINTEGRALHEIGHT as u32,
                EPISODES,
                (20, 478, 472, 160),
            );
            self.control(
                hwnd,
                "BUTTON",
                "Play selected episode",
                button,
                EPISODE,
                (20, 650, 472, 34),
            );
            self.control(
                hwnd,
                "STATIC",
                "Ctrl+O: open   |   Ctrl+P: play/pause   |   Ctrl+S: stop",
                0,
                0,
                (20, 704, 472, 22),
            );
            SetWindowTextW(radio, wide(&self.settings.radio_url).as_ptr());
            SetWindowTextW(feed, wide(&self.settings.feed_url).as_ptr());
            DragAcceptFiles(hwnd, 1);
            self.controls = Some(Controls {
                display,
                source,
                error,
                play,
                volume,
                radio,
                feed,
                load,
                feed_title,
                episodes,
            });
            self.tray_added = self.tray(hwnd, NIM_ADD);
            SetTimer(hwnd, TIMER, 100, None);
        }
        if self.children.iter().any(|(child, ..)| child.is_null())
            || self.font.is_null()
            || self.display_font.is_null()
        {
            return Err("Could not create native player controls.".into());
        }
        Ok(())
    }

    unsafe fn make_fonts(&mut self) {
        // SAFETY: fonts are owned by App; previous fonts are deleted only after
        // every child has switched to the replacement in rescale().
        self.font = unsafe {
            CreateFontW(
                -self.px(14),
                0,
                0,
                0,
                FW_NORMAL as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                CLEARTYPE_QUALITY as u32,
                0,
                wide("Segoe UI").as_ptr(),
            )
        };
        self.display_font = unsafe {
            CreateFontW(
                -self.px(26),
                0,
                0,
                0,
                FW_BOLD as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                CLEARTYPE_QUALITY as u32,
                0,
                wide("Consolas").as_ptr(),
            )
        };
    }

    unsafe fn rescale(&mut self, hwnd: HWND, dpi: u32, rect: *const RECT) {
        let old_font = self.font;
        let old_display = self.display_font;
        self.dpi = dpi.max(96);
        unsafe {
            self.make_fonts();
            for &(child, x, y, w, h) in &self.children {
                MoveWindow(child, self.px(x), self.px(y), self.px(w), self.px(h), 1);
                SendMessageW(child, WM_SETFONT, self.font as usize, 1);
            }
            if let Some(c) = &self.controls {
                SendMessageW(c.display, WM_SETFONT, self.display_font as usize, 1);
            }
            DeleteObject(old_font);
            DeleteObject(old_display);
            let rect = &*rect;
            SetWindowPos(
                hwnd,
                null_mut(),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            InvalidateRect(hwnd, null(), 1);
        }
    }

    unsafe fn show_error(&self, message: &str) {
        if let Some(c) = &self.controls {
            unsafe { SetWindowTextW(c.error, wide(message).as_ptr()) };
        }
    }

    fn play_source(&mut self, source: Source) {
        match &source {
            Source::File(path) => self.engine.play_file(path.clone()),
            Source::Url(url) => self.engine.play_url(url.clone()),
        }
        self.last_source = Some(source);
    }

    unsafe fn command(&mut self, hwnd: HWND, id: u16) {
        match id {
            OPEN => unsafe { self.open_file(hwnd) },
            PLAY => match self.engine.status().state.as_str() {
                "playing" | "loading" => self.engine.pause(),
                "paused" => self.engine.resume(),
                _ => {
                    if let Some(source) = self.last_source.clone() {
                        self.play_source(source)
                    }
                }
            },
            STOP => self.engine.stop(),
            RADIO => {
                let url = unsafe { text(self.controls.as_ref().unwrap().radio) };
                if valid_url(&url) {
                    self.play_source(Source::Url(url));
                } else {
                    unsafe { self.show_error("Enter an http:// or https:// audio stream URL.") };
                }
            }
            FEED => unsafe { self.load_feed() },
            EPISODE => {
                let index = unsafe {
                    SendMessageW(self.controls.as_ref().unwrap().episodes, LB_GETCURSEL, 0, 0)
                };
                if let Some(episode) = self.episodes.get(index as usize) {
                    self.play_source(Source::Url(episode.audio_url.clone()));
                }
            }
            EXIT => {
                unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
            }
            ABOUT => unsafe {
                MessageBoxW(hwnd, wide(concat!("Tapewave ", env!("CARGO_PKG_VERSION"), "\nNative Windows x64 audio player\n\nLocal files, internet radio and podcasts.\nMIT License.\n\nMinimize to tray; close the window to exit.")).as_ptr(), wide("About Tapewave").as_ptr(), MB_OK | MB_ICONINFORMATION);
            },
            _ => {}
        }
    }

    unsafe fn open_file(&mut self, hwnd: HWND) {
        // A large UTF-16 buffer preserves non-ASCII paths without a lossy conversion.
        let mut path = vec![0u16; 32768];
        let filter: Vec<u16> =
            "Audio files\0*.mp3;*.flac;*.ogg;*.wav;*.aac;*.m4a;*.mp4\0All files\0*.*\0\0"
                .encode_utf16()
                .collect();
        let mut dialog: OPENFILENAMEW = unsafe { zeroed() };
        dialog.lStructSize = size_of::<OPENFILENAMEW>() as u32;
        dialog.hwndOwner = hwnd;
        dialog.lpstrFilter = filter.as_ptr();
        dialog.lpstrFile = path.as_mut_ptr();
        dialog.nMaxFile = path.len() as u32;
        dialog.Flags = OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_EXPLORER;
        if unsafe { GetOpenFileNameW(&mut dialog) } != 0 {
            use std::os::windows::ffi::OsStringExt;
            let end = path.iter().position(|c| *c == 0).unwrap_or(path.len());
            self.play_source(Source::File(
                std::ffi::OsString::from_wide(&path[..end]).into(),
            ));
        }
    }

    unsafe fn load_feed(&mut self) {
        if self.feed_result.is_some() {
            return;
        }
        let c = self.controls.as_ref().unwrap();
        let url = unsafe { text(c.feed) };
        if !valid_url(&url) {
            unsafe { self.show_error("Enter an http:// or https:// podcast feed URL.") };
            return;
        }
        unsafe {
            EnableWindow(c.load, 0);
            SetWindowTextW(c.feed_title, wide("Loading podcast...").as_ptr());
            self.show_error("");
        }
        let (tx, rx) = mpsc::channel();
        self.feed_result = Some(rx);
        thread::spawn(move || {
            let _ = tx.send(podcast::fetch(&url));
        });
    }

    unsafe fn refresh(&mut self, hwnd: HWND) {
        let c = self.controls.as_ref().unwrap();
        let status = self.engine.status();
        if self.last_status.as_ref() != Some(&status) {
            unsafe {
                SetWindowTextW(c.display, wide(&status.state.to_uppercase()).as_ptr());
                SetWindowTextW(
                    c.source,
                    wide(status.source.as_deref().unwrap_or("Nothing playing")).as_ptr(),
                );
                SetWindowTextW(
                    c.play,
                    wide(if status.state == "playing" || status.state == "loading" {
                        "Pause"
                    } else {
                        "Play"
                    })
                    .as_ptr(),
                );
                SendMessageW(
                    c.volume,
                    TBM_SETPOS,
                    1,
                    (status.volume * 100.0).round() as isize,
                );
                // UI validation/feed errors remain visible until a playback state change.
                self.show_error(status.error.as_deref().unwrap_or(""));
                self.tray(hwnd, NIM_MODIFY);
            }
            self.last_status = Some(status);
        }
        let result = self
            .feed_result
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Podcast worker stopped unexpectedly.".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = result {
            self.feed_result = None;
            unsafe { EnableWindow(c.load, 1) };
            match result {
                Ok(feed) => {
                    unsafe {
                        SetWindowTextW(
                            c.feed_title,
                            wide(&format!(
                                "{} ({} episodes)",
                                feed.title,
                                feed.episodes.len()
                            ))
                            .as_ptr(),
                        );
                        SendMessageW(c.episodes, LB_RESETCONTENT, 0, 0);
                        for episode in &feed.episodes {
                            let label = format!(
                                "{}  {}",
                                episode
                                    .published
                                    .as_deref()
                                    .unwrap_or("")
                                    .get(..10)
                                    .unwrap_or(""),
                                episode.title
                            );
                            SendMessageW(
                                c.episodes,
                                LB_ADDSTRING,
                                0,
                                wide(&label).as_ptr() as isize,
                            );
                        }
                        if !feed.episodes.is_empty() {
                            SendMessageW(c.episodes, LB_SETCURSEL, 0, 0);
                        }
                    }
                    self.episodes = feed.episodes;
                }
                Err(error) => unsafe {
                    SetWindowTextW(
                        c.feed_title,
                        wide("Could not load feed. Try again.").as_ptr(),
                    );
                    self.show_error(&error);
                },
            }
        }
    }

    unsafe fn tray(&self, hwnd: HWND, action: u32) -> bool {
        let mut data: NOTIFYICONDATAW = unsafe { zeroed() };
        data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
        data.hWnd = hwnd;
        data.uID = 1;
        data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        data.uCallbackMessage = TRAY_MESSAGE;
        data.hIcon =
            unsafe { LoadIconW(GetModuleHandleW(null()), std::ptr::without_provenance(1)) };
        let tooltip = wide(&format!("Tapewave - {}", self.engine.status().state));
        let len = tooltip.len().min(data.szTip.len() - 1);
        data.szTip[..len].copy_from_slice(&tooltip[..len]);
        unsafe { Shell_NotifyIconW(action, &data) != 0 }
    }

    unsafe fn tray_menu(&mut self, hwnd: HWND) {
        let mut point: POINT = unsafe { zeroed() };
        unsafe {
            GetCursorPos(&mut point);
            let menu = CreatePopupMenu();
            AppendMenuW(
                menu,
                MF_STRING,
                PLAY as usize,
                wide("Play / Pause").as_ptr(),
            );
            AppendMenuW(menu, MF_STRING, STOP as usize, wide("Stop").as_ptr());
            AppendMenuW(
                menu,
                MF_STRING,
                ABOUT as usize,
                wide("About Tapewave").as_ptr(),
            );
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            AppendMenuW(
                menu,
                MF_STRING,
                EXIT as usize,
                wide("Exit Tapewave").as_ptr(),
            );
            SetForegroundWindow(hwnd);
            let id = TrackPopupMenu(
                menu,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                point.x,
                point.y,
                0,
                hwnd,
                null(),
            );
            DestroyMenu(menu);
            if id != 0 {
                self.command(hwnd, id as u16);
            }
            PostMessageW(hwnd, WM_NULL, 0, 0);
        }
    }

    unsafe fn save_settings(&self) {
        let mut settings = self.settings.clone();
        settings.volume = self.engine.status().volume;
        if let Some(c) = &self.controls {
            settings.radio_url = unsafe { text(c.radio) };
            settings.feed_url = unsafe { text(c.feed) };
        }
        if let Err(error) = settings.save() {
            // Saving failure must not prevent shutdown.
            eprintln!("Could not save settings: {error}");
        }
    }
}

fn valid_url(url: &str) -> bool {
    let url = url.to_ascii_lowercase();
    (url.starts_with("https://") || url.starts_with("http://"))
        && !url.chars().any(char::is_whitespace)
        && url
            .split_once("://")
            .is_some_and(|(_, host)| !host.is_empty())
}

unsafe fn text(hwnd: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(hwnd) }.max(0) as usize;
    let mut buffer = vec![0u16; len + 1];
    let read = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..read.max(0) as usize])
        .trim()
        .to_string()
}

unsafe extern "system" fn window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Destruction can be dispatched synchronously from another handler. These
    // paths deliberately do not borrow App, so nested dispatch cannot alias it.
    if message == WM_DESTROY {
        unsafe {
            KillTimer(hwnd, TIMER);
            let mut data: NOTIFYICONDATAW = zeroed();
            data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
            data.hWnd = hwnd;
            data.uID = 1;
            Shell_NotifyIconW(NIM_DELETE, &data);
            PostQuitMessage(0);
        }
        return 0;
    }
    if message == WM_NCDESTROY {
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        }
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    if message == WM_NCCREATE {
        // SAFETY: WM_NCCREATE supplies a valid CREATESTRUCTW; lpCreateParams
        // is the stable Box<App> owned by run() throughout the message loop.
        let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
        unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize) };
        return 1;
    }
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *mut App;
    if ptr.is_null() {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }

    // Do not hold &mut App across modal APIs (file picker, popup menus) or
    // Win32 calls that dispatch nested messages. Each handler's mutable access
    // is narrowly scoped; timer/control re-entry is guarded below.
    thread_local! { static IN_HANDLER: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
    let nested = IN_HANDLER.with(|busy| busy.replace(true));
    struct Guard(bool);
    impl Drop for Guard {
        fn drop(&mut self) {
            IN_HANDLER.with(|busy| busy.set(self.0));
        }
    }
    let _guard = Guard(nested);
    if nested {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    }
    let app = unsafe { &mut *ptr };
    match message {
        WM_CREATE => match unsafe { app.create_controls(hwnd) } {
            Ok(()) => 0,
            Err(error) => {
                show_startup_error(&error);
                -1
            }
        },
        WM_COMMAND => {
            let id = (wparam & 0xffff) as u16;
            let notification = ((wparam >> 16) & 0xffff) as u32;
            if id == EPISODES && notification == LBN_DBLCLK {
                unsafe { app.command(hwnd, EPISODE) };
            } else if notification == BN_CLICKED || notification == 1 {
                unsafe { app.command(hwnd, id) };
            }
            0
        }
        WM_HSCROLL => {
            if app
                .controls
                .as_ref()
                .is_some_and(|c| c.volume == lparam as HWND)
            {
                let volume = unsafe { SendMessageW(lparam as HWND, TBM_GETPOS, 0, 0) };
                app.engine.set_volume(volume as f32 / 100.0);
            }
            0
        }
        WM_TIMER if wparam == TIMER => {
            unsafe { app.refresh(hwnd) };
            0
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => unsafe {
            let dc = wparam as HDC;
            SetTextColor(
                dc,
                if app
                    .controls
                    .as_ref()
                    .is_some_and(|c| c.display == lparam as HWND)
                {
                    ACCENT
                } else {
                    TEXT
                },
            );
            let edit = message != WM_CTLCOLORSTATIC;
            SetBkColor(dc, if edit { PANEL } else { BG });
            if edit {
                app.panel as isize
            } else {
                app.background as isize
            }
        },
        WM_ERASEBKGND => unsafe {
            let mut rect: RECT = zeroed();
            GetClientRect(hwnd, &mut rect);
            FillRect(wparam as HDC, &rect, app.background);
            1
        },
        WM_DPICHANGED => {
            unsafe { app.rescale(hwnd, (wparam & 0xffff) as u32, lparam as *const RECT) };
            0
        }
        WM_DROPFILES => {
            let drop = wparam as HDROP;
            unsafe {
                let len = DragQueryFileW(drop, 0, null_mut(), 0);
                if len > 0 {
                    use std::os::windows::ffi::OsStringExt;
                    let mut path = vec![0u16; len as usize + 1];
                    DragQueryFileW(drop, 0, path.as_mut_ptr(), path.len() as u32);
                    app.play_source(Source::File(
                        std::ffi::OsString::from_wide(&path[..len as usize]).into(),
                    ));
                }
                DragFinish(drop);
            }
            0
        }
        WM_APPCOMMAND => {
            match ((lparam >> 16) & 0x7ff) as u32 {
                APPCOMMAND_MEDIA_PLAY_PAUSE => unsafe { app.command(hwnd, PLAY) },
                APPCOMMAND_MEDIA_STOP => app.engine.stop(),
                _ => return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
            }
            1
        }
        TRAY_MESSAGE => {
            match lparam as u32 {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => unsafe {
                    ShowWindow(hwnd, SW_RESTORE);
                    SetForegroundWindow(hwnd);
                },
                WM_RBUTTONUP | WM_CONTEXTMENU => unsafe { app.tray_menu(hwnd) },
                _ => {}
            }
            0
        }
        WM_SIZE if wparam == SIZE_MINIMIZED as usize && app.tray_added => {
            unsafe { ShowWindow(hwnd, SW_HIDE) };
            0
        }
        WM_CLOSE => {
            unsafe { app.save_settings() };
            app.engine.stop();
            unsafe { DestroyWindow(hwnd) };
            0
        }
        WM_QUERYENDSESSION => 1,
        WM_ENDSESSION if wparam != 0 => {
            unsafe { app.save_settings() };
            app.engine.stop();
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

pub fn run() -> Result<(), String> {
    // SAFETY: this function owns the UI thread and all OS resources. App's box
    // stays alive until the window is destroyed and message dispatch has ended.
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let common = INITCOMMONCONTROLSEX {
            dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_BAR_CLASSES,
        };
        InitCommonControlsEx(&common);
        let instance = GetModuleHandleW(null());
        let name = wide("TapewaveNativeWindow");
        // Also lets the installer detect the running application before upgrading.
        let mutex = CreateMutexW(null(), 0, wide("Local\\TapewaveNativePlayer").as_ptr());
        if mutex.is_null() {
            return Err("Could not create the application lock.".into());
        }
        struct AppLock(HANDLE);
        impl Drop for AppLock {
            fn drop(&mut self) {
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
        let _lock = AppLock(mutex);
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let existing = FindWindowW(name.as_ptr(), null());
            if !existing.is_null() {
                ShowWindow(existing, SW_RESTORE);
                SetForegroundWindow(existing);
            }
            return Ok(());
        }
        let mut class: WNDCLASSW = zeroed();
        class.lpfnWndProc = Some(window_proc);
        class.hInstance = instance;
        class.hCursor = LoadCursorW(null_mut(), IDC_ARROW);
        class.hIcon = LoadIconW(instance, std::ptr::without_provenance(1));
        class.lpszClassName = name.as_ptr();
        if RegisterClassW(&class) == 0 {
            return Err("Could not register the Tapewave window.".into());
        }

        let mut app = Box::new(App::new());
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;
        let dpi = GetDpiForSystem().max(96);
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: (512 * dpi / 96) as i32,
            bottom: (748 * dpi / 96) as i32,
        };
        AdjustWindowRectExForDpi(&mut rect, style, 0, 0, dpi);
        let hwnd = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            name.as_ptr(),
            wide("Tapewave").as_ptr(),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            rect.right - rect.left,
            rect.bottom - rect.top,
            null_mut(),
            null_mut(),
            instance,
            (&mut *app as *mut App).cast::<c_void>(),
        );
        if hwnd.is_null() {
            return Err("Could not create the Tapewave window.".into());
        }
        let menu = CreateMenu();
        let file_menu = CreatePopupMenu();
        AppendMenuW(
            file_menu,
            MF_STRING,
            OPEN as usize,
            wide("&Open file...\tCtrl+O").as_ptr(),
        );
        AppendMenuW(file_menu, MF_SEPARATOR, 0, null());
        AppendMenuW(file_menu, MF_STRING, EXIT as usize, wide("E&xit").as_ptr());
        AppendMenuW(menu, MF_POPUP, file_menu as usize, wide("&File").as_ptr());
        let help_menu = CreatePopupMenu();
        AppendMenuW(
            help_menu,
            MF_STRING,
            ABOUT as usize,
            wide("&About Tapewave").as_ptr(),
        );
        AppendMenuW(menu, MF_POPUP, help_menu as usize, wide("&Help").as_ptr());
        SetMenu(hwnd, menu);
        // Include the native menu in the outer window dimensions.
        let mut bounds = RECT {
            left: 0,
            top: 0,
            right: (512 * dpi / 96) as i32,
            bottom: (748 * dpi / 96) as i32,
        };
        AdjustWindowRectExForDpi(&mut bounds, style, 1, 0, dpi);
        SetWindowPos(
            hwnd,
            null_mut(),
            0,
            0,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            SWP_NOMOVE | SWP_NOZORDER,
        );
        if let Some(path) = std::env::args_os().nth(1) {
            app.play_source(Source::File(path.into()));
        }
        ShowWindow(hwnd, SW_SHOW);
        let accelerators = [
            ACCEL {
                fVirt: FVIRTKEY | FCONTROL,
                key: b'O' as u16,
                cmd: OPEN,
            },
            ACCEL {
                fVirt: FVIRTKEY | FCONTROL,
                key: b'P' as u16,
                cmd: PLAY,
            },
            ACCEL {
                fVirt: FVIRTKEY | FCONTROL,
                key: b'S' as u16,
                cmd: STOP,
            },
        ];
        let table = CreateAcceleratorTableW(accelerators.as_ptr(), accelerators.len() as i32);
        let mut message: MSG = zeroed();
        loop {
            let result = GetMessageW(&mut message, null_mut(), 0, 0);
            if result <= 0 {
                if result == -1 {
                    DestroyWindow(hwnd);
                    if !table.is_null() {
                        DestroyAcceleratorTable(table);
                    }
                    return Err("The Windows message loop failed.".into());
                }
                break;
            }
            if TranslateAcceleratorW(hwnd, table, &message) == 0
                && IsDialogMessageW(hwnd, &message) == 0
            {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        if !table.is_null() {
            DestroyAcceleratorTable(table);
        }
    }
    Ok(())
}

pub fn show_startup_error(message: &str) {
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(message).as_ptr(),
            wide("Tapewave").as_ptr(),
            MB_OK | MB_ICONERROR,
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_http_sources() {
        assert!(valid_url("https://example.com/radio.mp3"));
        assert!(valid_url("HTTP://example.com/feed"));
        for url in [
            "",
            "https://",
            "file:///music.mp3",
            "ftp://example.com",
            "https://bad host",
        ] {
            assert!(!valid_url(url), "{url}");
        }
    }

    #[test]
    fn utf16_preserves_unicode_and_strips_embedded_nul() {
        assert_eq!(wide("音楽\0"), vec![0x97f3, 0x697d, 0]);
    }
}
