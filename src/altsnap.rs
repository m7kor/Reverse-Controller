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
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
    fn ShowWindowAsync(hWnd: HWND, nCmdShow: i32) -> BOOL;
    fn IsZoomed(hWnd: HWND) -> BOOL;
    fn WindowFromPoint(Point: POINT) -> HWND;
    fn GetAncestor(hwnd: HWND, gaFlags: u32) -> HWND;
    fn GetClassNameW(hWnd: HWND, lpClassName: *mut u16, nMaxCount: i32) -> i32;
    fn GetCursorPos(lpPoint: *mut POINT) -> BOOL;
    fn PostMessageW(hWnd: HWND, Msg: u32, wParam: WPARAM, lParam: LPARAM) -> BOOL;
    fn GetAsyncKeyState(vKey: i32) -> i16;
    fn SetForegroundWindow(hWnd: HWND) -> BOOL;
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

#[derive(Clone, Debug)]
enum MouseDragMode {
    Move {
        hwnd: usize,
        last_pt: (i32, i32),
        start_pt: (i32, i32),
    },
    Resize {
        hwnd: usize,
        last_pt: (i32, i32),
        quadrant: ResizeQuadrant,
    },
}

static ACTIVE_DRAG: Mutex<Option<MouseDragMode>> = Mutex::new(None);

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
        let ov = crate::platform::overlay_hwnd() as HWND;
        if !ov.is_null() && (root == ov || under == ov) {
            return None;
        }
        let mut buf = [0u16; 64];
        let len = GetClassNameW(root, buf.as_mut_ptr(), 64);
        if len > 0 {
            let class = String::from_utf16_lossy(&buf[..len as usize]);
            if class == "Progman" || class == "WorkerW" || class == "Shell_TrayWnd" {
                return None;
            }
        }
        Some(root)
    }
}

/// Move window relative to current position with optional magnetic border snapping.
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
        let mut pt = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut pt) == 0 {
            return;
        }
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
                let (wa_x, wa_y, wa_w, wa_h) = crate::platform::work_area_at(pt.x, pt.y);
                let wa_right = wa_x + wa_w;
                let wa_bottom = wa_y + wa_h;

                // Left snap
                if (new_x - wa_x).abs() <= snap_distance {
                    new_x = wa_x;
                }
                // Right snap
                if (new_x + width - wa_right).abs() <= snap_distance {
                    new_x = wa_right - width;
                }
                // Top snap
                if (new_y - wa_y).abs() <= snap_distance {
                    if auto_maximize && new_y <= wa_y {
                        ShowWindowAsync(root, SW_MAXIMIZE);
                        return;
                    }
                    new_y = wa_y;
                }
                // Bottom snap
                if (new_y + height - wa_bottom).abs() <= snap_distance {
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
            crate::win32::set_cursor_pos(pt.x + dx, pt.y + dy);
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
                let (wa_x, wa_y, wa_w, wa_h) = crate::platform::work_area_at(pt.x, pt.y);
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
            crate::win32::set_cursor_pos(pt.x + dx, pt.y + dy);
        }
    }
}

/// Snap window to a half, quarter, center, or maximize/minimize on current monitor.
pub fn snap_window(root: HWND, quad: SnapQuadrant) {
    if root.is_null() {
        return;
    }
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        let (wa_x, wa_y, wa_w, wa_h) = crate::platform::work_area_at(pt.x, pt.y);

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

/// Roll up (shade) window to its caption height or restore saved height.
pub fn toggle_roll_up(root: HWND) -> bool {
    if root.is_null() {
        return false;
    }
    let h_key = root as usize;
    let mut g = ROLLED_WINDOWS.lock().unwrap();
    let map = g.get_or_insert_with(HashMap::new);

    unsafe {
        let mut rect: RECT = std::mem::zeroed();
        if GetWindowRect(root, &mut rect) == 0 {
            return false;
        }
        if let Some(orig_h) = map.remove(&h_key) {
            // Restore height
            SetWindowPos(
                root,
                std::ptr::null_mut(),
                rect.left,
                rect.top,
                rect.right - rect.left,
                orig_h,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
            );
            crate::osd::show_info("🗖", "Window Restored", "Restored window to full height");
            false
        } else {
            // Roll up
            let cur_h = rect.bottom - rect.top;
            map.insert(h_key, cur_h);
            let caption_h = 36;
            SetWindowPos(
                root,
                std::ptr::null_mut(),
                rect.left,
                rect.top,
                rect.right - rect.left,
                caption_h,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
            );
            crate::osd::show_info("🗕", "Window Rolled Up", "Shaded window to title bar");
            true
        }
    }
}

/// Adjust or cycle window transparency (100% -> 80% -> 60% -> 40% -> 100%).
pub fn cycle_transparency(root: HWND, _step: u8) -> u8 {
    if root.is_null() {
        return 255;
    }
    let h_key = root as usize;
    let mut g = WINDOW_OPACITY.lock().unwrap();
    let map = g.get_or_insert_with(HashMap::new);
    let cur = map.get(&h_key).copied().unwrap_or(255);

    let next = match cur {
        255 => 204, // 80%
        204 => 153, // 60%
        153 => 102, // 40%
        _ => 255,   // 100%
    };
    map.insert(h_key, next);
    set_window_opacity(root, next);
    let pct = (next as f32 / 255.0 * 100.0).round() as u32;
    crate::osd::show_info("👁", &format!("Opacity: {pct}%"), &format!("Window opacity set to {pct}%"));
    next
}

/// Set window opacity directly (255 = 100% fully opaque, removes WS_EX_LAYERED).
pub fn set_window_opacity(root: HWND, alpha: u8) {
    if root.is_null() {
        return;
    }
    unsafe {
        let style = GetWindowLongPtrW(root, GWL_EXSTYLE) as usize;
        if alpha == 255 {
            SetWindowLongPtrW(root, GWL_EXSTYLE, (style & !WS_EX_LAYERED) as isize);
            SetWindowPos(
                root,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE,
            );
        } else {
            if (style & WS_EX_LAYERED) == 0 {
                SetWindowLongPtrW(root, GWL_EXSTYLE, (style | WS_EX_LAYERED) as isize);
            }
            SetLayeredWindowAttributes(root, 0, alpha, LWA_ALPHA);
        }
    }
}

/// Lower window to the bottom of the Z-order (Send to Back).
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
        crate::osd::show_info("⬇", "Sent to Back", "Window sent to bottom of desktop stack");
    }
}

/// Close window gracefully under the cursor.
pub fn close_window(root: HWND) {
    if root.is_null() {
        return;
    }
    unsafe {
        PostMessageW(root, WM_CLOSE, 0, 0);
    }
}

/// Toggle maximize or restore window under cursor.
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

fn is_modifier_down(mod_name: &str) -> bool {
    unsafe {
        match mod_name.to_ascii_lowercase().as_str() {
            "win" | "windows" => GetAsyncKeyState(0x5B) < 0 || GetAsyncKeyState(0x5C) < 0,
            "ctrl" | "control" => GetAsyncKeyState(0x11) < 0,
            "shift" => GetAsyncKeyState(0x10) < 0,
            _ => GetAsyncKeyState(0x12) < 0, // Default: Alt (VK_MENU)
        }
    }
}

unsafe extern "system" fn altsnap_mouse_proc(code: i32, wparam: usize, lparam: isize) -> isize {
    if code < 0 {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    }

    let cfg_lock = GLOBAL_CONFIG.lock().unwrap();
    let (enabled, modifier, snap_dist, snap_edges, auto_max, step) = if let Some(arc) = cfg_lock.as_ref() {
        if let Ok(cfg) = arc.lock() {
            (
                cfg.altsnap.enabled,
                cfg.altsnap.modifier.clone(),
                cfg.altsnap.snap_distance,
                cfg.altsnap.snap_to_edges,
                cfg.altsnap.auto_maximize,
                cfg.altsnap.transparency_step,
            )
        } else {
            (true, "Alt".to_string(), 15, true, true, 20)
        }
    } else {
        (true, "Alt".to_string(), 15, true, true, 20)
    };
    drop(cfg_lock);

    if !enabled {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    }

    let mod_down = is_modifier_down(&modifier);
    let hook_struct = &*(lparam as *const MSLLHOOKSTRUCT);
    let pt = hook_struct.pt;

    if !mod_down {
        // If modifier was released while dragging, finish drag cleanly
        if let Ok(mut drag) = ACTIVE_DRAG.lock() {
            if drag.is_some() {
                *drag = None;
            }
        }
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    }

    match wparam {
        WM_LBUTTONDOWN => {
            if let Some(hwnd) = get_window_under_cursor() {
                SetForegroundWindow(hwnd);
                if let Ok(mut drag) = ACTIVE_DRAG.lock() {
                    *drag = Some(MouseDragMode::Move {
                        hwnd: hwnd as usize,
                        last_pt: (pt.x, pt.y),
                        start_pt: (pt.x, pt.y),
                    });
                }
                return 1; // Intercept click
            }
        }
        WM_RBUTTONDOWN => {
            if let Some(hwnd) = get_window_under_cursor() {
                let mut rect: RECT = std::mem::zeroed();
                if GetWindowRect(hwnd, &mut rect) != 0 {
                    let quad = detect_resize_quadrant(&rect, pt.x, pt.y);
                    if let Ok(mut drag) = ACTIVE_DRAG.lock() {
                        *drag = Some(MouseDragMode::Resize {
                            hwnd: hwnd as usize,
                            last_pt: (pt.x, pt.y),
                            quadrant: quad,
                        });
                    }
                    return 1;
                }
            }
        }
        WM_MBUTTONDOWN => {
            if let Some(hwnd) = get_window_under_cursor() {
                toggle_maximize(hwnd);
                return 1;
            }
        }
        WM_MOUSEWHEEL => {
            if let Some(hwnd) = get_window_under_cursor() {
                cycle_transparency(hwnd, step);
                return 1;
            }
        }
        WM_MOUSEMOVE => {
            let drag_opt = if let Ok(drag) = ACTIVE_DRAG.lock() {
                drag.clone()
            } else {
                None
            };

            if let Some(mode) = drag_opt {
                match mode {
                    MouseDragMode::Move { hwnd, last_pt, .. } => {
                        let dx = pt.x - last_pt.0;
                        let dy = pt.y - last_pt.1;
                        if dx != 0 || dy != 0 {
                            move_window_relative(hwnd as HWND, dx, dy, snap_dist, snap_edges, auto_max);
                            if let Ok(mut drag) = ACTIVE_DRAG.lock() {
                                if let Some(MouseDragMode::Move { last_pt: ref mut lp, .. }) = *drag {
                                    *lp = (pt.x, pt.y);
                                }
                            }
                        }
                        return 1;
                    }
                    MouseDragMode::Resize { hwnd, last_pt, quadrant } => {
                        let dx = pt.x - last_pt.0;
                        let dy = pt.y - last_pt.1;
                        if dx != 0 || dy != 0 {
                            resize_window_relative(hwnd as HWND, dx, dy, quadrant, snap_dist, snap_edges);
                            if let Ok(mut drag) = ACTIVE_DRAG.lock() {
                                if let Some(MouseDragMode::Resize { last_pt: ref mut lp, .. }) = *drag {
                                    *lp = (pt.x, pt.y);
                                }
                            }
                        }
                        return 1;
                    }
                }
            }
        }
        WM_LBUTTONUP => {
            if let Ok(mut drag) = ACTIVE_DRAG.lock() {
                if let Some(MouseDragMode::Move { .. }) = *drag {
                    *drag = None;
                    return 1;
                }
            }
        }
        WM_RBUTTONUP => {
            if let Ok(mut drag) = ACTIVE_DRAG.lock() {
                if let Some(MouseDragMode::Resize { .. }) = *drag {
                    *drag = None;
                    return 1;
                }
            }
        }
        _ => {}
    }

    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

/// Start background low-level mouse hook for AltSnap desktop mouse operations.
pub fn start_mouse_hook(running: Arc<AtomicBool>, config: Arc<Mutex<crate::config::Config>>) {
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
}
