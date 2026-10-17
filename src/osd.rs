//! Minimalist Native OSD (On-Screen Display) / HUD Overlay.
//!
//! Provides an ultra-sleek, minimalist floating translucent HUD pill at the top-center
//! of the screen with smooth alpha fade-in/sustain/fade-out transitions.
//! Displays real-time status for:
//! - Windows Master Volume with visual progress bar and mute indicator
//! - Controller Battery Level (Full/Med/Low/Empty) and connection state (Wireless / Wired)
//! - Active Profile switching (Desktop / Media / Custom)
//! - Window Snapping Grid assist (Left / Right / Maximize / Minimize)
//! - Controller Companion Engine Enable / Disable state
//! - Drag-Lock toggling
//! - Dual-Stick Split Keyboard mode notifications

#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

type HWND = *mut c_void;
type HDC = *mut c_void;
type HFONT = *mut c_void;
type HBRUSH = *mut c_void;
type HPEN = *mut c_void;
type HRGN = *mut c_void;
type HBITMAP = *mut c_void;
type HINSTANCE = *mut c_void;
type HRESULT = i32;
type WPARAM = usize;
type LPARAM = isize;
type LRESULT = isize;

const WM_DESTROY: u32 = 0x0002;
const WM_PAINT: u32 = 0x000F;
const WM_TIMER: u32 = 0x0113;
const WM_USER: u32 = 0x0400;
const WM_OSD_UPDATE: u32 = WM_USER + 101;

const WS_POPUP: u32 = 0x8000_0000;
const WS_EX_TOPMOST: u32 = 0x0000_0008;
const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
const WS_EX_LAYERED: u32 = 0x0008_0000;
const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
const WS_EX_NOACTIVATE: u32 = 0x0800_0000;

const LWA_ALPHA: u32 = 0x0000_0002;
const SWP_NOSIZE: u32 = 0x0001;
const SWP_NOMOVE: u32 = 0x0002;
const SWP_NOACTIVATE: u32 = 0x0010;
const SWP_SHOWWINDOW: u32 = 0x0040;
const HWND_TOPMOST: HWND = -1isize as HWND;

const SW_HIDE: i32 = 0;
const SW_SHOWNOACTIVATE: i32 = 4;

const DT_LEFT: u32 = 0x0000_0000;
const DT_CENTER: u32 = 0x0000_0001;
const DT_RIGHT: u32 = 0x0000_0002;
const DT_VCENTER: u32 = 0x0000_0004;
const DT_SINGLELINE: u32 = 0x0000_0020;
const DT_NOPREFIX: u32 = 0x0000_0800;

const TRANSPARENT_BK: i32 = 1;
const SRCCOPY: u32 = 0x00CC_0020;

#[repr(C)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct PAINTSTRUCT {
    hdc: HDC,
    fErase: i32,
    rcPaint: RECT,
    fRestore: i32,
    fIncUpdate: i32,
    rgbReserved: [u8; 32],
}

use crate::tray::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, PostMessageW,
    RegisterClassExW, TranslateMessage, MSG, WNDCLASSEXW,
};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GUID {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

#[link(name = "user32")]
extern "system" {
    fn SetLayeredWindowAttributes(hwnd: HWND, crKey: u32, bAlpha: u8, dwFlags: u32) -> i32;
    fn SetWindowPos(
        hwnd: HWND,
        after: HWND,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
    fn ShowWindow(hwnd: HWND, cmd: i32) -> i32;
    fn SetWindowRgn(hwnd: HWND, rgn: HRGN, redraw: i32) -> i32;
    fn SetTimer(hwnd: HWND, id: usize, elapse: u32, timer_fn: *mut c_void) -> usize;
    fn KillTimer(hwnd: HWND, id: usize) -> i32;
    fn InvalidateRect(hwnd: HWND, rect: *const RECT, erase: i32) -> i32;
    fn BeginPaint(hwnd: HWND, ps: *mut PAINTSTRUCT) -> HDC;
    fn EndPaint(hwnd: HWND, ps: *const PAINTSTRUCT) -> i32;
    fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> i32;
    fn DrawTextW(hdc: HDC, str: *const u16, count: i32, rect: *mut RECT, format: u32) -> i32;
    fn FillRect(hdc: HDC, rect: *const RECT, brush: HBRUSH) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn CreateRoundRectRgn(x1: i32, y1: i32, x2: i32, y2: i32, w: i32, h: i32) -> HRGN;
    fn CreateSolidBrush(color: u32) -> HBRUSH;
    fn CreatePen(style: i32, width: i32, color: u32) -> HPEN;
    fn SelectObject(hdc: HDC, obj: *mut c_void) -> *mut c_void;
    fn DeleteObject(obj: *mut c_void) -> i32;
    fn DeleteDC(hdc: HDC) -> i32;
    fn CreateCompatibleDC(hdc: HDC) -> HDC;
    fn CreateCompatibleBitmap(hdc: HDC, cx: i32, cy: i32) -> HBITMAP;
    fn BitBlt(
        dest: HDC,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        src: HDC,
        sx: i32,
        sy: i32,
        rop: u32,
    ) -> i32;
    fn SetBkMode(hdc: HDC, mode: i32) -> i32;
    fn SetTextColor(hdc: HDC, color: u32) -> u32;
    fn CreateFontW(
        cHeight: i32,
        cWidth: i32,
        cEscapement: i32,
        cOrientation: i32,
        cWeight: i32,
        bItalic: u32,
        bUnderline: u32,
        bStrikeOut: u32,
        iCharSet: u32,
        iOutPrecision: u32,
        iClipPrecision: u32,
        iQuality: u32,
        iPitchAndFamily: u32,
        pszFaceName: *const u16,
    ) -> HFONT;
    fn RoundRect(hdc: HDC, left: i32, top: i32, right: i32, bottom: i32, width: i32, height: i32) -> i32;
}

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(pvReserved: *mut c_void, dwCoInit: u32) -> HRESULT;
    fn CoCreateInstance(
        rclsid: *const GUID,
        pUnkOuter: *mut c_void,
        dwClsContext: u32,
        riid: *const GUID,
        ppv: *mut *mut c_void,
    ) -> HRESULT;
}

#[derive(Clone, Debug)]
pub struct OsdPayload {
    pub icon: String,
    pub title: String,
    pub subtitle: String,
    pub progress: Option<f32>, // 0.0 .. 1.0
    pub progress_color: u32,   // RGB
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum OsdPhase {
    Hidden,
    FadeIn,
    Sustain,
    FadeOut,
}

struct OsdState {
    payload: OsdPayload,
    phase: OsdPhase,
    alpha: u8,
    sustain_until: Instant,
}

static OSD_HWND: AtomicUsize = AtomicUsize::new(0);
static PENDING_PAYLOAD: Mutex<Option<OsdPayload>> = Mutex::new(None);
static LIVE_STATE: Mutex<Option<OsdState>> = Mutex::new(None);
static OSD_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Enable or disable minimalist floating OSD / HUD notifications.
pub fn set_osd_enabled(enabled: bool) {
    OSD_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Query whether OSD notifications are currently enabled.
pub fn is_osd_enabled() -> bool {
    OSD_ENABLED.load(Ordering::Relaxed)
}

const OSD_WIDTH: i32 = 320;
const OSD_HEIGHT: i32 = 64;
const TIMER_ID: usize = 1001;

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

/// Spawns the dedicated OSD HUD window and message loop.
pub fn spawn() {
    std::thread::Builder::new()
        .name("cc-osd-hud".into())
        .spawn(move || {
            run_osd_thread();
        })
        .map(|_| ())
        .unwrap_or_else(|e| crate::platform::log_debug(&format!("[osd] could not start: {e}")));
}

/// Display a volume notification with an active percentage meter.
pub fn show_volume(percent: u32, muted: bool) {
    let pct = percent.min(100);
    let (icon, color) = if muted || pct == 0 {
        ("🔇", rgb(231, 76, 60))
    } else if pct < 35 {
        ("🔈", rgb(0, 160, 245))
    } else if pct < 75 {
        ("🔉", rgb(0, 175, 255))
    } else {
        ("🔊", rgb(30, 200, 255))
    };

    let title = if muted {
        "Volume Muted".to_string()
    } else {
        format!("Volume {pct}%")
    };

    show_osd(OsdPayload {
        icon: icon.to_string(),
        title,
        subtitle: if muted { "Sound Output Off".into() } else { "Master Audio".into() },
        progress: Some(pct as f32 / 100.0),
        progress_color: color,
    });
}

/// Query real master volume via Windows Core Audio (WASAPI Endpoint Volume).
pub fn query_master_volume() -> Option<(u32, bool)> {
    unsafe {
        const CLSID_MM_DEVICE_ENUMERATOR: GUID = GUID {
            data1: 0xBCDE0395,
            data2: 0xE52F,
            data3: 0x467C,
            data4: [0x8E, 0x3D, 0xC4, 0x57, 0x92, 0x91, 0x69, 0x2E],
        };
        const IID_IMM_DEVICE_ENUMERATOR: GUID = GUID {
            data1: 0xA95664D2,
            data2: 0x9614,
            data3: 0x4F35,
            data4: [0xA7, 0x46, 0xDE, 0x8D, 0xB6, 0x36, 0x17, 0xE6],
        };
        const IID_IAUDIO_ENDPOINT_VOLUME: GUID = GUID {
            data1: 0x5CDF2C82,
            data2: 0x841E,
            data3: 0x4546,
            data4: [0x97, 0x22, 0x0C, 0xF7, 0x40, 0x78, 0x22, 0x9A],
        };

        let _ = CoInitializeEx(std::ptr::null_mut(), 0);

        let mut enumerator: *mut c_void = std::ptr::null_mut();
        let hr = CoCreateInstance(
            &CLSID_MM_DEVICE_ENUMERATOR,
            std::ptr::null_mut(),
            1, // CLSCTX_INPROC_SERVER
            &IID_IMM_DEVICE_ENUMERATOR,
            &mut enumerator,
        );
        if hr != 0 || enumerator.is_null() {
            return None;
        }

        // IMMDeviceEnumerator::GetDefaultAudioEndpoint(eRender=0, eMultimedia=1, &device)
        type FnGetDefaultEndpoint = unsafe extern "system" fn(
            *mut c_void,
            i32,
            i32,
            *mut *mut c_void,
        ) -> HRESULT;
        let vtable_enum = *(enumerator as *mut *mut usize);
        let get_default_endpoint: FnGetDefaultEndpoint = std::mem::transmute(*vtable_enum.add(4));

        let mut device: *mut c_void = std::ptr::null_mut();
        let hr = get_default_endpoint(enumerator, 0, 1, &mut device);
        // Release enumerator
        type FnRelease = unsafe extern "system" fn(*mut c_void) -> u32;
        let release_enum: FnRelease = std::mem::transmute(*vtable_enum.add(2));
        release_enum(enumerator);

        if hr != 0 || device.is_null() {
            return None;
        }

        // IMMDevice::Activate(&IID_IAudioEndpointVolume, CLSCTX_ALL=23, null, &endpoint_vol)
        type FnActivate = unsafe extern "system" fn(
            *mut c_void,
            *const GUID,
            u32,
            *mut c_void,
            *mut *mut c_void,
        ) -> HRESULT;
        let vtable_dev = *(device as *mut *mut usize);
        let activate: FnActivate = std::mem::transmute(*vtable_dev.add(3));

        let mut endpoint: *mut c_void = std::ptr::null_mut();
        let hr = activate(
            device,
            &IID_IAUDIO_ENDPOINT_VOLUME,
            23,
            std::ptr::null_mut(),
            &mut endpoint,
        );
        let release_dev: FnRelease = std::mem::transmute(*vtable_dev.add(2));
        release_dev(device);

        if hr != 0 || endpoint.is_null() {
            return None;
        }

        let vtable_ep = *(endpoint as *mut *mut usize);
        // IAudioEndpointVolume::GetMasterVolumeLevelScalar(&mut f32) (index 9)
        type FnGetScalar = unsafe extern "system" fn(*mut c_void, *mut f32) -> HRESULT;
        let get_scalar: FnGetScalar = std::mem::transmute(*vtable_ep.add(9));

        // IAudioEndpointVolume::GetMute(&mut i32) (index 15)
        type FnGetMute = unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT;
        let get_mute: FnGetMute = std::mem::transmute(*vtable_ep.add(15));

        let mut vol: f32 = 0.0;
        let mut mute: i32 = 0;
        let _ = get_scalar(endpoint, &mut vol);
        let _ = get_mute(endpoint, &mut mute);

        let release_ep: FnRelease = std::mem::transmute(*vtable_ep.add(2));
        release_ep(endpoint);

        Some(((vol * 100.0).round() as u32, mute != 0))
    }
}

/// Convenience function: query real volume and show it on OSD.
pub fn trigger_volume_feedback() {
    if !is_osd_enabled() {
        return;
    }
    // Called as the volume key goes down: give Windows a moment to apply the
    // change, otherwise the meter shows the level from before the press.
    std::thread::spawn(|| {
        std::thread::sleep(std::time::Duration::from_millis(90));
        if let Some((vol, mute)) = query_master_volume() {
            show_volume(vol, mute);
        } else {
            show_info("🔊", "Volume Changed", "Master Audio");
        }
    });
}

/// Display controller battery status.
pub fn show_battery(level_desc: &str, is_wired: bool) {
    let (icon, title, progress, color) = if is_wired {
        ("🔌", "Controller Connected", Some(1.0), rgb(46, 204, 113))
    } else {
        match level_desc {
            "Full" => ("🔋", "Battery Full (~100%)", Some(1.0), rgb(46, 204, 113)),
            "Medium" => ("🔋", "Battery Medium (~60%)", Some(0.60), rgb(52, 152, 219)),
            "Low" => ("🪫", "Battery Low (~20%)", Some(0.20), rgb(243, 156, 18)),
            _ => ("🪫", "Battery Empty (<10%)", Some(0.05), rgb(231, 76, 60)),
        }
    };

    show_osd(OsdPayload {
        icon: icon.to_string(),
        title: title.to_string(),
        subtitle: if is_wired { "USB Cable Active".into() } else { "Wireless Gamepad".into() },
        progress,
        progress_color: color,
    });
}

/// Display active profile activation.
pub fn show_profile(profile_name: &str) {
    show_osd(OsdPayload {
        icon: "🎮".to_string(),
        title: format!("Profile: {profile_name}"),
        subtitle: "Active Gamepad Profile Loaded".to_string(),
        progress: None,
        progress_color: rgb(0, 180, 240),
    });
}

/// Display window snap assist feedback.
pub fn show_snap(direction: &str) {
    show_osd(OsdPayload {
        icon: "🪟".to_string(),
        title: format!("Window Snapped {direction}"),
        subtitle: "Grid Layout Assist".to_string(),
        progress: None,
        progress_color: rgb(155, 89, 182),
    });
}

/// Display drag-lock toggle feedback.
pub fn show_drag_lock(enabled: bool) {
    let (icon, title, subtitle, color) = if enabled {
        ("🔒", "Drag-Lock ON", "Left Click Held Down", rgb(241, 196, 15))
    } else {
        ("🔓", "Drag-Lock OFF", "Normal Mouse Release", rgb(189, 195, 199))
    };
    show_osd(OsdPayload {
        icon: icon.to_string(),
        title: title.to_string(),
        subtitle: subtitle.to_string(),
        progress: None,
        progress_color: color,
    });
}

/// Display engine enable/disable state.
pub fn show_engine_status(enabled: bool) {
    let (icon, title, subtitle, color) = if enabled {
        ("⚡", "Controller Companion: ON", "Input Mapping Enabled", rgb(46, 204, 113))
    } else {
        ("⏸", "Controller Companion: OFF", "Input Mapping Paused", rgb(231, 76, 60))
    };
    show_osd(OsdPayload {
        icon: icon.to_string(),
        title: title.to_string(),
        subtitle: subtitle.to_string(),
        progress: None,
        progress_color: color,
    });
}

/// Display custom info notification.
pub fn show_info(icon: &str, title: &str, subtitle: &str) {
    show_osd(OsdPayload {
        icon: icon.to_string(),
        title: title.to_string(),
        subtitle: subtitle.to_string(),
        progress: None,
        progress_color: rgb(52, 152, 219),
    });
}

fn show_osd(payload: OsdPayload) {
    if !is_osd_enabled() {
        return;
    }
    if let Ok(mut pending) = PENDING_PAYLOAD.lock() {
        *pending = Some(payload);
    }
    let hwnd = OSD_HWND.load(Ordering::SeqCst) as HWND;
    if !hwnd.is_null() {
        unsafe {
            PostMessageW(hwnd, WM_OSD_UPDATE, 0, 0);
        }
    }
}

unsafe extern "system" fn osd_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_OSD_UPDATE => {
            if let Ok(mut pending) = PENDING_PAYLOAD.lock() {
                if let Some(p) = pending.take() {
                    let mut lock = LIVE_STATE.lock().unwrap_or_else(|e| e.into_inner());
                    let state = lock.get_or_insert_with(|| OsdState {
                        payload: p.clone(),
                        phase: OsdPhase::Hidden,
                        alpha: 0,
                        sustain_until: Instant::now(),
                    });
                    state.payload = p;
                    state.phase = OsdPhase::FadeIn;
                    state.sustain_until = Instant::now() + Duration::from_millis(1800);

                    // Position at top-center of work area
                    let (wx, wy, ww, _wh) = crate::platform::work_area_active();
                    let x = wx + (ww - OSD_WIDTH) / 2;
                    let y = wy + 38;

                    SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        x,
                        y,
                        OSD_WIDTH,
                        OSD_HEIGHT,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                    SetTimer(hwnd, TIMER_ID, 16, std::ptr::null_mut());
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            }
            0
        }
        WM_TIMER => {
            if wparam == TIMER_ID {
                let mut should_kill = false;
                let mut new_alpha = 0u8;
                let mut should_redraw = false;

                if let Ok(mut lock) = LIVE_STATE.lock() {
                    if let Some(state) = lock.as_mut() {
                        match state.phase {
                            OsdPhase::FadeIn => {
                                let next = (state.alpha as u16) + 32;
                                if next >= 240 {
                                    state.alpha = 240;
                                    state.phase = OsdPhase::Sustain;
                                    state.sustain_until = Instant::now() + Duration::from_millis(1800);
                                } else {
                                    state.alpha = next as u8;
                                }
                                new_alpha = state.alpha;
                                should_redraw = true;
                            }
                            OsdPhase::Sustain => {
                                if Instant::now() >= state.sustain_until {
                                    state.phase = OsdPhase::FadeOut;
                                }
                                new_alpha = state.alpha;
                            }
                            OsdPhase::FadeOut => {
                                if state.alpha <= 20 {
                                    state.alpha = 0;
                                    state.phase = OsdPhase::Hidden;
                                    new_alpha = 0;
                                    should_kill = true;
                                } else {
                                    state.alpha = state.alpha.saturating_sub(20);
                                    new_alpha = state.alpha;
                                    should_redraw = true;
                                }
                            }
                            OsdPhase::Hidden => {
                                should_kill = true;
                            }
                        }
                    }
                }

                SetLayeredWindowAttributes(hwnd, 0, new_alpha, LWA_ALPHA);
                if should_redraw {
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                if should_kill {
                    KillTimer(hwnd, TIMER_ID);
                    ShowWindow(hwnd, SW_HIDE);
                }
            }
            0
        }
        WM_PAINT => {
            let mut ps = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            if !hdc.is_null() {
                paint_osd(hwnd, hdc);
                EndPaint(hwnd, &ps);
            }
            0
        }
        WM_DESTROY => {
            KillTimer(hwnd, TIMER_ID);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn paint_osd(hwnd: HWND, hdc: HDC) {
    let mut rect = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetClientRect(hwnd, &mut rect);
    let w = rect.right;
    let h = rect.bottom;

    // Double buffer setup
    let mem_dc = CreateCompatibleDC(hdc);
    let mem_bmp = CreateCompatibleBitmap(hdc, w, h);
    let old_bmp = SelectObject(mem_dc, mem_bmp);

    // Deep sleek acrylic-dark background
    let bg_color = rgb(20, 20, 26);
    let bg_brush = CreateSolidBrush(bg_color);
    let border_color = rgb(65, 65, 80);
    let border_pen = CreatePen(0, 1, border_color);

    let old_brush = SelectObject(mem_dc, bg_brush);
    let old_pen = SelectObject(mem_dc, border_pen);

    // Rounded rectangle border
    RoundRect(mem_dc, 0, 0, w, h, 20, 20);

    SelectObject(mem_dc, old_brush);
    SelectObject(mem_dc, old_pen);
    DeleteObject(bg_brush);
    DeleteObject(border_pen);

    SetBkMode(mem_dc, TRANSPARENT_BK);

    let lock = LIVE_STATE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(state) = lock.as_ref() {
        let font_icon = CreateFontW(
            28, 0, 0, 0, 600, 0, 0, 0, 1, 0, 0, 5, 0,
            to_wide("Segoe UI Emoji").as_ptr(),
        );
        let font_title = CreateFontW(
            17, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 5, 0,
            to_wide("Segoe UI").as_ptr(),
        );
        let font_sub = CreateFontW(
            13, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 5, 0,
            to_wide("Segoe UI").as_ptr(),
        );

        // Draw Icon
        let old_f = SelectObject(mem_dc, font_icon);
        SetTextColor(mem_dc, rgb(255, 255, 255));
        let mut icon_rc = RECT { left: 14, top: 10, right: 52, bottom: h - 10 };
        let icon_wide = to_wide(&state.payload.icon);
        DrawTextW(mem_dc, icon_wide.as_ptr(), -1, &mut icon_rc, DT_CENTER | DT_VCENTER | DT_SINGLELINE);

        // Draw Title
        SelectObject(mem_dc, font_title);
        SetTextColor(mem_dc, rgb(245, 245, 250));
        let mut title_rc = RECT { left: 58, top: 10, right: w - 16, bottom: 32 };
        let title_wide = to_wide(&state.payload.title);
        DrawTextW(mem_dc, title_wide.as_ptr(), -1, &mut title_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);

        // Progress Bar or Subtitle
        if let Some(prog) = state.payload.progress {
            let bar_left = 58;
            let bar_top = 37;
            let bar_w = w - bar_left - 18;
            let bar_h = 9;

            // Track background
            let track_brush = CreateSolidBrush(rgb(42, 42, 54));
            let track_rc = RECT {
                left: bar_left,
                top: bar_top,
                right: bar_left + bar_w,
                bottom: bar_top + bar_h,
            };
            FillRect(mem_dc, &track_rc, track_brush);
            DeleteObject(track_brush);

            // Filled progress
            let fill_w = ((bar_w as f32) * prog.clamp(0.0, 1.0)).round() as i32;
            if fill_w > 0 {
                let fill_brush = CreateSolidBrush(state.payload.progress_color);
                let fill_rc = RECT {
                    left: bar_left,
                    top: bar_top,
                    right: bar_left + fill_w,
                    bottom: bar_top + bar_h,
                };
                FillRect(mem_dc, &fill_rc, fill_brush);
                DeleteObject(fill_brush);
            }
        } else {
            SelectObject(mem_dc, font_sub);
            SetTextColor(mem_dc, rgb(165, 165, 185));
            let mut sub_rc = RECT { left: 58, top: 32, right: w - 16, bottom: 52 };
            let sub_wide = to_wide(&state.payload.subtitle);
            DrawTextW(mem_dc, sub_wide.as_ptr(), -1, &mut sub_rc, DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        }

        SelectObject(mem_dc, old_f);
        DeleteObject(font_icon);
        DeleteObject(font_title);
        DeleteObject(font_sub);
    }

    // Blit from double buffer
    BitBlt(hdc, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);

    SelectObject(mem_dc, old_bmp);
    DeleteObject(mem_bmp);
    DeleteDC(mem_dc);
}

fn run_osd_thread() {
    unsafe {
        let class_name = to_wide("CCOsdHudClass");
        let wnd_class = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: osd_wnd_proc as *const () as usize,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: std::ptr::null_mut(),
            hIcon: std::ptr::null_mut(),
            hCursor: std::ptr::null_mut(),
            hbrBackground: std::ptr::null_mut(),
            lpszMenuName: std::ptr::null(),
            lpszClassName: class_name.as_ptr(),
            hIconSm: std::ptr::null_mut(),
        };

        RegisterClassExW(&wnd_class);

        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
            class_name.as_ptr(),
            to_wide("CCOsdHud").as_ptr(),
            WS_POPUP,
            -4000,
            -4000,
            OSD_WIDTH,
            OSD_HEIGHT,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );

        if hwnd.is_null() {
            eprintln!("[osd] CreateWindowExW failed");
            return;
        }

        // Apply rounded region mask
        let rgn = CreateRoundRectRgn(0, 0, OSD_WIDTH, OSD_HEIGHT, 20, 20);
        SetWindowRgn(hwnd, rgn, 1);

        // Start hidden with 0 alpha
        SetLayeredWindowAttributes(hwnd, 0, 0, LWA_ALPHA);
        ShowWindow(hwnd, SW_HIDE);

        OSD_HWND.store(hwnd as usize, Ordering::SeqCst);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rgb_helper() {
        assert_eq!(rgb(255, 0, 0), 0x000000FF);
        assert_eq!(rgb(0, 255, 0), 0x0000FF00);
        assert_eq!(rgb(0, 0, 255), 0x00FF0000);
    }

    #[test]
    fn test_to_wide() {
        let w = to_wide("Test");
        assert_eq!(w, vec![b'T' as u16, b'e' as u16, b's' as u16, b't' as u16, 0]);
    }

    #[test]
    fn test_osd_payload() {
        let p = OsdPayload {
            icon: "🔊".into(),
            title: "Volume 50%".into(),
            subtitle: "Master Audio".into(),
            progress: Some(0.5),
            progress_color: rgb(0, 160, 245),
        };
        assert_eq!(p.icon, "🔊");
        assert_eq!(p.progress, Some(0.5));
    }
}
