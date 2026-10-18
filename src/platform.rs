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
// LONG: the registry API returns 32 bits; the upper half of the register is
// undefined, so a wider type could misread success as an error.
type LSTATUS = i32;
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
    fn RegisterHotKey(hwnd: HWND, id: i32, fsModifiers: u32, vk: u32) -> BOOL;
    fn UnregisterHotKey(hwnd: HWND, id: i32) -> BOOL;
    fn PostThreadMessageW(idThread: DWORD, msg: u32, wParam: WPARAM, lParam: LPARAM) -> BOOL;
    fn PeekMessageW(msg: *mut MSG, hwnd: HWND, min: u32, max: u32, remove: u32) -> BOOL;
    fn WindowFromPoint(point: POINT) -> HWND;
    fn GetClassNameW(hwnd: HWND, buf: *mut u16, max: i32) -> i32;
    fn LoadCursorW(inst: HINSTANCE, name: usize) -> HCURSOR;
    fn GetAsyncKeyState(vk: i32) -> i16;
}

const WM_HOTKEY: u32 = 0x0312;
const WM_USER_RELOAD_HOTKEY: u32 = 0x0400 + 201;
static HOTKEY_THREAD_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

const WM_LBUTTONDOWN: i32 = 0x0201;
const WM_LBUTTONUP: i32 = 0x0202;
const WM_KEYDOWN: i32 = 0x0100;
const WM_SYSKEYDOWN: i32 = 0x0104;

/// Hotkey chords recognised by the global hotkey registrar.
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

pub const VK_LBUTTON: u32 = 0x01;
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

/// Register the global hotkey chord to watch for.
static HOTKEY: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(pack_hotkey(MOD_CONTROL, 'K' as u32));

/// Pack a chord into the single u64 the atomics can hold.
pub const fn pack_hotkey(mods: u32, vk: u32) -> u64 {
    ((mods as u64) << 32) | vk as u64
}

/// Start listening for `mods` + `vk` system-wide.
pub fn hotkey_watch_start(mods: u32, vk: u32) {
    set_hotkey(Some((mods, vk)));
}

/// Map internal modifier flags to Win32 RegisterHotKey modifiers.
fn to_register_hotkey_modifiers(mods: u32) -> u32 {
    let mut out = 0x4000; // MOD_NOREPEAT
    if mods & MOD_SHIFT != 0 {
        out |= 0x0004;
    }
    if mods & MOD_CONTROL != 0 {
        out |= 0x0002;
    }
    if mods & MOD_MENU != 0 {
        out |= 0x0001;
    }
    if mods & MOD_WIN != 0 {
        out |= 0x0008;
    }
    out
}

/// Change (or with `None`, disable) the global toggle shortcut at runtime.
///
/// Uses native Win32 `RegisterHotKey` instead of dangerous low-level hooks (`WH_KEYBOARD_LL`).
/// This guarantees 0% chance of blocking or freezing the physical keyboard on the system.
pub fn set_hotkey(chord: Option<(u32, u32)>) {
    use std::sync::atomic::Ordering;
    static THREAD_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let packed = match chord {
        Some((mods, vk)) => pack_hotkey(mods, vk),
        None => 0,
    };
    HOTKEY.store(packed, Ordering::SeqCst);
    if THREAD_STARTED.swap(true, Ordering::SeqCst) {
        let tid = HOTKEY_THREAD_ID.load(Ordering::SeqCst);
        if tid != 0 {
            unsafe {
                PostThreadMessageW(tid, WM_USER_RELOAD_HOTKEY, 0, 0);
            }
        }
        return;
    }
    std::thread::spawn(|| unsafe {
        attach_thread_to_desktop();
        let tid = GetCurrentThreadId();
        HOTKEY_THREAD_ID.store(tid, Ordering::SeqCst);

        // Force creation of the message queue for this thread
        let mut msg: MSG = std::mem::zeroed();
        PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, 0);

        let mut registered = false;
        let apply = |reg: &mut bool| {
            if *reg {
                UnregisterHotKey(std::ptr::null_mut(), 1);
                *reg = false;
            }
            let want = HOTKEY.load(Ordering::SeqCst);
            let want_mods = (want >> 32) as u32;
            let want_vk = (want & 0xFFFF_FFFF) as u32;
            if want_vk != 0 {
                let win_mods = to_register_hotkey_modifiers(want_mods);
                if RegisterHotKey(std::ptr::null_mut(), 1, win_mods, want_vk) != 0 {
                    *reg = true;
                    log_debug(&format!("[hotkey] Registered chord vk={want_vk:#x} mods={win_mods:#x} via RegisterHotKey"));
                } else {
                    log_debug(&format!("[hotkey] RegisterHotKey failed for vk={want_vk:#x} mods={win_mods:#x}"));
                }
            }
        };

        apply(&mut registered);

        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            if msg.message == WM_HOTKEY {
                HOTKEY_FIRED.store(true, Ordering::SeqCst);
            } else if msg.message == WM_USER_RELOAD_HOTKEY {
                apply(&mut registered);
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if registered {
            UnregisterHotKey(std::ptr::null_mut(), 1);
        }
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
    LAST_CLICK_WAS_CONTROLLER.load(std::sync::atomic::Ordering::Relaxed) && last_click_within_ms(500)
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

static CONTROLLER_LBUTTON_DOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CLICK_START_POS: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);
static IS_DRAGGING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Record a controller left-click natively in-process. Inspects text-box status
/// immediately without any low-level OS mouse hooks.
pub fn note_controller_left_click() {
    let now_ms = (crate::steady_seconds() * 1000.0) as u64;
    LAST_LCLICK_TIME.store(now_ms, std::sync::atomic::Ordering::SeqCst);
    LAST_CLICK_WAS_CONTROLLER.store(true, std::sync::atomic::Ordering::SeqCst);
    CONTROLLER_LBUTTON_DOWN.store(true, std::sync::atomic::Ordering::SeqCst);
    IS_DRAGGING.store(false, std::sync::atomic::Ordering::SeqCst);
    push_foreground();
    // Only remember where the click landed. Deciding whether that is a text
    // box can mean a cross-process accessibility query, which blocks while
    // the target app is busy; it runs on the text-click watcher instead of
    // the input thread, so a hung window can never freeze the controller.
    unsafe {
        let mut pt: POINT = std::mem::zeroed();
        if GetCursorPos(&mut pt) != 0 {
            if let Ok(mut pending) = PENDING_CLICK.lock() {
                *pending = Some((pt.x, pt.y));
            }
            if let Ok(mut start) = CLICK_START_POS.lock() {
                *start = Some((pt.x, pt.y));
            }
        }
    }
}

pub fn note_controller_left_release() {
    CONTROLLER_LBUTTON_DOWN.store(false, std::sync::atomic::Ordering::SeqCst);
}

pub fn is_controller_lbutton_down() -> bool {
    CONTROLLER_LBUTTON_DOWN.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn check_controller_drag() {
    if !CONTROLLER_LBUTTON_DOWN.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    unsafe {
        let mut cur: POINT = std::mem::zeroed();
        if GetCursorPos(&mut cur) != 0 {
            if let Ok(start) = CLICK_START_POS.lock() {
                if let Some((sx, sy)) = *start {
                    let dx = (cur.x - sx).abs();
                    let dy = (cur.y - sy).abs();
                    // If pointer moved more than 8 pixels while holding left click,
                    // the user is selecting text or dragging. Cancel any virtual keyboard popup.
                    if dx > 8 || dy > 8 {
                        IS_DRAGGING.store(true, std::sync::atomic::Ordering::SeqCst);
                        CLICK_IN_TEXT.store(false, std::sync::atomic::Ordering::SeqCst);
                    }
                }
            }
        }
    }
}

pub fn is_controller_dragging() -> bool {
    IS_DRAGGING.load(std::sync::atomic::Ordering::Relaxed)
}

/// Where the last controller click landed, waiting to be classified.
static PENDING_CLICK: std::sync::Mutex<Option<(i32, i32)>> = std::sync::Mutex::new(None);

/// Classify a pending controller click (call from a background thread).
fn classify_pending_click() {
    let pending = PENDING_CLICK.lock().ok().and_then(|mut p| p.take());
    if let Some((x, y)) = pending {
        if !is_controller_dragging() && point_is_text_field(POINT { x, y }) {
            CLICK_IN_TEXT.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

pub fn reset_last_click_was_controller() {
    LAST_CLICK_WAS_CONTROLLER.store(false, std::sync::atomic::Ordering::SeqCst);
}

pub fn is_physical_mouse_button_down() -> bool {
    if controller_used_within_ms(300) {
        return false;
    }
    let down = unsafe { GetAsyncKeyState(VK_LBUTTON as i32) < 0 };
    if down {
        LAST_CLICK_WAS_CONTROLLER.store(false, std::sync::atomic::Ordering::Relaxed);
    }
    down
}

pub fn cursor_is_inside_overlay() -> bool {
    let ov = overlay_hwnd() as HWND;
    if ov.is_null() {
        return false;
    }
    unsafe {
        let mut pt: POINT = std::mem::zeroed();
        if GetCursorPos(&mut pt) == 0 {
            return false;
        }
        let under = WindowFromPoint(pt);
        if under.is_null() {
            return false;
        }
        if under == ov || GetAncestor(under, GA_ROOT) == ov || GetParent(under) == ov {
            return true;
        }
        let mut rect: RECT = std::mem::zeroed();
        if GetWindowRect(ov, &mut rect) != 0 {
            return pt.x >= rect.left && pt.x <= rect.right && pt.y >= rect.top && pt.y <= rect.bottom;
        }
        false
    }
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

/// Is the pointer showing the hand cursor (a link or button in browsers,
/// Electron apps and many modern programs)? Reading the cursor shape is a
/// cheap local call, unlike accessibility queries, which also make browsers
/// switch on their full accessibility tree and slow down.
pub fn cursor_is_hand() -> bool {
    #[link(name = "user32")]
    extern "system" {
        fn GetCursorInfo(pci: *mut CURSORINFO) -> BOOL;
        fn LoadCursorW(inst: HINSTANCE, name: usize) -> HCURSOR;
    }
    static HAND: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    let hand = *HAND.get_or_init(|| unsafe { LoadCursorW(std::ptr::null_mut(), 32649) as usize }); // IDC_HAND
    if hand == 0 {
        return false;
    }
    unsafe {
        let mut ci: CURSORINFO = std::mem::zeroed();
        ci.cbSize = std::mem::size_of::<CURSORINFO>() as DWORD;
        GetCursorInfo(&mut ci) != 0 && ci.flags != 0 && ci.hCursor as usize == hand
    }
}

#[allow(dead_code)]
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

        // 1. If click lands inside Windows Taskbar or Desktop (or any of their child controls),
        // it is selecting programs/files/windows, NOT entering text.
        let mut cur = under;
        while !cur.is_null() {
            let mut buf = [0u16; 64];
            let len = GetClassNameW(cur, buf.as_mut_ptr(), 64);
            if len > 0 {
                let c = String::from_utf16_lossy(&buf[..len as usize]);
                if c == "Shell_TrayWnd"
                    || c == "Shell_SecondaryTrayWnd"
                    || c == "TopLevelWindowForOverflowXamlIsland"
                    || c == "TaskListThumbnailWnd"
                    || c == "Progman"
                    || c == "WorkerW"
                {
                    return caret_active();
                }
            }
            let parent = GetParent(cur);
            if parent == cur {
                break;
            }
            cur = parent;
        }
    }

    let class = window_class_at(pt);
    let class_name = class.as_deref().unwrap_or("");

    // 2. If click lands inside Explorer file list, it's selecting files.
    if class_name == "SysListView32"
        || class_name == "SHELLDLL_DefView"
        || class_name == "DirectUIHWND"
        || class_name == "CabinetWClass"
        || class_name == "ShellTabWindowClass"
        || class_name == "XamlExplorerHostIslandWindow"
    {
        return caret_active();
    }

    // 3. Known native edit class.
    if EDIT_CLASSES.iter().any(|e| class_name.starts_with(e)) {
        return true;
    }

    // 4. A real caret on the foreground thread.
    if caret_active() {
        return true;
    }

    // 5. Query MSAA to catch modern WinUI/XAML (Settings search, Start Menu search), Chrome, Discord, etc.
    check_accessible_text(pt)
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

/// Controller clicks are tracked natively via [`note_controller_left_click`].
/// A global WH_MOUSE_LL hook is deliberately avoided to ensure 100% native
/// mouse performance with zero stutter, zero hook timeouts, and zero cursor freezing.
pub fn mouse_watch_start() {}

/// Consume the "clicked inside a text box" edge (true at most once per click).
/// Classifies a pending click first, so call this off the input thread.
pub fn take_text_click() -> bool {
    classify_pending_click();
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
const SWP_NOMOVE: u32 = 0x0002;

#[link(name = "user32")]
extern "system" {
    fn ShowWindow(hwnd: HWND, cmd: i32) -> BOOL;
    fn ShowWindowAsync(hwnd: HWND, cmd: i32) -> BOOL;
    fn IsIconic(hwnd: HWND) -> BOOL;
    fn GetCurrentThreadId() -> DWORD;
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
const SWP_ASYNCWINDOWPOS: u32 = 0x4000;

/// Extra `SetWindowPos` flag when `hwnd` belongs to another thread: the call
/// is posted instead of waiting for that thread, so the input loop, tray and
/// watchers never stall behind a busy window.
fn async_flag(hwnd: HWND) -> u32 {
    let owner = unsafe { GetWindowThreadProcessId(hwnd, std::ptr::null_mut()) };
    if owner != 0 && owner != unsafe { GetCurrentThreadId() } {
        SWP_ASYNCWINDOWPOS
    } else {
        0
    }
}
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
            ShowWindowAsync(root, SW_RESTORE);
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
                // Asynchronous: the window belongs to another program, and a
                // synchronous move would wait for it, freezing input if it hangs.
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_ASYNCWINDOWPOS,
            );
            crate::win32::set_cursor_pos(pt.x + dx, pt.y + dy);
        }
    }
}

fn find_target_hwnd(title: &str) -> HWND {
    let is_overlay_query = title == "CCVKbd" || title == "Controller Companion Keyboard";
    if is_overlay_query {
        let h = overlay_hwnd() as HWND;
        if !h.is_null() && unsafe { IsWindow(h) != 0 } {
            return h;
        }
    }
    let wide = to_wide(title);
    let mut hwnd = unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) };
    if hwnd.is_null() && is_overlay_query {
        let alt_title = if title == "CCVKbd" {
            "Controller Companion Keyboard"
        } else {
            "CCVKbd"
        };
        let alt = to_wide(alt_title);
        hwnd = unsafe { FindWindowW(std::ptr::null(), alt.as_ptr()) };
    }
    if !hwnd.is_null() && is_overlay_query {
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
            (-1isize) as HWND,
            x,
            y,
            w,
            h,
            SWP_NOACTIVATE | async_flag(hwnd),
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
        let (insert_after, flags) = if x < -2000 || y < -2000 {
            (std::ptr::null_mut(), SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | async_flag(hwnd))
        } else {
            ((-1isize) as HWND, SWP_NOSIZE | SWP_NOACTIVATE | async_flag(hwnd))
        };
        SetWindowPos(
            hwnd,
            insert_after,
            x,
            y,
            0,
            0,
            flags,
        );
        true
    }
}

/// Reassert HWND_TOPMOST so the overlay stays in front of system flyouts
/// (such as Windows 11 SearchHost.exe or StartMenuExperienceHost.exe)
/// without stealing focus.
pub fn bring_to_top(title: &str) -> bool {
    unsafe {
        let hwnd = find_target_hwnd(title);
        if hwnd.is_null() {
            return false;
        }
        SetWindowPos(
            hwnd,
            (-1isize) as HWND,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | async_flag(hwnd),
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

        let ov = overlay_hwnd() as HWND;
        if !ov.is_null() && w == ov {
            return false;
        }

        // Ignore desktop background, taskbar shells, Start Menu, Settings, and UWP app frames
        let mut class_buf = [0u16; 64];
        let class_len = GetClassNameW(w, class_buf.as_mut_ptr(), class_buf.len() as i32);
        if class_len > 0 {
            let class_name = String::from_utf16_lossy(&class_buf[..class_len as usize]);
            if class_name == "Progman"
                || class_name == "WorkerW"
                || class_name == "Shell_TrayWnd"
                || class_name == "Shell_SecondaryTrayWnd"
                || class_name == "Windows.UI.Core.CoreWindow"
                || class_name == "ApplicationFrameWindow"
                || class_name == "Xaml_WindowedPopupClass"
                || class_name == "TopLevelWindowForOverflowXamlIsland"
                || class_name.starts_with("Windows.UI.")
            {
                return false;
            }
        }

        // Known Windows system, shell, and Settings processes are never fullscreen games
        if let Some(exe) = foreground_process_name() {
            let exe_lower = exe.to_lowercase();
            if exe_lower == "explorer.exe"
                || exe_lower == "systemsettings.exe"
                || exe_lower == "startmenuexperiencehost.exe"
                || exe_lower == "searchhost.exe"
                || exe_lower == "shellexperiencehost.exe"
                || exe_lower == "textinputhost.exe"
                || exe_lower == "applicationframehost.exe"
                || exe_lower == "lockapp.exe"
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
        let cmd = if visible { SW_SHOWNOACTIVATE } else { SW_HIDE };
        if async_flag(hwnd) != 0 {
            ShowWindowAsync(hwnd, cmd);
        } else {
            ShowWindow(hwnd, cmd);
        }
        if visible {
            SetWindowPos(
                hwnd,
                (-1isize) as HWND,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
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
pub fn enforce_overlay_style(title: &str, backdrop: bool) -> bool {
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
            const SWP_FRAMECHANGED: u32 = 0x0020;
            const SWP_NOMOVE: u32 = 0x0002;
            const SWP_NOSIZE: u32 = 0x0001;
            const SWP_NOACTIVATE: u32 = 0x0010;
            SetWindowPos(
                hwnd,
                (-1isize) as HWND,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
        // Enable Windows 11 immersive dark mode and rounded corner preference
        let dark_mode: BOOL = 1;
        let _ = DwmSetWindowAttribute(
            hwnd,
            20, // DWMWA_USE_IMMERSIVE_DARK_MODE
            &dark_mode as *const _ as *const c_void,
            std::mem::size_of::<BOOL>() as DWORD,
        );
        // The keyboard is a rounded acrylic panel. The radial menu is round, so
        // it gets neither a backdrop nor window corners: either would paint a
        // grey square behind the circle.
        let corner_round: u32 = if backdrop { 2 } else { 1 }; // DWMWCP_ROUND / DONOTROUND
        let _ = DwmSetWindowAttribute(
            hwnd,
            33, // DWMWA_WINDOW_CORNER_PREFERENCE
            &corner_round as *const _ as *const c_void,
            std::mem::size_of::<u32>() as DWORD,
        );
        // DWMWA_SYSTEMBACKDROP_TYPE = 38: DWMSBT_TRANSIENTWINDOW (acrylic) = 3, DWMSBT_NONE = 1
        let backdrop_type: u32 = if backdrop { 3 } else { 1 };
        let _ = DwmSetWindowAttribute(
            hwnd,
            38, // DWMWA_SYSTEMBACKDROP_TYPE
            &backdrop_type as *const _ as *const c_void,
            std::mem::size_of::<u32>() as DWORD,
        );
        true
    }
}

/// Give a window the app icon embedded in the executable (title bar and
/// taskbar). Does nothing when the build has no icon resource.
pub fn set_app_icon(hwnd: HWND) {
    #[link(name = "user32")]
    extern "system" {
        fn LoadImageW(inst: HINSTANCE, name: usize, kind: u32, cx: i32, cy: i32, flags: u32) -> *mut c_void;
        fn SendMessageW(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> HINSTANCE;
    }
    const IMAGE_ICON: u32 = 1;
    const WM_SETICON: u32 = 0x0080;
    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        for (which, size) in [(1usize, 32), (0usize, 16)] {
            // ICON_BIG = 1, ICON_SMALL = 0; resource id 1 (build.rs)
            let icon = LoadImageW(module, 1, IMAGE_ICON, size, size, 0);
            if !icon.is_null() {
                SendMessageW(hwnd, WM_SETICON, which, icon as LPARAM);
            }
        }
    }
}

/// Apply Windows 11 immersive dark mode, rounded corners, and Mica backdrop to a standard application window.
pub fn enforce_window_modern_theme(title: &str) -> bool {
    unsafe {
        let hwnd = find_target_hwnd(title);
        if hwnd.is_null() {
            return false;
        }
        set_app_icon(hwnd);
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

#[link(name = "user32")]
extern "system" {
    fn MonitorFromPoint(pt: POINT, flags: DWORD) -> *mut c_void;
}

fn monitor_work_area(mon: *mut c_void) -> Option<(i32, i32, i32, i32)> {
    if mon.is_null() {
        return None;
    }
    unsafe {
        let mut mi: MONITORINFO = std::mem::zeroed();
        mi.cbSize = std::mem::size_of::<MONITORINFO>() as DWORD;
        if GetMonitorInfoW(mon, &mut mi) == 0 {
            return None;
        }
        let r = mi.rcWork;
        let (w, h) = (r.right - r.left, r.bottom - r.top);
        (w > 100 && h > 100).then_some((r.left, r.top, w, h))
    }
}

/// Work area of the monitor the user is working on: the one holding the
/// window being typed into, else the foreground window, else the cursor.
pub fn work_area_active() -> (i32, i32, i32, i32) {
    let ov = overlay_hwnd();
    let target = FOREGROUND.load(std::sync::atomic::Ordering::SeqCst);
    let fg = foreground_hwnd();
    for hwnd in [target, fg] {
        if hwnd != 0 && hwnd != ov && unsafe { IsWindow(hwnd as HWND) } != 0 {
            let mon = unsafe { MonitorFromWindow(hwnd as HWND, MONITOR_DEFAULTTONEAREST) };
            if let Some(area) = monitor_work_area(mon) {
                return area;
            }
        }
    }
    if let Some((x, y)) = crate::win32::get_cursor_pos() {
        return work_area_at(x, y);
    }
    work_area()
}

/// Every monitor's work area, ordered left to right, then top to bottom.
pub fn monitor_work_areas() -> Vec<(i32, i32, i32, i32)> {
    #[link(name = "user32")]
    extern "system" {
        fn EnumDisplayMonitors(
            hdc: *mut c_void,
            clip: *const RECT,
            callback: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut RECT, LPARAM) -> BOOL,
            data: LPARAM,
        ) -> BOOL;
    }
    unsafe extern "system" fn collect(mon: *mut c_void, _: *mut c_void, _: *mut RECT, data: LPARAM) -> BOOL {
        let out = &mut *(data as *mut Vec<(i32, i32, i32, i32)>);
        if let Some(area) = monitor_work_area(mon) {
            out.push(area);
        }
        1
    }
    let mut areas: Vec<(i32, i32, i32, i32)> = Vec::new();
    unsafe {
        EnumDisplayMonitors(
            std::ptr::null_mut(),
            std::ptr::null(),
            collect,
            &mut areas as *mut _ as LPARAM,
        );
    }
    areas.sort_by_key(|a| (a.0, a.1));
    areas
}

/// The monitor after the one containing `(x, y)`, wrapping around; `None`
/// with a single monitor.
pub fn next_monitor_area(
    areas: &[(i32, i32, i32, i32)],
    x: i32,
    y: i32,
) -> Option<(i32, i32, i32, i32)> {
    if areas.len() < 2 {
        return None;
    }
    let inside = |a: &(i32, i32, i32, i32)| x >= a.0 && x < a.0 + a.2 && y >= a.1 && y < a.1 + a.3;
    let current = areas.iter().position(inside).unwrap_or(0);
    Some(areas[(current + 1) % areas.len()])
}

/// Jump the pointer to the centre of the next monitor.
pub fn cursor_to_next_monitor() {
    let Some((x, y)) = crate::win32::get_cursor_pos() else {
        return;
    };
    if let Some((ax, ay, aw, ah)) = next_monitor_area(&monitor_work_areas(), x, y) {
        crate::win32::set_cursor_pos(ax + aw / 2, ay + ah / 2);
    }
}

/// Put the pointer in the middle of the active window (or the current
/// monitor when no window has focus).
pub fn center_cursor_on_foreground() {
    let fg = foreground_hwnd();
    let mut rc: RECT = unsafe { std::mem::zeroed() };
    if fg != 0 && fg != overlay_hwnd() && unsafe { GetWindowRect(fg as HWND, &mut rc) } != 0 {
        crate::win32::set_cursor_pos((rc.left + rc.right) / 2, (rc.top + rc.bottom) / 2);
        return;
    }
    let (ax, ay, aw, ah) = work_area_active();
    crate::win32::set_cursor_pos(ax + aw / 2, ay + ah / 2);
}

/// Work area of the monitor nearest to the screen point `(x, y)`.
pub fn work_area_at(x: i32, y: i32) -> (i32, i32, i32, i32) {
    let mon = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    monitor_work_area(mon).unwrap_or_else(work_area)
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
    if cached != 0 && unsafe { IsWindow(cached as HWND) != 0 } {
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

/// Update the tracked typing target window.
pub fn set_foreground_target(hwnd: usize) {
    if hwnd != 0 && hwnd != overlay_hwnd() {
        FOREGROUND.store(hwnd, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Give focus back to the window captured by [`push_foreground`] or [`set_foreground_target`].
pub fn restore_foreground() {
    let h = FOREGROUND.load(std::sync::atomic::Ordering::SeqCst);
    if h == 0 {
        return;
    }
    unsafe {
        let fg = GetForegroundWindow();
        let fg_usize = fg as usize;
        let ov = overlay_hwnd();
        if (fg_usize == 0 || (ov != 0 && fg_usize == ov) || fg_usize != h) && IsWindow(h as HWND) != 0 {
            let cur_thread = GetCurrentThreadId();
            let fg_thread = if !fg.is_null() {
                GetWindowThreadProcessId(fg, std::ptr::null_mut())
            } else {
                0
            };
            if cur_thread != 0 && fg_thread != 0 && cur_thread != fg_thread {
                AttachThreadInput(cur_thread, fg_thread, 1);
            }
            SetForegroundWindow(h as HWND);
            if cur_thread != 0 && fg_thread != 0 && cur_thread != fg_thread {
                AttachThreadInput(cur_thread, fg_thread, 0);
            }
        }
    }
}

/// Explicitly set foreground window to a specific HWND if valid.
pub fn set_foreground_window_force(hwnd: usize) {
    if hwnd == 0 {
        return;
    }
    unsafe {
        let cur = GetForegroundWindow();
        let cur_usize = cur as usize;
        if cur_usize == hwnd {
            return;
        }
        if IsWindow(hwnd as HWND) != 0 {
            let cur_thread = GetCurrentThreadId();
            let fg_thread = if !cur.is_null() {
                GetWindowThreadProcessId(cur, std::ptr::null_mut())
            } else {
                0
            };
            if cur_thread != 0 && fg_thread != 0 && cur_thread != fg_thread {
                AttachThreadInput(cur_thread, fg_thread, 1);
            }
            SetForegroundWindow(hwnd as HWND);
            if cur_thread != 0 && fg_thread != 0 && cur_thread != fg_thread {
                AttachThreadInput(cur_thread, fg_thread, 0);
            }
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

/// Query the currently active Windows keyboard layout handle (HKL) for the foreground window or current thread.
#[cfg(feature = "gui")]
pub fn active_input_hkl() -> usize {
    unsafe {
        let hwnd = GetForegroundWindow();
        if !hwnd.is_null() {
            let mut pid: DWORD = 0;
            let tid = GetWindowThreadProcessId(hwnd, &mut pid);
            if tid != 0 {
                let hkl = GetKeyboardLayout(tid);
                if hkl != 0 {
                    return hkl;
                }
            }
        }
        GetKeyboardLayout(0)
    }
}


/// The child control that had keyboard focus, so it can be restored exactly.
static FOREGROUND_FOCUS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[link(name = "user32")]
extern "system" {
    fn IsWindow(hwnd: HWND) -> BOOL;
    fn SetForegroundWindow(hwnd: HWND) -> BOOL;
    fn AttachThreadInput(id_attach: DWORD, id_attach_to: DWORD, f_attach: BOOL) -> BOOL;
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
#[allow(dead_code)] // Available to explicit clipboard tools; the keyboard pastes without reading it.
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
const HKEY_LOCAL_MACHINE: HKEY = 0x8000_0002 as HKEY;
const KEY_WRITE: DWORD = 0x20006;
const REG_SZ: DWORD = 1;
const REG_DWORD: DWORD = 4;
const ERROR_SUCCESS: LSTATUS = 0;

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Disables Windows built-in XAML/UWP Gamepad-to-VirtualKey navigation
/// (`ControllerProcessor\ControllerToVKMapping`).
///
/// By default in Windows 10/11, the OS intercepts XInput gamepads inside the Start Menu,
/// Windows Settings, and UWP apps, which hides the mouse cursor and forces
/// directional arrow navigation with a thick white focus rectangle.
/// Setting `Enabled` = 0 ensures full mouse cursor control is always available everywhere.
pub fn disable_windows_gamepad_navigation() {
    unsafe {
        const KEY_WOW64_64KEY: DWORD = 0x0100;
        const KEY_WOW64_32KEY: DWORD = 0x0200;
        for root in [HKEY_LOCAL_MACHINE, HKEY_CURRENT_USER] {
            for access_flag in [0, KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
                let subkey = to_wide("SOFTWARE\\Microsoft\\Input\\Settings\\ControllerProcessor\\ControllerToVKMapping");
                let mut hkey: HKEY = std::ptr::null_mut();
                let mut disp: DWORD = 0;
                let status = RegCreateKeyExW(
                    root,
                    subkey.as_ptr(),
                    0,
                    std::ptr::null_mut(),
                    0,
                    KEY_WRITE | access_flag,
                    std::ptr::null_mut(),
                    &mut hkey,
                    &mut disp,
                );
                if status == ERROR_SUCCESS && !hkey.is_null() {
                    let val_name = to_wide("Enabled");
                    let data: u32 = 0;
                    RegSetValueExW(
                        hkey,
                        val_name.as_ptr(),
                        0,
                        REG_DWORD,
                        &data as *const u32 as *const u8,
                        4,
                    );
                    RegCloseKey(hkey);
                }
            }
        }
    }
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

/// [`foreground_process_name`], cached for ~250ms so the 125Hz loop can ask
/// every frame without opening a process handle each time.
pub fn foreground_process_name_cached() -> Option<String> {
    use std::sync::Mutex;
    type Entry = (Instant, usize, Option<String>);
    static CACHE: Mutex<Option<Entry>> = Mutex::new(None);
    let hwnd = foreground_hwnd();
    let Ok(mut g) = CACHE.lock() else {
        return foreground_process_name();
    };
    if let Some((at, h, name)) = g.as_ref() {
        if *h == hwnd && at.elapsed() < Duration::from_millis(250) {
            return name.clone();
        }
    }
    let name = foreground_process_name();
    *g = Some((Instant::now(), hwnd, name.clone()));
    name
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
    // monitor. This catches the majority of modern games that use borderless
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
        if let Some(exe) = foreground_process_name() {
            let lower = exe.to_lowercase();
            if lower == "startmenuexperiencehost.exe"
                || lower == "searchhost.exe"
                || lower == "shellexperiencehost.exe"
                || lower == "explorer.exe"
            {
                return false;
            }
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

/// Bring an existing top-level window with exactly this title to the front.
/// Returns false when no such window exists.
pub fn focus_window_titled(title: &str) -> bool {
    let wide = to_wide(title);
    let hwnd = unsafe { FindWindowW(std::ptr::null(), wide.as_ptr()) };
    if hwnd.is_null() {
        return false;
    }
    unsafe {
        if IsIconic(hwnd) != 0 {
            ShowWindowAsync(hwnd, SW_RESTORE);
        }
        SetForegroundWindow(hwnd);
    }
    true
}

/// Initialise COM on the calling thread (needed for accessibility queries).
pub fn com_init_thread() {
    #[link(name = "ole32")]
    extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> i32;
    }
    unsafe {
        CoInitializeEx(std::ptr::null_mut(), 0x2); // COINIT_APARTMENTTHREADED
    }
}

/// Uninitialise COM on the calling thread.
pub fn com_uninit_thread() {
    #[link(name = "ole32")]
    extern "system" {
        fn CoUninitialize();
    }
    unsafe {
        CoUninitialize();
    }
}

/// Release modifier keys and mouse buttons that Windows reports as held.
///
/// Used when the app starts and from the panic hook: an injected key-down
/// whose key-up never arrives (because the app crashed or was killed) leaves
/// Ctrl, Shift or the left button stuck system-wide. Only keys that are down
/// get a key-up, so nothing stray reaches the focused app.
pub fn release_stuck_input() {
    const KEYS: [u16; 8] = [0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5C]; // L/R Shift, Ctrl, Alt, Win
    let mut inputs = Vec::new();
    for vk in KEYS {
        if crate::win32::async_key_down(vk as i32) {
            let mut flags = crate::win32::KEYEVENTF_KEYUP;
            if matches!(vk, 0xA3 | 0xA5 | 0x5B | 0x5C) {
                flags |= crate::win32::KEYEVENTF_EXTENDEDKEY;
            }
            inputs.push(crate::win32::key_input(vk, crate::win32::map_vk_to_scancode(vk), flags));
        }
    }
    for (vk, up) in [
        (0x01, crate::win32::MOUSEEVENTF_LEFTUP),
        (0x02, crate::win32::MOUSEEVENTF_RIGHTUP),
        (0x04, crate::win32::MOUSEEVENTF_MIDDLEUP),
    ] {
        if crate::win32::async_key_down(vk) {
            inputs.push(crate::win32::mouse_input(0, 0, 0, up));
        }
    }
    if !inputs.is_empty() {
        crate::win32::send_inputs(&inputs);
    }
    crate::win32::global_stop_all_vibration();
}

/// Write a line to %APPDATA%\ControllerCompanionRust\debug.log for live diagnostics.
pub fn log_debug(msg: &str) {
    use std::io::Write;
    // Tests must not write into the user's real log.
    if cfg!(test) {
        return;
    }
    let path = crate::config::default_config_path()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("debug.log");
    // Keep one previous log and start a fresh one past 1 MB, so the log can
    // never grow without bound on a machine that runs for months.
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > 1024 * 1024) {
        let _ = std::fs::rename(&path, path.with_extension("old.log"));
    }
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
    let params = join_command_line(args);
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

/// Join arguments into one Windows command line, quoting any that contain
/// spaces, tabs or quotes so a path like `C:\My Settings\config.json`
/// survives the relaunch intact.
pub fn join_command_line(args: &[String]) -> String {
    args.iter()
        .map(|a| {
            if !a.is_empty() && !a.contains([' ', '\t', '"']) {
                return a.clone();
            }
            let mut out = String::from('"');
            let mut backslashes = 0;
            for c in a.chars() {
                match c {
                    '\\' => backslashes += 1,
                    '"' => {
                        out.extend(std::iter::repeat_n('\\', backslashes * 2 + 1));
                        out.push('"');
                        backslashes = 0;
                    }
                    _ => {
                        out.extend(std::iter::repeat_n('\\', backslashes));
                        out.push(c);
                        backslashes = 0;
                    }
                }
            }
            out.extend(std::iter::repeat_n('\\', backslashes * 2));
            out.push('"');
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Remove the elevated logon task, if one exists. Best effort: deleting it
/// needs administrator rights, and a missing task is not an error.
pub fn remove_scheduled_task() -> Result<(), String> {
    let out = std::process::Command::new("schtasks")
        .args(["/query", "/tn", "ControllerCompanionRust"])
        .output()
        .map_err(|e| format!("failed to run schtasks: {e}"))?;
    if !out.status.success() {
        return Ok(());
    }
    let out = std::process::Command::new("schtasks")
        .args(["/delete", "/tn", "ControllerCompanionRust", "/f"])
        .output()
        .map_err(|e| format!("failed to run schtasks: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
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
    fn next_monitor_wraps_and_needs_two_screens() {
        let one = [(0, 0, 1920, 1040)];
        assert_eq!(next_monitor_area(&one, 10, 10), None);
        let two = [(-1280, 0, 1280, 984), (0, 0, 1920, 1040)];
        assert_eq!(next_monitor_area(&two, 100, 100), Some(two[0]));
        assert_eq!(next_monitor_area(&two, -100, 100), Some(two[1]));
        // A point between screens starts from the first one.
        assert_eq!(next_monitor_area(&two, 5000, 5000), Some(two[1]));
    }

    #[test]
    fn command_line_quotes_paths_with_spaces_and_escapes_quotes() {
        let args = vec![
            "--config".to_string(),
            r"C:\My Settings\config.json".to_string(),
            "plain".to_string(),
            r#"say "hi""#.to_string(),
            r"C:\dir with space\".to_string(),
        ];
        assert_eq!(
            join_command_line(&args),
            r#"--config "C:\My Settings\config.json" plain "say \"hi\"" "C:\dir with space\\""#
        );
        assert_eq!(join_command_line(&[String::new()]), r#""""#);
    }

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
