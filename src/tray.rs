//! Native system-tray icon + context menu (raw Win32, no external crates).
//!
//! This reproduces the original app's tray behaviour: an icon with a tooltip,
//! left-click to open settings, right-click for a menu (Enable/Disable toggle,
//! Open Settings, Reload Config, Quit). Runs on its own thread with its own
//! message loop.

#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use crate::engine::Engine;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

type HWND = *mut c_void;
type HICON = *mut c_void;
type HCURSOR = *mut c_void;
type HBRUSH = *mut c_void;
type HINSTANCE = *mut c_void;
type HMENU = *mut c_void;
type WPARAM = usize;
type LPARAM = isize;
type LRESULT = isize;

static TRAY_HWND: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());

const WM_DESTROY: u32 = 0x0002;
const WM_COMMAND: u32 = 0x0111;
const WM_NULL: u32 = 0x0000;
const WM_CONTEXTMENU: u32 = 0x007B;
const WM_RBUTTONUP: u32 = 0x0205;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_LBUTTONDBLCLK: u32 = 0x0203;
const WM_APP: u32 = 0x8000;
const CBTRAY: u32 = WM_APP + 1;

const WS_OVERLAPPED: u32 = 0x00000000;
const WS_EX_TOOLWINDOW: u32 = 0x00000080;
const CW_USEDEFAULT: i32 = i32::MIN;
const IDI_APPLICATION: usize = 32512;
const IDC_ARROW: usize = 32512;

const NIM_ADD: u32 = 0x0000_0000;
const NIM_MODIFY: u32 = 0x0000_0001;
const NIM_DELETE: u32 = 0x0000_0002;
const NIM_SETVERSION: u32 = 0x0000_0004;
const NIF_MESSAGE: u32 = 0x0000_0001;
const NIF_ICON: u32 = 0x0000_0002;
const NIF_TIP: u32 = 0x0000_0004;
const NIF_SHOWTIP: u32 = 0x0000_0080;
const NOTIFYICON_VERSION_4: u32 = 0x0000_0004;

const TPM_RIGHTBUTTON: u32 = 0x0000_0002;
const TPM_RETURNCMD: u32 = 0x0000_0100;
const TPM_NONOTIFY: u32 = 0x0000_0080;
const MF_STRING: u32 = 0x0000_0000;
const MF_SEPARATOR: u32 = 0x0000_0800;
const MF_CHECKED: u32 = 0x0000_0008;
const MF_UNCHECKED: u32 = 0x0000_0000;
const MF_GRAYED: u32 = 0x0000_0001;

const IDM_TOGGLE: u16 = 1001;
const IDM_SETTINGS: u16 = 1002;
const IDM_RELOAD: u16 = 1003;
const IDM_QUIT: u16 = 1004;
const IDM_VKEYBOARD: u16 = 1005;
const IDM_ELEVATE: u16 = 1006;
const IDM_DOCTOR: u16 = 1007;

#[repr(C)]
struct GUID {
    Data1: u32,
    Data2: u16,
    Data3: u16,
    Data4: [u8; 8],
}

#[repr(C)]
pub(crate) struct POINT {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

#[repr(C)]
pub(crate) struct MSG {
    pub(crate) hwnd: HWND,
    pub(crate) message: u32,
    pub(crate) wParam: WPARAM,
    pub(crate) lParam: LPARAM,
    pub(crate) time: u32,
    pub(crate) pt: POINT,
    pub(crate) lPrivate: u32,
}

#[repr(C)]
pub(crate) struct WNDCLASSEXW {
    pub(crate) cbSize: u32,
    pub(crate) style: u32,
    pub(crate) lpfnWndProc: usize,
    pub(crate) cbClsExtra: i32,
    pub(crate) cbWndExtra: i32,
    pub(crate) hInstance: HINSTANCE,
    pub(crate) hIcon: HICON,
    pub(crate) hCursor: HCURSOR,
    pub(crate) hbrBackground: HBRUSH,
    pub(crate) lpszMenuName: *const u16,
    pub(crate) lpszClassName: *const u16,
    pub(crate) hIconSm: HICON,
}

#[repr(C)]
struct NOTIFYICONDATAW {
    cbSize: u32,
    hWnd: HWND,
    uID: u32,
    uFlags: u32,
    uCallbackMessage: u32,
    hIcon: HICON,
    szTip: [u16; 128],
    dwState: u32,
    dwStateMask: u32,
    szInfo: [u16; 256],
    uTimeoutOrVersion: u32,
    szInfoTitle: [u16; 64],
    dwInfoFlags: u32,
    guidItem: GUID,
    hBalloonIcon: HICON,
}

#[link(name = "user32")]
extern "system" {
    pub(crate) fn RegisterClassExW(cls: *const WNDCLASSEXW) -> u16;
    pub(crate) fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: HWND,
        menu: HMENU,
        inst: HINSTANCE,
        param: *mut c_void,
    ) -> HWND;
    pub(crate) fn DefWindowProcW(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT;
    pub(crate) fn DestroyWindow(hwnd: HWND) -> i32;
    pub(crate) fn GetMessageW(msg: *mut MSG, hwnd: HWND, min: u32, max: u32) -> i32;
    pub(crate) fn TranslateMessage(msg: *const MSG) -> i32;
    pub(crate) fn DispatchMessageW(msg: *const MSG) -> LRESULT;
    pub(crate) fn PostQuitMessage(code: i32) -> ();
    pub(crate) fn PostMessageW(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> i32;
    fn CreatePopupMenu() -> HMENU;
    fn AppendMenuW(menu: HMENU, flags: u32, id: usize, text: *const u16) -> i32;
    fn SetForegroundWindow(hwnd: HWND) -> i32;
    fn TrackPopupMenuEx(menu: HMENU, flags: u32, x: i32, y: i32, hwnd: HWND, tpm: *mut c_void) -> u32;
    fn DestroyMenu(menu: HMENU) -> i32;
    fn LoadIconW(inst: HINSTANCE, name: usize) -> HICON;
    fn LoadCursorW(inst: HINSTANCE, name: usize) -> HCURSOR;
    fn GetCursorPos(pt: *mut POINT) -> i32;
    fn RegisterWindowMessageW(msg: *const u16) -> u32;
}

#[link(name = "shell32")]
extern "system" {
    fn Shell_NotifyIconW(msg: u32, data: *mut NOTIFYICONDATAW) -> i32;
}

// ---------------------------------------------------------------------------
// Status icon: a small gamepad drawn in code, coloured by the engine state.
// ---------------------------------------------------------------------------

#[repr(C)]
struct BITMAPINFOHEADER {
    biSize: u32,
    biWidth: i32,
    biHeight: i32,
    biPlanes: u16,
    biBitCount: u16,
    biCompression: u32,
    biSizeImage: u32,
    biXPelsPerMeter: i32,
    biYPelsPerMeter: i32,
    biClrUsed: u32,
    biClrImportant: u32,
}

#[repr(C)]
struct ICONINFO {
    fIcon: i32,
    xHotspot: u32,
    yHotspot: u32,
    hbmMask: *mut c_void,
    hbmColor: *mut c_void,
}

#[link(name = "gdi32")]
extern "system" {
    fn CreateDIBSection(
        hdc: *mut c_void,
        info: *const BITMAPINFOHEADER,
        usage: u32,
        bits: *mut *mut c_void,
        section: *mut c_void,
        offset: u32,
    ) -> *mut c_void;
    fn CreateBitmap(w: i32, h: i32, planes: u32, bpp: u32, bits: *const c_void) -> *mut c_void;
    fn DeleteObject(obj: *mut c_void) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn CreateIconIndirect(info: *const ICONINFO) -> HICON;
}

const ICON_SIZE: usize = 32;

/// Coverage (0..=1) of the gamepad silhouette at a point, in a 32×32 grid.
fn gamepad_coverage(x: f32, y: f32) -> bool {
    let rounded_rect = |x0: f32, y0: f32, x1: f32, y1: f32, r: f32| {
        let cx = x.clamp(x0 + r, x1 - r);
        let cy = y.clamp(y0 + r, y1 - r);
        (x - cx).powi(2) + (y - cy).powi(2) <= r * r
    };
    let circle = |cx: f32, cy: f32, r: f32| (x - cx).powi(2) + (y - cy).powi(2) <= r * r;
    let body = rounded_rect(3.0, 9.0, 29.0, 21.0, 5.5) || circle(8.5, 21.0, 5.0) || circle(23.5, 21.0, 5.0);
    // D-pad and two face buttons are cut out so the shape reads as a pad.
    let dpad = (x - 9.5).abs() <= 1.0 && (y - 15.5).abs() <= 3.5 || (y - 15.5).abs() <= 1.0 && (x - 9.5).abs() <= 3.5;
    let buttons = circle(21.0, 14.0, 1.7) || circle(24.5, 17.0, 1.7);
    body && !dpad && !buttons
}

/// Premultiplied BGRA pixels (top row first) for a gamepad in `rgb`, with an
/// optional status dot in the top-right corner.
fn icon_pixels(rgb: (u8, u8, u8), dot: Option<(u8, u8, u8)>) -> Vec<u32> {
    const SS: usize = 4; // 4×4 supersampling for smooth edges
    let mut px = vec![0u32; ICON_SIZE * ICON_SIZE];
    for row in 0..ICON_SIZE {
        for col in 0..ICON_SIZE {
            let mut hits = 0usize;
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = col as f32 + (sx as f32 + 0.5) / SS as f32;
                    let y = row as f32 + (sy as f32 + 0.5) / SS as f32;
                    hits += gamepad_coverage(x, y) as usize;
                }
            }
            let mut a = (hits * 255 / (SS * SS)) as u32;
            let (mut r, mut g, mut b) = (rgb.0 as u32, rgb.1 as u32, rgb.2 as u32);
            if let Some(d) = dot {
                let (dx, dy) = (col as f32 + 0.5 - 26.0, row as f32 + 0.5 - 6.0);
                let dist = (dx * dx + dy * dy).sqrt();
                if dist <= 5.5 {
                    // A dark ring keeps the dot readable on light taskbars.
                    let (cr, cg, cb) = if dist <= 4.0 { (d.0, d.1, d.2) } else { (20, 20, 24) };
                    r = cr as u32;
                    g = cg as u32;
                    b = cb as u32;
                    a = 255;
                }
            }
            px[row * ICON_SIZE + col] = (a << 24) | ((r * a / 255) << 16) | ((g * a / 255) << 8) | (b * a / 255);
        }
    }
    px
}

fn create_icon(pixels: &[u32]) -> HICON {
    unsafe {
        let header = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: ICON_SIZE as i32,
            biHeight: -(ICON_SIZE as i32), // top-down rows
            biPlanes: 1,
            biBitCount: 32,
            biCompression: 0,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let color = CreateDIBSection(std::ptr::null_mut(), &header, 0, &mut bits, std::ptr::null_mut(), 0);
        if color.is_null() || bits.is_null() {
            return LoadIconW(std::ptr::null_mut(), IDI_APPLICATION);
        }
        std::ptr::copy_nonoverlapping(pixels.as_ptr(), bits as *mut u32, pixels.len());
        let mask = CreateBitmap(ICON_SIZE as i32, ICON_SIZE as i32, 1, 1, std::ptr::null());
        let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info);
        DeleteObject(color);
        DeleteObject(mask);
        if icon.is_null() {
            LoadIconW(std::ptr::null_mut(), IDI_APPLICATION)
        } else {
            icon
        }
    }
}

/// The tray icon for a runtime mode (`None` = not known yet). Icons are made
/// once and reused for the life of the process.
fn status_icon(mode: Option<crate::runtime_status::RuntimeMode>) -> HICON {
    use crate::runtime_status::RuntimeMode as M;
    static ICONS: [AtomicPtr<c_void>; 5] = [
        AtomicPtr::new(std::ptr::null_mut()),
        AtomicPtr::new(std::ptr::null_mut()),
        AtomicPtr::new(std::ptr::null_mut()),
        AtomicPtr::new(std::ptr::null_mut()),
        AtomicPtr::new(std::ptr::null_mut()),
    ];
    let (slot, body, dot) = match mode {
        Some(M::Active) => (0, (255, 170, 0), Some((46, 204, 113))),
        Some(M::PausedFullscreen) => (1, (255, 170, 0), Some((52, 152, 219))),
        Some(M::Disabled) => (2, (150, 155, 165), Some((231, 76, 60))),
        Some(M::Disconnected) => (3, (150, 155, 165), None),
        None => (4, (255, 170, 0), None),
    };
    let cached = ICONS[slot].load(Ordering::Relaxed);
    if !cached.is_null() {
        return cached;
    }
    let icon = create_icon(&icon_pixels(body, dot));
    ICONS[slot].store(icon, Ordering::Relaxed);
    icon
}

/// Tooltip text for the tray icon (Windows shows at most 127 characters).
fn status_tip(status: &crate::runtime_status::RuntimeStatus) -> String {
    let mut tip = format!("Reverse Controller — {}", status.mode.label());
    if status.connected && !status.controller_name.is_empty() {
        tip.push_str(&format!("\n{}", status.controller_name));
    }
    if status.connected {
        tip.push_str(&format!("\nProfile: {}", status.active_profile));
        if status.battery_wired {
            tip.push_str(" · Wired");
        } else if let Some(level) = status.battery_level {
            let battery = match level {
                0 => "Empty",
                1 => "Low",
                2 => "Medium",
                _ => "Full",
            };
            tip.push_str(&format!(" · Battery: {battery}"));
        }
    }
    tip.chars().take(127).collect()
}

/// Last tooltip shown, so the icon is only touched when something changes.
static LAST_TIP: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Show the engine's state on the tray icon. Cheap to call often: it only
/// talks to the shell when the tooltip or icon would change.
pub fn refresh_status(status: &crate::runtime_status::RuntimeStatus) {
    let hwnd = TRAY_HWND.load(Ordering::Relaxed);
    if hwnd.is_null() {
        return;
    }
    let tip = status_tip(status);
    let key = format!("{:?}|{tip}", status.mode);
    if let Ok(mut last) = LAST_TIP.lock() {
        if last.as_deref() == Some(key.as_str()) {
            return;
        }
        *last = Some(key);
    }
    unsafe {
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.hIcon = status_icon(Some(status.mode));
        let wide = to_wide(&tip);
        let n = wide.len().min(127);
        nid.szTip[..n].copy_from_slice(&wide[..n]);
        nid.szTip[n] = 0;
        nid.uFlags = NIF_ICON | NIF_TIP | NIF_SHOWTIP;
        Shell_NotifyIconW(NIM_MODIFY, &mut nid);
    }
}

/// Forget the shown state so the next refresh repaints the icon (used after
/// Explorer restarts).
fn invalidate_status() {
    if let Ok(mut last) = LAST_TIP.lock() {
        *last = None;
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
    fn GetLastError() -> u32;
}

struct TrayCtx {
    engine: Arc<Mutex<Engine>>,
    running: Arc<AtomicBool>,
    config_path: PathBuf,
    open_settings: Box<dyn Fn() + Send + Sync>,
    vk: Option<Arc<AtomicBool>>,
}

// Tray runs on its own dedicated thread; store the context for the WndProc.
thread_local! {
    static TRAY_CTX: std::cell::Cell<*mut TrayCtx> = const { std::cell::Cell::new(std::ptr::null_mut()) };
}

static WM_TASKBARCREATED: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let taskbar_created = WM_TASKBARCREATED.load(Ordering::Relaxed);
    if taskbar_created != 0 && msg == taskbar_created {
        // Explorer restarted: re-register the notification icon
        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON;
        nid.uCallbackMessage = CBTRAY;
        nid.hIcon = status_icon(None);
        Shell_NotifyIconW(NIM_ADD, &mut nid);
        nid.uTimeoutOrVersion = NOTIFYICON_VERSION_4;
        set_tip(hwnd, &mut nid, "Reverse Controller");
        invalidate_status();
        return 0;
    }

    match msg {
        CBTRAY => {
            // Tray callback: low word of lParam is the mouse event.
            let ev = (l & 0xFFFF) as u32;
            if ev == WM_RBUTTONUP || ev == WM_CONTEXTMENU {
                show_menu(hwnd);
            } else if ev == WM_LBUTTONUP || ev == WM_LBUTTONDBLCLK {
                with_ctx(|ctx| (ctx.open_settings)());
            }
            0
        }
        WM_COMMAND => {
            let id = (w & 0xFFFF) as u16;
            handle_cmd(id);
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, w, l),
    }
}

fn with_ctx<R>(f: impl FnOnce(&TrayCtx) -> R) -> Option<R> {
    TRAY_CTX.with(|c| {
        let p = c.get();
        if p.is_null() {
            None
        } else {
            Some(f(unsafe { &*p }))
        }
    })
}

unsafe fn set_tip(hwnd: HWND, nid: *mut NOTIFYICONDATAW, text: &str) {
    let wide = to_wide(text);
    let n = wide.len().min(127);
    let tip = &mut (*nid).szTip;
    tip[..n].copy_from_slice(&wide[..n]);
    tip[n] = 0;
    (*nid).uFlags = NIF_TIP | NIF_SHOWTIP;
    Shell_NotifyIconW(NIM_MODIFY, nid);
    let _ = hwnd;
}

/// Ask for the icon to be repainted on the next status refresh.
pub fn update_tip_status(_enabled: bool) {
    invalidate_status();
}

fn show_menu(hwnd: HWND) {
    let Some(enabled) = with_ctx(|ctx| ctx.engine.lock().map(|e| e.is_enabled()).unwrap_or(false)) else {
        return;
    };
    let label = with_ctx(|ctx| ctx.engine.lock().map(|e| e.status_label()).unwrap_or_default())
        .unwrap_or_default();
    unsafe {
        let menu = CreatePopupMenu();
        let toggle_flags = MF_STRING | if enabled { MF_CHECKED } else { MF_UNCHECKED };
        let toggle_txt = to_wide(if enabled {
            "✔ Enable Controller (تشغيل التحكم - مفعّل)"
        } else {
            "  Enable Controller (تشغيل التحكم - معطّل)"
        });
        AppendMenuW(menu, toggle_flags, IDM_TOGGLE as usize, toggle_txt.as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        let settings_txt = to_wide("Settings (الإعدادات)");
        AppendMenuW(menu, MF_STRING, IDM_SETTINGS as usize, settings_txt.as_ptr());
        let reload_txt = to_wide("Reload Config (إعادة تحميل)");
        AppendMenuW(menu, MF_STRING, IDM_RELOAD as usize, reload_txt.as_ptr());
        if with_ctx(|c| c.vk.is_some()).unwrap_or(false) {
            let vk_state = with_ctx(|c| {
                c.vk.as_ref().map(|f| f.load(Ordering::SeqCst)).unwrap_or(false)
            })
            .unwrap_or(false);
            let vk_txt = to_wide(if vk_state { "Hide Keyboard (إخفاء الكيبورد)" } else { "Show Keyboard (إظهار الكيبورد)" });
            AppendMenuW(menu, MF_STRING, IDM_VKEYBOARD as usize, vk_txt.as_ptr());
        }
        let doctor_txt = to_wide("Diagnostics Report (تقرير التشخيص)");
        AppendMenuW(menu, MF_STRING, IDM_DOCTOR as usize, doctor_txt.as_ptr());
        if !crate::platform::is_elevated() {
            AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            let elevate_txt = to_wide("Restart as Administrator (تشغيل كمسؤول)");
            AppendMenuW(menu, MF_STRING, IDM_ELEVATE as usize, elevate_txt.as_ptr());
        }
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        let status_txt = to_wide(&label);
        AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, status_txt.as_ptr());
        let quit_txt = to_wide("Quit (إنهاء البرنامج)");
        AppendMenuW(menu, MF_STRING, IDM_QUIT as usize, quit_txt.as_ptr());

        SetForegroundWindow(hwnd);
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        let cmd = TrackPopupMenuEx(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
            pt.x,
            pt.y,
            hwnd,
            std::ptr::null_mut(),
        );
        DestroyMenu(menu);
        PostMessageW(hwnd, WM_NULL, 0, 0);
        handle_cmd(cmd as u16);
    }
}

fn handle_cmd(id: u16) {
    match id {
        IDM_TOGGLE => {
            with_ctx(|ctx| {
                if let Ok(mut e) = ctx.engine.lock() {
                    let now = e.toggle_enabled();
                    eprintln!("[tray] enabled = {now}");
                    update_tip_status(now);
                }
            });
        }
        IDM_SETTINGS => {
            crate::platform::log_debug("[tray] IDM_SETTINGS clicked -> invoking open_settings");
            with_ctx(|ctx| (ctx.open_settings)());
        }
        IDM_RELOAD => {
            with_ctx(|ctx| {
                if let Ok(mut e) = ctx.engine.lock() {
                    match e.reload_from_disk(&ctx.config_path) {
                        Ok(()) => eprintln!("[tray] config reloaded from {}", ctx.config_path.display()),
                        Err(error) => eprintln!("[tray] config reload failed: {error}"),
                    }
                }
            });
        }
        #[cfg(feature = "gui")]
        IDM_VKEYBOARD => {
            with_ctx(|ctx| crate::vkeyboard::toggle_keyboard(ctx.vk.clone()));
        }
        IDM_DOCTOR => {
            with_ctx(|ctx| crate::doctor::open_report(&ctx.config_path));
        }
        IDM_ELEVATE => {
            if let Ok(()) = crate::platform::relaunch_elevated(&[]) {
                with_ctx(|ctx| {
                    ctx.running.store(false, Ordering::Relaxed);
                });
                unsafe { PostQuitMessage(0) };
            }
        }
        IDM_QUIT => {
            with_ctx(|ctx| {
                ctx.running.store(false, Ordering::Relaxed);
            });
            unsafe { PostQuitMessage(0) };
        }
        _ => {}
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Run the tray on the current thread. Blocks until the message loop quits.
pub fn run(
    engine: Arc<Mutex<Engine>>,
    running: Arc<AtomicBool>,
    config_path: PathBuf,
    open_settings: Box<dyn Fn() + Send + Sync>,
    vk: Option<Arc<AtomicBool>>,
) {
    let mut ctx = Box::new(TrayCtx { engine, running, config_path, open_settings, vk });
    let ctx_ptr: *mut TrayCtx = &mut *ctx;
    TRAY_CTX.with(|c| c.set(ctx_ptr));

    crate::platform::attach_thread_to_desktop();

    unsafe {
        let inst = GetModuleHandleW(std::ptr::null());
        let class_name = to_wide("CCRustTrayHidden");
        let wnd_proc_ptr = wnd_proc as *const () as usize;
        let cls = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: wnd_proc_ptr,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: inst,
            hIcon: LoadIconW(std::ptr::null_mut(), IDI_APPLICATION),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: LoadIconW(std::ptr::null_mut(), IDI_APPLICATION),
        };
        RegisterClassExW(&cls);

        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class_name.as_ptr(),
            std::ptr::null(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            inst,
            std::ptr::null_mut(),
        );
        if hwnd.is_null() {
            eprintln!("[tray] failed to create message window; running without tray");
            TRAY_CTX.with(|c| c.set(std::ptr::null_mut()));
            return;
        }
        TRAY_HWND.store(hwnd, Ordering::Relaxed);

        #[link(name = "user32")]
        extern "system" {
            fn ChangeWindowMessageFilterEx(
                hwnd: HWND,
                message: u32,
                action: u32,
                p_change_filter_struct: *mut c_void,
            ) -> i32;
        }
        ChangeWindowMessageFilterEx(hwnd, CBTRAY, 1, std::ptr::null_mut());
        ChangeWindowMessageFilterEx(hwnd, WM_COMMAND, 1, std::ptr::null_mut());

        let wm_taskbar = RegisterWindowMessageW(to_wide("TaskbarCreated").as_ptr());
        WM_TASKBARCREATED.store(wm_taskbar, Ordering::SeqCst);
        if wm_taskbar != 0 {
            ChangeWindowMessageFilterEx(hwnd, wm_taskbar, 1, std::ptr::null_mut());
        }

        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_MESSAGE | NIF_ICON;
        nid.uCallbackMessage = CBTRAY;
        nid.hIcon = status_icon(None);
        if Shell_NotifyIconW(NIM_ADD, &mut nid) == 0 {
            let err = GetLastError();
            eprintln!("[tray] Shell_NotifyIcon ADD failed (code: {err}) - running in non-desktop or background mode");
        }
        nid.uTimeoutOrVersion = NOTIFYICON_VERSION_4;
        set_tip(hwnd, &mut nid, "Reverse Controller");
        invalidate_status();

        // Message loop.
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        Shell_NotifyIconW(NIM_DELETE, &mut nid);
        DestroyWindow(hwnd);
        TRAY_HWND.store(std::ptr::null_mut(), Ordering::Relaxed);
        TRAY_CTX.with(|c| c.set(std::ptr::null_mut()));
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_status::{RuntimeMode, RuntimeStatus};

    #[test]
    fn icon_has_a_soft_gamepad_shape_with_transparent_corners() {
        let px = icon_pixels((255, 170, 0), None);
        assert_eq!(px.len(), ICON_SIZE * ICON_SIZE);
        let alpha = |x: usize, y: usize| px[y * ICON_SIZE + x] >> 24;
        assert_eq!(alpha(0, 0), 0, "corners stay transparent");
        assert_eq!(alpha(16, 12), 255, "centre of the body is solid");
        assert_eq!(alpha(9, 15), 0, "the d-pad is cut out");
        assert!((0..ICON_SIZE * ICON_SIZE).any(|i| (1..255).contains(&(px[i] >> 24))), "edges are anti-aliased");
        let dotted = icon_pixels((255, 170, 0), Some((46, 204, 113)));
        assert_eq!(dotted[6 * ICON_SIZE + 26] & 0x00FF_FFFF, (46 << 16) | (204 << 8) | 113);
    }

    #[test]
    fn tooltip_names_state_profile_and_battery_within_the_shell_limit() {
        let status = RuntimeStatus {
            mode: RuntimeMode::Active,
            enabled: true,
            connected: true,
            controller_index: 0,
            active_profile: "Browser".into(),
            battery_level: Some(1),
            battery_wired: false,
            controller_name: "PlayStation 5 (DualSense)".into(),
        };
        let tip = status_tip(&status);
        assert!(tip.contains("active") && tip.contains("Browser") && tip.contains("Low"), "{tip}");
        assert!(tip.contains("DualSense"), "{tip}");
        let long = RuntimeStatus { active_profile: "x".repeat(400), ..status };
        assert!(status_tip(&long).chars().count() <= 127);
    }
}
