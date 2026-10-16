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
        nid.hIcon = LoadIconW(std::ptr::null_mut(), IDI_APPLICATION);
        Shell_NotifyIconW(NIM_ADD, &mut nid);
        nid.uTimeoutOrVersion = NOTIFYICON_VERSION_4;
        let tip = if crate::platform::is_elevated() {
            "Controller Companion (Administrator)"
        } else {
            "Controller Companion (مفعّل - ENABLED)"
        };
        set_tip(hwnd, &mut nid, tip);
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

pub fn update_tip_status(enabled: bool) {
    let hwnd = TRAY_HWND.load(Ordering::Relaxed);
    if !hwnd.is_null() {
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        let tip = if enabled {
            "Controller Companion (مفعّل - ENABLED)"
        } else {
            "Controller Companion (معطّل - DISABLED)"
        };
        unsafe { set_tip(hwnd, &mut nid, tip) };
    }
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
            with_ctx(|ctx| {
                if let Some(f) = &ctx.vk {
                    let next = !f.load(Ordering::SeqCst);
                    f.store(next, Ordering::SeqCst);
                    if next {
                        crate::platform::push_foreground();
                        let prefs = ctx.engine.lock().map(|e| e.config().keyboard.clone()).unwrap_or_default();
                        let work = crate::platform::work_area();
                        let origin = prefs.anchor(work, None);
                        let base_w = (crate::config::KB_BASE_W * prefs.scale).round() as i32;
                        let base_h = (crate::config::KB_BASE_H * prefs.scale).round() as i32;
                        crate::platform::place_window("CCVKbd", origin.0, origin.1, base_w, base_h);
                        crate::platform::set_window_visible("CCVKbd", true);
                        crate::vkeyboard::request_vkeyboard_repaint();
                        crate::platform::log_debug("[tray] Show Keyboard clicked -> placed and shown");
                    } else {
                        crate::platform::move_window("CCVKbd", -4000, -4000);
                        crate::vkeyboard::request_vkeyboard_repaint();
                        crate::platform::trim_working_set();
                        crate::platform::log_debug("[tray] Hide Keyboard clicked -> parked offscreen");
                    }
                }
            });
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
        nid.hIcon = LoadIconW(std::ptr::null_mut(), IDI_APPLICATION);
        if Shell_NotifyIconW(NIM_ADD, &mut nid) == 0 {
            let err = GetLastError();
            eprintln!("[tray] Shell_NotifyIcon ADD failed (code: {err}) - running in non-desktop or background mode");
        }
        nid.uTimeoutOrVersion = NOTIFYICON_VERSION_4;
        let tip = if crate::platform::is_elevated() {
            "Controller Companion (Administrator)"
        } else {
            "Controller Companion (مفعّل - ENABLED)"
        };
        set_tip(hwnd, &mut nid, tip);

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

