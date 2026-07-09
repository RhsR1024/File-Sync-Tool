//! Windows-native bootstrap UI: confirmation/error dialogs and a Win32
//! progress window with MessageBox fallback. WebView2/Tauri are unavailable
//! here, so everything is raw Win32.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

pub const DIALOG_TITLE: &str = "File Sync Tool";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Preparing,
    Downloading,
    Verifying,
    Installing,
    Restarting,
}

#[derive(Default)]
pub struct ProgressState {
    phase: AtomicU8,
    downloaded: AtomicU64,
    total: AtomicU64,
    cancelled: AtomicBool,
    done: AtomicBool,
}

impl ProgressState {
    pub fn set_phase(&self, phase: Phase) {
        self.phase.store(phase as u8, Ordering::SeqCst);
    }

    pub fn phase(&self) -> Phase {
        match self.phase.load(Ordering::SeqCst) {
            0 => Phase::Preparing,
            1 => Phase::Downloading,
            2 => Phase::Verifying,
            3 => Phase::Installing,
            _ => Phase::Restarting,
        }
    }

    pub fn set_progress(&self, downloaded: u64, total: Option<u64>) {
        self.downloaded.store(downloaded, Ordering::SeqCst);
        self.total.store(total.unwrap_or(0), Ordering::SeqCst);
    }

    pub fn progress(&self) -> (u64, u64) {
        (
            self.downloaded.load(Ordering::SeqCst),
            self.total.load(Ordering::SeqCst),
        )
    }

    pub fn request_cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    pub fn mark_done(&self) {
        self.done.store(true, Ordering::SeqCst);
    }

    pub fn is_done(&self) -> bool {
        self.done.load(Ordering::SeqCst)
    }
}

pub fn format_downloading_text(downloaded: u64, total: Option<u64>) -> String {
    fn mb(bytes: u64) -> f64 {
        bytes as f64 / (1024.0 * 1024.0)
    }

    match total {
        Some(total) if total > 0 => {
            let percent = (downloaded as f64 / total as f64 * 100.0).min(100.0);
            format!(
                "正在下载 WebView2 Runtime / Downloading WebView2 Runtime... {percent:.0}% ({:.1} MB / {:.1} MB)",
                mb(downloaded),
                mb(total)
            )
        }
        _ => format!(
            "正在下载 WebView2 Runtime / Downloading WebView2 Runtime... downloaded {:.1} MB",
            mb(downloaded)
        ),
    }
}

pub fn phase_text(state: &ProgressState) -> String {
    let (downloaded, total) = state.progress();
    match state.phase() {
        Phase::Preparing => {
            "正在连接内部更新服务器 / Connecting to the internal update server...".into()
        }
        Phase::Downloading => format_downloading_text(downloaded, (total > 0).then_some(total)),
        Phase::Verifying => "正在校验安装包完整性 / Verifying installer integrity...".into(),
        Phase::Installing => {
            "正在静默安装 WebView2 Runtime，请勿关闭 / Installing WebView2 Runtime silently..."
                .into()
        }
        Phase::Restarting => {
            "安装完成，正在重启 File Sync Tool / Restarting File Sync Tool...".into()
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
pub fn confirm_install() -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONQUESTION, MB_SYSTEMMODAL, MB_YESNO,
    };

    let text = wide(
        "File Sync Tool 需要 Microsoft Edge WebView2 Runtime 才能启动。\n\
         本机未检测到该组件。\n\n\
         是否现在从内部更新服务器下载并安装？\n\n\
         File Sync Tool requires Microsoft Edge WebView2 Runtime to start.\n\
         The component was not detected on this computer.\n\
         Install it now from the internal update server?",
    );
    let title = wide(DIALOG_TITLE);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_YESNO | MB_ICONQUESTION | MB_SYSTEMMODAL,
        ) == IDYES
    }
}

#[cfg(target_os = "windows")]
pub fn show_error(message: &str) {
    message_box(
        message,
        windows::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
    );
}

#[cfg(target_os = "windows")]
pub fn show_info(message: &str) {
    message_box(
        message,
        windows::Win32::UI::WindowsAndMessaging::MB_ICONINFORMATION,
    );
}

#[cfg(target_os = "windows")]
fn message_box(message: &str, icon: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_OK, MB_SYSTEMMODAL};

    let text = wide(message);
    let title = wide(DIALOG_TITLE);
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_SYSTEMMODAL | icon,
        );
    }
}

#[cfg(not(target_os = "windows"))]
pub fn confirm_install() -> bool {
    false
}

#[cfg(not(target_os = "windows"))]
pub fn show_error(_message: &str) {}

#[cfg(not(target_os = "windows"))]
pub fn show_info(_message: &str) {}

#[cfg(not(target_os = "windows"))]
pub fn try_create_progress_window(_state: Arc<ProgressState>) -> Option<()> {
    None
}

#[cfg(not(target_os = "windows"))]
pub fn run_message_loop() {}

#[cfg(target_os = "windows")]
mod progress_window {
    use super::*;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{GetStockObject, COLOR_WINDOW, DEFAULT_GUI_FONT, HBRUSH};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Controls::{
        InitCommonControlsEx, ICC_PROGRESS_CLASS, INITCOMMONCONTROLSEX, PBM_SETMARQUEE, PBM_SETPOS,
        PBM_SETRANGE32, PBS_MARQUEE, PROGRESS_CLASSW,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
    use windows::Win32::UI::WindowsAndMessaging::*;

    const CLASS_NAME: PCWSTR = w!("fst-wv2-bootstrap");
    const ID_CANCEL: i32 = 100;
    const ID_TEXT: i32 = 101;
    const ID_BAR: i32 = 102;
    const TIMER_ID: usize = 1;
    const BAR_RANGE: i32 = 1000;

    pub fn try_create(state: Arc<ProgressState>) -> Option<()> {
        unsafe {
            let icc = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_PROGRESS_CLASS,
            };
            let _ = InitCommonControlsEx(&icc);
            let hinstance = GetModuleHandleW(None).ok()?;

            let wc = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: hinstance.into(),
                lpszClassName: CLASS_NAME,
                hCursor: LoadCursorW(None, IDC_ARROW).ok()?,
                hbrBackground: HBRUSH(((COLOR_WINDOW.0 + 1) as isize) as *mut _),
                ..Default::default()
            };
            if RegisterClassW(&wc) == 0 {
                return None;
            }

            let width = 460;
            let height = 170;
            let x = (GetSystemMetrics(SM_CXSCREEN) - width) / 2;
            let y = (GetSystemMetrics(SM_CYSCREEN) - height) / 2;
            let state_ptr = Arc::into_raw(state) as *const core::ffi::c_void;
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                CLASS_NAME,
                w!("File Sync Tool"),
                WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_VISIBLE,
                x,
                y,
                width,
                height,
                None,
                None,
                hinstance,
                Some(state_ptr),
            );
            match hwnd {
                Ok(hwnd) if !hwnd.is_invalid() => Some(()),
                _ => {
                    drop(Arc::from_raw(state_ptr as *const ProgressState));
                    None
                }
            }
        }
    }

    pub fn run_message_loop() {
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    unsafe fn state_of(hwnd: HWND) -> Option<&'static ProgressState> {
        let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const ProgressState;
        (!ptr.is_null()).then(|| &*ptr)
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_CREATE => {
                let create = &*(lparam.0 as *const CREATESTRUCTW);
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, create.lpCreateParams as isize);
                let hinstance = create.hInstance;
                let font = GetStockObject(DEFAULT_GUI_FONT);

                let text = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("STATIC"),
                    w!("..."),
                    WS_CHILD | WS_VISIBLE,
                    16,
                    16,
                    412,
                    36,
                    hwnd,
                    HMENU(ID_TEXT as *mut core::ffi::c_void),
                    hinstance,
                    None,
                );
                let bar = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    PROGRESS_CLASSW,
                    PCWSTR::null(),
                    WS_CHILD | WS_VISIBLE,
                    16,
                    58,
                    412,
                    20,
                    hwnd,
                    HMENU(ID_BAR as *mut core::ffi::c_void),
                    hinstance,
                    None,
                );
                let cancel = CreateWindowExW(
                    WINDOW_EX_STYLE::default(),
                    w!("BUTTON"),
                    w!("取消 / Cancel"),
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP,
                    308,
                    92,
                    120,
                    28,
                    hwnd,
                    HMENU(ID_CANCEL as *mut core::ffi::c_void),
                    hinstance,
                    None,
                );
                for child in [&text, &bar, &cancel] {
                    if let Ok(child) = child {
                        SendMessageW(*child, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
                    }
                }
                if let Ok(bar) = bar {
                    SendMessageW(bar, PBM_SETRANGE32, WPARAM(0), LPARAM(BAR_RANGE as isize));
                }
                let _ = SetTimer(hwnd, TIMER_ID, 100, None);
                LRESULT(0)
            }
            WM_TIMER => {
                if let Some(state) = state_of(hwnd) {
                    refresh(hwnd, state);
                    if state.is_done() {
                        let _ = KillTimer(hwnd, TIMER_ID);
                        let _ = DestroyWindow(hwnd);
                    }
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                if (wparam.0 & 0xffff) as i32 == ID_CANCEL {
                    request_cancel(hwnd);
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                request_cancel(hwnd);
                LRESULT(0)
            }
            WM_NCDESTROY => {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *const ProgressState;
                if !ptr.is_null() {
                    drop(Arc::from_raw(ptr));
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    unsafe fn request_cancel(hwnd: HWND) {
        if let Some(state) = state_of(hwnd) {
            if state.phase() == Phase::Installing || state.phase() == Phase::Restarting {
                return;
            }
            state.request_cancel();
            if let Ok(cancel) = GetDlgItem(hwnd, ID_CANCEL) {
                let _ = EnableWindow(cancel, false);
            }
        }
    }

    unsafe fn refresh(hwnd: HWND, state: &ProgressState) {
        let text = wide(&phase_text(state));
        if let Ok(label) = GetDlgItem(hwnd, ID_TEXT) {
            let _ = SetWindowTextW(label, PCWSTR(text.as_ptr()));
        }

        let Ok(bar) = GetDlgItem(hwnd, ID_BAR) else {
            return;
        };
        match state.phase() {
            Phase::Downloading | Phase::Verifying | Phase::Preparing => {
                let (downloaded, total) = state.progress();
                if total > 0 {
                    let pos = (downloaded.saturating_mul(BAR_RANGE as u64) / total) as isize;
                    SendMessageW(bar, PBM_SETPOS, WPARAM(pos as usize), LPARAM(0));
                }
            }
            Phase::Installing | Phase::Restarting => {
                let style = GetWindowLongPtrW(bar, GWL_STYLE);
                if style & (PBS_MARQUEE as isize) == 0 {
                    SetWindowLongPtrW(bar, GWL_STYLE, style | PBS_MARQUEE as isize);
                    SendMessageW(bar, PBM_SETMARQUEE, WPARAM(1), LPARAM(0));
                    if let Ok(cancel) = GetDlgItem(hwnd, ID_CANCEL) {
                        let _ = EnableWindow(cancel, false);
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "windows")]
pub fn try_create_progress_window(state: Arc<ProgressState>) -> Option<()> {
    progress_window::try_create(state)
}

#[cfg(target_os = "windows")]
pub fn run_message_loop() {
    progress_window::run_message_loop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_text_shows_percent_when_total_known() {
        let text = format_downloading_text(50 * 1024 * 1024, Some(100 * 1024 * 1024));
        assert!(text.contains("50%"), "unexpected text: {text}");
        assert!(text.contains("50.0 MB"), "unexpected text: {text}");
        assert!(text.contains("100.0 MB"), "unexpected text: {text}");
    }

    #[test]
    fn download_text_degrades_without_total() {
        let text = format_downloading_text(3 * 1024 * 1024, None);
        assert!(text.contains("3.0 MB"), "unexpected text: {text}");
        assert!(!text.contains('%'), "unexpected text: {text}");
    }

    #[test]
    fn progress_state_round_trips_phase() {
        let state = ProgressState::default();
        assert_eq!(state.phase(), Phase::Preparing);
        state.set_phase(Phase::Installing);
        assert_eq!(state.phase(), Phase::Installing);
    }
}
