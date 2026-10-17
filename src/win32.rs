//! Raw Win32 FFI surface used by the core engine.
//!
//! The functions and struct layouts here mirror the P/Invoke boundary that REA
//! extracted from `ControllerCompanion.Lib.dll`
//! (`inspect-managed-native-boundaries`): `SendInput`, `MapVirtualKeyW`,
//! `GetAsyncKeyState`, `SystemParametersInfoW` from user32, plus the XInput
//! family loaded dynamically by name (1.4 -> 1.3 -> 9.1.0), matching the probe
//! order of the original `XInput.Instance` getter.

#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::mem::size_of;
use std::sync::atomic::{AtomicPtr, Ordering};

static GLOBAL_SET_STATE: AtomicPtr<()> = AtomicPtr::new(std::ptr::null_mut());

pub const INPUT_KEYBOARD: u32 = 1;
pub const INPUT_MOUSE: u32 = 0;

// Mouse event flags
pub const MOUSEEVENTF_MOVE: u32 = 0x0001;
pub const MOUSEEVENTF_LEFTDOWN: u32 = 0x0002;
pub const MOUSEEVENTF_LEFTUP: u32 = 0x0004;
pub const MOUSEEVENTF_RIGHTDOWN: u32 = 0x0008;
pub const MOUSEEVENTF_RIGHTUP: u32 = 0x0010;
pub const MOUSEEVENTF_MIDDLEDOWN: u32 = 0x0020;
pub const MOUSEEVENTF_MIDDLEUP: u32 = 0x0040;
pub const MOUSEEVENTF_XDOWN: u32 = 0x0080;
pub const MOUSEEVENTF_XUP: u32 = 0x0100;
pub const MOUSEEVENTF_WHEEL: u32 = 0x0800;
pub const MOUSEEVENTF_HWHEEL: u32 = 0x1000;
pub const XBUTTON1: u32 = 0x0001;
pub const XBUTTON2: u32 = 0x0002;
pub const WHEEL_DELTA: i32 = 120;

// Keyboard event flags
pub const KEYEVENTF_EXTENDEDKEY: u32 = 0x0001;
pub const KEYEVENTF_KEYUP: u32 = 0x0002;
pub const KEYEVENTF_UNICODE: u32 = 0x0004;
pub const KEYEVENTF_SCANCODE: u32 = 0x0008;
pub const MAPVK_VK_TO_VSC: u32 = 0;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct MOUSEINPUT {
    pub dx: i32,
    pub dy: i32,
    pub mouseData: u32,
    pub dwFlags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct KEYBDINPUT {
    pub wVk: u16,
    pub wScan: u16,
    pub dwFlags: u32,
    pub time: u32,
    pub dwExtraInfo: usize,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct HARDWAREINPUT {
    pub uMsg: u32,
    pub wParamL: u16,
    pub wParamH: u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub union INPUT_UNION {
    pub mi: MOUSEINPUT,
    pub ki: KEYBDINPUT,
    pub hi: HARDWAREINPUT,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct INPUT {
    pub kind: u32,
    pub u: INPUT_UNION,
}

// Verify the x64 INPUT layout matches Win32 (40 bytes) at compile time.
const _: () = assert!(size_of::<INPUT>() == 40, "INPUT must be 40 bytes on x64");

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct POINT {
    pub x: i32,
    pub y: i32,
}

#[link(name = "user32")]
extern "system" {
    pub fn SendInput(nInputs: u32, pInputs: *const INPUT, cbSize: i32) -> u32;
    pub fn MapVirtualKeyW(uCode: u32, uMapType: u32) -> u32;
    pub fn MapVirtualKeyExW(uCode: u32, uMapType: u32, dwhkl: usize) -> u32;
    pub fn ToUnicodeEx(
        wVirtKey: u32,
        wScanCode: u32,
        lpKeyState: *const u8,
        pwszBuff: *mut u16,
        cchBuff: i32,
        wFlags: u32,
        dwhkl: usize,
    ) -> i32;
    pub fn GetKeyboardLayout(idThread: u32) -> usize;
    pub fn GetAsyncKeyState(nVirtKey: i32) -> i16;
    pub fn SystemParametersInfoW(
        uiAction: u32,
        uiParam: u32,
        pvParam: *mut std::ffi::c_void,
        fWinIni: u32,
    ) -> i32;
    pub fn GetCursorPos(lpPoint: *mut POINT) -> i32;
    pub fn SetCursorPos(x: i32, y: i32) -> i32;
}

pub const MAPVK_VSC_TO_VK_EX: u32 = 3;

/// Query the character produced by a scan code under a specific Windows keyboard layout (HKL).
/// `shift`: whether Shift is held.
pub fn scancode_to_char(scan: u32, shift: bool, hkl: usize) -> Option<char> {
    unsafe {
        let vk = MapVirtualKeyExW(scan, MAPVK_VSC_TO_VK_EX, hkl);
        if vk == 0 {
            return None;
        }
        let mut key_state = [0u8; 256];
        if shift {
            key_state[0x10] = 0x80; // VK_SHIFT
        }
        let mut buf = [0u16; 8];
        // wFlags = 4 (TM_NO_INVERT: do not alter keyboard state/dead keys)
        let ret = ToUnicodeEx(
            vk,
            scan,
            key_state.as_ptr(),
            buf.as_mut_ptr(),
            buf.len() as i32,
            4,
            hkl,
        );
        if ret > 0 {
            let ch = char::decode_utf16(buf[..ret as usize].iter().copied())
                .next()
                .and_then(|r| r.ok());
            // Filter out non-printable ASCII control characters (e.g. 0x00..0x1F)
            ch.filter(|&c| c >= ' ' && c != '\x7f')
        } else {
            None
        }
    }
}


pub fn get_cursor_pos() -> Option<(i32, i32)> {
    let mut pt = POINT::default();
    if unsafe { GetCursorPos(&mut pt) } != 0 {
        Some((pt.x, pt.y))
    } else {
        None
    }
}

pub fn set_cursor_pos(x: i32, y: i32) -> bool {
    unsafe { SetCursorPos(x, y) != 0 }
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleA(lpModuleName: *const u8) -> *mut std::ffi::c_void;
    fn GetProcAddress(hModule: *mut std::ffi::c_void, lpProcName: *const u8) -> *mut std::ffi::c_void;
    pub fn CreateMutexW(
        lpMutexAttributes: *mut std::ffi::c_void,
        bInitialOwner: i32,
        lpName: *const u16,
    ) -> *mut std::ffi::c_void;
    pub fn GetLastError() -> u32;
    pub fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
    pub fn GetUserDefaultUILanguage() -> u16;
}

/// Query the primary Windows UI language ID (e.g. 0x01 for Arabic, 0x09 for English).
pub fn user_ui_primary_language() -> u16 {
    let lang_id = unsafe { GetUserDefaultUILanguage() };
    lang_id & 0x03FF
}

pub const ERROR_ALREADY_EXISTS: u32 = 183;

/// Attempt to create a named Windows mutex for single-instance enforcement.
/// Returns (handle, already_exists).
pub fn create_named_mutex(name: &str) -> (Option<*mut std::ffi::c_void>, bool) {
    let mut wide: Vec<u16> = name.encode_utf16().collect();
    wide.push(0);
    unsafe {
        let handle = CreateMutexW(std::ptr::null_mut(), 0, wide.as_ptr());
        if handle.is_null() {
            return (None, false);
        }
        let err = GetLastError();
        let already_exists = err == ERROR_ALREADY_EXISTS;
        (Some(handle), already_exists)
    }
}

pub const SPI_GETKEYBOARDSPEED: u32 = 0x001A;
pub const SPI_GETKEYBOARDDELAY: u32 = 0x0016;

/// Keyboard repeat timing (seconds), read once from the OS like the original
/// `SendInputKeyPresser.GetKeyboardDelay/GetKeyboardRepeat`.
#[derive(Clone, Copy)]
pub struct RepeatTiming {
    pub delay: f64,
    pub speed: f64,
}

impl Default for RepeatTiming {
    fn default() -> Self {
        static CACHED: std::sync::OnceLock<RepeatTiming> = std::sync::OnceLock::new();
        *CACHED.get_or_init(|| {
            let (mut delay, mut speed) = (2u32, 31u32);
            unsafe {
                let mut d: u32 = 0;
                if SystemParametersInfoW(SPI_GETKEYBOARDDELAY, 0, &mut d as *mut u32 as _, 0) != 0 {
                    delay = d.min(3);
                }
                let mut s: u32 = 0;
                if SystemParametersInfoW(SPI_GETKEYBOARDSPEED, 0, &mut s as *mut u32 as _, 0) != 0 {
                    speed = s.min(31);
                }
            }
            let delay_secs = match delay {
                1 => 0.30,
                2 => 0.35,
                3 => 0.45,
                _ => 0.25,
            };
            let cps = (18.0 + (speed as f64 / 31.0) * 16.0).max(18.0);
            RepeatTiming { delay: delay_secs, speed: 1.0 / cps }
        })
    }
}

pub fn send_inputs(inputs: &[INPUT]) -> u32 {
    if inputs.is_empty() {
        return 0;
    }
    // Unit tests exercise the whole input pipeline; they must never type or
    // click into whatever window happens to be focused while they run.
    if cfg!(test) {
        return inputs.len() as u32;
    }
    unsafe {
        let mut sent = SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32);
        if sent == 0 {
            // Mirror C# SendInput retry: briefly wait and retry in case of transient queue contention
            for _ in 0..5 {
                std::thread::sleep(std::time::Duration::from_millis(1));
                sent = SendInput(inputs.len() as u32, inputs.as_ptr(), size_of::<INPUT>() as i32);
                if sent != 0 {
                    break;
                }
            }
        }
        if sent == 0 {
            let err = GetLastError();
            crate::platform::log_debug(&format!("[SendInput] FAILED: count={} err={err} (0x{err:X})", inputs.len()));
        }
        sent
    }
}

pub fn map_vk_to_scancode(vk: u16) -> u16 {
    unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) as u16 }
}

pub fn async_key_down(vk: i32) -> bool {
    unsafe { GetAsyncKeyState(vk) as u16 & 0x8000 != 0 }
}

// ---------------------------------------------------------------------------
// XInput (loaded dynamically by name, probing 1.4 -> 1.3 -> 9.1.0)
// ---------------------------------------------------------------------------

pub const XINPUT_DEVSUBTYPE_GAMEPAD: u8 = 0x01;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct XINPUT_GAMEPAD {
    pub wButtons: u16,
    pub bLeftTrigger: u8,
    pub bRightTrigger: u8,
    pub sThumbLX: i16,
    pub sThumbLY: i16,
    pub sThumbRX: i16,
    pub sThumbRY: i16,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct XINPUT_STATE {
    pub dwPacketNumber: u32,
    pub Gamepad: XINPUT_GAMEPAD,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct XINPUT_VIBRATION {
    pub wLeftMotorSpeed: u16,
    pub wRightMotorSpeed: u16,
}

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
pub struct XINPUT_BATTERY_INFORMATION {
    pub BatteryType: u8,
    pub BatteryLevel: u8,
}

type FnGetState = unsafe extern "system" fn(u32, *mut XINPUT_STATE) -> u32;
type FnSetState = unsafe extern "system" fn(u32, *const XINPUT_VIBRATION) -> u32;
type FnGetBattery = unsafe extern "system" fn(u32, u8, *mut XINPUT_BATTERY_INFORMATION) -> u32;
/// `XInputGetStateEx` (ordinal #100) — includes the Guide button in bit 0x0400.
type FnGetStateEx = unsafe extern "system" fn(u32, *mut XINPUT_STATE) -> u32;
/// `XInputPowerOffController` (ordinal #103) — powers off a wireless controller.
type FnPowerOff = unsafe extern "system" fn(u32) -> u32;

pub struct XInputApi {
    _lib: libloading::Library,
    get_state: FnGetState,
    get_state_ex: Option<FnGetStateEx>,
    set_state: Option<FnSetState>,
    get_battery: Option<FnGetBattery>,
    power_off: Option<FnPowerOff>,
    pub dll_name: &'static str,
}

/// Resolve an XInput export by ordinal number.
///
/// Mirrors the original C# `NativeMethods.GetProcAddress(handle, new IntPtr(ordinal))`.
/// `MAKEINTRESOURCE(n)` sets the pointer to `n` with the high bits zero, which
/// `GetProcAddress` interprets as an ordinal lookup rather than a name lookup.
unsafe fn resolve_ordinal<T: Copy>(dll_name: &str, ordinal: u16) -> Option<T> {
    let mut name_buf = [0u8; 64];
    let name_bytes = dll_name.as_bytes();
    if name_bytes.len() >= name_buf.len() {
        return None;
    }
    name_buf[..name_bytes.len()].copy_from_slice(name_bytes);
    let hmod = GetModuleHandleA(name_buf.as_ptr());
    if hmod.is_null() {
        return None;
    }
    // MAKEINTRESOURCE(ordinal): pointer whose value IS the ordinal number.
    let proc = GetProcAddress(hmod, ordinal as usize as *const u8);
    if proc.is_null() {
        None
    } else {
        Some(std::mem::transmute_copy(&proc))
    }
}

impl XInputApi {
    /// Probe XInput DLLs in the same order the original `XInput.Instance` does.
    /// Uses named exports only (`XInputGetState`, `XInputSetState`,
    /// `XInputGetBatteryInformation`) for maximum robustness.
    ///
    /// Ordinal-only entry points (`GetStateEx` #100 / `PowerOffController` #103)
    /// are resolved on a best-effort basis.  When available (xinput1_4.dll and
    /// xinput1_3.dll), the Guide button reads correctly and power-off works
    /// natively.  When unavailable, Guide reads as not-pressed and power-off
    /// falls back to a sustained rumble.
    pub fn load() -> Result<XInputApi, Box<dyn std::error::Error>> {
        let candidates: [&str; 3] = ["xinput1_4.dll", "xinput1_3.dll", "xinput9_1_0.dll"];
        let mut last_err = String::from("no xinput dll found");
        for name in candidates {
            let lib = match unsafe { libloading::Library::new(name) } {
                Ok(l) => l,
                Err(e) => {
                    last_err = format!("{name}: {e}");
                    continue;
                }
            };
            let get_state: FnGetState = match unsafe { lib.get(b"XInputGetState\0") } {
                Ok(g) => *g,
                Err(e) => {
                    last_err = format!("{name}: XInputGetState missing ({e})");
                    continue;
                }
            };
            let set_state: Option<FnSetState> =
                unsafe { lib.get(b"XInputSetState\0").ok().map(|g| *g) };
            let get_battery: Option<FnGetBattery> = unsafe {
                lib.get(b"XInputGetBatteryInformation\0").ok().map(|g| *g)
            };
            // Undocumented ordinal exports — best-effort, not all DLLs export
            // these.  xinput1_4.dll and xinput1_3.dll typically do.
            let get_state_ex: Option<FnGetStateEx> =
                unsafe { resolve_ordinal(name, 100) };
            let power_off: Option<FnPowerOff> =
                unsafe { resolve_ordinal(name, 103) };
            if get_state_ex.is_some() {
                eprintln!("[xinput] ordinal #100 (GetStateEx / Guide button) resolved from {name}");
            }
            if power_off.is_some() {
                eprintln!("[xinput] ordinal #103 (PowerOffController) resolved from {name}");
            }

            if let Some(f) = set_state {
                GLOBAL_SET_STATE.store(f as *const () as *mut (), Ordering::SeqCst);
            }

            return Ok(XInputApi {
                _lib: lib,
                get_state,
                get_state_ex,
                set_state,
                get_battery,
                power_off,
                dll_name: name,
            });
        }
        Err(format!("failed to load XInput ({last_err})").into())
    }

    pub fn get_state(&self, user: u32) -> Option<XINPUT_STATE> {
        let mut st = XINPUT_STATE::default();
        // Prefer GetStateEx (ordinal #100) which includes the Guide button
        // (bit 0x0400 in wButtons).  Fall back to the named export.
        let rc = if let Some(f) = self.get_state_ex {
            unsafe { f(user, &mut st) }
        } else {
            unsafe { (self.get_state)(user, &mut st) }
        };
        if rc == 0 { Some(st) } else { None }
    }

    /// Raw `XInputGetState` return code, for diagnostics.
    ///
    /// 0 = success, 2 = ERROR_FILE_NOT_FOUND, 1167 = ERROR_DEVICE_NOT_CONNECTED.
    pub fn state_rc(&self, user: u32) -> u32 {
        let mut st = XINPUT_STATE::default();
        if let Some(f) = self.get_state_ex {
            unsafe { f(user, &mut st) }
        } else {
            unsafe { (self.get_state)(user, &mut st) }
        }
    }

    pub fn set_vibration(&self, user: u32, left: u16, right: u16) {
        if let Some(f) = self.set_state {
            let v = XINPUT_VIBRATION { wLeftMotorSpeed: left, wRightMotorSpeed: right };
            unsafe { f(user, &v) };
        }
    }

    /// Stop vibration on all 4 controller slots (0..4) immediately.
    pub fn stop_all_vibration(&self) {
        if let Some(f) = self.set_state {
            let v = XINPUT_VIBRATION { wLeftMotorSpeed: 0, wRightMotorSpeed: 0 };
            for slot in 0..4 {
                unsafe { f(slot, &v) };
            }
        }
    }

    /// Power off a wireless controller.  Uses the dedicated ordinal #103 export
    /// when available; falls back to sustained max rumble otherwise.
    pub fn power_off_controller(&self, user: u32) {
        if let Some(f) = self.power_off {
            unsafe { f(user) };
        } else {
            // Fallback: max rumble tends to cause a wireless pad to drop its link.
            self.set_vibration(user, 0xFFFF, 0xFFFF);
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(500));
                global_stop_all_vibration();
            });
        }
    }

    pub fn battery(&self, user: u32) -> Option<XINPUT_BATTERY_INFORMATION> {
        let f = self.get_battery?;
        let mut b = XINPUT_BATTERY_INFORMATION::default();
        if unsafe { f(user, XINPUT_DEVSUBTYPE_GAMEPAD, &mut b) } == 0 {
            Some(b)
        } else {
            None
        }
    }
}

/// Global fail-safe: stop vibration on all 4 controller slots from ANY thread or timer,
/// without requiring ownership or locking of the Engine.
pub fn global_stop_all_vibration() {
    let ptr = GLOBAL_SET_STATE.load(Ordering::Relaxed);
    if !ptr.is_null() {
        let f: FnSetState = unsafe { std::mem::transmute(ptr) };
        let v = XINPUT_VIBRATION { wLeftMotorSpeed: 0, wRightMotorSpeed: 0 };
        for slot in 0..4 {
            unsafe { f(slot, &v) };
        }
    }
}

/// Magic signature set on all inputs injected by this controller companion,
/// allowing the mouse hook to distinguish physical mouse clicks from controller clicks.
pub const CONTROLLER_EXTRA_INFO: usize = 0x5842_4F58; // "XBOX"

// Helpers to build INPUT records.
#[inline]
pub fn mouse_input(dx: i32, dy: i32, data: u32, flags: u32) -> INPUT {
    INPUT {
        kind: INPUT_MOUSE,
        u: INPUT_UNION {
            mi: MOUSEINPUT { dx, dy, mouseData: data, dwFlags: flags, time: 0, dwExtraInfo: CONTROLLER_EXTRA_INFO },
        },
    }
}

#[inline]
pub fn key_input(vk: u16, scan: u16, flags: u32) -> INPUT {
    INPUT {
        kind: INPUT_KEYBOARD,
        u: INPUT_UNION {
            ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: CONTROLLER_EXTRA_INFO },
        },
    }
}

/// Send a quick virtual key tap (Key Down followed immediately by Key Up).
pub fn send_tap(vk: u16, scan: u16) {
    let down = key_input(vk, scan, 0);
    let up = key_input(vk, scan, KEYEVENTF_KEYUP);
    send_inputs(&[down, up]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "interactive desktop test: moves the real pointer; run explicitly with --ignored"]
    fn test_send_input_mouse() {
        crate::platform::attach_to_interactive_desktop();
        let mut pt_before = POINT::default();
        let mut pt_after = POINT::default();
        unsafe { GetCursorPos(&mut pt_before); }
        let inp = mouse_input(20, 20, 0, MOUSEEVENTF_MOVE);
        let ret = unsafe { SendInput(1, &inp, std::mem::size_of::<INPUT>() as i32) };
        std::thread::sleep(std::time::Duration::from_millis(20));
        unsafe { GetCursorPos(&mut pt_after); }
        println!("test_send_input_mouse: ret={ret} before=({},{}) after=({},{})", pt_before.x, pt_before.y, pt_after.x, pt_after.y);
        assert_eq!(ret, 1);
    }

    #[test]
    fn test_probe_all_slots() {
        let xinput = XInputApi::load().unwrap();
        for i in 0..4 {
            let st = xinput.get_state(i);
            let bat = xinput.battery(i);
            println!("Slot {i}: state={st:?}, battery={bat:?}");
        }
    }

    #[test]
    fn test_scancode_to_char_active_layout() {
        let hkl = unsafe { GetKeyboardLayout(0) };
        println!("Active thread HKL: {hkl:#x}");
        // Scan 0x10 is typically 'q' (or 'a' on azerty, 'ض' on arabic)
        let c_base = scancode_to_char(0x10, false, hkl);
        let c_shift = scancode_to_char(0x10, true, hkl);
        println!("Scan 0x10 -> base: {c_base:?}, shift: {c_shift:?}");
        assert!(c_base.is_some(), "scancode_to_char should resolve on standard desktop layouts");
    }
}

