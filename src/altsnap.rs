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

#[derive(Clone, Debug)]
enum MouseDragMode {
    Move {
        hwnd: usize,
        start_cursor: (i32, i32),
        start_window_rect: (i32, i32, i32, i32), // (left, top, width, height)
        visual_offset: (i32, i32),                // (frame_left - rect_left, frame_top - rect_top)
    },
    Resize {
        hwnd: usize,
        start_cursor: (i32, i32),
        start_window_rect: (i32, i32, i32, i32),
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

    // Fast path: if no drag is currently active, check if modifier keys are pressed.
    // (Alt = 0x12, Ctrl = 0x11, Win = 0x5B/0x5C, Shift = 0x10)
    let alt_down = (GetAsyncKeyState(0x12) as u16 & 0x8000) != 0;
    let ctrl_down = (GetAsyncKeyState(0x11) as u16 & 0x8000) != 0;
    let win_down = (GetAsyncKeyState(0x5B) as u16 & 0x8000) != 0 || (GetAsyncKeyState(0x5C) as u16 & 0x8000) != 0;
    let shift_down = (GetAsyncKeyState(0x10) as u16 & 0x8000) != 0;

    let has_active_drag = if let Ok(drag) = ACTIVE_DRAG.lock() {
        drag.is_some()
    } else {
        false
    };

    if !alt_down && !ctrl_down && !win_down && !shift_down && !has_active_drag {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    }

    // Read config settings
    let (enabled, modifier, snap_dist, snap_edges, auto_max, step) = if let Ok(guard) = GLOBAL_CONFIG.lock() {
        if let Some(arc) = guard.as_ref() {
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
        }
    } else {
        (true, "Alt".to_string(), 15, true, true, 20)
    };

    if !enabled {
        return CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam);
    }

    let mod_down = is_modifier_down(&modifier);
    let hook_struct = &*(lparam as *const MSLLHOOKSTRUCT);
    let pt = hook_struct.pt;

    match wparam {
        WM_LBUTTONDOWN if mod_down => {
            if let Some(hwnd) = get_window_under_cursor() {
                SetForegroundWindow(hwnd);
                let (win_x, win_y, win_w, win_h) = if IsZoomed(hwnd) != 0 {
                    ShowWindow(hwnd, SW_RESTORE);
                    let mut r: RECT = std::mem::zeroed();
                    GetWindowRect(hwnd, &mut r);
                    let w = (r.right - r.left).max(200);
                    let h = (r.bottom - r.top).max(150);
                    let x = pt.x - w / 2;
                    let y = pt.y - 20;
                    SetWindowPos(
                        hwnd,
                        std::ptr::null_mut(),
                        x,
                        y,
                        w,
                        h,
                        SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                    );
                    (x, y, w, h)
                } else {
                    let mut r: RECT = std::mem::zeroed();
                    GetWindowRect(hwnd, &mut r);
                    (r.left, r.top, r.right - r.left, r.bottom - r.top)
                };

                let frame = get_window_frame_rect(hwnd).unwrap_or(RECT {
                    left: win_x,
                    top: win_y,
                    right: win_x + win_w,
                    bottom: win_y + win_h,
                });
                let vis_off_x = frame.left - win_x;
                let vis_off_y = frame.top - win_y;

                if let Ok(mut drag) = ACTIVE_DRAG.lock() {
                    *drag = Some(MouseDragMode::Move {
                        hwnd: hwnd as usize,
                        start_cursor: (pt.x, pt.y),
                        start_window_rect: (win_x, win_y, win_w, win_h),
                        visual_offset: (vis_off_x, vis_off_y),
                    });
                }
                return 1; // Intercept click
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
                    let w = rect.right - rect.left;
                    let h = rect.bottom - rect.top;
                    if let Ok(mut drag) = ACTIVE_DRAG.lock() {
                        *drag = Some(MouseDragMode::Resize {
                            hwnd: hwnd as usize,
                            start_cursor: (pt.x, pt.y),
                            start_window_rect: (rect.left, rect.top, w, h),
                            quadrant: quad,
                        });
                    }
                    return 1; // Intercept right click
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
                    MouseDragMode::Move {
                        hwnd,
                        start_cursor,
                        start_window_rect,
                        visual_offset,
                    } => {
                        let raw_dx = pt.x - start_cursor.0;
                        let raw_dy = pt.y - start_cursor.1;

                        let mut outer_x = start_window_rect.0 + raw_dx;
                        let mut outer_y = start_window_rect.1 + raw_dy;
                        let width = start_window_rect.2;
                        let height = start_window_rect.3;

                        if snap_edges && snap_dist > 0 {
                            let (wa_x, wa_y, wa_w, wa_h) = crate::platform::work_area_at(pt.x, pt.y);
                            let wa_right = wa_x + wa_w;
                            let wa_bottom = wa_y + wa_h;

                            let vis_x = outer_x + visual_offset.0;
                            let vis_y = outer_y + visual_offset.1;
                            let vis_w = width - visual_offset.0 * 2;
                            let vis_h = height - visual_offset.1;

                            if (vis_x - wa_x).abs() <= snap_dist {
                                outer_x = wa_x - visual_offset.0;
                            } else if (vis_x + vis_w - wa_right).abs() <= snap_dist {
                                outer_x = wa_right - vis_w - visual_offset.0;
                            }

                            if (vis_y - wa_y).abs() <= snap_dist {
                                outer_y = wa_y - visual_offset.1;
                            } else if (vis_y + vis_h - wa_bottom).abs() <= snap_dist {
                                outer_y = wa_bottom - vis_h - visual_offset.1;
                            }
                        }

                        SetWindowPos(
                            hwnd as HWND,
                            std::ptr::null_mut(),
                            outer_x,
                            outer_y,
                            0,
                            0,
                            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                        );
                        return 1;
                    }
                    MouseDragMode::Resize {
                        hwnd,
                        start_cursor,
                        start_window_rect,
                        quadrant,
                    } => {
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

                        match quadrant {
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
                            let (wa_x, wa_y, wa_w, wa_h) = crate::platform::work_area_at(pt.x, pt.y);
                            let wa_right = wa_x + wa_w;
                            let wa_bottom = wa_y + wa_h;

                            if matches!(quadrant, ResizeQuadrant::TopLeft | ResizeQuadrant::BottomLeft | ResizeQuadrant::Left)
                                && (left - wa_x).abs() <= snap_dist
                            {
                                width += left - wa_x;
                                left = wa_x;
                            }
                            if matches!(quadrant, ResizeQuadrant::TopRight | ResizeQuadrant::BottomRight | ResizeQuadrant::Right)
                                && (left + width - wa_right).abs() <= snap_dist
                            {
                                width = wa_right - left;
                            }
                            if matches!(quadrant, ResizeQuadrant::TopLeft | ResizeQuadrant::TopRight | ResizeQuadrant::Top)
                                && (top - wa_y).abs() <= snap_dist
                            {
                                height += top - wa_y;
                                top = wa_y;
                            }
                            if matches!(quadrant, ResizeQuadrant::BottomLeft | ResizeQuadrant::BottomRight | ResizeQuadrant::Bottom)
                                && (top + height - wa_bottom).abs() <= snap_dist
                            {
                                height = wa_bottom - top;
                            }
                        }

                        SetWindowPos(
                            hwnd as HWND,
                            std::ptr::null_mut(),
                            left,
                            top,
                            width,
                            height,
                            SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
                        );
                        return 1;
                    }
                }
            }
        }
        WM_LBUTTONUP => {
            let was_dragging = if let Ok(mut drag) = ACTIVE_DRAG.lock() {
                if let Some(MouseDragMode::Move { hwnd, .. }) = drag.take() {
                    Some(hwnd)
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(hwnd) = was_dragging {
                if auto_max {
                    let (_wa_x, wa_y, _wa_w, _wa_h) = crate::platform::work_area_at(pt.x, pt.y);
                    if pt.y <= wa_y + 4 {
                        ShowWindowAsync(hwnd as HWND, SW_MAXIMIZE);
                    }
                }
                return 1;
            }
        }
        WM_RBUTTONUP => {
            let was_resizing = if let Ok(mut drag) = ACTIVE_DRAG.lock() {
                matches!(drag.take(), Some(MouseDragMode::Resize { .. }))
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
