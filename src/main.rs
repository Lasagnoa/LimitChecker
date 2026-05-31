#![windows_subsystem = "windows"]

mod credentials;
mod poller;
mod settings;

use poller::{SharedUsageInfo, UsageInfo};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use windows_sys::Win32::Foundation::*;
use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::*;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WM_TRAYICON: u32 = WM_USER + 1;
const WM_POLL_DONE: u32 = WM_USER + 2;
const TIMER_POLL: usize = 1;
const TIMER_COUNTDOWN: usize = 2;
const TIMER_HOVER_CHECK: usize = 3;
const IDM_REFRESH: u32 = 1001;
const IDM_LOGIN_CLAUDE: u32 = 1002;
const IDM_LOGIN_CODEX: u32 = 1004;
const IDM_EXIT: u32 = 1003;
const IDM_INTERVAL_1: u32 = 1010;
const IDM_INTERVAL_5: u32 = 1011;
const IDM_INTERVAL_15: u32 = 1012;
const IDM_INTERVAL_60: u32 = 1013;
const IDM_OPEN_CLAUDE: u32 = 1020;
const IDM_OPEN_CODEX: u32 = 1021;
// デバッグログ ON/OFF
const IDM_LOG_TOGGLE: u32 = 1060;

static POPUP_VISIBLE: AtomicBool = AtomicBool::new(false);
static POPUP_PINNED: AtomicBool = AtomicBool::new(false);
pub static LOG_ENABLED: AtomicBool = AtomicBool::new(false);
static MAIN_HWND: AtomicUsize = AtomicUsize::new(0);
static LAST_TRAY_MOUSEMOVE_MS: AtomicU64 = AtomicU64::new(0);
static CLICK_CURSOR_X: AtomicUsize = AtomicUsize::new(0);
static CLICK_CURSOR_Y: AtomicUsize = AtomicUsize::new(0);

// --- Color scheme (BGR format) ---
const COL_BG: u32 = 0x001C1C1C;
const COL_CLAUDE: u32 = 0x005777D9; // #D97757 Claude orange (BGR)
const COL_CODEX: u32 = 0x00CC9966; // #6699CC Codex blue (BGR)
const COL_TRACK: u32 = 0x00444444;
const COL_WHITE: u32 = 0x00FFFFFF;

// ===== Layout constants (user-specified) =====

// Window
const POPUP_W: i32 = 320;
const WIN_RADIUS: i32 = 4;
const PAD: i32 = 4; // all 4 sides

// Segment bar
const SEG_COUNT: i32 = 10;
const SEG_W: i32 = 20;
const SEG_H: i32 = 14;
const SEG_GAP: i32 = 2;
const SEG_RADIUS: i32 = 3;

// Bar row layout
const BAR_ROW_H: i32 = 20;
const LABEL_W: i32 = 34; // label area, text centered
const LABEL_BAR_GAP: i32 = 4;
const BAR_TOTAL_W: i32 = 218; // 10*20 + 9*2
const BAR_PCT_GAP: i32 = 4;
const PCT_W: i32 = 40; // right-aligned

// Detail row (reset time)
const DETAIL_ROW_H: i32 = 10;
const DETAIL_X: i32 = PAD + LABEL_W + LABEL_BAR_GAP;
const TITLE_ROW_H: i32 = 15;
const TITLE_BAR_GAP: i32 = 2;

// Vertical gaps
const BAR_DETAIL_GAP: i32 = 2;
const BLOCK_GAP: i32 = 4;

// Font name
const FONT_NAME: &str = "BIZ UD\u{30B4}\u{30B7}\u{30C3}\u{30AF}"; // BIZ UDゴシック

struct AppState {
    shared_info: SharedUsageInfo,
    hwnd: HWND,
    popup_hwnd: HWND,
    poll_interval_ms: u32,
}

static mut APP: Option<AppState> = None;

fn app() -> &'static mut AppState {
    unsafe { APP.as_mut().unwrap() }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiLanguage {
    Japanese,
    English,
}

#[derive(Debug, Clone, Copy)]
enum TextId {
    RefreshNow,
    IntervalMenu,
    Interval1Min,
    Interval5Min,
    Interval15Min,
    Interval60Min,
    OpenClaude,
    OpenChatGpt,
    LoginClaude,
    LoginCodex,
    DebugLog,
    Exit,
    LoginRequired,
    Reset,
}

fn ui_language() -> UiLanguage {
    // LANGID lower 10 bits are the primary language ID. Japanese is 0x11.
    let lang_id = unsafe { GetUserDefaultUILanguage() };
    if (lang_id & 0x03ff) == 0x11 {
        UiLanguage::Japanese
    } else {
        UiLanguage::English
    }
}

fn tr(id: TextId) -> &'static str {
    match ui_language() {
        UiLanguage::Japanese => match id {
            TextId::RefreshNow => "今すぐ更新",
            TextId::IntervalMenu => "更新間隔",
            TextId::Interval1Min => "1分",
            TextId::Interval5Min => "5分",
            TextId::Interval15Min => "15分",
            TextId::Interval60Min => "60分",
            TextId::OpenClaude => "Claude.aiを開く",
            TextId::OpenChatGpt => "ChatGPTを開く",
            TextId::LoginClaude => "Claude再ログイン",
            TextId::LoginCodex => "Codex再ログイン",
            TextId::DebugLog => "ログ(デバッグ用)",
            TextId::Exit => "終了",
            TextId::LoginRequired => "ログインが必要です",
            TextId::Reset => "リセット",
        },
        UiLanguage::English => match id {
            TextId::RefreshNow => "Refresh now",
            TextId::IntervalMenu => "Update interval",
            TextId::Interval1Min => "1 min",
            TextId::Interval5Min => "5 min",
            TextId::Interval15Min => "15 min",
            TextId::Interval60Min => "60 min",
            TextId::OpenClaude => "Open Claude.ai",
            TextId::OpenChatGpt => "Open ChatGPT",
            TextId::LoginClaude => "Log in to Claude again",
            TextId::LoginCodex => "Log in to Codex again",
            TextId::DebugLog => "Log (debug)",
            TextId::Exit => "Exit",
            TextId::LoginRequired => "Login required",
            TextId::Reset => "Reset",
        },
    }
}

fn exe_dir() -> std::path::PathBuf {
    std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

fn find_icon_path() -> Option<String> {
    let exe_ico = exe_dir().join("icon.ico");
    if exe_ico.exists() {
        return Some(exe_ico.to_string_lossy().to_string());
    }
    None
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// 現在のアプリ設定をファイルに保存する
fn save_current_settings() {
    let s = settings::Settings {
        poll_interval_ms: app().poll_interval_ms,
        log_enabled: LOG_ENABLED.load(Ordering::SeqCst),
    };
    settings::save(&s);
}

fn main() {
    // 多重起動防止: 同じウィンドウクラスが既に存在する場合は終了
    unsafe {
        let class_name = to_wide("LimitCheckerClass");
        let existing = FindWindowW(class_name.as_ptr(), std::ptr::null());
        if !existing.is_null() {
            // 既に別のインスタンスが起動中のため終了
            return;
        }
    }

    // 設定ファイルのロード
    let s = settings::load();
    LOG_ENABLED.store(s.log_enabled, Ordering::SeqCst);

    let shared_info = poller::create_shared_info();

    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());

        let class_name = to_wide("LimitCheckerClass");
        let wc = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: LoadIconW(std::ptr::null_mut(), IDI_APPLICATION),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            0,
            class_name.as_ptr(),
            to_wide("Limit Checker").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1,
            1,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );

        let popup_class = to_wide("LimitCheckerPopupClass");
        let wc_popup = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(popup_wnd_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinstance,
            hIcon: std::ptr::null_mut(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: popup_class.as_ptr(),
        };
        RegisterClassW(&wc_popup);

        let popup_hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            popup_class.as_ptr(),
            to_wide("").as_ptr(),
            WS_POPUP,
            0,
            0,
            10,
            10,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );

        APP = Some(AppState {
            shared_info: shared_info.clone(),
            hwnd,
            popup_hwnd,
            poll_interval_ms: s.poll_interval_ms,
        });

        add_tray_icon(hwnd);
        SetTimer(hwnd, TIMER_POLL, s.poll_interval_ms, None);
        SetTimer(hwnd, TIMER_COUNTDOWN, 30_000, None);
        MAIN_HWND.store(hwnd as usize, Ordering::SeqCst);

        let shared_clone = shared_info.clone();
        std::thread::spawn(move || {
            poller::poll_once(&shared_clone);
            let h = MAIN_HWND.load(Ordering::SeqCst) as HWND;
            PostMessageW(h, WM_POLL_DONE, 0, 0);
        });

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        remove_tray_icon(hwnd);
    }
}

unsafe fn add_tray_icon(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid.uFlags = NIF_ICON | NIF_MESSAGE;
    nid.uCallbackMessage = WM_TRAYICON;

    let mut hicon: HICON = std::ptr::null_mut();
    if let Some(ref path) = find_icon_path() {
        let wide = to_wide(path);
        let h = LoadImageW(
            std::ptr::null_mut(),
            wide.as_ptr(),
            IMAGE_ICON,
            0,
            0,
            LR_LOADFROMFILE | LR_DEFAULTSIZE,
        );
        if !h.is_null() {
            hicon = h as HICON;
        }
    }
    if hicon.is_null() {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let h = LoadIconW(hinstance, 1 as *const u16);
        if !h.is_null() {
            hicon = h;
        } else {
            hicon = LoadIconW(std::ptr::null_mut(), IDI_APPLICATION);
        }
    }
    nid.hIcon = hicon;
    Shell_NotifyIconW(NIM_ADD, &nid);
}

unsafe fn remove_tray_icon(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    Shell_NotifyIconW(NIM_DELETE, &nid);
}

/// 1プロバイダー分のセクション高さ (タイトル + 5H行 + 7D行)
fn provider_section_h() -> i32 {
    TITLE_ROW_H
        + TITLE_BAR_GAP
        + (BAR_ROW_H + BAR_DETAIL_GAP + DETAIL_ROW_H)
        + BLOCK_GAP
        + (BAR_ROW_H + BAR_DETAIL_GAP + DETAIL_ROW_H)
}

/// 設定済みプロバイダーの数に応じてポップアップ高さを計算する
fn calc_popup_height(info: &UsageInfo) -> i32 {
    let sec_h = provider_section_h();
    let claude_h = if info.claude_configured { sec_h } else { 0 };
    let codex_h = if info.codex_configured { sec_h } else { 0 };
    // 両方表示する場合はセクション間に BLOCK_GAP を追加
    let gap = if info.claude_configured && info.codex_configured {
        BLOCK_GAP
    } else {
        0
    };
    // どちらも未設定なら最低限の高さ (メッセージ表示用)
    if !info.claude_configured && !info.codex_configured {
        return PAD + BAR_ROW_H + PAD;
    }
    PAD + claude_h + gap + codex_h + PAD
}

unsafe fn show_popup(_hwnd: HWND) {
    let popup_hwnd = app().popup_hwnd;

    let info = app().shared_info.lock().unwrap();
    let popup_h = calc_popup_height(&info);
    drop(info);

    let mut pt: POINT = std::mem::zeroed();
    GetCursorPos(&mut pt);

    CLICK_CURSOR_X.store(pt.x as usize, Ordering::SeqCst);
    CLICK_CURSOR_Y.store(pt.y as usize, Ordering::SeqCst);

    let mut rc: RECT = std::mem::zeroed();
    SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut rc as *mut _ as *mut _, 0);

    let x = (pt.x - POPUP_W / 2).max(rc.left).min(rc.right - POPUP_W);
    let y = if pt.y > (rc.bottom - rc.top) / 2 {
        pt.y - popup_h - 24
    } else {
        pt.y + 24
    };

    SetWindowPos(
        popup_hwnd,
        HWND_TOPMOST,
        x,
        y,
        POPUP_W,
        popup_h,
        SWP_NOACTIVATE,
    );

    let rgn = CreateRoundRectRgn(0, 0, POPUP_W, popup_h, WIN_RADIUS, WIN_RADIUS);
    SetWindowRgn(popup_hwnd, rgn, 1);

    ShowWindow(popup_hwnd, SW_SHOWNOACTIVATE);
    InvalidateRect(popup_hwnd, std::ptr::null(), 1);
    POPUP_VISIBLE.store(true, Ordering::SeqCst);

    let main_hwnd = app().hwnd;
    SetTimer(main_hwnd, TIMER_HOVER_CHECK, 100, None);
}

unsafe fn show_popup_click(hwnd: HWND) {
    POPUP_PINNED.store(true, Ordering::SeqCst);
    show_popup(hwnd);
}

unsafe fn hide_popup() {
    if !POPUP_VISIBLE.load(Ordering::SeqCst) {
        return;
    }
    let popup_hwnd = app().popup_hwnd;
    ShowWindow(popup_hwnd, SW_HIDE);
    POPUP_VISIBLE.store(false, Ordering::SeqCst);
    POPUP_PINNED.store(false, Ordering::SeqCst);
    let main_hwnd = app().hwnd;
    KillTimer(main_hwnd, TIMER_HOVER_CHECK);
}

unsafe fn is_cursor_on_popup() -> bool {
    let mut pt: POINT = std::mem::zeroed();
    GetCursorPos(&mut pt);
    let popup_hwnd = app().popup_hwnd;
    let mut popup_rc: RECT = std::mem::zeroed();
    GetWindowRect(popup_hwnd, &mut popup_rc);
    popup_rc.left -= 2;
    popup_rc.top -= 2;
    popup_rc.right += 2;
    popup_rc.bottom += 2;
    pt.x >= popup_rc.left
        && pt.x <= popup_rc.right
        && pt.y >= popup_rc.top
        && pt.y <= popup_rc.bottom
}

unsafe fn has_cursor_moved_from_click() -> bool {
    let mut pt: POINT = std::mem::zeroed();
    GetCursorPos(&mut pt);
    let cx = CLICK_CURSOR_X.load(Ordering::SeqCst) as i32;
    let cy = CLICK_CURSOR_Y.load(Ordering::SeqCst) as i32;
    let dx = (pt.x - cx).abs();
    let dy = (pt.y - cy).abs();
    dx > 10 || dy > 10
}

unsafe fn show_context_menu(hwnd: HWND) {
    let hmenu = CreatePopupMenu();
    let interval = app().poll_interval_ms;

    // 今すぐ更新
    AppendMenuW(
        hmenu,
        MF_STRING,
        IDM_REFRESH as usize,
        to_wide(tr(TextId::RefreshNow)).as_ptr(),
    );
    AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());

    // 更新間隔サブメニュー
    let hsub = CreatePopupMenu();
    let check = |ms: u32| -> u32 {
        if interval == ms {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        }
    };
    AppendMenuW(
        hsub,
        check(60_000),
        IDM_INTERVAL_1 as usize,
        to_wide(tr(TextId::Interval1Min)).as_ptr(),
    );
    AppendMenuW(
        hsub,
        check(5 * 60_000),
        IDM_INTERVAL_5 as usize,
        to_wide(tr(TextId::Interval5Min)).as_ptr(),
    );
    AppendMenuW(
        hsub,
        check(15 * 60_000),
        IDM_INTERVAL_15 as usize,
        to_wide(tr(TextId::Interval15Min)).as_ptr(),
    );
    AppendMenuW(
        hsub,
        check(60 * 60_000),
        IDM_INTERVAL_60 as usize,
        to_wide(tr(TextId::Interval60Min)).as_ptr(),
    );
    AppendMenuW(
        hmenu,
        MF_POPUP,
        hsub as usize,
        to_wide(tr(TextId::IntervalMenu)).as_ptr(),
    );

    AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(
        hmenu,
        MF_STRING,
        IDM_OPEN_CLAUDE as usize,
        to_wide(tr(TextId::OpenClaude)).as_ptr(),
    );
    AppendMenuW(
        hmenu,
        MF_STRING,
        IDM_OPEN_CODEX as usize,
        to_wide(tr(TextId::OpenChatGpt)).as_ptr(),
    );
    AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(
        hmenu,
        MF_STRING,
        IDM_LOGIN_CLAUDE as usize,
        to_wide(tr(TextId::LoginClaude)).as_ptr(),
    );
    AppendMenuW(
        hmenu,
        MF_STRING,
        IDM_LOGIN_CODEX as usize,
        to_wide(tr(TextId::LoginCodex)).as_ptr(),
    );
    AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
    // デバッグログ ON/OFF (チェックマークで状態を表示)
    let log_flag = if LOG_ENABLED.load(Ordering::SeqCst) {
        MF_STRING | MF_CHECKED
    } else {
        MF_STRING
    };
    AppendMenuW(
        hmenu,
        log_flag,
        IDM_LOG_TOGGLE as usize,
        to_wide(tr(TextId::DebugLog)).as_ptr(),
    );
    AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
    AppendMenuW(
        hmenu,
        MF_STRING,
        IDM_EXIT as usize,
        to_wide(tr(TextId::Exit)).as_ptr(),
    );

    let mut pt: POINT = std::mem::zeroed();
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    TrackPopupMenu(
        hmenu,
        TPM_BOTTOMALIGN | TPM_LEFTALIGN,
        pt.x,
        pt.y,
        0,
        hwnd,
        std::ptr::null(),
    );
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(hmenu);
}

unsafe fn do_refresh() {
    let shared = app().shared_info.clone();
    std::thread::spawn(move || {
        poller::poll_once(&shared);
        let h = MAIN_HWND.load(Ordering::SeqCst) as HWND;
        PostMessageW(h, WM_POLL_DONE, 0, 0);
    });
}

unsafe fn set_poll_interval(ms: u32) {
    app().poll_interval_ms = ms;
    let hwnd = app().hwnd;
    KillTimer(hwnd, TIMER_POLL);
    SetTimer(hwnd, TIMER_POLL, ms, None);
    save_current_settings();
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_TRAYICON => {
            let event = (lparam & 0xFFFF) as u32;
            match event {
                WM_LBUTTONUP => {
                    show_popup_click(hwnd);
                }
                WM_RBUTTONUP => {
                    hide_popup();
                    show_context_menu(hwnd);
                }
                WM_MOUSEMOVE => {
                    LAST_TRAY_MOUSEMOVE_MS.store(now_ms(), Ordering::SeqCst);
                    if !POPUP_VISIBLE.load(Ordering::SeqCst) {
                        POPUP_PINNED.store(false, Ordering::SeqCst);
                        show_popup(hwnd);
                    }
                }
                _ => {}
            }
            0
        }
        WM_COMMAND => {
            let cmd = (wparam & 0xFFFF) as u32;
            match cmd {
                IDM_REFRESH => do_refresh(),
                IDM_LOGIN_CLAUDE => {
                    std::thread::spawn(|| {
                        let _ = std::process::Command::new("claude")
                            .args(["auth", "login"])
                            .spawn();
                    });
                }
                IDM_LOGIN_CODEX => {
                    std::thread::spawn(|| {
                        let _ = std::process::Command::new("codex").args(["login"]).spawn();
                    });
                }
                IDM_EXIT => {
                    remove_tray_icon(hwnd);
                    PostQuitMessage(0);
                }
                IDM_INTERVAL_1 => set_poll_interval(60_000),
                IDM_INTERVAL_5 => set_poll_interval(5 * 60_000),
                IDM_INTERVAL_15 => set_poll_interval(15 * 60_000),
                IDM_INTERVAL_60 => set_poll_interval(60 * 60_000),
                IDM_OPEN_CLAUDE => {
                    let _ = std::process::Command::new("cmd")
                        .args(["/c", "start", "https://claude.ai"])
                        .spawn();
                }
                IDM_OPEN_CODEX => {
                    let _ = std::process::Command::new("cmd")
                        .args(["/c", "start", "https://chatgpt.com/"])
                        .spawn();
                }
                // デバッグログのON/OFFをトグル
                IDM_LOG_TOGGLE => {
                    let current = LOG_ENABLED.load(Ordering::SeqCst);
                    LOG_ENABLED.store(!current, Ordering::SeqCst);
                    save_current_settings();
                }
                _ => {}
            }
            0
        }
        WM_TIMER => {
            match wparam {
                TIMER_POLL => do_refresh(),
                TIMER_COUNTDOWN => {
                    if POPUP_VISIBLE.load(Ordering::SeqCst) {
                        InvalidateRect(app().popup_hwnd, std::ptr::null(), 1);
                    }
                }
                TIMER_HOVER_CHECK => {
                    if POPUP_VISIBLE.load(Ordering::SeqCst) {
                        if has_cursor_moved_from_click() && !is_cursor_on_popup() {
                            hide_popup();
                        }
                    }
                }
                _ => {}
            }
            0
        }
        WM_POLL_DONE => {
            if POPUP_VISIBLE.load(Ordering::SeqCst) {
                InvalidateRect(app().popup_hwnd, std::ptr::null(), 1);
            }
            0
        }
        WM_DESTROY => {
            remove_tray_icon(hwnd);
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ========================== Popup rendering ==========================

unsafe fn draw_rounded_segment(hdc: HDC, x: i32, y: i32, w: i32, h: i32, color: u32) {
    let brush = CreateSolidBrush(color);
    let null_pen = GetStockObject(NULL_PEN);
    let old_brush = SelectObject(hdc, brush as _);
    let old_pen = SelectObject(hdc, null_pen);
    RoundRect(hdc, x, y, x + w + 1, y + h + 1, SEG_RADIUS, SEG_RADIUS);
    SelectObject(hdc, old_pen);
    SelectObject(hdc, old_brush);
    DeleteObject(brush as _);
}

unsafe fn draw_segmented_bar(hdc: HDC, x: i32, y: i32, pct: f64, color: u32) {
    let fill_count_f = (pct / 100.0) * SEG_COUNT as f64;

    for i in 0..SEG_COUNT {
        let sx = x + i * (SEG_W + SEG_GAP);
        let fi = i as f64;

        if fi + 1.0 <= fill_count_f {
            draw_rounded_segment(hdc, sx, y, SEG_W, SEG_H, color);
        } else if fi < fill_count_f {
            draw_rounded_segment(hdc, sx, y, SEG_W, SEG_H, COL_TRACK);
            let frac = fill_count_f - fi;
            let fill_w = ((SEG_W as f64) * frac).round() as i32;
            if fill_w > 0 {
                let clip_rgn = CreateRoundRectRgn(
                    sx,
                    y,
                    sx + fill_w + 1,
                    y + SEG_H + 1,
                    SEG_RADIUS,
                    SEG_RADIUS,
                );
                SelectClipRgn(hdc, clip_rgn);
                draw_rounded_segment(hdc, sx, y, SEG_W, SEG_H, color);
                SelectClipRgn(hdc, std::ptr::null_mut());
                DeleteObject(clip_rgn as _);
            }
        } else {
            draw_rounded_segment(hdc, sx, y, SEG_W, SEG_H, COL_TRACK);
        }
    }
}

/// Draw bar row: "5h [■■■□□□□□□□] 15%"
unsafe fn draw_bar_row(hdc: HDC, y: i32, label: &str, pct: f64, color: u32, bold_font: HFONT) {
    let bar_y = y + (BAR_ROW_H - SEG_H) / 2;

    // Label — centered in LABEL_W area
    SelectObject(hdc, bold_font as _);
    SetTextColor(hdc, COL_WHITE);
    let wide_label = to_wide(label);
    let mut label_rc = RECT {
        left: PAD,
        top: y,
        right: PAD + LABEL_W,
        bottom: y + BAR_ROW_H,
    };
    DrawTextW(
        hdc,
        wide_label.as_ptr(),
        -1,
        &mut label_rc,
        DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
    );

    // Bar
    let bar_x = PAD + LABEL_W + LABEL_BAR_GAP;
    draw_segmented_bar(hdc, bar_x, bar_y, pct, color);

    // Percentage — right-aligned in PCT_W area
    let pct_x = PAD + LABEL_W + LABEL_BAR_GAP + BAR_TOTAL_W + BAR_PCT_GAP;
    SetTextColor(hdc, COL_WHITE);
    let pct_text = format!("{:.0}%", pct);
    let wide_pct = to_wide(&pct_text);
    let mut pct_rc = RECT {
        left: pct_x,
        top: y,
        right: pct_x + PCT_W,
        bottom: y + BAR_ROW_H,
    };
    DrawTextW(
        hdc,
        wide_pct.as_ptr(),
        -1,
        &mut pct_rc,
        DT_RIGHT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
    );
}

unsafe fn draw_title_row(hdc: HDC, y: i32, title: &str, color: u32, title_font: HFONT) {
    SelectObject(hdc, title_font as _);
    SetTextColor(hdc, color);
    let wide = to_wide(title);
    let mut rc = RECT {
        left: PAD,
        top: y,
        right: POPUP_W - PAD,
        bottom: y + TITLE_ROW_H,
    };
    DrawTextW(
        hdc,
        wide.as_ptr(),
        -1,
        &mut rc,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
    );
}

/// Draw detail row: "4h 16m ( Reset : 03/17 16:00 (UTC+9) )"
unsafe fn draw_detail_row(
    hdc: HDC,
    y: i32,
    reset: &chrono::DateTime<chrono::Utc>,
    detail_font: HFONT,
) {
    SelectObject(hdc, detail_font as _);
    SetTextColor(hdc, COL_WHITE);
    let countdown = poller::format_reset_countdown(reset);
    let reset_local = poller::format_reset_local(reset);
    let detail_text = format!("{} ( {} : {} )", countdown, tr(TextId::Reset), reset_local);
    let wide = to_wide(&detail_text);
    let mut rc = RECT {
        left: DETAIL_X,
        top: y,
        right: POPUP_W - PAD,
        bottom: y + DETAIL_ROW_H,
    };
    DrawTextW(
        hdc,
        wide.as_ptr(),
        -1,
        &mut rc,
        DT_LEFT | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
    );
}

unsafe extern "system" fn popup_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);

            let mut rc: RECT = std::mem::zeroed();
            GetClientRect(hwnd, &mut rc);

            // Background
            let bg_brush = CreateSolidBrush(COL_BG);
            FillRect(hdc, &rc, bg_brush);
            DeleteObject(bg_brush as _);

            SetBkMode(hdc, 1); // TRANSPARENT

            let font_name = to_wide(FONT_NAME);

            // Bold 20px — for labels and percentages
            let bold_font =
                CreateFontW(20, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 5, 0, font_name.as_ptr());
            // Regular 10px — for detail rows
            let detail_font =
                CreateFontW(10, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0, font_name.as_ptr());
            // Bold 15px - for provider titles
            let title_font =
                CreateFontW(15, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 5, 0, font_name.as_ptr());
            let old_font = SelectObject(hdc, bold_font as _);

            let info = app().shared_info.lock().unwrap();
            let info_clone = info.clone();
            drop(info);

            let mut cy = PAD;

            // Claude セクション (トークンが見つかった場合のみ表示)
            if info_clone.claude_configured {
                draw_title_row(hdc, cy, "Claude", COL_CLAUDE, title_font);
                cy += TITLE_ROW_H + TITLE_BAR_GAP;

                draw_bar_row(
                    hdc,
                    cy,
                    "5H",
                    info_clone.claude.session_pct,
                    COL_CLAUDE,
                    bold_font,
                );
                cy += BAR_ROW_H + BAR_DETAIL_GAP;
                if let Some(ref reset) = info_clone.claude.session_resets_at {
                    draw_detail_row(hdc, cy, reset, detail_font);
                }
                cy += DETAIL_ROW_H + BLOCK_GAP;

                draw_bar_row(
                    hdc,
                    cy,
                    "7D",
                    info_clone.claude.weekly_pct,
                    COL_CLAUDE,
                    bold_font,
                );
                cy += BAR_ROW_H + BAR_DETAIL_GAP;
                if let Some(ref reset) = info_clone.claude.weekly_resets_at {
                    draw_detail_row(hdc, cy, reset, detail_font);
                }
                cy += DETAIL_ROW_H;

                // Codex も表示する場合はセクション間ギャップを追加
                if info_clone.codex_configured {
                    cy += BLOCK_GAP;
                }
            }

            // Codex セクション (トークンが見つかった場合のみ表示)
            if info_clone.codex_configured {
                draw_title_row(hdc, cy, "Codex", COL_CODEX, title_font);
                cy += TITLE_ROW_H + TITLE_BAR_GAP;

                draw_bar_row(
                    hdc,
                    cy,
                    "5H",
                    info_clone.codex.session_pct,
                    COL_CODEX,
                    bold_font,
                );
                cy += BAR_ROW_H + BAR_DETAIL_GAP;
                if let Some(ref reset) = info_clone.codex.session_resets_at {
                    draw_detail_row(hdc, cy, reset, detail_font);
                }
                cy += DETAIL_ROW_H + BLOCK_GAP;

                draw_bar_row(
                    hdc,
                    cy,
                    "7D",
                    info_clone.codex.weekly_pct,
                    COL_CODEX,
                    bold_font,
                );
                cy += BAR_ROW_H + BAR_DETAIL_GAP;
                if let Some(ref reset) = info_clone.codex.weekly_resets_at {
                    draw_detail_row(hdc, cy, reset, detail_font);
                }
                let _ = cy;
            }

            // 両方未設定の場合はメッセージを表示
            if !info_clone.claude_configured && !info_clone.codex_configured {
                SelectObject(hdc, bold_font as _);
                SetTextColor(hdc, COL_WHITE);
                let msg = to_wide(tr(TextId::LoginRequired));
                let mut msg_rc = RECT {
                    left: PAD,
                    top: PAD,
                    right: POPUP_W - PAD,
                    bottom: PAD + BAR_ROW_H,
                };
                DrawTextW(
                    hdc,
                    msg.as_ptr(),
                    -1,
                    &mut msg_rc,
                    DT_CENTER | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
                );
            }

            // エラーはポップアップに表示せずログファイルにのみ出力する

            SelectObject(hdc, old_font);
            DeleteObject(bold_font as _);
            DeleteObject(detail_font as _);
            DeleteObject(title_font as _);
            EndPaint(hwnd, &ps);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
