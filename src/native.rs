//! Windows integration only. The themed UI is a native egui/wgpu surface,
//! not HTML. Tray messages run on their own thread; app state stays on the UI thread.
use crate::theme::Palette;
use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::{
    mem::{size_of, zeroed},
    ptr::{null, null_mut},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
};
use windows_sys::Win32::System::SystemServices::{
    APPCOMMAND_MEDIA_NEXTTRACK, APPCOMMAND_MEDIA_PLAY_PAUSE, APPCOMMAND_MEDIA_PREVIOUSTRACK,
    APPCOMMAND_MEDIA_STOP,
};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    System::{LibraryLoader::GetModuleHandleW, Threading::CreateMutexW},
    UI::{Accessibility::*, Shell::*, WindowsAndMessaging::*},
};

const TRAY_MESSAGE: u32 = WM_APP + 1;
const SHUTDOWN: u32 = WM_APP + 2;

pub fn wide(value: &str) -> Vec<u16> {
    value
        .replace('\0', "")
        .encode_utf16()
        .chain(Some(0))
        .collect()
}

#[derive(Clone, Copy)]
pub enum Event {
    Restore,
    Toggle,
    Stop,
    Next,
    Previous,
    Exit,
}

struct Bridge {
    tx: Sender<Event>,
    ctx: egui::Context,
}
impl Bridge {
    fn send(&self, event: Event) {
        let _ = self.tx.send(event);
        self.ctx.request_repaint();
    }
}

pub struct Integration {
    pub events: Receiver<Event>,
    pub tray_ready: Arc<AtomicBool>,
    thread_id: u32,
}

impl Integration {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = mpsc::channel();
        let ready = Arc::new(AtomicBool::new(false));
        let icon = unsafe { LoadIconW(GetModuleHandleW(null()), std::ptr::without_provenance(1)) };
        if let Some(window) = cc.winit_window()
            && let Ok(handle) = window.window_handle()
            && let RawWindowHandle::Win32(handle) = handle.as_raw()
        {
            let hwnd = handle.hwnd.get() as HWND;
            unsafe {
                SendMessageW(hwnd, WM_SETICON, 0, icon as isize);
                SendMessageW(hwnd, WM_SETICON, 1, icon as isize);
                let bridge = Box::into_raw(Box::new(Bridge {
                    tx: tx.clone(),
                    ctx: cc.egui_ctx.clone(),
                }));
                if SetWindowSubclass(hwnd, Some(media_proc), 1, bridge as usize) == 0 {
                    drop(Box::from_raw(bridge));
                }
            }
        }
        let bridge = Bridge {
            tx,
            ctx: cc.egui_ctx.clone(),
        };
        let state = Arc::clone(&ready);
        let icon = icon as usize;
        let (thread_tx, thread_rx) = mpsc::channel();
        std::thread::spawn(move || unsafe {
            let _ = thread_tx.send(windows_sys::Win32::System::Threading::GetCurrentThreadId());
            let instance = GetModuleHandleW(null());
            let class_name = wide("TapewaveTrayWindow");
            let mut class: WNDCLASSW = zeroed();
            class.lpfnWndProc = Some(tray_proc);
            class.hInstance = instance;
            class.lpszClassName = class_name.as_ptr();
            if RegisterClassW(&class) == 0 {
                return;
            }
            let ptr = Box::into_raw(Box::new(bridge));
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class_name.as_ptr(),
                wide("Tapewave controls").as_ptr(),
                0,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                instance,
                ptr.cast(),
            );
            if hwnd.is_null() {
                drop(Box::from_raw(ptr));
                return;
            }
            let mut data = tray_data(hwnd);
            data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
            data.uCallbackMessage = TRAY_MESSAGE;
            data.hIcon = icon as HICON;
            let tip = wide("Tapewave - right-click for playback controls");
            data.szTip[..tip.len()].copy_from_slice(&tip);
            state.store(Shell_NotifyIconW(NIM_ADD, &data) != 0, Ordering::Relaxed);
            let mut message: MSG = zeroed();
            while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                if message.message == SHUTDOWN {
                    break;
                }
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            state.store(false, Ordering::Relaxed);
            Shell_NotifyIconW(NIM_DELETE, &tray_data(hwnd));
            DestroyWindow(hwnd);
            // The pointer is freed here, not inside a nested window callback.
            drop(Box::from_raw(ptr));
        });
        Self {
            events: rx,
            tray_ready: ready,
            thread_id: thread_rx.recv().unwrap_or(0),
        }
    }
}

impl Drop for Integration {
    fn drop(&mut self) {
        if self.thread_id != 0 {
            unsafe {
                PostThreadMessageW(self.thread_id, SHUTDOWN, 0, 0);
            }
        }
    }
}

unsafe fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut data: NOTIFYICONDATAW = unsafe { zeroed() };
    data.cbSize = size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = hwnd;
    data.uID = 1;
    data
}

unsafe extern "system" fn tray_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if message == WM_NCCREATE {
        let create = unsafe { &*(lparam as *const CREATESTRUCTW) };
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
        }
        return 1;
    }
    let ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Bridge;
    if message == TRAY_MESSAGE && !ptr.is_null() {
        let bridge = unsafe { &*ptr };
        match lparam as u32 {
            WM_LBUTTONUP => bridge.send(Event::Restore),
            WM_RBUTTONUP | WM_CONTEXTMENU => unsafe {
                let menu = CreatePopupMenu();
                for (id, label) in [
                    (1, "Show Tapewave"),
                    (2, "Play / Pause"),
                    (3, "Stop"),
                    (4, "Next"),
                    (5, "Previous"),
                    (6, "Exit"),
                ] {
                    AppendMenuW(menu, MF_STRING, id, wide(label).as_ptr());
                }
                let mut point: POINT = zeroed();
                GetCursorPos(&mut point);
                SetForegroundWindow(hwnd);
                let choice = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    point.x,
                    point.y,
                    0,
                    hwnd,
                    null(),
                );
                DestroyMenu(menu);
                let event = match choice {
                    1 => Some(Event::Restore),
                    2 => Some(Event::Toggle),
                    3 => Some(Event::Stop),
                    4 => Some(Event::Next),
                    5 => Some(Event::Previous),
                    6 => Some(Event::Exit),
                    _ => None,
                };
                if let Some(event) = event {
                    bridge.send(event);
                }
                PostMessageW(hwnd, WM_NULL, 0, 0);
            },
            _ => {}
        }
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
}

unsafe extern "system" fn media_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    data: usize,
) -> LRESULT {
    if message == WM_NCDESTROY {
        unsafe {
            RemoveWindowSubclass(hwnd, Some(media_proc), id);
            drop(Box::from_raw(data as *mut Bridge));
        }
    } else if message == WM_APPCOMMAND {
        let event = match ((lparam >> 16) & 0x7ff) as u32 {
            APPCOMMAND_MEDIA_PLAY_PAUSE => Some(Event::Toggle),
            APPCOMMAND_MEDIA_STOP => Some(Event::Stop),
            APPCOMMAND_MEDIA_NEXTTRACK => Some(Event::Next),
            APPCOMMAND_MEDIA_PREVIOUSTRACK => Some(Event::Previous),
            _ => None,
        };
        if let Some(event) = event {
            unsafe {
                (&*(data as *const Bridge)).send(event);
            }
            return 1;
        }
    }
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

pub fn high_contrast() -> Option<Palette> {
    unsafe {
        let mut hc: HIGHCONTRASTW = zeroed();
        hc.cbSize = size_of::<HIGHCONTRASTW>() as u32;
        if SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            hc.cbSize,
            (&mut hc as *mut HIGHCONTRASTW).cast(),
            0,
        ) == 0
            || hc.dwFlags & HCF_HIGHCONTRASTON == 0
        {
            return None;
        }
        fn rgb(value: u32) -> [u8; 3] {
            [value as u8, (value >> 8) as u8, (value >> 16) as u8]
        }
        Some(Palette {
            background: rgb(GetSysColor(COLOR_WINDOW)),
            panel: rgb(GetSysColor(COLOR_BTNFACE)),
            text: rgb(GetSysColor(COLOR_WINDOWTEXT)),
            secondary: rgb(GetSysColor(COLOR_WINDOWTEXT)),
            accent: rgb(GetSysColor(COLOR_HIGHLIGHT)),
            border: rgb(GetSysColor(COLOR_WINDOWTEXT)),
            selection: rgb(GetSysColor(COLOR_WINDOW)),
            warning: rgb(GetSysColor(COLOR_WINDOWTEXT)),
        })
    }
}

pub fn run() -> Result<(), String> {
    struct Lock(HANDLE);
    impl Drop for Lock {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let smoke = cfg!(debug_assertions)
        && std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--ui-smoke-test"));
    let app_name = if smoke {
        "Tapewave UI test"
    } else {
        "Tapewave"
    };
    let mutex_name = if smoke {
        "Local\\TapewaveNativeUiTest"
    } else {
        "Local\\TapewaveNativePlayer"
    };
    let mutex = unsafe { CreateMutexW(null(), 0, wide(mutex_name).as_ptr()) };
    if mutex.is_null() {
        return Err("Could not create the application lock".into());
    }
    let existing = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let _lock = Lock(mutex);
    if existing {
        unsafe {
            let hwnd = FindWindowW(null(), wide(app_name).as_ptr());
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
            }
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([880.0, 640.0])
            .with_min_inner_size([620.0, 480.0])
            .with_drag_and_drop(true),
        persist_window: !smoke,
        persistence_path: if smoke {
            std::env::args_os()
                .nth(2)
                .map(|p| std::path::PathBuf::from(p).join("test-window.ron"))
        } else {
            std::env::var_os("LOCALAPPDATA")
                .map(|p| std::path::PathBuf::from(p).join("Tapewave/window.ron"))
        },
        ..Default::default()
    };
    eframe::run_native(
        app_name,
        options,
        Box::new(|cc| Ok(Box::new(crate::desktop::Tapewave::new(cc)))),
    )
    .map_err(|error| error.to_string())
}

pub fn show_startup_error(message: &str) {
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(message).as_ptr(),
            wide("Tapewave").as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}
