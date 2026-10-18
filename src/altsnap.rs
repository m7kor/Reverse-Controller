//! AltSnap native window management engine.
//!
//! Replicates RamonUnch/AltSnap features inside Reverse Controller:
//!   - Alt-Click + Drag: Move window under cursor from anywhere (not just titlebar)
//!   - Alt-RightClick + Drag: Resize window from nearest quadrant/corner
//!   - Magnetic Window Snapping to monitor borders (Aero Snap)
//!   - Quick Snap to Halves, Quarters, Maximize, Minimize, Restore, Center
//!   - Always on Top toggle (Pin window)
//!   - Roll up / Shade window to titlebar
//!   - Transparency / Opacity cycling & scroll adjustment
//!   - Lower window (Send to Back) & Close under cursor
//!   - Both physical Mouse (with Alt/Win/Ctrl/Shift modifier) AND Controller stick/button actions!

#![allow(non_snake_case, dead_code, clippy::not_unsafe_ptr_arg_deref)]

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

type HWND = *mut c_void;
type BOOL = i32;
type DWORD = u32;
type WPARAM = usize;
type LPARAM = isize;
type LRESULT = isize;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
struct MSLLHOOKSTRUCT {
    pt: POINT,
    mouse_data: u32,
    flags: u32,
    time: u32,
    dw_extra_info: usize,
}

use crate::tray::{DispatchMessageW, GetMessageW, TranslateMessage, MSG};

type HOOKPROC = unsafe extern "system" fn(code: i32, wparam: usize, lparam: isize) -> isize;

const GA_ROOT: u32 = 2;
const SW_RESTORE: i32 = 9;
const SW_MAXIMIZE: i32 = 3;
const SW_MINIMIZE: i32 = 6;
const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const SWP_FRAMECHANGED: u32 = 0x0020;
const SWP_ASYNCWINDOWPOS: u32 = 0x4000;

const HWND_TOPMOST: isize = -1;
const HWND_NOTOPMOST: isize = -2;
const HWND_BOTTOM: isize = 1;

const GWL_EXSTYLE: i32 = -20;
const WS_EX_TOPMOST: usize = 0x00000008;
const WS_EX_LAYERED: usize = 0x00080000;
const LWA_ALPHA: u32 = 0x00000002;
const WM_CLOSE: u32 = 0x0010;

const WM_LBUTTONDOWN: usize = 0x0201;
const WM_LBUTTONUP: usize = 0x0202;
const WM_MOUSEMOVE: usize = 0x0200;
const WM_RBUTTONDOWN: usize = 0x0204;
const WM_RBUTTONUP: usize = 0x0205;
const WM_MBUTTONDOWN: usize = 0x0207;
const WM_MOUSEWHEEL: usize = 0x020A;
const WH_MOUSE_LL: i32 = 14;

#[link(name = "user32")]
extern "system" {
    fn SetWindowsHookExW(idHook: i32, lpfn: HOOKPROC, hmod: *mut c_void, dwThreadId: u32) -> *mut c_void;
    fn UnhookWindowsHookEx(hhk: *mut c_void) -> BOOL;
    fn CallNextHookEx(hhk: *mut c_void, nCode: i32, wParam: usize, lParam: isize) -> isize;
    fn PostThreadMessageW(idThread: u32, msg: u32, wParam: WPARAM, lParam: LPARAM) -> BOOL;
    fn GetCurrentThreadId() -> u32;
    fn GetWindowLongPtrW(hWnd: HWND, nIndex: i32) -> isize;
    fn SetWindowLongPtrW(hWnd: HWND, nIndex: i32, dwNewLong: isize) -> isize;
    fn SetLayeredWindowAttributes(hwnd: HWND, crKey: u32, bAlpha: u8, dwFlags: u32) -> BOOL;
    fn GetWindowRect(hWnd: HWND, lpRect: *mut RECT) -> BOOL;
    fn SetWindowPos(
        hWnd: HWND,
        hWndInsertAfter: HWND,
        X: i32,
        Y: i32,
        cx: i32,
        cy: i32,
        uFlags: u32,
    ) -> BOOL;
    fn ShowWindow(hWnd: HWND, nCmdShow: i32) -> BOOL;
    fn ShowWindowAsync(hWnd: HWND, nCmdShow: i32) -> BOOL;
    fn IsZoomed(hWnd: HWND) -> BOOL;
    fn IsIconic(hWnd: HWND) -> BOOL;
    fn IsWindowVisible(hWnd: HWND) -> BOOL;
    fn WindowFromPoint(Point: POINT) -> HWND;
    fn GetAncestor(hwnd: HWND, gaFlags: u32) -> HWND;
    fn GetClassNameW(hWnd: HWND, lpClassName: *mut u16, nMaxCount: i32) -> i32;
    fn GetCursorPos(lpPoint: *mut POINT) -> BOOL;
    fn PostMessageW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> BOOL;
    fn GetAsyncKeyState(vKey: i32) -> i16;
    fn SetForegroundWindow(hWnd: HWND) -> BOOL;
    fn GetWindowPlacement(hWnd: HWND, lpwndpl: *mut WINDOWPLACEMENT) -> BOOL;
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct WINDOWPLACEMENT {
    pub length: u32,
    pub flags: u32,
    pub showCmd: u32,
    pub ptMinPosition: POINT,
    pub ptMaxPosition: POINT,
    pub rcNormalPosition: RECT,
}

#[link(name = "dwmapi")]
extern "system" {
    fn DwmGetWindowAttribute(
        hwnd: HWND,
        dwAttribute: DWORD,
        pvAttribute: *mut c_void,
        cbAttribute: DWORD,
    ) -> i32;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapQuadrant {
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Maximize,
    Minimize,
    Restore,
    Center,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResizeQuadrant {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Left,
    Right,
    Top,
    Bottom,
}

static ROLLED_WINDOWS: Mutex<Option<HashMap<usize, i32>>> = Mutex::new(None);
static WINDOW_OPACITY: Mutex<Option<HashMap<usize, u8>>> = Mutex::new(None);
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);
static GLOBAL_CONFIG: Mutex<Option<Arc<Mutex<crate::config::Config>>>> = Mutex::new(None);

pub static ALTSNAP_ENABLED: AtomicBool = AtomicBool::new(true);
pub static ALTSNAP_MODIFIER_VK: AtomicU32 = AtomicU32::new(0x12);
pub static ALTSNAP_SNAP_DIST: AtomicI32 = AtomicI32::new(15);
pub static ALTSNAP_SNAP_EDGES: AtomicBool = AtomicBool::new(true);
pub static ALTSNAP_AUTO_MAXIMIZE: AtomicBool = AtomicBool::new(true);
pub static ALTSNAP_TRANSPARENCY_STEP: AtomicU8 = AtomicU8::new(20);
pub static IS_DRAGGING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
enum MouseDragMode {
    Move {
        hwnd: usize,
        start_cursor: (i32, i32),
        start_window_rect: (i32, i32, i32, i32), // (left, top, width, height)
        borders: (i32, i32, i32, i32),           // (border_left, border_top, border_right, border_bottom)
        work_area: (i32, i32, i32, i32),         // (wa_x, wa_y, wa_w, wa_h)
        unmaximized: bool,                        // whether this drag started from un-maximizing
        last_pos: (i32, i32),
    },
    Resize {
        hwnd: usize,
        start_cursor: (i32, i32),
        start_window_rect: (i32, i32, i32, i32),
        work_area: (i32, i32, i32, i32),
        quadrant: ResizeQuadrant,
        last_rect: (i32, i32, i32, i32),
    },
}

static ACTIVE_DRAG: Mutex<Option<MouseDragMode>> = Mutex::new(None);

/// Update lock-free atomics from configuration so low-level hooks never block on mutexes.
pub fn update_config(cfg: &crate::config::AltSnapSettings) {
    ALTSNAP_ENABLED.store(cfg.enabled, Ordering::Relaxed);
    let vk = match cfg.modifier.as_str() {
        "Ctrl" => 0x11,
        "Shift" => 0x10,
        "Win" => 0x5B,
        _ => 0x12,
    };
    ALTSNAP_MODIFIER_VK.store(vk, Ordering::Relaxed);
    ALTSNAP_SNAP_DIST.store(cfg.snap_distance, Ordering::Relaxed);
    ALTSNAP_SNAP_EDGES.store(cfg.snap_to_edges, Ordering::Relaxed);
    ALTSNAP_AUTO_MAXIMIZE.store(cfg.auto_maximize, Ordering::Relaxed);
    ALTSNAP_TRANSPARENCY_STEP.store(cfg.transparency_step, Ordering::Relaxed);
}

pub fn get_hook_thread_id() -> u32 {
    HOOK_THREAD_ID.load(Ordering::SeqCst)
}

/// Detect quadrant from point `(cx, cy)` within window `rect`.
pub fn detect_resize_quadrant(rect: &RECT, cx: i32, cy: i32) -> ResizeQuadrant {
    let w = (rect.right - rect.left).max(1) as f32;
    let h = (rect.bottom - rect.top).max(1) as f32;
    let rx = ((cx - rect.left) as f32 / w).clamp(0.0, 1.0);
    let ry = ((cy - rect.top) as f32 / h).clamp(0.0, 1.0);

    if rx < 0.33 && ry < 0.33 {
        ResizeQuadrant::TopLeft
    } else if rx > 0.66 && ry < 0.33 {
        ResizeQuadrant::TopRight
    } else if rx < 0.33 && ry > 0.66 {
        ResizeQuadrant::BottomLeft
    } else if rx > 0.66 && ry > 0.66 {
        ResizeQuadrant::BottomRight
    } else if rx < 0.33 {
        ResizeQuadrant::Left
    } else if rx > 0.66 {
        ResizeQuadrant::Right
    } else if ry < 0.33 {
        ResizeQuadrant::Top
    } else if ry > 0.66 {
        ResizeQuadrant::Bottom
    } else {
        ResizeQuadrant::BottomRight
    }
}

/// Get true visible frame rectangle of window (accounting for Windows 10/11 invisible borders).
pub fn get_window_frame_rect(root: HWND) -> Option<RECT> {
    if root.is_null() {
        return None;
    }
    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        let hr = DwmGetWindowAttribute(
            root,
            9, // DWMWA_EXTENDED_FRAME_BOUNDS
            &mut rect as *mut _ as *mut c_void,
            std::mem::size_of::<RECT>() as DWORD,
        );
        if (hr == 0 && (rect.right > rect.left) && (rect.bottom > rect.top))
            || GetWindowRect(root, &mut rect) != 0
        {
            Some(rect)
        } else {
            None
        }
    }
}

/// Get valid top-level window under the pointer, ignoring desktop/shell and overlay windows.
pub fn get_window_under_cursor() -> Option<HWND> {
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut pt) == 0 {
            return None;
        }
        let under = WindowFromPoint(pt);
        if under.is_null() {
            return None;
        }
        let root = GetAncestor(under, GA_ROOT);
        if root.is_null() {
            return None;
        }
        if IsWindowVisible(root) == 0 || IsIconic(root) != 0 {
            return None;
        }
        let ov = crate::platform::overlay_hwnd() as HWND;
        if !ov.is_null() && (root == ov || under == ov) {
            return None;
        }
        let mut buf = [0u16; 64];
        let len = GetClassNameW(root, buf.as_mut_ptr(), 64);
        if len > 0 {
            let class = String::from_utf16_lossy(&buf[..len as usize]);
            if class == "Progman"
                || class == "WorkerW"
                || class == "Shell_TrayWnd"
                || class == "Shell_SecondaryTrayWnd"
                || class == "Windows.UI.Core.CoreWindow"
            {
                return None;
            }
        }
        Some(root)
    }
}

/// Move window relative to current position with optional magnetic border snapping.
/// (Called by controller actions; mouse drag is handled via anchor tracking in hook).
pub fn move_window_relative(
    root: HWND,
    dx: i32,
    dy: i32,
    snap_distance: i32,
    snap_to_edges: bool,
    auto_maximize: bool,
) {
    if (dx == 0 && dy == 0) || root.is_null() {
        return;
    }
    unsafe {
        if IsZoomed(root) != 0 {
            ShowWindowAsync(root, SW_RESTORE);
        }
        let mut rect: RECT = std::mem::zeroed();
        if GetWindowRect(root, &mut rect) != 0 {
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            let mut new_x = rect.left + dx;
            let mut new_y = rect.top + dy;

            if snap_to_edges && snap_distance > 0 {
                let cx = (rect.left + rect.right) / 2;
                let cy = (rect.top + rect.bottom) / 2;
                let (wa_x, wa_y, wa_w, wa_h) = crate::platform::work_area_at(cx, cy);
                let wa_right = wa_x + wa_w;
                let wa_bottom = wa_y + wa_h;

                if (new_x - wa_x).abs() <= snap_distance {
                    new_x = wa_x;
                } else if (new_x + width - wa_right).abs() <= snap_distance {
                    new_x = wa_right - width;
                }

                if (new_y - wa_y).abs() <= snap_distance {
                    if auto_maximize && new_y <= wa_y {
                        ShowWindowAsync(root, SW_MAXIMIZE);
                        return;
                    }
                    new_y = wa_y;
                } else if (new_y + height - wa_bottom).abs() <= snap_distance {
                    new_y = wa_bottom - height;
                }
            }

            SetWindowPos(
                root,
                std::ptr::null_mut(),
                new_x,
                new_y,
                width,
                height,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
            );
        }
    }
}

/// Resize window relative to its nearest corner/edge with optional magnetic snapping.
pub fn resize_window_relative(
    root: HWND,
    dx: i32,
    dy: i32,
    quadrant: ResizeQuadrant,
    snap_distance: i32,
    snap_to_edges: bool,
) {
    if (dx == 0 && dy == 0) || root.is_null() {
        return;
    }
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut pt) == 0 {
            return;
        }
        if IsZoomed(root) != 0 {
            ShowWindowAsync(root, SW_RESTORE);
        }
        let mut rect: RECT = std::mem::zeroed();
        if GetWindowRect(root, &mut rect) != 0 {
            let mut left = rect.left;
            let mut top = rect.top;
            let mut width = rect.right - rect.left;
            let mut height = rect.bottom - rect.top;

            let min_w = 160;
            let min_h = 100;

            match quadrant {
                ResizeQuadrant::TopLeft => {
                    let new_w = (width - dx).max(min_w);
                    left += width - new_w;
                    width = new_w;
                    let new_h = (height - dy).max(min_h);
                    top += height - new_h;
                    height = new_h;
                }
                ResizeQuadrant::TopRight => {
                    width = (width + dx).max(min_w);
                    let new_h = (height - dy).max(min_h);
                    top += height - new_h;
                    height = new_h;
                }
                ResizeQuadrant::BottomLeft => {
                    let new_w = (width - dx).max(min_w);
                    left += width - new_w;
                    width = new_w;
                    height = (height + dy).max(min_h);
                }
                ResizeQuadrant::BottomRight => {
                    width = (width + dx).max(min_w);
                    height = (height + dy).max(min_h);
                }
                ResizeQuadrant::Left => {
                    let new_w = (width - dx).max(min_w);
                    left += width - new_w;
                    width = new_w;
                }
                ResizeQuadrant::Right => {
                    width = (width + dx).max(min_w);
                }
                ResizeQuadrant::Top => {
                    let new_h = (height - dy).max(min_h);
                    top += height - new_h;
                    height = new_h;
                }
                ResizeQuadrant::Bottom => {
                    height = (height + dy).max(min_h);
                }
            }

            if snap_to_edges && snap_distance > 0 {
                let cx = (rect.left + rect.right) / 2;
                let cy = (rect.top + rect.bottom) / 2;
                let (wa_x, wa_y, wa_w, wa_h) = crate::platform::work_area_at(cx, cy);
                let wa_right = wa_x + wa_w;
                let wa_bottom = wa_y + wa_h;

                if matches!(quadrant, ResizeQuadrant::TopLeft | ResizeQuadrant::BottomLeft | ResizeQuadrant::Left)
                    && (left - wa_x).abs() <= snap_distance
                {
                    width += left - wa_x;
                    left = wa_x;
                }
                if matches!(quadrant, ResizeQuadrant::TopRight | ResizeQuadrant::BottomRight | ResizeQuadrant::Right)
                    && (left + width - wa_right).abs() <= snap_distance
                {
                    width = wa_right - left;
                }
                if matches!(quadrant, ResizeQuadrant::TopLeft | ResizeQuadrant::TopRight | ResizeQuadrant::Top)
                    && (top - wa_y).abs() <= snap_distance
                {
                    height += top - wa_y;
                    top = wa_y;
                }
                if matches!(quadrant, ResizeQuadrant::BottomLeft | ResizeQuadrant::BottomRight | ResizeQuadrant::Bottom)
                    && (top + height - wa_bottom).abs() <= snap_distance
                {
                    height = wa_bottom - top;
                }
            }

            SetWindowPos(
                root,
                std::ptr::null_mut(),
                left,
                top,
                width,
                height,
                SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
            );
        }
    }
}

/// Snap window to a half, quarter, center, or maximize/minimize on current monitor.
pub fn snap_window(root: HWND, quad: SnapQuadrant) {
    if root.is_null() {
        return;
    }
    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        GetWindowRect(root, &mut rect);
        let cx = if rect.right > rect.left { (rect.left + rect.right) / 2 } else { 0 };
        let cy = if rect.bottom > rect.top { (rect.top + rect.bottom) / 2 } else { 0 };
        let (wa_x, wa_y, wa_w, wa_h) = crate::platform::work_area_at(cx, cy);

        match quad {
            SnapQuadrant::Maximize => {
                ShowWindowAsync(root, SW_MAXIMIZE);
                return;
            }
            SnapQuadrant::Minimize => {
                ShowWindowAsync(root, SW_MINIMIZE);
                return;
            }
            SnapQuadrant::Restore => {
                ShowWindowAsync(root, SW_RESTORE);
                return;
            }
            _ => {}
        }

        if IsZoomed(root) != 0 {
            ShowWindowAsync(root, SW_RESTORE);
        }

        let (x, y, w, h) = match quad {
            SnapQuadrant::LeftHalf => (wa_x, wa_y, wa_w / 2, wa_h),
            SnapQuadrant::RightHalf => (wa_x + wa_w / 2, wa_y, wa_w - wa_w / 2, wa_h),
            SnapQuadrant::TopHalf => (wa_x, wa_y, wa_w, wa_h / 2),
            SnapQuadrant::BottomHalf => (wa_x, wa_y + wa_h / 2, wa_w, wa_h - wa_h / 2),
            SnapQuadrant::TopLeft => (wa_x, wa_y, wa_w / 2, wa_h / 2),
            SnapQuadrant::TopRight => (wa_x + wa_w / 2, wa_y, wa_w - wa_w / 2, wa_h / 2),
            SnapQuadrant::BottomLeft => (wa_x, wa_y + wa_h / 2, wa_w / 2, wa_h - wa_h / 2),
            SnapQuadrant::BottomRight => (wa_x + wa_w / 2, wa_y + wa_h / 2, wa_w - wa_w / 2, wa_h - wa_h / 2),
            SnapQuadrant::Center => {
                let mut rect: RECT = std::mem::zeroed();
                GetWindowRect(root, &mut rect);
                let cur_w = (rect.right - rect.left).min(wa_w);
                let cur_h = (rect.bottom - rect.top).min(wa_h);
                (wa_x + (wa_w - cur_w) / 2, wa_y + (wa_h - cur_h) / 2, cur_w, cur_h)
            }
            _ => return,
        };

        SetWindowPos(
            root,
            std::ptr::null_mut(),
            x,
            y,
            w,
            h,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        );
    }
}

/// Toggle WS_EX_TOPMOST (Pin window on top) with OSD alert.
pub fn toggle_topmost(root: HWND) -> bool {
    if root.is_null() {
        return false;
    }
    unsafe {
        let style = GetWindowLongPtrW(root, GWL_EXSTYLE);
        let is_topmost = (style as usize & WS_EX_TOPMOST) != 0;
        let new_pos = if is_topmost { HWND_NOTOPMOST as HWND } else { HWND_TOPMOST as HWND };
        SetWindowPos(
            root,
            new_pos,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        );
        if is_topmost {
            crate::osd::show_info("📌", "Always on Top: OFF", "Window is no longer pinned");
            false
        } else {
            crate::osd::show_info("📌", "Always on Top: ON", "Window pinned on top of other windows");
            true
        }
    }
}

/// Roll up / Shade window to only its titlebar height (~32px) and back.
pub fn toggle_roll_up(root: HWND) {
    if root.is_null() {
        return;
    }
    unsafe {
        let id = root as usize;
        let mut rolled_map = ROLLED_WINDOWS.lock().unwrap();
        let map = rolled_map.get_or_insert_with(HashMap::new);

        if let Some(orig_h) = map.remove(&id) {
            let mut rect: RECT = std::mem::zeroed();
            if GetWindowRect(root, &mut rect) != 0 {
                let w = rect.right - rect.left;
                SetWindowPos(
                    root,
                    std::ptr::null_mut(),
                    rect.left,
                    rect.top,
                    w,
                    orig_h,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                );
            }
            crate::osd::show_info("🗔", "Unrolled Window", "Restored original window height");
        } else {
            let mut rect: RECT = std::mem::zeroed();
            if GetWindowRect(root, &mut rect) != 0 {
                let orig_h = rect.bottom - rect.top;
                let w = rect.right - rect.left;
                let titlebar_h = 32;
                map.insert(id, orig_h);
                SetWindowPos(
                    root,
                    std::ptr::null_mut(),
                    rect.left,
                    rect.top,
                    w,
                    titlebar_h,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                );
                crate::osd::show_info("🗔", "Rolled Up Window", "Shaded to title bar");
            }
        }
    }
}

/// Cycle or step window transparency (opacity between 30% and 100%).
pub fn cycle_transparency(root: HWND, step_percent: u8) {
    if root.is_null() {
        return;
    }
    let id = root as usize;
    let mut op_map = WINDOW_OPACITY.lock().unwrap();
    let map = op_map.get_or_insert_with(HashMap::new);
    let cur_alpha = *map.get(&id).unwrap_or(&255);

    let step = ((step_percent as f32 / 100.0) * 255.0).round() as u8;
    let next_alpha = if cur_alpha <= 76 {
        255
    } else {
        cur_alpha.saturating_sub(step).max(76)
    };

    map.insert(id, next_alpha);
    set_window_opacity(root, next_alpha);
    let pct = ((next_alpha as f32 / 255.0) * 100.0).round() as u32;
    crate::osd::show_info("👻", &format!("Transparency: {pct}%"), "Window opacity adjusted");
}

/// Set exact window alpha (255 = fully opaque, 0 = invisible).
pub fn set_window_opacity(root: HWND, alpha: u8) {
    if root.is_null() {
        return;
    }
    unsafe {
        let style = GetWindowLongPtrW(root, GWL_EXSTYLE);
        if alpha == 255 {
            SetWindowLongPtrW(root, GWL_EXSTYLE, style & !(WS_EX_LAYERED as isize));
            SetLayeredWindowAttributes(root, 0, 255, LWA_ALPHA);
        } else {
            SetWindowLongPtrW(root, GWL_EXSTYLE, style | (WS_EX_LAYERED as isize));
            SetLayeredWindowAttributes(root, 0, alpha, LWA_ALPHA);
        }
        SetWindowPos(
            root,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED | SWP_ASYNCWINDOWPOS,
        );
    }
}

/// Lower window to the bottom of the z-order (Send to Back).
pub fn lower_window(root: HWND) {
    if root.is_null() {
        return;
    }
    unsafe {
        SetWindowPos(
            root,
            HWND_BOTTOM as HWND,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
        );
        crate::osd::show_info("⬇", "Send to Back", "Window sent to bottom of desktop stack");
    }
}

/// Toggle maximize/restore on window.
pub fn toggle_maximize(root: HWND) {
    if root.is_null() {
        return;
    }
    unsafe {
        if IsZoomed(root) != 0 {
            ShowWindowAsync(root, SW_RESTORE);
        } else {
            ShowWindowAsync(root, SW_MAXIMIZE);
        }
    }
}

/// Minimize window under cursor.
pub fn minimize_window(root: HWND) {
    if root.is_null() {
        return;
    }
    unsafe {
        ShowWindowAsync(root, SW_MINIMIZE);
    }
}

/// Close window gracefully under cursor via WM_CLOSE.
pub fn close_window(root: HWND) {
    if root.is_null() {
        return;
    }
    unsafe {
        PostMessageW(root, WM_CLOSE, 0, 0);
    }
}

fn is_modifier_down(modifier: &str) -> bool {
    let vk = match modifier {
        "Ctrl" => 0x11, // VK_CONTROL
        "Shift" => 0x10, // VK_SHIFT
        "Win" => 0x5B, // VK_LWIN
        _ => 0x12, // VK_MENU (Alt)
    };
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

unsafe extern "system" fn altsnap_mouse_proc(code: i32, wparam: usize, lparam: isize) -> isize {
    if code < 0 || lparam == 0 {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    }

    if !ALTSNAP_ENABLED.load(Ordering::Relaxed) {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    }

    let is_dragging = IS_DRAGGING.load(Ordering::Relaxed);
    let mod_vk = ALTSNAP_MODIFIER_VK.load(Ordering::Relaxed) as i32;
    let mod_down = (GetAsyncKeyState(mod_vk) as u16 & 0x8000) != 0;

    // Fast-path: If no drag is active and modifier key is not down, immediately pass through.
    if !is_dragging && !mod_down {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    }

    let hook_struct = &*(lparam as *const MSLLHOOKSTRUCT);
    let pt = hook_struct.pt;

    match wparam {
        WM_LBUTTONDOWN if mod_down => {
            if let Some(hwnd) = get_window_under_cursor() {
                SetForegroundWindow(hwnd);
                let (win_x, win_y, win_w, win_h, unmaximized) = if IsZoomed(hwnd) != 0 {
                    let mut wp: WINDOWPLACEMENT = std::mem::zeroed();
                    wp.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
                    let (mut rw, mut rh) = (960, 600);
                    if GetWindowPlacement(hwnd, &mut wp) != 0 {
                        let nw = wp.rcNormalPosition.right - wp.rcNormalPosition.left;
                        let nh = wp.rcNormalPosition.bottom - wp.rcNormalPosition.top;
                        if nw > 200 && nh > 150 {
                            rw = nw;
                            rh = nh;
                        }
                    }
                    ShowWindow(hwnd, SW_RESTORE);
                    let x = pt.x - rw / 2;
                    let y = (pt.y - 15).max(0);
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        x,
                        y,
                        rw,
                        rh,
                        SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                    );
                    (x, y, rw, rh, true)
                } else {
                    let mut r: RECT = std::mem::zeroed();
                    GetWindowRect(hwnd, &mut r);
                    (r.left, r.top, (r.right - r.left).max(100), (r.bottom - r.top).max(50), false)
                };

                let frame = get_window_frame_rect(hwnd).unwrap_or(RECT {
                    left: win_x,
                    top: win_y,
                    right: win_x + win_w,
                    bottom: win_y + win_h,
                });
                let border_left = (frame.left - win_x).max(0);
                let border_top = (frame.top - win_y).max(0);
                let border_right = ((win_x + win_w) - frame.right).max(0);
                let border_bottom = ((win_y + win_h) - frame.bottom).max(0);
                let wa = crate::platform::work_area_at(pt.x, pt.y);

                if let Ok(mut drag) = ACTIVE_DRAG.lock() {
                    *drag = Some(MouseDragMode::Move {
                        hwnd: hwnd as usize,
                        start_cursor: (pt.x, pt.y),
                        start_window_rect: (win_x, win_y, win_w, win_h),
                        borders: (border_left, border_top, border_right, border_bottom),
                        work_area: wa,
                        unmaximized,
                        last_pos: (win_x, win_y),
                    });
                    IS_DRAGGING.store(true, Ordering::Relaxed);
                }
                return 1;
            }
        }
        WM_RBUTTONDOWN if mod_down => {
            if let Some(hwnd) = get_window_under_cursor() {
                SetForegroundWindow(hwnd);
                if IsZoomed(hwnd) != 0 {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                let mut rect: RECT = std::mem::zeroed();
                if GetWindowRect(hwnd, &mut rect) != 0 {
                    let quad = detect_resize_quadrant(&rect, pt.x, pt.y);
                    let w = (rect.right - rect.left).max(100);
                    let h = (rect.bottom - rect.top).max(50);
                    let wa = crate::platform::work_area_at(pt.x, pt.y);
                    if let Ok(mut drag) = ACTIVE_DRAG.lock() {
                        *drag = Some(MouseDragMode::Resize {
                            hwnd: hwnd as usize,
                            start_cursor: (pt.x, pt.y),
                            start_window_rect: (rect.left, rect.top, w, h),
                            work_area: wa,
                            quadrant: quad,
                            last_rect: (rect.left, rect.top, w, h),
                        });
                        IS_DRAGGING.store(true, Ordering::Relaxed);
                    }
                    return 1;
                }
            }
        }
        WM_MBUTTONDOWN if mod_down => {
            if let Some(hwnd) = get_window_under_cursor() {
                toggle_maximize(hwnd);
                return 1;
            }
        }
        WM_MOUSEWHEEL if mod_down => {
            if let Some(hwnd) = get_window_under_cursor() {
                let step = ALTSNAP_TRANSPARENCY_STEP.load(Ordering::Relaxed);
                cycle_transparency(hwnd, step);
                return 1;
            }
        }
        WM_MOUSEMOVE => {
            // Cancel drag if Escape key is pressed
            if (GetAsyncKeyState(0x1B /* VK_ESCAPE */) as u16 & 0x8000) != 0 {
                if let Ok(mut drag) = ACTIVE_DRAG.lock() {
                    if let Some(mode) = drag.take() {
                        IS_DRAGGING.store(false, Ordering::Relaxed);
                        match mode {
                            MouseDragMode::Move { hwnd, start_window_rect, .. }
                            | MouseDragMode::Resize { hwnd, start_window_rect, .. } => {
                                SetWindowPos(
                                    hwnd as HWND,
                                    std::ptr::null_mut(),
                                    start_window_rect.0,
                                    start_window_rect.1,
                                    start_window_rect.2,
                                    start_window_rect.3,
                                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                                );
                            }
                        }
                    }
                }
                return 1;
            }

            if let Ok(mut drag_guard) = ACTIVE_DRAG.lock() {
                if let Some(mode) = drag_guard.as_mut() {
                    let snap_edges = ALTSNAP_SNAP_EDGES.load(Ordering::Relaxed);
                    let snap_dist = ALTSNAP_SNAP_DIST.load(Ordering::Relaxed);

                    match mode {
                        MouseDragMode::Move {
                            hwnd,
                            start_cursor,
                            start_window_rect,
                            borders,
                            work_area,
                            unmaximized,
                            last_pos,
                        } => {
                            // Safety check: Is left mouse button physically down?
                            if (GetAsyncKeyState(0x01 /* VK_LBUTTON */) as u16 & 0x8000) == 0 {
                                IS_DRAGGING.store(false, Ordering::Relaxed);
                                *drag_guard = None;
                                return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
                            }

                            let raw_dx = pt.x - start_cursor.0;
                            let raw_dy = pt.y - start_cursor.1;

                            let mut outer_x = start_window_rect.0 + raw_dx;
                            let mut outer_y = start_window_rect.1 + raw_dy;
                            let width = start_window_rect.2;
                            let height = start_window_rect.3;

                            if snap_edges && snap_dist > 0 {
                                if pt.x < work_area.0
                                    || pt.x >= work_area.0 + work_area.2
                                    || pt.y < work_area.1
                                    || pt.y >= work_area.1 + work_area.3
                                {
                                    *work_area = crate::platform::work_area_at(pt.x, pt.y);
                                }
                                let wa_x = work_area.0;
                                let wa_y = work_area.1;
                                let wa_right = wa_x + work_area.2;
                                let wa_bottom = wa_y + work_area.3;

                                let vis_x = outer_x + borders.0;
                                let vis_y = outer_y + borders.1;
                                let vis_w = width - borders.0 - borders.2;
                                let vis_h = height - borders.1 - borders.3;

                                // Prevent horizontal edge trap: only snap if window fits with room to spare
                                if vis_w < work_area.2 - snap_dist * 3 {
                                    if (vis_x - wa_x).abs() <= snap_dist {
                                        outer_x = wa_x - borders.0;
                                    } else if (vis_x + vis_w - wa_right).abs() <= snap_dist {
                                        outer_x = wa_right - vis_w - borders.0;
                                    }
                                }

                                // Prevent vertical edge trap: only snap if window fits and not in unmaximize grace zone
                                let in_unmaximize_grace = *unmaximized && (pt.y - start_cursor.1).abs() < snap_dist * 3;
                                if !in_unmaximize_grace && vis_h < work_area.3 - snap_dist * 3 {
                                    if (vis_y - wa_y).abs() <= snap_dist {
                                        outer_y = wa_y - borders.1;
                                    } else if (vis_y + vis_h - wa_bottom).abs() <= snap_dist {
                                        outer_y = wa_bottom - vis_h - borders.1;
                                    }
                                }
                            }

                            if outer_x != last_pos.0 || outer_y != last_pos.1 {
                                *last_pos = (outer_x, outer_y);
                                SetWindowPos(
                                    *hwnd as HWND,
                                    std::ptr::null_mut(),
                                    outer_x,
                                    outer_y,
                                    0,
                                    0,
                                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                                );
                            }
                            return 1;
                        }
                        MouseDragMode::Resize {
                            hwnd,
                            start_cursor,
                            start_window_rect,
                            work_area,
                            quadrant,
                            last_rect,
                        } => {
                            // Safety check: Is right mouse button physically down?
                            if (GetAsyncKeyState(0x02 /* VK_RBUTTON */) as u16 & 0x8000) == 0 {
                                IS_DRAGGING.store(false, Ordering::Relaxed);
                                *drag_guard = None;
                                return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
                            }

                            let raw_dx = pt.x - start_cursor.0;
                            let raw_dy = pt.y - start_cursor.1;

                            let (mut left, mut top, mut width, mut height) = (
                                start_window_rect.0,
                                start_window_rect.1,
                                start_window_rect.2,
                                start_window_rect.3,
                            );
                            let min_w = 160;
                            let min_h = 100;

                            match *quadrant {
                                ResizeQuadrant::TopLeft => {
                                    let nw = (width - raw_dx).max(min_w);
                                    left += width - nw;
                                    width = nw;
                                    let nh = (height - raw_dy).max(min_h);
                                    top += height - nh;
                                    height = nh;
                                }
                                ResizeQuadrant::TopRight => {
                                    width = (width + raw_dx).max(min_w);
                                    let nh = (height - raw_dy).max(min_h);
                                    top += height - nh;
                                    height = nh;
                                }
                                ResizeQuadrant::BottomLeft => {
                                    let nw = (width - raw_dx).max(min_w);
                                    left += width - nw;
                                    width = nw;
                                    height = (height + raw_dy).max(min_h);
                                }
                                ResizeQuadrant::BottomRight => {
                                    width = (width + raw_dx).max(min_w);
                                    height = (height + raw_dy).max(min_h);
                                }
                                ResizeQuadrant::Left => {
                                    let nw = (width - raw_dx).max(min_w);
                                    left += width - nw;
                                    width = nw;
                                }
                                ResizeQuadrant::Right => {
                                    width = (width + raw_dx).max(min_w);
                                }
                                ResizeQuadrant::Top => {
                                    let nh = (height - raw_dy).max(min_h);
                                    top += height - nh;
                                    height = nh;
                                }
                                ResizeQuadrant::Bottom => {
                                    height = (height + raw_dy).max(min_h);
                                }
                            }

                            if snap_edges && snap_dist > 0 {
                                if pt.x < work_area.0
                                    || pt.x >= work_area.0 + work_area.2
                                    || pt.y < work_area.1
                                    || pt.y >= work_area.1 + work_area.3
                                {
                                    *work_area = crate::platform::work_area_at(pt.x, pt.y);
                                }
                                let wa_x = work_area.0;
                                let wa_y = work_area.1;
                                let wa_right = wa_x + work_area.2;
                                let wa_bottom = wa_y + work_area.3;

                                if matches!(*quadrant, ResizeQuadrant::TopLeft | ResizeQuadrant::BottomLeft | ResizeQuadrant::Left)
                                    && (left - wa_x).abs() <= snap_dist
                                {
                                    width += left - wa_x;
                                    left = wa_x;
                                }
                                if matches!(*quadrant, ResizeQuadrant::TopRight | ResizeQuadrant::BottomRight | ResizeQuadrant::Right)
                                    && (left + width - wa_right).abs() <= snap_dist
                                {
                                    width = wa_right - left;
                                }
                                if matches!(*quadrant, ResizeQuadrant::TopLeft | ResizeQuadrant::TopRight | ResizeQuadrant::Top)
                                    && (top - wa_y).abs() <= snap_dist
                                {
                                    height += top - wa_y;
                                    top = wa_y;
                                }
                                if matches!(*quadrant, ResizeQuadrant::BottomLeft | ResizeQuadrant::BottomRight | ResizeQuadrant::Bottom)
                                    && (top + height - wa_bottom).abs() <= snap_dist
                                {
                                    height = wa_bottom - top;
                                }
                            }

                            let cur_rect = (left, top, width, height);
                            if cur_rect != *last_rect {
                                *last_rect = cur_rect;
                                SetWindowPos(
                                    *hwnd as HWND,
                                    std::ptr::null_mut(),
                                    left,
                                    top,
                                    width,
                                    height,
                                    SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                                );
                            }
                            return 1;
                        }
                    }
                }
            }
        }
        WM_LBUTTONUP => {
            IS_DRAGGING.store(false, Ordering::Relaxed);
            let was_dragging = if let Ok(mut drag) = ACTIVE_DRAG.lock() {
                drag.take()
            } else {
                None
            };

            if let Some(MouseDragMode::Move { hwnd, work_area, unmaximized, start_cursor, .. }) = was_dragging {
                if ALTSNAP_AUTO_MAXIMIZE.load(Ordering::Relaxed) {
                    let wa_x = work_area.0;
                    let wa_y = work_area.1;
                    let wa_right = wa_x + work_area.2;
                    let wa_bottom = wa_y + work_area.3;

                    let total_moved = (pt.x - start_cursor.0).abs() + (pt.y - start_cursor.1).abs();
                    if !(unmaximized && total_moved < 30) {
                        if pt.y <= wa_y {
                            ShowWindowAsync(hwnd as HWND, SW_MAXIMIZE);
                        } else if pt.x <= wa_x && pt.y <= wa_y + 40 {
                            snap_window(hwnd as HWND, SnapQuadrant::TopLeft);
                        } else if pt.x >= wa_right - 1 && pt.y <= wa_y + 40 {
                            snap_window(hwnd as HWND, SnapQuadrant::TopRight);
                        } else if pt.x <= wa_x && pt.y >= wa_bottom - 40 {
                            snap_window(hwnd as HWND, SnapQuadrant::BottomLeft);
                        } else if pt.x >= wa_right - 1 && pt.y >= wa_bottom - 40 {
                            snap_window(hwnd as HWND, SnapQuadrant::BottomRight);
                        } else if pt.x <= wa_x {
                            snap_window(hwnd as HWND, SnapQuadrant::LeftHalf);
                        } else if pt.x >= wa_right - 1 {
                            snap_window(hwnd as HWND, SnapQuadrant::RightHalf);
                        }
                    }
                }
                return 1;
            }
        }
        WM_RBUTTONUP => {
            IS_DRAGGING.store(false, Ordering::Relaxed);
            let was_resizing = if let Ok(mut drag) = ACTIVE_DRAG.lock() {
                drag.take().is_some()
            } else {
                false
            };

            if was_resizing {
                return 1;
            }
        }
        _ => {}
    }

    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// Start background low-level mouse hook for AltSnap desktop mouse operations.
pub fn start_mouse_hook(running: Arc<AtomicBool>, config: Arc<Mutex<crate::config::Config>>) {
    if let Ok(cfg) = config.lock() {
        update_config(&cfg.altsnap);
    }
    if let Ok(mut cfg_guard) = GLOBAL_CONFIG.lock() {
        *cfg_guard = Some(config);
    }
    std::thread::Builder::new()
        .name("cc-altsnap-mouse".into())
        .spawn(move || {
            crate::platform::attach_to_interactive_desktop();
            unsafe {
                HOOK_THREAD_ID.store(GetCurrentThreadId(), Ordering::SeqCst);
                let hook = SetWindowsHookExW(
                    WH_MOUSE_LL,
                    altsnap_mouse_proc,
                    std::ptr::null_mut(),
                    0,
                );
                if hook.is_null() {
                    crate::platform::log_debug("[altsnap] failed to install WH_MOUSE_LL hook");
                    return;
                }
                crate::platform::log_debug("[altsnap] WH_MOUSE_LL hook installed successfully");

                let mut msg: MSG = std::mem::zeroed();
                while running.load(Ordering::Relaxed) {
                    let ret = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
                    if ret <= 0 {
                        break;
                    }
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }

                UnhookWindowsHookEx(hook);
                crate::platform::log_debug("[altsnap] WH_MOUSE_LL hook unhooked cleanly");
            }
        })
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_resize_quadrant() {
        let rect = RECT { left: 100, top: 100, right: 400, bottom: 400 };
        assert_eq!(detect_resize_quadrant(&rect, 110, 110), ResizeQuadrant::TopLeft);
        assert_eq!(detect_resize_quadrant(&rect, 390, 110), ResizeQuadrant::TopRight);
        assert_eq!(detect_resize_quadrant(&rect, 110, 390), ResizeQuadrant::BottomLeft);
        assert_eq!(detect_resize_quadrant(&rect, 390, 390), ResizeQuadrant::BottomRight);
        assert_eq!(detect_resize_quadrant(&rect, 250, 110), ResizeQuadrant::Top);
        assert_eq!(detect_resize_quadrant(&rect, 250, 390), ResizeQuadrant::Bottom);
        assert_eq!(detect_resize_quadrant(&rect, 110, 250), ResizeQuadrant::Left);
        assert_eq!(detect_resize_quadrant(&rect, 390, 250), ResizeQuadrant::Right);
    }

    #[test]
    fn test_magnetic_snapping_independent_axes() {
        let snap_dist: i32 = 15;
        let wa: (i32, i32, i32, i32) = (0, 0, 1920, 1080);
        let borders: (i32, i32, i32, i32) = (0, 0, 0, 0);

        let outer_x: i32 = 5; // within snap_dist (15) of wa.0 (0)
        let outer_y: i32 = 300; // far from edges
        let vis_x: i32 = outer_x + borders.0;
        let vis_y: i32 = outer_y + borders.1;

        let snapped_x = if (vis_x - wa.0).abs() <= snap_dist { wa.0 - borders.0 } else { outer_x };
        let snapped_y = if (vis_y - wa.1).abs() <= snap_dist { wa.1 - borders.1 } else { outer_y };

        assert_eq!(snapped_x, 0, "X must snap to 0");
        assert_eq!(snapped_y, 300, "Y must remain 300 and NOT be altered by X snapping");
    }

    #[test]
    fn test_unmaximize_grace_prevents_immediate_top_edge_trap() {
        let snap_dist: i32 = 15;
        let unmaximized = true;
        let start_cursor_y: i32 = 10;
        let pt_y: i32 = 20; // user pulled down 10px

        let in_unmaximize_grace = unmaximized && (pt_y - start_cursor_y).abs() < snap_dist * 3;
        assert!(in_unmaximize_grace, "Must be in grace zone so top edge doesn't trap window");

        let pt_y_pulled_down: i32 = 60; // user pulled down 50px
        let in_unmaximize_grace_after = unmaximized && (pt_y_pulled_down - start_cursor_y).abs() < snap_dist * 3;
        assert!(!in_unmaximize_grace_after, "Grace zone expires once user intentionally moves away from top edge");
    }
}
