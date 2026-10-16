//! OS-integration helpers ported from the original app's loopers/installers:
//! fullscreen detection (`FullScreenLoopDisabler`) and autostart registration
//! (`StartupTaskInstaller`, non-Steam variant).

#![allow(non_snake_case, non_upper_case_globals, dead_code)]

use std::ffi::c_void;
use std::time::{Duration, Instant};

type HWND = *mut c_void;
type HANDLE = *mut c_void;
type HKEY = *mut c_void;
type HINSTANCE = *mut c_void;
type LPARAM = isize;
type LRESULT = isize;
type LSTATUS = isize;
type DWORD = u32;
type BOOL = i32;
type WPARAM = usize;
type HCURSOR = *mut c_void;

#[repr(C)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct MONITORINFO {
    cbSize: DWORD,
    rcMonitor: RECT,
    rcWork: RECT,
    dwFlags: DWORD,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[link(name = "user32")]
extern "system" {
    fn SetWindowsHookExW(id: i32, hook: LRESULT, module: HINSTANCE, thread: DWORD) -> HANDLE;
    fn CallNextHookEx(hook: HANDLE, code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
    fn UnhookWindowsHookEx(hook: HANDLE) -> BOOL;
    fn WindowFromPoint(point: POINT) -> HWND;
    fn GetClassNameW(hwnd: HWND, buf: *mut u16, max: i32) -> i32;
    fn LoadCursorW(inst: HINSTANCE, name: usize) -> HCURSOR;
    fn GetAsyncKeyState(vk: i32) -> i16;
}

const WH_MOUSE_LL: i32 = 14;
const WH_KEYBOARD_LL: i32 = 13;
const WM_LBUTTONDOWN: i32 = 0x0201;
const WM_LBUTTONUP: i32 = 0x0202;
const WM_KEYDOWN: i32 = 0x0100;
const WM_SYSKEYDOWN: i32 = 0x0104;

/// Hotkey chords recognised by the global keyboard hook.
///
/// Modifiers are **flags**, not virtual-key codes: the VK values (0x10, 0x11,
/// 0x12, 0x5B) overlap bitwise, so OR-ing them together would make Ctrl and
/// Shift indistinguishable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HotkeyChord {
    pub modifiers: u32,
    pub vk: u32,
}

pub const MOD_SHIFT: u32 = 1 << 0;
pub const MOD_CONTROL: u32 = 1 << 1;
pub const MOD_MENU: u32 = 1 << 2;
pub const MOD_WIN: u32 = 1 << 3;

pub const VK_SHIFT: u32 = 0x10;
pub const VK_CONTROL: u32 = 0x11;
pub const VK_MENU: u32 = 0x12;
pub const VK_LWIN: u32 = 0x5B;

/// Set when the configured hotkey fires, so the main loop can act on it.
static HOTKEY_FIRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Consume the hotkey edge (true at most once per press).
pub fn take_hotkey() -> bool {
    HOTKEY_FIRED.swap(false, std::sync::atomic::Ordering::SeqCst)
}

#[repr(C)]
struct KBDLLHOOKSTRUCT {
    vk_code: u32,
    scan_code: u32,
    flags: u32,
    time: DWORD,
    dw_extra_info: usize,
}

/// Register the global hotkey chord to watch for.
static HOTKEY: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(pack_hotkey(MOD_CONTROL, 'K' as u32));

/// Pack a chord into the single u64 the atomics can hold.
pub const fn pack_hotkey(mods: u32, vk: u32) -> u64 {
    ((mods as u64) << 32) | vk as u64
}

/// Start listening for `mods` + `vk` system-wide.
///
/// The hook must swallow the key so it does not also reach the focused app —
/// otherwise the hotkey would type a stray letter every time it is pressed.
pub fn hotkey_watch_start(mods: u32, vk: u32) {
    HOTKEY.store(pack_hotkey(mods, vk), std::sync::atomic::Ordering::SeqCst);
    std::thread::spawn(|| unsafe {
        let hook = SetWindowsHookExW(
            WH_KEYBOARD_LL,
            keyboard_hook_proc as *const () as LRESULT,
            std::ptr::null_mut(),
            0,
        );
        if hook.is_null() {
            return;
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        UnhookWindowsHookEx(hook);
    });
}



#[repr(C)]
struct MSLLHOOKSTRUCT {
    pt: POINT,
    mouse_data: DWORD,
    flags: DWORD,
    time: DWORD,
    dw_extra_info: usize,
}

/// Window classes that behave as text inputs across Win32, WinUI, Chromium, and Electron.
/// Incorporates strict text classes from the native smooth scroll profile.
const EDIT_CLASSES: &[&str] = &[
    "Edit",
    "TEdit",
    "TMemo",
    "RichEdit",
    "RICHEDIT",
    "RichEdit20W",
    "RichEdit20A",
    "RICHEDIT50W",
    "ThunderTextBox",
    "RichEditD2DPT",
    "RichEditD2DT",
    "RICHEDIT50",
    "TextEdit",
    "TextEditor",
    "_WwG",
    "Notepad",
    "NotepadEdit",
    "TextBox",
    "Windows.UI.Input.InputSite",
    "Microsoft.UI.Xaml",
    "SearchBox",
    "Scintilla",
];

/// Was the last left-click inside a text input? Updated by a low-level mouse
/// hook installed by [`mouse_watch_start`].
static CLICK_IN_TEXT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static LAST_LCLICK_TIME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LAST_CONTROLLER_TIME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LAST_CLICK_WAS_CONTROLLER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static PHYSICAL_MOUSE_CLICKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn note_controller_activity() {
    let now_ms = (crate::steady_seconds() * 1000.0) as u64;
    LAST_CONTROLLER_TIME.store(now_ms, std::sync::atomic::Ordering::Relaxed);
}

pub fn controller_used_within_ms(ms: u64) -> bool {
    let last = LAST_CONTROLLER_TIME.load(std::sync::atomic::Ordering::Relaxed);
    if last == 0 {
        return false;
    }
    let now = (crate::steady_seconds() * 1000.0) as u64;
    now.saturating_sub(last) <= ms
}

pub fn last_click_was_controller() -> bool {
    LAST_CLICK_WAS_CONTROLLER.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn take_physical_mouse_click() -> bool {
    PHYSICAL_MOUSE_CLICKED.swap(false, std::sync::atomic::Ordering::SeqCst)
}

pub fn last_click_within_ms(ms: u64) -> bool {
    let last = LAST_LCLICK_TIME.load(std::sync::atomic::Ordering::SeqCst);
    if last == 0 {
        return false;
    }
    let now = (crate::steady_seconds() * 1000.0) as u64;
    now.saturating_sub(last) <= ms
}

pub fn clear_last_click() {
    LAST_LCLICK_TIME.store(0, std::sync::atomic::Ordering::SeqCst);
}

#[repr(C)]
struct CURSORINFO {
    cbSize: DWORD,
    flags: DWORD,
    hCursor: HCURSOR,
    ptScreenPos: POINT,
}

#[repr(C)]
struct ICONINFO {
    fIcon: BOOL,
    xHotspot: DWORD,
    yHotspot: DWORD,
    hbmMask: *mut c_void,
    hbmColor: *mut c_void,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct VARIANT {
    vt: u16,
    w_reserved1: u16,
    w_reserved2: u16,
    w_reserved3: u16,
    data: [u8; 16],
}

#[link(name = "oleacc")]
extern "system" {
    fn AccessibleObjectFromPoint(
        pt: POINT,
        ppacc: *mut *mut c_void,
        pvarChild: *mut VARIANT,
    ) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn DeleteObject(ho: *mut c_void) -> BOOL;
}

pub fn is_cursor_ibeam() -> bool {
    #[link(name = "user32")]
    extern "system" {
        fn GetCursorInfo(pci: *mut CURSORINFO) -> BOOL;
        fn LoadCursorW(inst: HINSTANCE, name: usize) -> HCURSOR;
    }
    unsafe {
        let mut ci: CURSORINFO = std::mem::zeroed();
        ci.cbSize = std::mem::size_of::<CURSORINFO>() as DWORD;
        if GetCursorInfo(&mut ci) != 0 && ci.flags != 0 && !ci.hCursor.is_null() {
            // Direct handle match with system default IDC_IBEAM (32513)
            let ibeam = LoadCursorW(std::ptr::null_mut(), 32513);
            if !ibeam.is_null() && ci.hCursor == ibeam {
                return true;
            }
        }
        false
    }
}

/// Query Microsoft Active Accessibility (MSAA) at screen coordinates `pt`.
/// Catches text boxes in Discord, Chrome, Edge, WinUI, VS Code, Slack, etc.
pub fn check_accessible_text(pt: POINT) -> bool {
    unsafe {
        let mut pacc: *mut c_void = std::ptr::null_mut();
        let mut var_child: VARIANT = std::mem::zeroed();
        let hr = AccessibleObjectFromPoint(pt, &mut pacc, &mut var_child);
        if hr == 0 && !pacc.is_null() {
            let vtable = *(pacc as *mut *mut usize);
            let get_acc_role_fn: unsafe extern "system" fn(*mut c_void, VARIANT, *mut VARIANT) -> i32 =
                std::mem::transmute(*vtable.add(13));
            let get_acc_state_fn: unsafe extern "system" fn(*mut c_void, VARIANT, *mut VARIANT) -> i32 =
                std::mem::transmute(*vtable.add(14));
            let release_fn: unsafe extern "system" fn(*mut c_void) -> u32 =
                std::mem::transmute(*vtable.add(2));

            let mut role_var: VARIANT = std::mem::zeroed();
            let hr_role = get_acc_role_fn(pacc, var_child, &mut role_var);

            let mut state_var: VARIANT = std::mem::zeroed();
            let hr_state = get_acc_state_fn(pacc, var_child, &mut state_var);

            release_fn(pacc);

            if hr_role == 0 && role_var.vt == 3 {
                let role = i32::from_ne_bytes(role_var.data[0..4].try_into().unwrap_or([0; 4]));
                let state = if hr_state == 0 && state_var.vt == 3 {
                    i32::from_ne_bytes(state_var.data[0..4].try_into().unwrap_or([0; 4]))
                } else {
                    0
                };

                // STATE_SYSTEM_READONLY is 0x00000040.
                // If it is read-only, it is a static label, file name, or desktop icon - NOT an editable field!
                if state & 0x40 != 0 {
                    return false;
                }

                // ROLE_SYSTEM_TEXT = 42, ROLE_SYSTEM_COMBOBOX = 46
                // Must be an editable (non-readonly) text input or combobox.
                if role == 42 || role == 46 {
                    return true;
                }
            }
        }
        false
    }
}

extern "system" fn mouse_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use std::sync::atomic::Ordering;
    let msg = wparam as i32;
    if code == 0 && lparam != 0 {
        unsafe {
            let hs = &*(lparam as *const MSLLHOOKSTRUCT);
            // Ignore any clicks that land on our own virtual keyboard overlay.
            let under = WindowFromPoint(hs.pt);
            let ov = overlay_hwnd() as HWND;
            let on_overlay = !under.is_null() && !ov.is_null() && (under == ov || GetParent(under) == ov);
            if !on_overlay && (msg == WM_LBUTTONDOWN || msg == WM_LBUTTONUP) {
                let is_injected = (hs.flags & 1) != 0;
                let has_magic = hs.dw_extra_info == crate::win32::CONTROLLER_EXTRA_INFO;
                let recent_ctrl = controller_used_within_ms(2500);
                let is_controller = has_magic || (is_injected && recent_ctrl);

                LAST_CLICK_WAS_CONTROLLER.store(is_controller, Ordering::SeqCst);

                if is_controller {
                    if msg == WM_LBUTTONDOWN {
                        push_foreground();
                    }
                    let now_ms = (crate::steady_seconds() * 1000.0) as u64;
                    LAST_LCLICK_TIME.store(now_ms, Ordering::SeqCst);
                    if is_cursor_ibeam() || point_is_text_field(hs.pt) {
                        CLICK_IN_TEXT.store(true, Ordering::SeqCst);
                    }
                } else {
                    // Real physical mouse clicked!
                    // Immediately clear text click flag so OSK never opens on physical mouse click!
                    CLICK_IN_TEXT.store(false, Ordering::SeqCst);
                    LAST_LCLICK_TIME.store(0, Ordering::SeqCst);
                    if msg == WM_LBUTTONDOWN {
                        PHYSICAL_MOUSE_CLICKED.store(true, Ordering::SeqCst);
                    }
                }
            }
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

extern "system" fn keyboard_hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    use std::sync::atomic::Ordering;
    let msg = wparam as i32;
    if code == 0
        && (msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN)
        && lparam != 0
    {
        let want = HOTKEY.load(Ordering::Relaxed);
        let want_mods = (want >> 32) as u32;
        let want_vk = (want & 0xFFFF_FFFF) as u32;
        let hs = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        if hs.vk_code == want_vk && current_modifiers() == want_mods {
            HOTKEY_FIRED.store(true, Ordering::SeqCst);
            // Returning 1 swallows the key so the focused app never sees it.
            return 1;
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// The modifier keys currently held, in the bit layout `HotkeyChord` uses.
///
/// `GetAsyncKeyState` is used rather than tracking the hook's own events: it is
/// correct even when a modifier was pressed before the hook existed, and this
/// runs on a hotkey press only.
fn current_modifiers() -> u32 {
    unsafe {
        let mut mods = 0;
        if GetAsyncKeyState(VK_SHIFT as i32) < 0 {
            mods |= MOD_SHIFT;
        }
        if GetAsyncKeyState(VK_CONTROL as i32) < 0 {
            mods |= MOD_CONTROL;
        }
        if GetAsyncKeyState(VK_MENU as i32) < 0 {
            mods |= MOD_MENU;
        }
        if GetAsyncKeyState(VK_LWIN as i32) < 0 {
            mods |= MOD_WIN;
        }
        mods
    }
}

/// Decide whether a click at `pt` landed on something that accepts typed text.
/// Avoids slow cross-process COM (`AccessibleObjectFromPoint`) or false-positive
/// I-Beam checks inside the low-level mouse hook.
fn point_is_text_field(pt: POINT) -> bool {
    unsafe {
        let under = WindowFromPoint(pt);
        if under.is_null() {
            return false;
        }
        let ov = overlay_hwnd() as HWND;
        if !ov.is_null() && (under == ov || GetParent(under) == ov) {
            return false;
        }
        let mut pid: DWORD = 0;
        GetWindowThreadProcessId(under, &mut pid);
        if pid == std::process::id() {
            return false;
        }
    }

    let class = window_class_at(pt);
    let class_name = class.as_deref().unwrap_or("");

    // 1. If click lands inside Desktop or Explorer file list, it's selecting files,
    // NOT entering text (unless a real edit caret is active, e.g. renaming a file).
    if class_name == "SysListView32"
        || class_name == "SHELLDLL_DefView"
        || class_name == "DirectUIHWND"
        || class_name == "Progman"
        || class_name == "WorkerW"
        || class_name == "CabinetWClass"
        || class_name == "ShellTabWindowClass"
        || class_name == "XamlExplorerHostIslandWindow"
    {
        return caret_active();
    }

    // 2. Known native edit class.
    if EDIT_CLASSES.iter().any(|e| class_name.starts_with(e)) {
        return true;
    }

    // 3. A real caret on the foreground thread.
    if caret_active() {
        return true;
    }

    // 4. Accessibility role check (catches modern apps: Discord, Chrome, WinUI, VS Code, Slack, Word)
    if check_accessible_text(pt) {
        return true;
    }
    false
}

/// Is the currently focused window sitting under the cursor?
///
/// Catches Chromium/Electron search boxes (Brave, Discord, in-app browsers) that
/// own keyboard focus after a click but expose neither an edit class nor a caret.
pub fn focused_window_under_cursor() -> bool {
    unsafe {
        let mut cur: POINT = std::mem::zeroed();
        if GetCursorPos(&mut cur) == 0 {
            return false;
        }
        let under = WindowFromPoint(cur);
        if under.is_null() || under == overlay_hwnd() as HWND {
            return false;
        }
        let mut gi: GUITHREADINFO = std::mem::zeroed();
        gi.cbSize = std::mem::size_of::<GUITHREADINFO>() as DWORD;
        if GetGUIThreadInfo(0, &mut gi) == 0 || gi.hwndFocus.is_null() {
            return false;
        }
        // The focused control must be an ancestor of (or equal to) the window
        // under the cursor.
        let mut root = under;
        loop {
            if root == gi.hwndFocus {
                return true;
            }
            let parent = GetParent(root);
            if parent.is_null() {
                return false;
            }
            root = parent;
        }
    }
}

fn window_class_at(pt: POINT) -> Option<String> {
    unsafe {
        let hwnd = WindowFromPoint(pt);
        if hwnd.is_null() {
            return None;
        }
        let mut buf = [0u16; 128];
        let n = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
        if n <= 0 {
            return None;
        }
        Some(String::from_utf16_lossy(&buf[..n as usize]))
    }
}

/// Install a global low-level mouse hook that records left-clicks landing in
/// text boxes. Returns the thread handle marker (the hook lives on this thread's
/// message loop, which the tray thread already provides).
pub fn mouse_watch_start() {
    std::thread::spawn(|| unsafe {
        attach_to_interactive_desktop();
        let hook = SetWindowsHookExW(
            WH_MOUSE_LL,
            mouse_hook_proc as *const () as LRESULT,
            std::ptr::null_mut(),
            0,
        );
        if hook.is_null() {
            log_debug("[mouse_watch] Failed to install WH_MOUSE_LL hook");
            return;
        }
        log_debug("[mouse_watch] WH_MOUSE_LL hook successfully installed");
        let mut msg = std::mem::zeroed::<MSG>();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        UnhookWindowsHookEx(hook);
    });
}

/// Consume the "clicked inside a text box" edge (true at most once per click).
pub fn take_text_click() -> bool {
    CLICK_IN_TEXT.swap(false, std::sync::atomic::Ordering::SeqCst)
}

#[repr(C)]
struct MSG {
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    time: DWORD,
    pt: POINT,
    l_private: DWORD,
}

#[link(name = "user32")]
extern "system" {
    fn GetMessageW(msg: *mut MSG, hwnd: HWND, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const MSG) -> i32;
    fn DispatchMessageW(msg: *const MSG) -> LRESULT;
}

#[link(name = "user32")]
extern "system" {
    fn GetForegroundWindow() -> HWND;
    fn GetWindowRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    fn MonitorFromWindow(hwnd: HWND, flags: DWORD) -> *mut c_void;
    fn GetMonitorInfoW(mon: *mut c_void, info: *mut MONITORINFO) -> BOOL;
    fn GetWindowThreadProcessId(hwnd: HWND, pid: *mut DWORD) -> DWORD;
    fn GetSystemMetrics(index: i32) -> i32;
    fn GetGUIThreadInfo(idThread: DWORD, gui: *mut GUITHREADINFO) -> BOOL;
    fn FindWindowW(class: *const u16, window: *const u16) -> HWND;
    fn GetCursorPos(point: *mut POINT) -> BOOL;
    fn GetWindowLongPtrW(hwnd: HWND, index: i32) -> isize;
    fn SetWindowLongPtrW(hwnd: HWND, index: i32, new: isize) -> isize;
    fn GetKeyboardLayout(idThread: DWORD) -> usize;
    fn GetWindowTextW(hwnd: HWND, lpString: *mut u16, nMaxCount: i32) -> i32;
}

const GWL_EXSTYLE: i32 = -20;
const WS_EX_TOPMOST: isize = 0x0000_0008;
const WS_EX_TOOLWINDOW: isize = 0x0000_0080;
const WS_EX_NOACTIVATE: isize = 0x0800_0000;

/// `WM_MOUSEACTIVATE` / `MA_NOACTIVATE`: refuse activation but still deliver the
/// click, which is what an on-screen keyboard needs.
const WM_MOUSEACTIVATE: u32 = 0x0021;
const MA_NOACTIVATE: isize = 3;

const SW_HIDE: i32 = 0;
const SW_SHOWNOACTIVATE: i32 = 4;
const SWP_NOSIZE: u32 = 0x0001;

#[link(name = "user32")]
extern "system" {
    fn ShowWindow(hwnd: HWND, cmd: i32) -> BOOL;
    fn SetWindowPos(
        hwnd: HWND,
        insert_after: HWND,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> BOOL;
    fn InvalidateRect(hwnd: HWND, rect: *const RECT, erase: BOOL) -> BOOL;
    fn GetAncestor(hwnd: HWND, flags: u32) -> HWND;
    fn IsZoomed(hwnd: HWND) -> BOOL;
}

const SWP_NOZORDER: u32 = 0x0004;
const SWP_NOACTIVATE: u32 = 0x0010;
const GA_ROOT: u32 = 2;
const SW_RESTORE: i32 = 9;

/// Native smooth window move / drag under cursor (AltSnap replacement).
///
/// Displaces the top-level window under the pointer by `(dx, dy)` and keeps the
/// mouse cursor pinned relative to the window geometry.
pub fn move_window_at_cursor(dx: i32, dy: i32) {
    if dx == 0 && dy == 0 {
        return;
    }
    unsafe {
        let mut pt = POINT { x: 0, y: 0 };
        if GetCursorPos(&mut pt) == 0 {
            return;
        }
        let under = WindowFromPoint(pt);
        if under.is_null() {
            return;
        }
        let root = GetAncestor(under, GA_ROOT);
        if root.is_null() {
            return;
        }
        let ov = overlay_hwnd() as HWND;
        if !ov.is_null() && (root == ov || under == ov) {
            return;
        }
        // Exclude system elements (Desktop, Shell, Taskbar)
        let mut buf = [0u16; 64];
        let len = GetClassNameW(root, buf.as_mut_ptr(), 64);
        if len > 0 {
            let class = String::from_utf16_lossy(&buf[..len as usize]);
            if class == "Progman" || class == "WorkerW" || class == "Shell_TrayWnd" {
                return;
            }
        }
        if IsZoomed(root) != 0 {
            ShowWindow(root, SW_RESTORE);
        }
        let mut rect: RECT = std::mem::zeroed();
        if GetWindowRect(root, &mut rect) != 0 {
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            SetWindowPos(
                root,
                std::ptr::null_mut(),
                rect.left + dx,
                rect.top + dy,
                width,
                height,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            );
            crate::win32::set_cursor_pos(pt.x + dx, pt.y + dy);
        }
    }
}

fn find_target_hwnd(title: &str) -> HWND {
    if title == "CCVKbd" {
        let h = overlay_hwnd() as HWND;
        if !h.is_null() && unsafe { IsWindow(h) != 0 } {
            return h;
        }
    }
    let wide = to_wide(title);
    let hwnd = unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) };
    if !hwnd.is_null() && title == "CCVKbd" {
        note_overlay(hwnd as usize);
    }
    hwnd
}

/// Move/resize the overlay so it hugs the content and sits at the bottom centre
/// of the screen. Returns true when the HWND was found.
pub fn place_window(title: &str, x: i32, y: i32, w: i32, h: i32) -> bool {
    unsafe {
        let hwnd = find_target_hwnd(title);
        if hwnd.is_null() {
            return false;
        }
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            w,
            h,
            SWP_NOZORDER | SWP_NOACTIVATE,
        );
        true
    }
}

/// Move the overlay without touching its size.
///
/// `SetWindowPos` is *not* a no-op for size unless `SWP_NOSIZE` is passed: a
/// plain call resizes the window, which makes winit re-run its layout and the
/// keyboard visibly stutters while being dragged.
pub fn move_window(title: &str, x: i32, y: i32) -> bool {
    unsafe {
        let hwnd = find_target_hwnd(title);
        if hwnd.is_null() {
            return false;
        }
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            x,
            y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        true
    }
}

/// Is the foreground window covering its entire monitor?
///
/// This is the exact test the original .NET app used (`SystemActiveWindowMonitor
/// .GetIsFullScreen`): compare the foreground window rect against the rect of the
/// monitor it sits on. It is what lets the keyboard appear over full-screen games,
/// where no Win32 edit control or caret exists to detect.
pub fn is_fullscreen() -> bool {
    unsafe {
        let w = GetForegroundWindow();
        if w.is_null() {
            return false;
        }

        // Ignore desktop background and taskbar shells
        let mut class_buf = [0u16; 64];
        let class_len = GetClassNameW(w, class_buf.as_mut_ptr(), class_buf.len() as i32);
        if class_len > 0 {
            let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]);
            if class_name == "Progman"
                || class_name == "WorkerW"
                || class_name == "Shell_TrayWnd"
                || class_name == "Windows.UI.Core.CoreWindow"
            {
                return false;
            }
        }

        let mut rc: RECT = std::mem::zeroed();
        if GetWindowRect(w, &mut rc) == 0 {
            return false;
        }
        // MONITOR_DEFAULTTONEAREST
        let mon = MonitorFromWindow(w, 2);
        if mon.is_null() {
            return false;
        }
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as DWORD;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return false;
        }
        let m = mi.rcMonitor;
        (rc.right - rc.left) == (m.right - m.left)
            && (rc.bottom - rc.top) == (m.bottom - m.top)
            && rc.top == m.top
            && rc.left == m.left
    }
}

/// eframe does not honour `ViewportCommand::Visible` on the native Windows
/// window, so show/hide is driven directly through Win32. Returns true when
/// the window was found (and therefore the style/visibility was applied).
pub fn set_window_visible(title: &str, visible: bool) -> bool {
    unsafe {
        let hwnd = find_target_hwnd(title);
        if hwnd.is_null() {
            return false;
        }
        ShowWindow(hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        if visible {
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
        true
    }
}

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(
        hwnd: HWND,
        dwAttribute: DWORD,
        pvAttribute: *const c_void,
        cbAttribute: DWORD,
    ) -> i32;
}

/// Force a window (found by exact title) to be a topmost, tool, no-activate
/// overlay so clicking its keys never steals focus from the target app.
pub fn enforce_overlay_style(title: &str) -> bool {
    unsafe {
        let hwnd = find_target_hwnd(title);
        if hwnd.is_null() {
            return false;
        }
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        // WS_EX_NOACTIVATE keeps the user's text box focused while the overlay is
        // on screen.
        let want = style | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
        if want != style {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, want);
        }
        // Enable Windows 11 immersive dark mode and rounded corner preference
        let dark_mode: BOOL = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            20, // DWMWA_USE_IMMERSIVE_DARK_MODE
            &dark_mode as *const _ as *const c_void,
            std::mem::size_of::<BOOL>() as DWORD,
        );
        let corner_round: u32 = 2; // DWMWCP_ROUND
        let _ = DwmSetWindowAttribute(
            hwnd,
            33, // DWMWA_WINDOW_CORNER_PREFERENCE
            &corner_round as *const _ as *const c_void,
            std::mem::size_of::<u32>() as DWORD,
        );
        // Enable Windows 11 Acrylic Backdrop (DWMWA_SYSTEMBACKDROP_TYPE = 38, DWMSBT_TRANSIENTWINDOW = 3)
        let backdrop_type: u32 = 3;
        let _ = DwmSetWindowAttribute(
            hwnd,
            38, // DWMWA_SYSTEMBACKDROP_TYPE
            &backdrop_type as *const _ as *const c_void,
            std::mem::size_of::<u32>() as DWORD,
        );
        true
    }
}

/// Apply Windows 11 immersive dark mode, rounded corners, and Mica backdrop to a standard application window.
pub fn enforce_window_modern_theme(title: &str) -> bool {
    unsafe {
        let hwnd = find_target_hwnd(title);
        if hwnd.is_null() {
            return false;
        }
        let dark_mode: BOOL = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            20, // DWMWA_USE_IMMERSIVE_DARK_MODE
            &dark_mode as *const _ as *const c_void,
            std::mem::size_of::<BOOL>() as DWORD,
        );
        let corner_round: u32 = 2; // DWMWCP_ROUND
        let _ = DwmSetWindowAttribute(
            hwnd,
            33, // DWMWA_WINDOW_CORNER_PREFERENCE
            &corner_round as *const _ as *const c_void,
            std::mem::size_of::<u32>() as DWORD,
        );
        // Enable Windows 11 Mica Backdrop (DWMWA_SYSTEMBACKDROP_TYPE = 38, DWMSBT_MAINWINDOW = 2)
        let backdrop_mica: u32 = 2;
        let _ = DwmSetWindowAttribute(
            hwnd,
            38, // DWMWA_SYSTEMBACKDROP_TYPE
            &backdrop_mica as *const _ as *const c_void,
            std::mem::size_of::<u32>() as DWORD,
        );
        true
    }
}

#[repr(C)]
struct GUITHREADINFO {
    cbSize: DWORD,
    flags: DWORD,
    hwndActive: HWND,
    hwndFocus: HWND,
    hwndCapture: HWND,
    hwndMenuOwner: HWND,
    hwndMoveSize: HWND,
    hwndCaret: HWND,
    rcCaret: RECT,
}

pub const SM_CXSCREEN: i32 = 0;
pub const SM_CYSCREEN: i32 = 1;
const SPI_GETWORKAREA: u32 = 0x0030;

#[link(name = "user32")]
extern "system" {
    fn SystemParametersInfoW(uiAction: u32, uiParam: u32, pvParam: *mut c_void, fWinIni: u32) -> BOOL;
}

/// Primary screen size in pixels: `(width, height)`.
pub fn screen_size() -> (i32, i32) {
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

/// Primary monitor work area in pixels (excluding taskbars and docked toolbars): `(x, y, width, height)`.
pub fn work_area() -> (i32, i32, i32, i32) {
    unsafe {
        let mut rc: RECT = std::mem::zeroed();
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut rc as *mut _ as *mut c_void, 0) != 0 {
            let w = rc.right - rc.left;
            let h = rc.bottom - rc.top;
            if w > 100 && h > 100 {
                return (rc.left, rc.top, w, h);
            }
        }
        let (sw, sh) = screen_size();
        (0, 0, sw, sh)
    }
}

/// The current foreground window handle (as usize) — changes when focus moves.
pub fn foreground_hwnd() -> usize {
    unsafe { GetForegroundWindow() as usize }
}

/// Remember the app that owns keyboard focus, then restore it if the overlay ever
/// took activation.
pub fn push_foreground() -> usize {
    let h = unsafe { GetForegroundWindow() as usize };
    if h == 0 {
        return 0;
    }
    if overlay_hwnd() != h {
        FOREGROUND.store(h, std::sync::atomic::Ordering::SeqCst);
    }
    h
}

#[link(name = "user32")]
extern "system" {
    fn GetParent(hwnd: HWND) -> HWND;
}

/// Cached handle of the on-screen keyboard window, so focus bookkeeping can skip
/// it without a `FindWindow` call on every keystroke.
static OVERLAY_HWND: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Handle of the overlay window (looked up lazily by title).
pub fn overlay_hwnd() -> usize {
    let cached = OVERLAY_HWND.load(std::sync::atomic::Ordering::SeqCst);
    if cached != 0 {
        return cached;
    }
    let wide = to_wide("CCVKbd");
    let h = unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) as usize };
    if h != 0 {
        OVERLAY_HWND.store(h, std::sync::atomic::Ordering::SeqCst);
    }
    h
}

/// Record the overlay's real handle so focus bookkeeping can skip it.
pub fn note_overlay(hwnd: usize) {
    if hwnd != 0 {
        OVERLAY_HWND.store(hwnd, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Give focus back to the window captured by [`push_foreground`] ONLY if our
/// overlay window accidentally became the foreground window.
///
/// Never uses `AttachThreadInput` or cross-process `SetFocus`, which can stall
/// or freeze the UI thread when closing the keyboard.
pub fn restore_foreground() {
    let h = FOREGROUND.load(std::sync::atomic::Ordering::SeqCst);
    if h == 0 {
        return;
    }
    unsafe {
        let fg = GetForegroundWindow() as usize;
        let ov = overlay_hwnd();
        // Since the overlay has WS_EX_NOACTIVATE, the target window already has
        // focus 99.9% of the time. Only intervene if the overlay itself became foreground.
        if (fg == 0 || (ov != 0 && fg == ov)) && IsWindow(h as HWND) != 0 {
            SetForegroundWindow(h as HWND);
        }
    }
}

/// Explicitly set foreground window to a specific HWND if valid.
pub fn set_foreground_window_force(hwnd: usize) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        let cur = GetForegroundWindow() as usize;
        if cur == hwnd {
            return;
        }
        if IsWindow(hwnd as HWND) != 0 {
            SetForegroundWindow(hwnd as HWND);
        }
    }
}

static FOREGROUND: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Query the currently active Windows keyboard layout / input language for the foreground window.
/// Returns `Some(Lang::Ar)` if an Arabic layout is active, or `Some(Lang::En)` if English / Latin.
#[cfg(feature = "gui")]
pub fn active_input_language() -> Option<crate::vkeyboard::Lang> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid: DWORD = 0;
        let tid = GetWindowThreadProcessId(hwnd, &mut pid);
        if tid == 0 {
            return None;
        }
        let hkl = GetKeyboardLayout(tid);
        if hkl == 0 {
            return None;
        }
        let lang_id = (hkl & 0xFFFF) as u16;
        let primary = lang_id & 0x03FF;
        if primary == 0x01 {
            Some(crate::vkeyboard::Lang::Ar)
        } else {
            Some(crate::vkeyboard::Lang::En)
        }
    }
}

/// The child control that had keyboard focus, so it can be restored exactly.
static FOREGROUND_FOCUS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[link(name = "user32")]
extern "system" {
    fn IsWindow(hwnd: HWND) -> BOOL;
    fn SetForegroundWindow(hwnd: HWND) -> BOOL;
}

/// Heuristic: is a text caret (insertion point) currently active on the
/// foreground thread? A visible caret strongly implies a focused text box.
pub fn caret_active() -> bool {
    unsafe {
        let mut gi: GUITHREADINFO = std::mem::zeroed();
        gi.cbSize = std::mem::size_of::<GUITHREADINFO>() as DWORD;
        if GetGUIThreadInfo(0, &mut gi) == 0 {
            return false;
        }
        !gi.hwndCaret.is_null()
            && (gi.rcCaret.right - gi.rcCaret.left).abs() > 0
            && (gi.rcCaret.bottom - gi.rcCaret.top).abs() > 0
    }
}

#[link(name = "user32")]
extern "system" {
    fn ClientToScreen(hwnd: HWND, point: *mut POINT) -> BOOL;
}

/// Retrieve the screen-space (x, y) coordinates of the active text caret if available.
pub fn caret_screen_pos() -> Option<(i32, i32)> {
    unsafe {
        let mut gi: GUITHREADINFO = std::mem::zeroed();
        gi.cbSize = std::mem::size_of::<GUITHREADINFO>() as DWORD;

        // Try querying foreground thread directly
        let mut has_caret = GetGUIThreadInfo(0, &mut gi) != 0 && !gi.hwndCaret.is_null();
        if !has_caret {
            let fg = GetForegroundWindow();
            if !fg.is_null() {
                let tid = GetWindowThreadProcessId(fg, std::ptr::null_mut());
                if tid != 0 && GetGUIThreadInfo(tid, &mut gi) != 0 && !gi.hwndCaret.is_null() {
                    has_caret = true;
                }
            }
        }

        if !has_caret || gi.hwndCaret.is_null() {
            return None;
        }

        let mut pt = POINT {
            x: gi.rcCaret.left,
            y: gi.rcCaret.top,
        };
        if ClientToScreen(gi.hwndCaret, &mut pt) != 0 {
            Some((pt.x, pt.y))
        } else {
            None
        }
    }
}

/// Read Unicode text currently stored in the Windows clipboard.
pub fn get_clipboard_text() -> Option<String> {
    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(hWndNewOwner: *mut c_void) -> BOOL;
        fn CloseClipboard() -> BOOL;
        fn GetClipboardData(uFormat: u32) -> *mut c_void;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalLock(hMem: *mut c_void) -> *mut c_void;
        fn GlobalUnlock(hMem: *mut c_void) -> BOOL;
    }
    const CF_UNICODETEXT: u32 = 13;
    unsafe {
        if OpenClipboard(std::ptr::null_mut()) == 0 {
            return None;
        }
        let hmem = GetClipboardData(CF_UNICODETEXT);
        if hmem.is_null() {
            CloseClipboard();
            return None;
        }
        let ptr = GlobalLock(hmem) as *const u16;
        if ptr.is_null() {
            CloseClipboard();
            return None;
        }
        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(ptr, len);
        let s = String::from_utf16_lossy(slice);
        GlobalUnlock(hmem);
        CloseClipboard();
        Some(s)
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn OpenProcess(access: DWORD, inherit: BOOL, pid: DWORD) -> HANDLE;
    fn CloseHandle(h: HANDLE) -> BOOL;
    fn QueryFullProcessImageNameW(
        h: HANDLE,
        flags: DWORD,
        exe: *mut u16,
        size: *mut DWORD,
    ) -> BOOL;
}

#[link(name = "advapi32")]
extern "system" {
    fn RegCreateKeyExW(
        root: HKEY,
        subkey: *const u16,
        reserved: DWORD,
        class: *mut u16,
        options: DWORD,
        samDesired: DWORD,
        security: *mut c_void,
        result: *mut HKEY,
        disposition: *mut DWORD,
    ) -> LSTATUS;
    fn RegSetValueExW(
        key: HKEY,
        value: *const u16,
        reserved: DWORD,
        kind: DWORD,
        data: *const u8,
        cbData: DWORD,
    ) -> LSTATUS;
    fn RegDeleteValueW(key: HKEY, value: *const u16) -> LSTATUS;
    fn RegCloseKey(key: HKEY) -> LSTATUS;
}

const PROCESS_QUERY_LIMITED_INFORMATION: DWORD = 0x1000;
const MONITOR_DEFAULTTONEAREST: DWORD = 2;
const HKEY_CURRENT_USER: HKEY = 0x8000_0001 as HKEY;
const KEY_WRITE: DWORD = 0x20006;
const REG_SZ: DWORD = 1;
const ERROR_SUCCESS: LSTATUS = 0;

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Best-effort foreground-process executable file name (lowercased), or None.
pub fn foreground_process_name() -> Option<String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut pid: DWORD = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if pid == 0 {
            return None;
        }
        let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if proc.is_null() {
            return None;
        }
        let mut buf = [0u16; 512];
        let mut size = buf.len() as DWORD;
        let ok = QueryFullProcessImageNameW(proc, 0, buf.as_mut_ptr(), &mut size);
        CloseHandle(proc);
        if ok == 0 {
            return None;
        }
        let s = String::from_utf16_lossy(&buf[..size as usize]);
        let name = std::path::Path::new(&s)
            .file_name()
            .map(|f| f.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }
}

/// True when the foreground window covers its whole monitor (borderless
/// fullscreen heuristic). Cached for ~250ms to keep the 125Hz loop cheap.
pub fn is_fullscreen_cached() -> bool {
    use std::sync::OnceLock;
    use std::sync::Mutex;
    struct Cache {
        at: Instant,
        value: bool,
    }
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cell = CACHE.get_or_init(|| {
        Mutex::new(Cache { at: Instant::now() - Duration::from_secs(10), value: false })
    });
    let Ok(mut g) = cell.lock() else {
        return compute_fullscreen();
    };
    if g.at.elapsed() > Duration::from_millis(250) {
        g.value = compute_fullscreen();
        g.at = Instant::now();
    }
    g.value
}

#[link(name = "shell32")]
extern "system" {
    fn SHQueryUserNotificationState(pquns: *mut i32) -> i32;
}

const QUNS_RUNNING_D3D_FULL_SCREEN: i32 = 3;

fn compute_fullscreen() -> bool {
    // Method 1: Exclusive D3D fullscreen (SHQueryUserNotificationState).
    let d3d_fs = unsafe {
        let mut state: i32 = 0;
        SHQueryUserNotificationState(&mut state) == 0 && state == QUNS_RUNNING_D3D_FULL_SCREEN
    };
    if d3d_fs {
        return true;
    }
    // Method 2: Borderless fullscreen — the foreground window covers its entire
    // monitor.  This catches the majority of modern games that use borderless
    // windowed mode rather than exclusive fullscreen.
    is_fullscreen()
}

/// True when Windows Alt+Tab or Task View switcher is the active foreground window.
/// Cached for ~60ms to keep the 125Hz loop cheap while remaining ultra-responsive.
pub fn is_task_switcher_cached() -> bool {
    use std::sync::OnceLock;
    use std::sync::Mutex;
    struct Cache {
        at: Instant,
        value: bool,
    }
    static CACHE: OnceLock<Mutex<Cache>> = OnceLock::new();
    let cell = CACHE.get_or_init(|| {
        Mutex::new(Cache { at: Instant::now() - Duration::from_secs(10), value: false })
    });
    let Ok(mut g) = cell.lock() else {
        return compute_task_switcher();
    };
    if g.at.elapsed() > Duration::from_millis(60) {
        g.value = compute_task_switcher();
        g.at = Instant::now();
    }
    g.value
}

fn compute_task_switcher() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return false;
        }
        let mut class_buf = [0u16; 128];
        let len = GetClassNameW(hwnd, class_buf.as_mut_ptr(), class_buf.len() as i32);
        if len > 0 {
            let class_name = String::from_utf16_lossy(&class_buf[..len as usize]);
            if class_name == "MultitaskingViewFrame"
                || class_name == "TaskSwitcherWnd"
                || class_name == "TaskSwitcherOverlayWnd"
            {
                return true;
            }
            if class_name == "XamlExplorerHostIslandWindow"
                || class_name == "Windows.UI.Core.CoreWindow"
                || class_name == "ApplicationFrameWindow"
            {
                let mut title_buf = [0u16; 128];
                let tlen = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32);
                if tlen > 0 {
                    let title = String::from_utf16_lossy(&title_buf[..tlen as usize]);
                    if title.contains("Task View")
                        || title.contains("Task Switching")
                        || title.contains("Switch to")
                        || title.contains("AltTab")
                        || title.contains("تبديل المهام")
                    {
                        return true;
                    }
                }
            }
        }
        false
    }
}

const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const VALUE_NAME: &str = "ControllerCompanionRust";

/// Add or remove an HKCU\\...\\Run entry so the app starts with Windows.
/// `exe_path` should be quoted if it contains spaces.
pub fn set_autostart(enable: bool, exe_path: &str) -> Result<(), String> {
    let subkey = to_wide(RUN_SUBKEY);
    let value_name = to_wide(VALUE_NAME);
    let wide_path = to_wide(exe_path);
    unsafe {
        let mut hkey: HKEY = std::ptr::null_mut();
        let mut disp: DWORD = 0;
        let st = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            std::ptr::null_mut(),
            0,
            KEY_WRITE,
            std::ptr::null_mut(),
            &mut hkey,
            &mut disp,
        );
        if st != ERROR_SUCCESS {
            return Err(format!("RegCreateKeyExW failed: {st}"));
        }
        if enable {
            let bytes = wide_path.len() as u32 * 2;
            let st2 = RegSetValueExW(
                hkey,
                value_name.as_ptr(),
                0,
                REG_SZ,
                wide_path.as_ptr() as *const u8,
                bytes,
            );
            RegCloseKey(hkey);
            if st2 != ERROR_SUCCESS {
                return Err(format!("RegSetValueExW failed: {st2}"));
            }
        } else {
            let st2 = RegDeleteValueW(hkey, value_name.as_ptr());
            RegCloseKey(hkey);
            // Deleting a missing value is fine.
            if st2 != ERROR_SUCCESS && st2 != 2 {
                return Err(format!("RegDeleteValueW failed: {st2}"));
            }
        }
    }
    Ok(())
}

/// Connect the current process and calling thread to the interactive desktop (WinSta0\default).
///
/// On Windows, processes launched by background runners, tasks, or subshells can inherit a
/// non-interactive window station or null desktop, causing `GetForegroundWindow()` to return NULL,
/// `Shell_NotifyIcon` to return 0x80004005, and `SendInput` to fail with ERROR_ACCESS_DENIED (5).
/// Calling this ensures input injection and tray notification work smoothly.
pub fn attach_to_interactive_desktop() {
    #[link(name = "user32")]
    extern "system" {
        fn OpenWindowStationW(lpszWinSta: *const u16, fInherit: i32, dwDesiredAccess: u32) -> *mut std::ffi::c_void;
        fn SetProcessWindowStation(hWinSta: *mut std::ffi::c_void) -> i32;
        fn OpenDesktopW(lpszDesktop: *const u16, dwFlags: u32, fInherit: i32, dwDesiredAccess: u32) -> *mut std::ffi::c_void;
        fn SetThreadDesktop(hDesktop: *mut std::ffi::c_void) -> i32;
    }

    let winsta_name: Vec<u16> = "WinSta0\0".encode_utf16().collect();
    let hwinsta = unsafe { OpenWindowStationW(winsta_name.as_ptr(), 0, 0x000F037F) };
    if !hwinsta.is_null() {
        unsafe { SetProcessWindowStation(hwinsta); }
    }

    let desk_name: Vec<u16> = "default\0".encode_utf16().collect();
    let hdesk = unsafe { OpenDesktopW(desk_name.as_ptr(), 0, 0, 0x000F01FF) };
    if !hdesk.is_null() {
        unsafe { SetThreadDesktop(hdesk); }
    }
}

/// Connect the calling thread to the interactive "default" desktop.
pub fn attach_thread_to_desktop() {
    #[link(name = "user32")]
    extern "system" {
        fn OpenDesktopW(lpszDesktop: *const u16, dwFlags: u32, fInherit: i32, dwDesiredAccess: u32) -> *mut std::ffi::c_void;
        fn SetThreadDesktop(hDesktop: *mut std::ffi::c_void) -> i32;
    }

    let desk_name: Vec<u16> = "default\0".encode_utf16().collect();
    let hdesk = unsafe { OpenDesktopW(desk_name.as_ptr(), 0, 0, 0x000F01FF) };
    if !hdesk.is_null() {
        unsafe { SetThreadDesktop(hdesk); }
    }
}

/// Write a line to %APPDATA%\ControllerCompanionRust\debug.log for live diagnostics.
pub fn log_debug(msg: &str) {
    use std::io::Write;
    let path = crate::config::default_config_path()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("debug.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[{:.3}] {msg}", crate::steady_seconds());
    }
}

/// Check if current process has Administrator privileges (High Integrity token).
pub fn is_elevated() -> bool {
    #[link(name = "advapi32")]
    extern "system" {
        fn OpenProcessToken(process_handle: HANDLE, desired_access: DWORD, token_handle: *mut HANDLE) -> BOOL;
        fn GetTokenInformation(token_handle: HANDLE, token_information_class: u32, token_information: *mut c_void, token_information_length: DWORD, return_length: *mut DWORD) -> BOOL;
        fn CloseHandle(handle: HANDLE) -> BOOL;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> HANDLE;
        fn SetProcessWorkingSetSize(
            hProcess: HANDLE,
            dwMinimumWorkingSetSize: usize,
            dwMaximumWorkingSetSize: usize,
        ) -> BOOL;
    }

    const TOKEN_QUERY: DWORD = 0x0008;
    const TOKEN_ELEVATION: u32 = 20;

    #[repr(C)]
    struct TokenElevation {
        token_is_elevated: DWORD,
    }

    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TokenElevation { token_is_elevated: 0 };
        let mut return_length: DWORD = 0;
        let success = GetTokenInformation(
            token,
            TOKEN_ELEVATION,
            &mut elevation as *mut _ as *mut c_void,
            std::mem::size_of::<TokenElevation>() as DWORD,
            &mut return_length,
        );
        CloseHandle(token);
        success != 0 && elevation.token_is_elevated != 0
    }
}

/// Trim process working set to free unused physical memory back to Windows.
pub fn trim_working_set() {
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentProcess() -> HANDLE;
            fn SetProcessWorkingSetSize(
                hProcess: HANDLE,
                dwMinimumWorkingSetSize: usize,
                dwMaximumWorkingSetSize: usize,
            ) -> BOOL;
        }
        SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

#[repr(C)]
struct PROCESSENTRY32W {
    dwSize: DWORD,
    cntUsage: DWORD,
    th32ProcessID: DWORD,
    th32DefaultHeapID: usize,
    th32ModuleID: DWORD,
    cntThreads: DWORD,
    th32ParentProcessID: DWORD,
    pcPriClassBase: i32,
    dwFlags: DWORD,
    szExeFile: [u16; 260],
}

/// Terminate other running instances of controller-companion.exe / ReverseController.exe
/// so that a newly launched instance (especially elevated) can cleanly take over without
/// being blocked by single-instance locks.
pub fn kill_other_instances() {
    #[link(name = "kernel32")]
    extern "system" {
        fn CreateToolhelp32Snapshot(dwFlags: DWORD, th32ProcessID: DWORD) -> HANDLE;
        fn Process32FirstW(hSnapshot: HANDLE, lppe: *mut PROCESSENTRY32W) -> BOOL;
        fn Process32NextW(hSnapshot: HANDLE, lppe: *mut PROCESSENTRY32W) -> BOOL;
        fn OpenProcess(dwDesiredAccess: DWORD, bInheritHandle: BOOL, dwProcessId: DWORD) -> HANDLE;
        fn TerminateProcess(hProcess: HANDLE, uExitCode: u32) -> BOOL;
        fn GetCurrentProcessId() -> DWORD;
    }

    const TH32CS_SNAPPROCESS: DWORD = 0x00000002;
    const PROCESS_TERMINATE: DWORD = 0x0001;

    let current_pid = unsafe { GetCurrentProcessId() };
    let current_exe_name = std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_lowercase()))
        .unwrap_or_default();

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if !snapshot.is_null() && snapshot != -1isize as HANDLE {
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as DWORD;

            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    if entry.th32ProcessID != current_pid && entry.th32ProcessID != 0 {
                        let len = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                        let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_lowercase();
                        let is_target = name == "controller-companion.exe"
                            || name == "reversecontroller.exe"
                            || (!current_exe_name.is_empty() && name == current_exe_name);
                        if is_target {
                            log_debug(&format!("[platform] Terminating previous instance PID={}", entry.th32ProcessID));
                            let hp = OpenProcess(PROCESS_TERMINATE, 0, entry.th32ProcessID);
                            if !hp.is_null() {
                                let _ = TerminateProcess(hp, 0);
                                let _ = CloseHandle(hp);
                            }
                        }
                    }
                    if Process32NextW(snapshot, &mut entry) == 0 {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snapshot);
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(80));
}

/// Relaunch this process elevated using ShellExecute "runas" (mirrors AdminElevator.cs).
pub fn relaunch_elevated(args: &[String]) -> Result<(), String> {
    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: HWND,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show_cmd: i32,
        ) -> HINSTANCE;
    }

    let exe = std::env::current_exe().map_err(|e| format!("{e}"))?;
    let exe_wide: Vec<u16> = exe.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect();
    let op_wide: Vec<u16> = "runas\0".encode_utf16().collect();
    let params = args.join(" ");
    let params_wide: Vec<u16> = params.encode_utf16().chain(std::iter::once(0)).collect();

    let res = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            op_wide.as_ptr(),
            exe_wide.as_ptr(),
            params_wide.as_ptr(),
            std::ptr::null_mut(),
            1, // SW_SHOWNORMAL
        )
    };
    if res as usize > 32 {
        Ok(())
    } else {
        Err(format!("ShellExecute runas returned error code {:?}", res))
    }
}

/// Register an elevated Scheduled Task at logon (matching original Controller Companion TaskRunLevel.Highest)
pub fn install_scheduled_task(exe_path: &str) -> Result<(), String> {
    let status = std::process::Command::new("schtasks")
        .args([
            "/create",
            "/tn",
            "ControllerCompanionRust",
            "/tr",
            &format!("\"{exe_path}\""),
            "/sc",
            "onlogon",
            "/rl",
            "highest",
            "/f",
        ])
        .output();
    match status {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => Err(String::from_utf8_lossy(&out.stderr).to_string()),
        Err(e) => Err(format!("failed to run schtasks: {e}")),
    }
}

#[cfg(test)]
mod hotkey_tests {
    use super::*;

    #[test]
    fn chords_round_trip_through_the_packed_word() {
        for (mods, vk) in [
            (0u32, 'A' as u32),
            (MOD_CONTROL, 'K' as u32),
            (MOD_CONTROL | MOD_SHIFT, 'S' as u32),
            (MOD_MENU, 0x73),  // F4
            (MOD_WIN, 0x09),  // Tab
        ] {
            let packed = pack_hotkey(mods, vk);
            assert_eq!((packed >> 32) as u32, mods, "modifiers lost for {mods:#x}");
            assert_eq!((packed & 0xFFFF_FFFF) as u32, vk, "vk lost for {vk:#x}");
        }
    }

    #[test]
    fn distinct_chords_do_not_collide() {
        // The packing puts modifiers in the high word and the key in the low
        // one; without that split Ctrl+K and plain K would be indistinguishable.
        assert_ne!(pack_hotkey(0, 'K' as u32), pack_hotkey(MOD_CONTROL, 'K' as u32));
        assert_ne!(
            pack_hotkey(MOD_CONTROL, 'K' as u32),
            pack_hotkey(MOD_CONTROL | MOD_SHIFT, 'K' as u32)
        );
    }

    #[test]
    fn modifier_flags_are_distinct_and_non_overlapping() {
        // The VK codes overlap bitwise (0x10 & 0x11 != 0); the flags must not,
        // or Ctrl+Shift+K would be indistinguishable from Ctrl+K.
        let all = [MOD_SHIFT, MOD_CONTROL, MOD_MENU, MOD_WIN];
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                assert_eq!(a & b, 0, "modifier flags must not overlap: {a:#x} {b:#x}");
            }
        }
        // And every combination must be distinct.
        let combos: Vec<u32> = (0..16u32).map(|i| {
            all.iter()
                .enumerate()
                .filter(|(b, _)| i & (1 << b) != 0)
                .fold(0, |acc, (_, f)| acc | f)
        })
        .collect();
        let mut sorted = combos.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), combos.len(), "modifier combinations collided");
    }

    #[test]
    fn the_hotkey_edge_is_consumed_once() {
        // `take_hotkey` must be an edge, not a level: without the swap a single
        // press would toggle the engine on and then off again.
        HOTKEY_FIRED.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(take_hotkey(), "first read must report the press");
        assert!(!take_hotkey(), "second read must report nothing");
    }
}
