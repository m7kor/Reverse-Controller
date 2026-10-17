//! The controller -> mouse/keyboard engine: a faithful Rust port of the
//! per-frame `ControllerLoop` / `BindingSet` / `MouseMovementBinding` logic.

use crate::actions::{Action, Modifier};
use crate::bindings::{default_profile, Input, PadBindingDef};
use crate::config::Config;
use crate::keys::{Key, KeyPresser};
use crate::mouse;
use crate::vknav::VkNav;
use crate::win32::{self as w32, RepeatTiming};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

// XInput button bit flags (mirror `XInput.Buttons`).
const B_DPAD_UP: u16 = 0x0001;
const B_DPAD_DOWN: u16 = 0x0002;
const B_DPAD_LEFT: u16 = 0x0004;
const B_DPAD_RIGHT: u16 = 0x0008;
const B_START: u16 = 0x0010;
const B_BACK: u16 = 0x0020;
const B_LEFT_THUMB: u16 = 0x0040;
const B_RIGHT_THUMB: u16 = 0x0080;
const B_LEFT_SHOULDER: u16 = 0x0100;
const B_RIGHT_SHOULDER: u16 = 0x0200;
const B_GUIDE: u16 = 0x0400;
const B_A: u16 = 0x1000;
const B_B: u16 = 0x2000;
const B_X: u16 = 0x4000;
const B_Y: u16 = 0x8000;

fn button_flag(input: Input) -> u16 {
    match input {
        Input::DPadUp => B_DPAD_UP,
        Input::DPadDown => B_DPAD_DOWN,
        Input::DPadLeft => B_DPAD_LEFT,
        Input::DPadRight => B_DPAD_RIGHT,
        Input::Start => B_START,
        Input::Back => B_BACK,
        Input::LeftThumb => B_LEFT_THUMB,
        Input::RightThumb => B_RIGHT_THUMB,
        Input::LeftShoulder => B_LEFT_SHOULDER,
        Input::RightShoulder => B_RIGHT_SHOULDER,
        Input::Guide => B_GUIDE,
        Input::A => B_A,
        Input::B => B_B,
        Input::X => B_X,
        Input::Y => B_Y,
        _ => 0,
    }
}

fn modifier_bit(m: Option<Modifier>) -> u8 {
    match m {
        None => 0,
        Some(Modifier::Back) => 1,
    }
}

fn input_active(input: Input, buttons: u16, lt: u8, rt: u8, trigger_threshold: u8) -> bool {
    match input {
        Input::LeftTrigger => lt >= trigger_threshold,
        Input::RightTrigger => rt >= trigger_threshold,
        _ => (buttons & button_flag(input)) != 0,
    }
}

/// Radial dead-zone plus the shaping the reference `v39 Smooth Native Scroll`
/// profile applies, in raw XInput units (0..32767):
///
///   * dead zone      — below `dead_zone` the stick is exactly zero.
///   * diagonal gate  — inside `diagonal_range` percent the stick is snapped
///     to one of 8 directions, so a slightly off-axis nudge does not creep
///     diagonally.
///   * max zone       — magnitude is clamped to `max_zone` so a full push
///     saturates smoothly instead of hitting a wall.
///   * return value is **raw units**, not a 0..1 ratio. Callers multiply by
///     pixels-per-unit and `dt`; normalising here made the acceleration
///     threshold (0.95) meaningless and starved the pointer of speed.
fn shape_stick(lx: f32, ly: f32, m: &crate::config::MouseSettings, dead_zone: f32) -> (f32, f32) {
    let raw = (lx * lx + ly * ly).sqrt();
    if raw <= dead_zone || raw == 0.0 {
        return (0.0, 0.0);
    }
    // Direction from the *raw* reading, before any clamping, so the unit vector
    // is genuinely unit length. Deriving it from a clamped magnitude instead
    // inflates the result back past the clamp (32767 instead of max_zone).
    let mut ux = lx / raw;
    let mut uy = ly / raw;
    // Saturate at the configured max zone.
    let max_zone = if m.max_zone > 0.0 { m.max_zone.min(32767.0) } else { 32767.0 };
    let mag = raw.min(max_zone);

    // 8-way gate near the centre.
    let gate = (m.diagonal_range.max(0.0) / 100.0) * max_zone;
    if gate > 0.0 && mag < gate {
        // Snap to the nearest of 8 directions, preserving magnitude.
        // NOTE: atan2 takes (y, x) — passing (x, y) rotated every direction by
        // 90 degrees, so pushing the stick right moved the pointer up.
        let octant = (uy.atan2(ux) / std::f32::consts::FRAC_PI_4).round();
        let a = octant * std::f32::consts::FRAC_PI_4;
        ux = a.cos();
        uy = a.sin();
    }
    (ux * mag, uy * mag)
}

/// Pointer displacement for one frame, in pixels, in *stick* orientation.
///
/// The caller applies the screen-Y sign and the leftover bookkeeping. Kept
/// separate from `mouse_move` so stick direction, the cruise curve, AltSnap and
/// the legacy acceleration ramp can all be tested without a mouse or a
/// controller.
///
/// `accel` and `cruise_time` are read and updated in place; the returned value already includes
/// the carried `leftover` from the previous frame.
fn pointer_delta(
    axes: (f32, f32),
    m: &crate::config::MouseSettings,
    accel: &mut f32,
    cruise_time: &mut f64,
    dt: f64,
    leftover_x: f32,
    leftover_y: f32,
) -> (f32, f32) {
    let (x, y) = axes;
    let mag = (x * x + y * y).sqrt();
    if mag == 0.0 {
        *accel = 0.0;
        *cruise_time = 0.0;
        return (leftover_x, leftover_y);
    }
    let norm = (mag / 32767.0).min(1.0);

    // Radial cruise & sustained hold cruise curve (mirrors & enhances AMGP cruise acceleration).
    let zone = if m.modifier_zone > 0.0 { m.modifier_zone.min(32767.0) } else { 32767.0 };

    // Temporal cruise: holding stick deflection continuously ramps velocity
    // up to 1.6x over ~1.5s after an initial 350ms dead-time.
    let time_mult = if mag > zone {
        *cruise_time += dt;
        if *cruise_time > 0.35 {
            let t = ((*cruise_time - 0.35) / 1.5).min(1.0) as f32;
            1.0 + 0.6 * t * t * (3.0 - 2.0 * t) // smoothstep ramp from 1.0 up to 1.6x
        } else {
            1.0
        }
    } else {
        *cruise_time = (*cruise_time - dt * 2.5).max(0.0);
        1.0
    };

    // AltSnap: once the stick is pushed hard it becomes a precision tool.
    let snap = altsnap_factor(mag, m);

    // Smooth linear acceleration ramp when pushed past threshold.
    if norm > m.acceleration_threshold {
        *accel = (*accel + m.acceleration * dt as f32).min(m.acceleration);
    } else {
        *accel = (*accel - m.acceleration * 3.0 * dt as f32).max(0.0);
    }

    let base = m.speed + *accel;
    let total = base * time_mult * snap;
    let sx = if m.speed_x > 0.0 { m.speed_x } else { m.speed };
    let sy = if m.speed_y > 0.0 { m.speed_y } else { m.speed };
    // Per-axis speed: scale each component by its own speed ratio.
    let rx = (sx / base.max(1.0)) * (x / 32767.0);
    let ry = (sy / base.max(1.0)) * (y / 32767.0);

    (
        rx * total * dt as f32 + leftover_x,
        ry * total * dt as f32 + leftover_y,
    )
}

/// Wheel ticks `(horizontal, vertical)` for one frame of scroll-stick input.
///
/// `MOUSEEVENTF_WHEEL` scrolls *up* for a positive value, and a normal stick
/// reports positive Y when pushed up, so the Y component passes straight
/// through. `flip_y` covers pads whose driver reports that axis already
/// inverted — without it, "push up" scrolls down.
///
/// Split out from the engine so the direction can be tested without a
/// controller, a mouse, or a window.
fn scroll_ticks(
    axes: (f32, f32),
    m: &crate::config::MouseSettings,
    zone: f32,
    dt: f64,
    flip_y: bool,
) -> (f32, f32) {
    let (x, y) = axes;
    let mag = (x * x + y * y).sqrt();
    if mag <= zone || mag == 0.0 {
        return (0.0, 0.0);
    }
    // Map the post-dead-zone deflection onto 0..1 so `wheel_speed` means
    // "ticks per second at full deflection".
    let mag = mag.min(32767.0);
    let norm = ((mag - zone) / (32767.0 - zone).max(1.0)).min(1.0);
    let mut ux = x / mag;
    let mut uy = y / mag;
    if m.scroll_four_way {
        // Snap to the nearest cardinal with 1.6:1 hysteresis to prevent diagonal drift
        if ux.abs() > uy.abs() * 1.6 {
            uy = 0.0;
        } else if uy.abs() > ux.abs() * 1.6 {
            ux = 0.0;
        }
    }
    let vx = if m.wheel_speed_x > 0.0 { m.wheel_speed_x } else { m.wheel_speed * 0.8 };
    let vy = if m.wheel_speed_y > 0.0 { m.wheel_speed_y } else { m.wheel_speed };
    let ticks_y = if flip_y { -uy } else { uy } * norm * vy * dt as f32;
    let ticks_x = ux * norm * vx * dt as f32;
    (ticks_x, ticks_y)
}

/// AltSnap speed multiplier for a given raw stick magnitude.
///
/// Pushing the stick hard turns it into a precision tool: the per-frame step
/// shrinks as deflection grows, so a fast drag stays smooth but can no longer
/// skip past a small target. The curve is continuous — exactly `1.0` at
/// `altsnap_zone` — so engaging AltSnap never causes a jump in pointer speed,
/// and it eases down to `altsnap_speed` at full deflection.
fn altsnap_factor(mag: f32, m: &crate::config::MouseSettings) -> f32 {
    if !m.altsnap_enabled || m.altsnap_zone <= 0.0 {
        return 1.0;
    }
    let z = m.altsnap_zone.min(32767.0);
    if mag <= z {
        return 1.0;
    }
    let t = ((mag - z) / (32767.0 - z).max(1.0)).clamp(0.0, 1.0);
    let s = t * t * (3.0 - 2.0 * t); // smoothstep: no kink at either end
    1.0 + (m.altsnap_speed.clamp(0.05, 1.0) - 1.0) * s
}

#[derive(Clone)]
struct DigitalEntry {
    def: PadBindingDef,
    is_down: bool,
    was_pressed: bool,
    repeat_down_tick: f64,
    repeat_last: f64,
}

#[derive(Clone)]
struct StickEntry {
    def: PadBindingDef,
}

#[derive(Default)]
struct MoveState {
    leftover_x: f32,
    leftover_y: f32,
    accel_accum: f32,
    cruise_time: f64,
    scroll_accum_x: f32,
    scroll_accum_y: f32,
}

/// A command a binding can raise for the engine to apply on the next frame.
#[derive(Clone, Debug)]
pub enum PendingCommand {
    ToggleEnabled,
    ActivateProfile(String),
    ToggleDragLock,
}

/// The engine-level command an action implies, if any. Everything else is
/// handled immediately inside `on_pressed`.
fn command_for(action: &Action) -> Option<PendingCommand> {
    match action {
        Action::ToggleEnabled => Some(PendingCommand::ToggleEnabled),
        Action::ActivateProfile(name) => Some(PendingCommand::ActivateProfile(name.clone())),
        Action::ToggleDragLock => Some(PendingCommand::ToggleDragLock),
        _ => None,
    }
}

pub struct Engine {
    xinput: w32::XInputApi,
    key: KeyPresser,
    config: Config,
    /// `--trace-pad`: print a diagnostic line about once a second.
    trace_pad: bool,
    last_trace: std::time::Instant,
    /// Engine-level commands raised by a binding (enable toggle, profile
    /// activation). Applied at the top of `update` so the rest of the frame
    /// sees consistent state.
    pending_command: Option<PendingCommand>,
    /// A profile the bindings asked to activate; the main thread picks it up.
    pub pending_profile: Option<String>,
    digital: Vec<DigitalEntry>,
    left_stick: Vec<StickEntry>,
    right_stick: Vec<StickEntry>,
    current_mods: u8,
    next_mods: u8,
    move_state: MoveState,
    was_connected: bool,
    /// Shared show/hide flag for the on-screen virtual keyboard overlay.
    vk_visible: Option<Arc<AtomicBool>>,
    /// Shared controller -> keyboard navigation feed (used when the OSK is up).
    vk_nav: Option<Arc<Mutex<VkNav>>>,
    // Navigation-mode edge/repeat state.
    nav_active: bool,
    nav_prev_buttons: u16,
    nav_suppress_buttons: u16,
    nav_was_moving: bool,
    nav_cooldown: f64,
    nav_was_moving_right: bool,
    nav_right_cooldown: f64,
    /// Previous analog trigger values, so a pull past the threshold is one edge.
    nav_prev_left_trigger: u8,
    nav_prev_right_trigger: u8,
    /// Overrides the real gamepad when set (`--inject-pad`).
    ///
    /// This exists so the input pipeline can be verified end-to-end on a
    /// machine with no controller attached, and so a regression in stick
    /// direction is caught by a test instead of by the user noticing.
    injected: Option<w32::XINPUT_GAMEPAD>,
    /// Mouse left-click drag-lock state.
    pub drag_locked: bool,
    /// Active haptic rumble expiration timestamp (steady seconds).
    rumble_until: f64,
    /// Guide button hold timer for wireless power off.
    guide_press_start: Option<f64>,
    guide_hold_powered_off: bool,
    /// Current frame Left Trigger analog value (0..255).
    current_lt: u8,
    /// Navigation mode hold-to-repeat tracking for Backspace and Space
    nav_backspace_down: f64,
    nav_backspace_next: f64,
    nav_space_down: f64,
    nav_space_next: f64,
    nav_caret_left_down: f64,
    nav_caret_left_next: f64,
    nav_caret_right_down: f64,
    nav_caret_right_next: f64,
    switcher_prev_buttons: u16,
    switcher_was_moving: bool,
    switcher_cooldown: f64,
    toggle_chord_down: bool,
    nav_grid_active: bool,
    nav_mouse_down: bool,
}

static PENDING_RUMBLE: Mutex<Option<(u16, u16, f64)>> = Mutex::new(None);

/// Trigger a brief tactile rumble pulse from any thread (keyboard, voice, radial menu).
#[allow(dead_code)]
pub fn trigger_haptic(left: u16, right: u16, duration_secs: f64) {
    if let Ok(mut lock) = PENDING_RUMBLE.lock() {
        *lock = Some((left, right, duration_secs));
    }
}

impl Engine {
    pub fn new(xinput: w32::XInputApi, config: Config) -> Engine {
        let mut eng = Engine {
            xinput,
            key: KeyPresser::new(),
            config,
            trace_pad: false,
            last_trace: std::time::Instant::now() - std::time::Duration::from_secs(10),
            pending_command: None,
            pending_profile: None,
            digital: Vec::new(),
            left_stick: Vec::new(),
            right_stick: Vec::new(),
            current_mods: 0,
            next_mods: 0,
            move_state: MoveState::default(),
            was_connected: false,
            vk_visible: None,
            vk_nav: None,
            nav_active: false,
            nav_prev_buttons: 0,
            nav_suppress_buttons: 0,
            nav_was_moving: false,
            nav_cooldown: 0.0,
            nav_was_moving_right: false,
            nav_right_cooldown: 0.0,
            nav_prev_left_trigger: 0,
            nav_prev_right_trigger: 0,
            injected: None,
            drag_locked: false,
            rumble_until: 0.0,
            guide_press_start: None,
            guide_hold_powered_off: false,
            current_lt: 0,
            nav_backspace_down: 0.0,
            nav_backspace_next: 0.0,
            nav_space_down: 0.0,
            nav_space_next: 0.0,
            nav_caret_left_down: 0.0,
            nav_caret_left_next: 0.0,
            nav_caret_right_down: 0.0,
            nav_caret_right_next: 0.0,
            switcher_prev_buttons: 0,
            switcher_was_moving: false,
            switcher_cooldown: 0.0,
            toggle_chord_down: false,
            nav_grid_active: false,
            nav_mouse_down: false,
        };
        eng.rebuild_bindings();
        eng
    }

    /// Provide the shared virtual-keyboard visibility flag (set by main).
    #[allow(dead_code)]
    pub fn set_vk_handle(&mut self, flag: Arc<AtomicBool>) {
        self.vk_visible = Some(flag);
    }

    /// Enable the `--trace-pad` diagnostic.
    pub fn set_trace_pad(&mut self, on: bool) {
        self.trace_pad = on;
    }

    /// Replace the real gamepad with a fixed synthetic state.
    ///
    /// Used by `--inject-pad` and by tests: it lets the whole input pipeline be
    /// exercised with no controller attached, so a direction regression shows
    /// up as a failing test rather than as "the mouse is the wrong way round".
    pub fn set_injected_gamepad(&mut self, gp: Option<w32::XINPUT_GAMEPAD>) {
        self.injected = gp;
    }

    /// Provide the shared keyboard-navigation feed (set by main).
    #[allow(dead_code)]
    pub fn set_vk_nav(&mut self, nav: Arc<Mutex<VkNav>>) {
        self.vk_nav = Some(nav);
    }

    /// (Re)build the runtime binding tables from `self.config.bindings`.
    fn rebuild_bindings(&mut self) {
        let bindings = if self.config.bindings.is_empty() {
            default_profile()
        } else {
            self.config.bindings.clone()
        };
        let mut digital: Vec<DigitalEntry> = bindings
            .iter()
            .filter(|b| !b.input.is_stick())
            .cloned()
            .map(|def| DigitalEntry { def, is_down: false, was_pressed: false, repeat_down_tick: 0.0, repeat_last: 0.0 })
            .collect();
        // Sort by (input, modifier descending) — matches the original ordering.
        digital.sort_by_key(|e| {
            let rank = if e.def.modifier.is_some() { 0u8 } else { 1u8 };
            (e.def.input as u8, rank)
        });
        self.digital = digital;
        self.left_stick = bindings
            .iter()
            .filter(|b| b.input == Input::LeftStick)
            .cloned()
            .map(|def| StickEntry { def })
            .collect();
        self.right_stick = bindings
            .iter()
            .filter(|b| b.input == Input::RightStick)
            .cloned()
            .map(|def| StickEntry { def })
            .collect();
    }

    /// Replace the active config (from the settings UI) and rebuild bindings.
    pub fn apply_config(&mut self, config: Config) {
        // Release anything currently held so state matches the new config.
        self.release_all_if_needed();
        self.key.release_all();
        self.config = config;
        self.rebuild_bindings();
        self.move_state = MoveState::default();
        self.current_mods = 0;
        self.next_mods = 0;
        #[cfg(feature = "gui")]
        crate::vkeyboard::update_prefs(self.config.keyboard.clone());
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Switch to a named profile, keeping the rest of the user's settings.
    ///
    /// Only the bindings and the recorded profile name change, so mouse speed,
    /// dead zones and keyboard preferences survive a switch.
    pub fn set_profile(&mut self, stem: &str, bindings: Vec<crate::bindings::PadBindingDef>) {
        self.release_all_if_needed();
        self.key.release_all();
        self.config.active_profile = stem.to_string();
        self.config.bindings = bindings;
        self.rebuild_bindings();
        self.move_state = MoveState::default();
        self.current_mods = 0;
        self.next_mods = 0;
    }

    pub fn set_enabled(&mut self, on: bool) {
        if self.config.enabled != on {
            self.config.enabled = on;
            if !on {
                self.xinput.stop_all_vibration();
                self.rumble_until = 0.0;
                self.release_all_if_needed();
            }
        }
    }

    pub fn toggle_enabled(&mut self) -> bool {
        let next = !self.config.enabled;
        self.set_enabled(next);
        self.rumble_pulse(20000, 25000, 0.12);
        crate::osd::show_engine_status(next);
        next
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn is_connected(&self) -> bool {
        self.xinput
            .get_state(self.config.controller.controller_index)
            .is_some()
    }

    /// (battery_type, battery_level) for the active controller, if available.
    pub fn battery_info(&self) -> Option<(u8, u8)> {
        self.xinput
            .battery(self.config.controller.controller_index)
            .map(|b| (b.BatteryType, b.BatteryLevel))
    }

    /// Read raw gamepad state for the settings visualizer. Its controller slot
    /// comes from the background process snapshot, which may have auto-scanned
    /// to another slot since this process started.
    #[allow(dead_code)]
    pub fn gamepad_state_for(&self, index: u32) -> Option<w32::XINPUT_GAMEPAD> {
        self.xinput.get_state(index).map(|s| s.Gamepad)
    }

    pub fn runtime_status(&self) -> crate::runtime_status::RuntimeStatus {
        let connected = self.is_connected();
        let enabled = self.is_enabled();
        let fullscreen_paused = self.config.disable_in_fullscreen
            && crate::platform::is_fullscreen_cached();
        let active_profile = if self.config.active_profile.is_empty() {
            crate::profiles::DEFAULT_NAME.to_string()
        } else {
            self.config.active_profile.clone()
        };
        crate::runtime_status::RuntimeStatus {
            mode: crate::runtime_status::RuntimeMode::from_state(
                connected,
                enabled,
                fullscreen_paused,
            ),
            enabled,
            connected,
            controller_index: self.config.controller.controller_index,
            active_profile,
            battery_level: self.battery_info().map(|(_, level)| level),
        }
    }

    /// Reload settings+bindings from disk and apply them.
    pub fn reload_from_disk(&mut self, path: &std::path::Path) -> Result<(), String> {
        let fresh = Config::try_load(path)?;
        self.apply_config(fresh);
        Ok(())
    }

    /// A short human-readable status line (used for the tray tooltip/menu).
    pub fn status_label(&self) -> String {
        format!("Controller Companion: {}", self.runtime_status().mode.label())
    }

    /// Trigger a brief haptic rumble pulse if haptic feedback is enabled.
    pub fn rumble_pulse(&mut self, left: u16, right: u16, duration: f64) {
        if !self.config.controller.haptic_feedback {
            return;
        }
        let idx = self.config.controller.controller_index;
        self.xinput.set_vibration(idx, left, right);
        self.rumble_until = crate::steady_seconds() + duration;

        // Guaranteed fail-safe timer: regardless of engine loop state or slot changes,
        // stop all motors across all 4 slots once duration expires!
        let sleep_dur = std::time::Duration::from_secs_f64(duration.max(0.01));
        std::thread::spawn(move || {
            std::thread::sleep(sleep_dur);
            w32::global_stop_all_vibration();
        });
    }

    /// Apply a command raised by a binding during the previous frame.
    ///
    /// `ToggleEnabled` is deliberately honoured even while the engine is
    /// disabled, which is what makes the reference profile's Select+Start
    /// chord able to switch the profile back on.
    fn apply_pending_command(&mut self) {
        let Some(cmd) = self.pending_command.take() else {
            return;
        };
        match cmd {
            PendingCommand::ToggleEnabled => {
                self.toggle_enabled();
            }
            PendingCommand::ActivateProfile(name) => {
                crate::osd::show_profile(&name);
                self.pending_profile = Some(name);
                self.rumble_pulse(15000, 15000, 0.06);
            }
            PendingCommand::ToggleDragLock => {
                self.drag_locked = !self.drag_locked;
                if self.drag_locked {
                    mouse::button_down(crate::mouse::MouseButton::Left);
                    self.rumble_pulse(18000, 22000, 0.08);
                } else {
                    mouse::button_up(crate::mouse::MouseButton::Left);
                    self.rumble_pulse(10000, 12000, 0.04);
                }
                crate::osd::show_drag_lock(self.drag_locked);
            }
        }
    }

    /// `--trace-pad`: report, once per second, *why* input is or is not
    /// reaching the bindings. Every gate that can silently swallow input is
    /// named, because "the mouse does nothing" is otherwise undiagnosable.
    fn trace_gate(&mut self) {
        if !self.trace_pad || self.last_trace.elapsed() < std::time::Duration::from_secs(1) {
            return;
        }
        self.last_trace = std::time::Instant::now();
        let idx = self.config.controller.controller_index;
        let fs = crate::platform::is_fullscreen_cached();
        let rc = self.xinput.state_rc(idx);
        let state = self.xinput.get_state(idx);
        match state {
            Some(s) => {
                let g = s.Gamepad;
                (g.wButtons, g.sThumbLX, g.sThumbLY)
            }
            None => {
                // rc 0 = success, 1167 = ERROR_DEVICE_NOT_CONNECTED.
                println!(
                    "[pad] slot {idx} via {}: rc={rc} ({}) enabled={} fullscreen={} -> input blocked",
                    self.xinput.dll_name,
                    if rc == 1167 { "device not connected" } else { "see XInput docs" },
                    self.config.enabled,
                    fs
                );
                return;
            }
        };
        let dz = self.config.controller.left_stick_dead_zone;
        let dz2 = self.config.controller.right_stick_dead_zone;
        // The match above already succeeded, so this cannot be None.
        let g = self.xinput.get_state(idx).map(|s| s.Gamepad).unwrap_or_default();
        let mag = ((g.sThumbLX as f32).powi(2) + (g.sThumbLY as f32).powi(2)).sqrt();
        let rmag = ((g.sThumbRX as f32).powi(2) + (g.sThumbRY as f32).powi(2)).sqrt();
        println!(
            "[pad] slot {idx} via {}: enabled={} fullscreen={fs} buttons=0x{:04X} \
             L=({},{}) magL={mag:.0}/dz={dz}  R=({},{}) magR={rmag:.0}/dz={dz2} \
             binds L={} R={}",
            self.xinput.dll_name,
            self.config.enabled,
            g.wButtons,
            g.sThumbLX,
            g.sThumbLY,
            g.sThumbRX,
            g.sThumbRY,
            self.left_stick.len(),
            self.right_stick.len(),
        );
    }

    /// Run one frame. Returns quickly if no pad is connected/enabled.
    pub fn update(&mut self, now: f64, dt: f64) {
        self.apply_pending_command();
        self.trace_gate();

        // Process thread-safe haptic rumble requests
        if let Ok(mut pending) = PENDING_RUMBLE.lock() {
            if let Some((l, r, d)) = pending.take() {
                self.rumble_pulse(l, r, d);
            }
        }

        // Handle active haptic rumble pulse timer
        if self.rumble_until > 0.0 && now >= self.rumble_until {
            self.xinput.stop_all_vibration();
            self.rumble_until = 0.0;
        }

        let mut idx = self.config.controller.controller_index;
        let gp = match self.injected {
            Some(g) => g,
            None => match self.xinput.get_state(idx) {
                Some(s) => s.Gamepad,
                None => {
                    // Auto-scan other slots 0..4 if active slot is disconnected
                    let mut found = None;
                    for slot in 0..4 {
                        if slot != idx {
                            if let Some(s) = self.xinput.get_state(slot) {
                                found = Some((slot, s.Gamepad));
                                break;
                            }
                        }
                    }
                    if let Some((new_slot, g)) = found {
                        // A replacement pad must not inherit held inputs or a
                        // recovery chord from the controller that disappeared.
                        self.release_all_if_needed();
                        self.toggle_chord_down = false;
                        self.config.controller.controller_index = new_slot;
                        idx = new_slot;
                        if !self.was_connected {
                            crate::platform::log_debug(&format!("[engine] Controller connected on slot {new_slot}"));
                            self.was_connected = true;
                            self.rumble_pulse(15000, 20000, 0.15);
                            if let Some(b) = self.xinput.battery(new_slot) {
                                let is_wired = b.BatteryType == 0x01;
                                let desc = match b.BatteryLevel {
                                    0x03 => "Full",
                                    0x02 => "Medium",
                                    0x01 => "Low",
                                    _ => "Empty",
                                };
                                crate::osd::show_battery(desc, is_wired);
                            }
                        }
                        g
                    } else {
                        if self.was_connected {
                            crate::platform::log_debug("[engine] All controllers disconnected");
                            self.was_connected = false;
                            self.rumble_until = 0.0;
                            self.xinput.stop_all_vibration();
                        }
                        self.release_all_if_needed();
                        self.toggle_chord_down = false;
                        return;
                    }
                }
            },
        };
        if !self.was_connected {
            self.was_connected = true;
            crate::platform::log_debug(&format!("[engine] Controller connected on slot {idx}"));
            self.rumble_pulse(15000, 20000, 0.15);
            if let Some(b) = self.xinput.battery(idx) {
                let is_wired = b.BatteryType == 0x01;
                let desc = match b.BatteryLevel {
                    0x03 => "Full",
                    0x02 => "Medium",
                    0x01 => "Low",
                    _ => "Empty",
                };
                crate::osd::show_battery(desc, is_wired);
            }
        }
        let mut buttons = gp.wButtons;

        // Dedicated Xbox Guide button hold-to-power-off (hold for 2.5s -> power off wireless pad)
        if buttons & B_GUIDE != 0 {
            if self.guide_press_start.is_none() {
                self.guide_press_start = Some(now);
            } else if let Some(start) = self.guide_press_start {
                if now - start >= 2.5 && !self.guide_hold_powered_off {
                    self.guide_hold_powered_off = true;
                    self.rumble_pulse(35000, 35000, 0.4);
                    self.xinput.power_off_controller(idx);
                }
            }
        } else {
            self.guide_press_start = None;
            self.guide_hold_powered_off = false;
        }
        if self.guide_hold_powered_off {
            buttons &= !B_GUIDE;
        }

        // --- Hardware Universal Toggle Chord: Back + Start ---
        // Pressing Back + Start toggles Controller Companion ON and OFF.
        // This is always active, even when disabled, so the user can freely toggle at any time!
        let chord_active = (buttons & (B_BACK | B_START)) == (B_BACK | B_START);
        if chord_active {
            if !self.toggle_chord_down {
                self.toggle_chord_down = true;
                let next = self.toggle_enabled();
                crate::platform::log_debug(&format!("[engine] Back+Start toggle chord -> enabled={next}"));
            }
            // Suppress Back and Start buttons while held so they don't fire secondary actions
            buttons &= !(B_BACK | B_START);
        } else {
            self.toggle_chord_down = false;
        }

        if !self.config.enabled {
            self.release_all_if_needed();
            return;
        }
        // Fullscreen auto-disable (port of FullScreenLoopDisabler).
        if self.config.disable_in_fullscreen && crate::platform::is_fullscreen_cached() {
            self.release_all_if_needed();
            return;
        }

        let (lx, ly) = shape_stick(
            gp.sThumbLX as f32,
            gp.sThumbLY as f32,
            &self.config.mouse,
            self.config.controller.left_stick_dead_zone as f32,
        );
        let (rx, ry) = shape_stick(
            gp.sThumbRX as f32,
            gp.sThumbRY as f32,
            &self.config.mouse,
            self.config.controller.right_stick_dead_zone as f32,
        );
        let lt = gp.bLeftTrigger;
        let rt = gp.bRightTrigger;
        self.current_lt = lt;

        let controller_active = gp.sThumbLX.abs() > 4000
            || gp.sThumbLY.abs() > 4000
            || gp.sThumbRX.abs() > 4000
            || gp.sThumbRY.abs() > 4000
            || buttons != 0
            || lt > 20
            || rt > 20;
        if controller_active {
            crate::platform::note_controller_activity();
        }



        // --- Keyboard navigation mode: active ONLY when OSK is actually rendered on screen ---
        #[cfg(feature = "gui")]
        let vk_up = self
            .vk_visible
            .as_ref()
            .map(|f| f.load(Ordering::SeqCst))
            .unwrap_or(false);
        #[cfg(not(feature = "gui"))]
        let vk_up = false;

        if vk_up {
            if !self.nav_active {
                // Entering nav mode: release anything the normal bindings hold.
                for e in &mut self.digital {
                    if e.is_down {
                        let action = e.def.action.clone();
                        on_released(&action, &mut self.key, &self.xinput, self.config.controller.controller_index);
                        e.is_down = false;
                        e.was_pressed = false;
                    }
                }
                self.key.release_all();
                self.move_state = MoveState::default();
                self.current_mods = 0;
                self.next_mods = 0;
                self.nav_active = true;
                if let Some(nav_arc) = &self.vk_nav {
                    if let Ok(mut nav) = nav_arc.lock() {
                        nav.clear_close_and_hide();
                    }
                }
                self.nav_prev_buttons = buttons;
                self.nav_suppress_buttons = buttons;
                self.nav_was_moving = false;
                self.nav_cooldown = 0.0;
                self.nav_was_moving_right = false;
                self.nav_right_cooldown = 0.0;
            }
            self.feed_nav(&gp, dt);

            // While virtual keyboard is visible, analog mouse motion and scrolling
            // MUST continue running smoothly so the mouse NEVER freezes!
            let left = std::mem::take(&mut self.left_stick);
            self.update_stick(&left, (lx, ly), now, dt, true);
            self.left_stick = left;

            let right = std::mem::take(&mut self.right_stick);
            let is_repositioning = if let Some(nav_arc) = &self.vk_nav {
                nav_arc.lock().map(|n| n.is_repositioning).unwrap_or(false)
            } else {
                false
            };
            if !is_repositioning {
                self.update_stick(&right, (rx, ry), now, dt, false);
            }
            self.right_stick = right;
            return;
        } else if self.nav_active {
            // Leaving nav mode: suppress any buttons still physically held down
            // (e.g. L3 or B used to close the keyboard) until the user releases them!
            self.nav_active = false;
            if self.nav_mouse_down {
                mouse::button_up(mouse::MouseButton::Left);
                self.nav_mouse_down = false;
            }
            self.nav_grid_active = false;
            if let Some(nav_arc) = &self.vk_nav {
                if let Ok(mut nav) = nav_arc.lock() {
                    nav.clear_close_and_hide();
                    nav.grid_nav_active = false;
                }
            }
            self.nav_prev_buttons = 0;
            self.nav_suppress_buttons = buttons;
            self.nav_was_moving = false;
            self.nav_cooldown = 0.0;
            self.nav_was_moving_right = false;
            self.nav_right_cooldown = 0.0;
            self.nav_backspace_down = 0.0;
            self.nav_backspace_next = 0.0;
            self.nav_space_down = 0.0;
            self.nav_space_next = 0.0;
            self.nav_caret_left_down = 0.0;
            self.nav_caret_left_next = 0.0;
            self.nav_caret_right_down = 0.0;
            self.nav_caret_right_next = 0.0;
            for e in &mut self.digital {
                e.is_down = false;
                e.was_pressed = false;
            }
            self.key.release_all();
            self.move_state = MoveState::default();
            self.current_mods = 0;
            self.next_mods = 0;
        }
        // Keep suppressing buttons held during keyboard exit until released.
        self.nav_suppress_buttons &= buttons;
        let buttons = buttons & !self.nav_suppress_buttons;
        // --- /keyboard navigation mode ---

        // --- Windows Task Switcher Assist mode ---
        if crate::platform::is_task_switcher_cached() {
            if self.feed_task_switcher(&gp, now, dt) {
                return;
            }
        } else {
            self.switcher_prev_buttons = 0;
            self.switcher_was_moving = false;
            self.switcher_cooldown = 0.0;
        }

        // --- Modifier production (double-buffered, 1-frame delay) ---
        self.current_mods = self.next_mods;
        let trigger_threshold = self.config.controller.trigger_threshold;
        let mut produced: u8 = 0;
        for e in &self.digital {
            if let Action::Modifier(m) = &e.def.action {
                let active = input_active(e.def.input, buttons, lt, rt, trigger_threshold);
                let gate = modifier_bit(e.def.modifier);
                if active && (gate == 0 || (self.current_mods & gate) == gate) {
                    produced |= modifier_bit(Some(*m));
                }
            }
        }
        self.next_mods = produced;

        // --- Digital (buttons + triggers) ---
        // Matches exact ButtonBindingEntries.Update logic from Controller Companion:
        // Entries are ordered by (input, modifier descending). When a modified binding matches,
        // it consumes that button for the frame so the base binding does not fire.
        let mut consumed_button: Option<Input> = None;
        let mut consumed_mod: u8 = 255;
        for e in &mut self.digital {
            let this_mod = modifier_bit(e.def.modifier);
            let not_consumed_yet = consumed_button != Some(e.def.input) || consumed_mod == this_mod;
            let active = input_active(e.def.input, buttons, lt, rt, trigger_threshold);
            let gate_ok = this_mod == 0 || (self.current_mods & this_mod) != 0;
            let should = not_consumed_yet && active && gate_ok;
            if should {
                consumed_button = Some(e.def.input);
                consumed_mod = this_mod;
            }
            let action = e.def.action.clone();
            // A binding that fires on this frame may ask the engine to change
            // global state; capture it before the (borrowing) step below.
            let raised = if e.is_down != should {
                command_for(&action)
            } else {
                None
            };
            step_digital(e, &action, should, now, dt, &mut self.key, &self.xinput, idx, self.vk_visible.as_ref());
            if let Some(c) = raised {
                self.pending_command = Some(c);
            }
        }

        // --- Sticks (continuous) ---
        // Temporarily take ownership to avoid cloning on every frame.
        let left = std::mem::take(&mut self.left_stick);
        self.update_stick(&left, (lx, ly), now, dt, true);
        self.left_stick = left;

        let right = std::mem::take(&mut self.right_stick);
        self.update_stick(&right, (rx, ry), now, dt, false);
        self.right_stick = right;
    }

    fn update_stick(
        &mut self,
        entries: &[StickEntry],
        axes: (f32, f32),
        _now: f64,
        dt: f64,
        _is_left: bool,
    ) {
        if entries.is_empty() {
            return;
        }
        let mut flag = false;
        let mut last_mod: u8 = 0;
        for e in entries {
            let this_mod = modifier_bit(e.def.modifier);
            let gate_ok = this_mod == 0 || (self.current_mods & this_mod) == this_mod;
            let active = (!flag || last_mod == this_mod) && gate_ok;
            if active {
                flag = true;
                last_mod = this_mod;
                self.feed_stick(&e.def.action, axes, dt);
            } else {
                self.feed_stick(&e.def.action, (0.0, 0.0), dt);
            }
        }
    }

    fn feed_stick(&mut self, action: &Action, axes: (f32, f32), dt: f64) {
        match action {
            Action::MouseMove { inverted } => {
                let inverted = *inverted;
                self.mouse_move(axes, dt, inverted);
            }
            Action::MouseScroll { flip_y } => {
                // Either the binding or the global override may request the flip.
                self.mouse_scroll(axes, dt, *flip_y != self.config.mouse.scroll_flip_y)
            }
            Action::WindowDrag => {
                self.window_drag(axes, dt);
            }
            _ => {}
        }
    }

    /// Continuous window drag / move driven by stick (native AltSnap replacement).
    fn window_drag(&mut self, axes: (f32, f32), dt: f64) {
        let (x, y) = axes;
        let mag = (x * x + y * y).sqrt();
        if mag <= self.config.controller.right_stick_dead_zone as f32 || mag == 0.0 {
            return;
        }
        let speed = 800.0f32; // smooth window move speed in pixels/s
        let dx = (x / 32767.0) * speed * dt as f32;
        let dy = (-y / 32767.0) * speed * dt as f32; // Invert stick Y for screen Y
        crate::platform::move_window_at_cursor(dx.round() as i32, dy.round() as i32);
    }

    /// `axes` are raw XInput units (see `shape_stick`). Movement is computed as
    /// pixels-per-second × `dt`, which is what `speed` is defined as, with the
    /// sub-pixel remainder carried in `leftover_*` so slow walks do not get
    /// rounded away to nothing.
    fn mouse_move(&mut self, axes: (f32, f32), dt: f64, inverted: bool) {
        if axes.0 == 0.0 && axes.1 == 0.0 {
            self.move_state.accel_accum = 0.0;
            self.move_state.cruise_time = 0.0;
            self.move_state.leftover_x = 0.0;
            self.move_state.leftover_y = 0.0;
            return;
        }
        let mut m = self.config.mouse.clone();
        if m.sniper_enabled && self.current_lt > 0 {
            let trigger_ratio = (self.current_lt as f32) / 255.0;
            let factor = 1.0 - (1.0 - m.sniper_scale) * trigger_ratio;
            m.speed *= factor;
            if m.speed_x > 0.0 { m.speed_x *= factor; }
            if m.speed_y > 0.0 { m.speed_y *= factor; }
            m.acceleration *= factor;
        }
        let (px, py) = pointer_delta(
            axes,
            &m,
            &mut self.move_state.accel_accum,
            &mut self.move_state.cruise_time,
            dt,
            self.move_state.leftover_x,
            self.move_state.leftover_y,
        );
        self.move_state.leftover_x = px.fract();
        self.move_state.leftover_y = py.fract();

        // Screen Y grows downwards; stick Y grows upwards, so the default
        // (`inverted == false`) negates it. The remainder is carried *after*
        // this sign is applied, otherwise a slow vertical walk alternates
        // between rounding up and rounding down and can stall.
        let y_scalar: f32 = if inverted { 1.0 } else { -1.0 };
        let (ix, iy) = (px.trunc() as i32, (py * y_scalar).trunc() as i32);
        if ix != 0 || iy != 0 {
            mouse::move_relative(ix, iy);
        }
    }

    /// Right stick → wheel. `axes` are raw XInput units.
    /// Accumulates sub-pixel ticks in `scroll_accum_*` so slow micro-deflection
    /// scrolling is butter-smooth and never dropped.
    fn mouse_scroll(&mut self, axes: (f32, f32), dt: f64, flip_y: bool) {
        let (ticks_x, ticks_y) = scroll_ticks(
            axes,
            &self.config.mouse,
            self.config.controller.right_stick_dead_zone as f32,
            dt,
            flip_y,
        );
        self.move_state.scroll_accum_y += ticks_y;
        self.move_state.scroll_accum_x += ticks_x;

        let step_y = self.move_state.scroll_accum_y.trunc() as i32;
        if step_y != 0 {
            self.move_state.scroll_accum_y -= step_y as f32;
            mouse::scroll_vertical(step_y);
        }

        let step_x = self.move_state.scroll_accum_x.trunc() as i32;
        if step_x != 0 {
            self.move_state.scroll_accum_x -= step_x as f32;
            mouse::scroll_horizontal(step_x);
        }
    }

    /// Feed controller input to the virtual keyboard while it is visible.
    /// The engine is in "keyboard mode": no normal bindings run this frame.
    fn feed_nav(&mut self, gp: &w32::XINPUT_GAMEPAD, dt: f64) {
        let b = gp.wButtons;
        let newly = b & !self.nav_prev_buttons;

        // Direction from D-pad arrows vs Mouse stick motion
        let dpad_moving = (b & (B_DPAD_RIGHT | B_DPAD_LEFT | B_DPAD_DOWN | B_DPAD_UP)) != 0;
        const MOUSE_STICK_DEADZONE: i32 = 7500;
        let mouse_moving = (gp.sThumbLX as i32).abs() > MOUSE_STICK_DEADZONE
            || (gp.sThumbLY as i32).abs() > MOUSE_STICK_DEADZONE
            || (gp.sThumbRX as i32).abs() > MOUSE_STICK_DEADZONE
            || (gp.sThumbRY as i32).abs() > MOUSE_STICK_DEADZONE;

        if dpad_moving {
            self.nav_grid_active = true;
        } else if mouse_moving {
            self.nav_grid_active = false;
        }

        let mut dx: i32 = 0;
        let mut dy: i32 = 0;
        if b & B_DPAD_RIGHT != 0 {
            dx += 1;
        }
        if b & B_DPAD_LEFT != 0 {
            dx -= 1;
        }
        if b & B_DPAD_DOWN != 0 {
            dy += 1;
        }
        if b & B_DPAD_UP != 0 {
            dy -= 1;
        }

        // Movement with first-step-immediate + hold-repeat.
        let moving = dx != 0 || dy != 0;
        let step = if moving {
            if !self.nav_was_moving {
                self.nav_was_moving = true;
                self.nav_cooldown = 0.28;
                true
            } else if self.nav_cooldown <= 0.0 {
                self.nav_cooldown = 0.09;
                true
            } else {
                false
            }
        } else {
            self.nav_was_moving = false;
            self.nav_cooldown = 0.0;
            false
        };
        if moving && !step {
            // Count down only while holding a direction.
            self.nav_cooldown -= dt;
        }

        // Right stick discrete movement (for Dual-Stick Split Keyboard mode).
        const STICK_TH: i16 = 16000;
        let mut rdx: i32 = 0;
        let mut rdy: i32 = 0;
        if gp.sThumbRX > STICK_TH {
            rdx = 1;
        } else if gp.sThumbRX < -STICK_TH {
            rdx = -1;
        }
        if gp.sThumbRY > STICK_TH {
            rdy = -1;
        } else if gp.sThumbRY < -STICK_TH {
            rdy = 1;
        }

        let moving_r = rdx != 0 || rdy != 0;
        let step_r = if moving_r {
            if !self.nav_was_moving_right {
                self.nav_was_moving_right = true;
                self.nav_right_cooldown = 0.28;
                true
            } else if self.nav_right_cooldown <= 0.0 {
                self.nav_right_cooldown = 0.09;
                true
            } else {
                false
            }
        } else {
            self.nav_was_moving_right = false;
            self.nav_right_cooldown = 0.0;
            false
        };
        if moving_r && !step_r {
            self.nav_right_cooldown -= dt;
        }

        if let Some(nav_arc) = self.vk_nav.clone() {
            if let Ok(mut nav) = nav_arc.lock() {
                nav.stick_x = gp.sThumbLX as f32;
                nav.stick_y = gp.sThumbLY as f32;
                if step {
                    nav.dx += dx;
                    nav.dy += dy;
                }
                if step_r {
                    nav.rx += rdx;
                    nav.ry += rdy;
                }
                nav.grid_nav_active = self.nav_grid_active;
                // Face buttons, mapped from the original `GetKeyboardModeBindings`:
                // If D-pad navigation is active: Button A selects the highlighted grid key.
                // If mouse pointer is being used: Button A sends native mouse left-click!
                if newly & B_A != 0 {
                    if self.nav_grid_active {
                        nav.select += 1;
                    } else {
                        crate::platform::note_controller_left_click();
                        mouse::button_down(mouse::MouseButton::Left);
                        self.nav_mouse_down = true;
                    }
                }
                if self.nav_mouse_down && (b & B_A == 0) {
                    mouse::button_up(mouse::MouseButton::Left);
                    self.nav_mouse_down = false;
                }
                let now = crate::steady_seconds();
                const TRIGGER_TH: u8 = 40;
                let rt_held = gp.bRightTrigger > TRIGGER_TH;

                // X button = Backspace (or Delete Whole Word if RT is held!)
                if newly & B_X != 0 {
                    if rt_held {
                        nav.delete_word += 1;
                    } else {
                        nav.backspace += 1;
                        self.nav_backspace_down = now;
                        self.nav_backspace_next = now + 0.40;
                    }
                } else if b & B_X != 0 && !rt_held {
                    if self.nav_backspace_down > 0.0 && now >= self.nav_backspace_next {
                        nav.backspace += 1;
                        self.nav_backspace_next = now + 0.065; // ~15 chars per sec clean deletion
                    }
                } else {
                    self.nav_backspace_down = 0.0;
                    self.nav_backspace_next = 0.0;
                }

                // Y button = Space (or Accept Top Prediction if RT is held!)
                if newly & B_Y != 0 {
                    if rt_held {
                        nav.accept_prediction += 1;
                    } else {
                        nav.space += 1;
                        self.nav_space_down = now;
                        self.nav_space_next = now + 0.40;
                    }
                } else if b & B_Y != 0 && !rt_held {
                    if self.nav_space_down > 0.0 && now >= self.nav_space_next {
                        nav.space += 1;
                        self.nav_space_next = now + 0.080;
                    }
                } else {
                    self.nav_space_down = 0.0;
                    self.nav_space_next = 0.0;
                }
                // PlayStation's circle button reports as B over XInput. The user
                // asked for it to close the keyboard, which is also the natural
                // "cancel" action.
                if newly & B_B != 0 {
                    nav.close += 1;
                    self.nav_suppress_buttons |= B_B;
                    if let Some(f) = &self.vk_visible {
                        f.store(false, Ordering::SeqCst);
                    }
                }
                // Shoulders: L1 (LB) moves caret left, R1 (RB) moves caret right
                // L1 (LB): Move caret left with continuous hold-to-repeat
                if newly & B_LEFT_SHOULDER != 0 {
                    nav.caret_left += 1;
                    self.nav_caret_left_down = now;
                    self.nav_caret_left_next = now + 0.35;
                } else if b & B_LEFT_SHOULDER != 0 {
                    if self.nav_caret_left_down > 0.0 && now >= self.nav_caret_left_next {
                        nav.caret_left += 1;
                        self.nav_caret_left_next = now + 0.065;
                    }
                } else {
                    self.nav_caret_left_down = 0.0;
                    self.nav_caret_left_next = 0.0;
                }

                // R1 (RB): Move caret right with continuous hold-to-repeat
                if newly & B_RIGHT_SHOULDER != 0 {
                    nav.caret_right += 1;
                    self.nav_caret_right_down = now;
                    self.nav_caret_right_next = now + 0.35;
                } else if b & B_RIGHT_SHOULDER != 0 {
                    if self.nav_caret_right_down > 0.0 && now >= self.nav_caret_right_next {
                        nav.caret_right += 1;
                        self.nav_caret_right_next = now + 0.065;
                    }
                } else {
                    self.nav_caret_right_down = 0.0;
                    self.nav_caret_right_next = 0.0;
                }

                // Analogue triggers:
                // L2 (LT): Go to numbers and symbols list (?123 / #+=)
                if gp.bLeftTrigger > TRIGGER_TH && self.nav_prev_left_trigger <= TRIGGER_TH {
                    nav.layer += 1;
                }
                // R2 (RT): Capitalize letters (Shift / Caps toggle)
                if gp.bRightTrigger > TRIGGER_TH && self.nav_prev_right_trigger <= TRIGGER_TH {
                    nav.shift += 1;
                }

                // Start types Enter, or Shift+Enter if RT (Shift trigger) is held!
                if newly & B_START != 0 {
                    if gp.bRightTrigger > TRIGGER_TH {
                        nav.shift_enter += 1;
                    } else {
                        nav.enter += 1;
                    }
                }
                // Back (Select) switches language (EN <-> Arabic).
                if newly & B_BACK != 0 {
                    nav.lang += 1;
                }
                // Sticks: L3 hides the keyboard, R3 toggles Keyboard Repositioning / Move Mode!
                // L3 + R3 pressed together toggles Dual-Stick Split mode.
                if (newly & (B_LEFT_THUMB | B_RIGHT_THUMB) != 0)
                    && (b & B_LEFT_THUMB != 0)
                    && (b & B_RIGHT_THUMB != 0)
                {
                    nav.toggle_split += 1;
                } else if newly & B_LEFT_THUMB != 0 {
                    nav.hide += 1;
                    self.nav_suppress_buttons |= B_LEFT_THUMB;
                    if let Some(f) = &self.vk_visible {
                        f.store(false, Ordering::SeqCst);
                    }
                } else if newly & B_RIGHT_THUMB != 0 {
                    nav.reposition_toggle += 1;
                }
                nav.right_stick_x = gp.sThumbRX as f32;
                nav.right_stick_y = gp.sThumbRY as f32;
            }
        }
        self.nav_prev_left_trigger = gp.bLeftTrigger;
        self.nav_prev_right_trigger = gp.bRightTrigger;

        self.nav_was_moving = moving;
        self.nav_prev_buttons = b;
    }

    /// Windows Task Switcher (Alt+Tab / Task View) controller navigation assist.
    /// Maps D-Pad, Left Stick, and shoulder buttons to arrow keys with repeat rate and haptic feedback.
    /// Maps A to Enter, B to Escape, X to Delete.
    fn feed_task_switcher(&mut self, gp: &w32::XINPUT_GAMEPAD, _now: f64, dt: f64) -> bool {
        let b = gp.wButtons;
        let newly = b & !self.switcher_prev_buttons;
        self.switcher_prev_buttons = b;

        const STICK_TH: i16 = 16000;
        let mut dx: i32 = 0;
        let mut dy: i32 = 0;

        if b & B_DPAD_RIGHT != 0 || gp.sThumbLX > STICK_TH {
            dx += 1;
        }
        if b & B_DPAD_LEFT != 0 || gp.sThumbLX < -STICK_TH {
            dx -= 1;
        }
        if b & B_DPAD_DOWN != 0 || gp.sThumbLY < -STICK_TH {
            dy += 1;
        }
        if b & B_DPAD_UP != 0 || gp.sThumbLY > STICK_TH {
            dy -= 1;
        }

        // Shoulder bumpers also step left/right
        if newly & B_LEFT_SHOULDER != 0 {
            dx -= 1;
        }
        if newly & B_RIGHT_SHOULDER != 0 {
            dx += 1;
        }

        let moving = dx != 0 || dy != 0;
        let step = if moving {
            if !self.switcher_was_moving {
                self.switcher_was_moving = true;
                self.switcher_cooldown = 0.28;
                true
            } else if self.switcher_cooldown <= 0.0 {
                self.switcher_cooldown = 0.16;
                true
            } else {
                false
            }
        } else {
            self.switcher_was_moving = false;
            self.switcher_cooldown = 0.0;
            false
        };
        if moving && !step {
            self.switcher_cooldown -= dt;
        }

        if step {
            if dx > 0 {
                w32::send_tap(0x27, 0x4D); // VK_RIGHT
                crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
                self.rumble_pulse(4000, 6000, 0.02);
            } else if dx < 0 {
                w32::send_tap(0x25, 0x4B); // VK_LEFT
                crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
                self.rumble_pulse(4000, 6000, 0.02);
            } else if dy > 0 {
                w32::send_tap(0x28, 0x50); // VK_DOWN
                crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
                self.rumble_pulse(4000, 6000, 0.02);
            } else if dy < 0 {
                w32::send_tap(0x26, 0x48); // VK_UP
                crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
                self.rumble_pulse(4000, 6000, 0.02);
            }
        }

        // Face buttons:
        // A = Select / Switch to window (VK_RETURN)
        if newly & B_A != 0 {
            w32::send_tap(0x0D, 0x1C);
            self.rumble_pulse(12000, 15000, 0.04);
            return true;
        }

        // B = Cancel / Dismiss (VK_ESCAPE)
        if newly & B_B != 0 {
            w32::send_tap(0x1B, 0x01);
            self.rumble_pulse(8000, 10000, 0.03);
            return true;
        }

        // X = Close window (VK_DELETE)
        if newly & B_X != 0 {
            w32::send_tap(0x2E, 0x53);
            self.rumble_pulse(16000, 20000, 0.05);
            return true;
        }

        true // Consume input and block mouse movement while in task switcher
    }

    fn release_all_if_needed(&mut self) {
        self.xinput.stop_all_vibration();
        self.rumble_until = 0.0;
        // This also runs after a disconnect has already cleared was_connected.
        // Held input must be released regardless of the hardware flag.
        for e in &mut self.digital {
            if e.is_down {
                let action = e.def.action.clone();
                on_released(&action, &mut self.key, &self.xinput, self.config.controller.controller_index);
                e.is_down = false;
            }
            e.was_pressed = false;
        }
        self.key.release_all();
        self.move_state = MoveState::default();
        self.current_mods = 0;
        self.next_mods = 0;
        self.nav_active = false;
        self.nav_prev_buttons = 0;
        self.nav_suppress_buttons = 0;
        self.nav_was_moving = false;
        self.nav_was_moving_right = false;
        if self.drag_locked {
            mouse::button_up(crate::mouse::MouseButton::Left);
            self.drag_locked = false;
        }
    }

    /// Graceful shutdown: release keys and stop all rumble.
    pub fn shutdown(&mut self) {
        self.xinput.stop_all_vibration();
        self.rumble_until = 0.0;
        self.release_all_if_needed();
        self.key.release_all();
    }
}

#[allow(clippy::too_many_arguments)]
fn step_digital(
    e: &mut DigitalEntry,
    action: &Action,
    should: bool,
    now: f64,
    _dt: f64,
    key: &mut KeyPresser,
    xinput: &w32::XInputApi,
    idx: u32,
    vk: Option<&Arc<AtomicBool>>,
) {
    if e.was_pressed && should {
        // Held.
        if let Action::Key { key: k, repeat: true } = action {
            let t = RepeatTiming::default();
            if now - e.repeat_down_tick >= t.delay && now - e.repeat_last >= t.speed {
                e.repeat_last += t.speed;
                key.repeat_tick(*k);
            }
        }
    } else if e.is_down && !should {
        if e.was_pressed {
            on_released(action, key, xinput, idx);
        }
        e.is_down = false;
        e.was_pressed = false;
    } else if !e.is_down && should {
        on_pressed(action, key, xinput, idx, now, vk);
        e.is_down = true;
        e.was_pressed = true;
        e.repeat_down_tick = now;
        e.repeat_last = now;
    }
}

fn on_pressed(
    action: &Action,
    key: &mut KeyPresser,
    xinput: &w32::XInputApi,
    idx: u32,
    now: f64,
    vk: Option<&Arc<AtomicBool>>,
) {
    match action {
        Action::Key { key: k, repeat } => {
            if *k == crate::keys::Key::VolumeUp || *k == crate::keys::Key::VolumeDown || *k == crate::keys::Key::VolumeMute {
                crate::osd::trigger_volume_feedback();
            }
            key.down(*k);
            let _ = repeat;
            let _ = now;
        }
        Action::MouseClick(btn) => {
            if *btn == mouse::MouseButton::Left {
                crate::platform::note_controller_left_click();
            }
            mouse::button_down(*btn);
        }
        Action::Combo(keys) => {
            for k in keys {
                key.down(*k);
            }
            for k in keys.iter().rev() {
                key.up(*k);
            }
        }
        Action::DisplaySwitch(mode) => {
            let _ = Command::new("displayswitch.exe").arg(mode.arg()).spawn();
        }
        Action::TurnOffController => xinput.power_off_controller(idx),
        Action::ShowDesktop => {
            // `Win+D`, the same combo the reference profile uses.
            key.down(crate::keys::Key::LWin);
            key.down(crate::keys::Key::D);
            key.up(crate::keys::Key::D);
            key.up(crate::keys::Key::LWin);
        }
        Action::VoiceDictation => {}
        Action::TaskView => {
            // `Win+Tab`: open Windows Task View
            key.down(crate::keys::Key::LWin);
            key.down(crate::keys::Key::Tab);
            key.up(crate::keys::Key::Tab);
            key.up(crate::keys::Key::LWin);
        }
        Action::SnapWindowLeft => {
            // `Win+Left`: snap window to left half
            key.down(crate::keys::Key::LWin);
            key.down(crate::keys::Key::Left);
            key.up(crate::keys::Key::Left);
            key.up(crate::keys::Key::LWin);
            crate::osd::show_snap("Left Half");
        }
        Action::SnapWindowRight => {
            // `Win+Right`: snap window to right half
            key.down(crate::keys::Key::LWin);
            key.down(crate::keys::Key::Right);
            key.up(crate::keys::Key::Right);
            key.up(crate::keys::Key::LWin);
            crate::osd::show_snap("Right Half");
        }
        Action::MaximizeWindow => {
            // `Win+Up`: maximize window
            key.down(crate::keys::Key::LWin);
            key.down(crate::keys::Key::Up);
            key.up(crate::keys::Key::Up);
            key.up(crate::keys::Key::LWin);
            crate::osd::show_snap("Maximized");
        }
        Action::MinimizeWindow => {
            // `Win+Down`: minimize window
            key.down(crate::keys::Key::LWin);
            key.down(crate::keys::Key::Down);
            key.up(crate::keys::Key::Down);
            key.up(crate::keys::Key::LWin);
            crate::osd::show_snap("Minimized");
        }
        Action::CloseActiveWindow => {
            // `Alt+F4`: close current active window
            key.down(crate::keys::Key::LMenu);
            key.down(crate::keys::Key::F4);
            key.up(crate::keys::Key::F4);
            key.up(crate::keys::Key::LMenu);
        }
        // `ToggleEnabled` / `ActivateProfile` / `ToggleDragLock` are engine-level commands: they
        // mutate state the engine owns, so they are dispatched from `update`
        // via `pending_command` rather than being executed here.
        Action::ToggleVirtualKeyboard => {
            #[cfg(feature = "gui")]
            crate::vkeyboard::toggle_keyboard(vk.cloned());
            #[cfg(not(feature = "gui"))]
            let _ = vk;
        }
        Action::RadialMenu => {
            #[cfg(feature = "gui")]
            crate::vkeyboard::toggle_radial_menu(vk.cloned());
            #[cfg(not(feature = "gui"))]
            let _ = vk;
        }
        // Modifier / DoNothing / MouseMove / MouseScroll / Excluded / ToggleDragLock: no edge action.
        _ => {}
    }
}

fn on_released(action: &Action, key: &mut KeyPresser, _xinput: &w32::XInputApi, _idx: u32) {
    match action {
        Action::Key { key: k, .. } => key.up(*k),
        Action::MouseClick(btn) => mouse::button_up(*btn),
        _ => {}
    }
}

// Referenced so `Key` import is used even if defaults change.
#[allow(dead_code)]
fn _unused(_: Key) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MouseSettings;

    fn ms() -> MouseSettings {
        MouseSettings::default()
    }

    #[test]
    fn disconnect_releases_digital_state_after_hardware_flag_clears() {
        let xinput = w32::XInputApi::load().expect("Windows XInput is available");
        let mut engine = Engine::new(xinput, Config::default());
        let entry = engine.digital.first_mut().expect("default profile has buttons");
        entry.def.action = Action::DoNothing;
        entry.is_down = true;
        entry.was_pressed = true;
        engine.was_connected = false;

        engine.release_all_if_needed();

        assert!(engine.digital.iter().all(|entry| !entry.is_down && !entry.was_pressed));
    }

    #[test]
    fn back_start_recovers_control_when_disabled_without_repeating() {
        let xinput = w32::XInputApi::load().expect("Windows XInput is available");
        let mut config = Config {
            enabled: false,
            ..Config::default()
        };
        config.controller.haptic_feedback = false;
        let mut engine = Engine::new(xinput, config);
        let mut pad = w32::XINPUT_GAMEPAD { wButtons: B_BACK | B_START, ..Default::default() };
        engine.set_injected_gamepad(Some(pad));
        engine.update(0.0, 0.008);
        assert!(engine.is_enabled());
        engine.update(0.008, 0.008);
        assert!(engine.is_enabled(), "holding the chord must toggle only once");

        pad.wButtons = 0;
        engine.set_injected_gamepad(Some(pad));
        engine.update(0.016, 0.008);
        pad.wButtons = B_BACK | B_START;
        engine.set_injected_gamepad(Some(pad));
        engine.update(0.024, 0.008);
        assert!(!engine.is_enabled());
    }

    #[test]
    fn dead_zone_blocks_everything_below_it() {
        let m = ms();
        let dz = 7849.0;
        // Straight down the dead zone, and just inside it.
        assert_eq!(shape_stick(dz - 1.0, 0.0, &m, dz), (0.0, 0.0));
        assert_eq!(shape_stick(dz * 0.5, dz * 0.5, &m, dz), (0.0, 0.0));
        // Exactly at the edge it starts to move.
        let (x, _) = shape_stick(dz + 500.0, 0.0, &m, dz);
        assert!(x > 0.0, "movement must begin past the dead zone, got {x}");
    }

    #[test]
    fn shaped_output_is_in_raw_xinput_units() {
        // Regression guard: this used to return a 0..1 ratio, which made the
        // acceleration threshold meaningless and starved the pointer of speed.
        let m = ms();
        let (x, y) = shape_stick(32767.0, 0.0, &m, 7849.0);
        assert!(x > 30000.0, "full right must stay in raw units, got {x}");
        assert!(y.abs() < 1.0);
    }

    #[test]
    fn diagonal_gate_snaps_near_centre_to_eight_ways() {
        let mut m = ms();
        m.diagonal_range = 12.0;
        // A tiny off-axis nudge must come out as a pure axis, not a diagonal.
        let (x, y) = shape_stick(3000.0, 400.0, &m, 100.0);
        assert!(
            y.abs() < 0.01 || x.abs() < 0.01,
            "expected a cardinal direction, got ({x}, {y})"
        );
        // Well past the gate the true angle is preserved again.
        let (x2, y2) = shape_stick(20000.0, 20000.0, &m, 100.0);
        assert!((x2 - y2).abs() < 1.0, "diagonal must stay diagonal: {x2},{y2}");
    }

    #[test]
    fn diagonal_default_is_pure_free_analog() {
        let m = ms();
        let (_x, y) = shape_stick(3000.0, 400.0, &m, 100.0);
        assert!(y > 350.0, "true analog angle must be preserved: {y}");
    }

    #[test]
    fn max_zone_saturates_instead_of_climbing_to_the_rail() {
        let mut m = ms();
        m.max_zone = 20000.0;
        // Beyond max_zone the magnitude must stop growing.
        let (x1, _) = shape_stick(20000.0, 0.0, &m, 1000.0);
        let (x2, _) = shape_stick(32767.0, 0.0, &m, 1000.0);
        assert!((x1 - x2).abs() < 1.0, "saturated: {x1} vs {x2}");
    }

    #[test]
    fn pointer_follows_the_stick_in_all_eight_directions() {
        // The regression that started all of this: `atan2` was called as
        // atan2(x, y), rotating every snapped direction by 90 degrees, so
        // "push right" moved the pointer up.
        let m = ms();
        let mut accel = 0.0;
        let dt = 1.0 / 125.0;
        // Past the default gate, so the 8-way snap is not applied and the true
        // angle must be preserved.
        let push = 32000.0f32;
        let d = std::f32::consts::FRAC_1_SQRT_2;
        let cases: [(f32, f32, &str); 8] = [
            (1.0, 0.0, "right"),
            (-1.0, 0.0, "left"),
            (0.0, 1.0, "up"),
            (0.0, -1.0, "down"),
            (d, d, "up-right"),
            (-d, d, "up-left"),
            (d, -d, "down-right"),
            (-d, -d, "down-left"),
        ];
        for (dx, dy, name) in cases {
            let mut cruise = 0.0;
            let (px, py) = pointer_delta((dx * push, dy * push), &m, &mut accel, &mut cruise, dt, 0.0, 0.0);
            // Compare the direction only: AltSnap and the cruise curve scale
            // the magnitude, never the direction.
            let got = (px.signum(), py.signum());
            let want = (dx.signum(), dy.signum());
            assert_eq!(got, want, "{name}: wrong direction");
        }
    }

    #[test]
    fn screen_y_inversion_makes_up_go_up() {
        // `inverted == false` is the default, and it must send a stick-up push
        // to a *negative* screen delta because screen Y grows downward.
        let m = ms();
        let mut accel = 0.0;
        let mut cruise = 0.0;
        let (_px, py) = pointer_delta((0.0, 30000.0), &m, &mut accel, &mut cruise, 1.0 / 125.0, 0.0, 0.0);
        assert!(py > 0.0, "stick up must produce positive stick-space Y");
        let iy = (-py).trunc() as i32;
        assert!(iy <= 0, "stick up must move the cursor up (negative screen Y), got {iy}");
        // And `inverted == true` does the opposite.
        let iy2 = (py * 1.0f32).trunc() as i32;
        assert!(iy2 >= 0, "inverted=true should move the cursor down, got {iy2}");
    }

    #[test]
    fn leftover_carries_sub_pixel_motion_instead_of_losing_it() {
        // At a low speed each frame is a fraction of a pixel; without the carry
        // the pointer would round to zero and never move.
        let mut m = ms();
        m.speed = 30.0; // 30 px/s at 125 Hz = 0.24 px/frame
        m.acceleration = 0.0;
        let dt = 1.0 / 125.0;
        let mut accel = 0.0;
        let mut cruise = 0.0;
        let mut lx = 0.0f32;
        let mut ly = 0.0f32;
        let mut moved = 0i32;
        for _ in 0..25 {
            let (px, py) = pointer_delta((20000.0, 0.0), &m, &mut accel, &mut cruise, dt, lx, ly);
            moved += px.trunc() as i32;
            lx = px.fract();
            ly = py.fract();
        }
        assert!(moved > 0, "slow movement rounded away to nothing over 25 frames");
    }

    #[test]
    fn leftover_is_stable_for_vertical_walks() {
        // The remainder used to be stored before the Y sign flip, so a slow
        // vertical walk oscillated between +0 and -0 and could stall.
        let mut m = ms();
        m.speed = 60.0;
        m.acceleration = 0.0;
        m.altsnap_enabled = false; // keep the per-frame step constant
        let dt = 1.0 / 125.0;
        let mut accel = 0.0;
        let mut cruise = 0.0;
        let mut ly = 0.0f32;
        let mut total = 0.0f32;
        for _ in 0..25 {
            let (px, py) = pointer_delta((0.0, 30000.0), &m, &mut accel, &mut cruise, dt, 0.0, ly);
            let screen_y = -py; // default: not inverted
            total += screen_y.trunc();
            ly = py.fract();
            let _ = px;
        }
        assert!(
            total < 0.0,
            "a sustained upward push must accumulate upward screen motion, got {total}"
        );
    }

    #[test]
    fn temporal_cruise_acceleration_scales_speed_over_time() {
        let m = ms();
        let mut accel = 0.0;
        let mut cruise = 0.0;
        let dt = 0.01;

        // Frame 1: Initial push past modifier_zone (30000 > 10000)
        let (p1, _) = pointer_delta((30000.0, 0.0), &m, &mut accel, &mut cruise, dt, 0.0, 0.0);

        // Simulate holding stick for 1.5 seconds
        for _ in 0..150 {
            let _ = pointer_delta((30000.0, 0.0), &m, &mut accel, &mut cruise, dt, 0.0, 0.0);
        }
        let (p2, _) = pointer_delta((30000.0, 0.0), &m, &mut accel, &mut cruise, dt, 0.0, 0.0);

        assert!(
            p2 > p1 * 1.3,
            "sustained stick hold must progressively cruise faster: p1={p1}, p2={p2}"
        );
    }

    #[test]
    fn scroll_up_is_positive_ticks() {
        // MOUSEEVENTF_WHEEL scrolls up for positive ticks and the stick's Y is
        // positive when pushed up. Negating made "push up" scroll down.
        let m = ms();
        let (_, up) = scroll_ticks((0.0, 30000.0), &m, 8689.0, 1.0 / 125.0, false);
        let (_, down) = scroll_ticks((0.0, -30000.0), &m, 8689.0, 1.0 / 125.0, false);
        assert!(up > 0.0, "pushing up must scroll up (positive ticks), got {up}");
        assert!(down < 0.0, "pushing down must scroll down, got {down}");
        assert!((up + down).abs() < 0.01, "up and down must be symmetric");
    }

    #[test]
    fn scroll_flip_y_reverses_only_the_vertical_axis() {
        let m = ms();
        let a = scroll_ticks((0.0, 30000.0), &m, 8689.0, 0.1, false);
        let b = scroll_ticks((0.0, 30000.0), &m, 8689.0, 0.1, true);
        assert_eq!(a.1, -b.1, "flip_y must invert vertical scroll");
        assert_eq!(a.0, b.0, "flip_y must leave horizontal scroll alone");
    }

    #[test]
    fn scroll_dead_zone_blocks_the_wheel() {
        let m = ms();
        let zone = 8689.0;
        assert_eq!(scroll_ticks((0.0, zone - 1.0), &m, zone, 0.1, false), (0.0, 0.0));
        assert_eq!(scroll_ticks((0.0, 0.0), &m, zone, 0.1, false), (0.0, 0.0));
    }

    #[test]
    fn four_way_scroll_kills_the_drift_axis() {
        let mut m = ms();
        m.scroll_four_way = true;
        m.wheel_speed_x = 4800.0;
        let zone = 8689.0;
        // Mostly vertical: no horizontal component may leak out.
        let (hx, vy) = scroll_ticks((2000.0, 30000.0), &m, zone, 0.1, false);
        assert_eq!(hx, 0.0, "vertical push must not scroll sideways");
        assert!(vy > 0.0);
        // Mostly horizontal: no vertical component.
        let (hx2, vy2) = scroll_ticks((30000.0, 2000.0), &m, zone, 0.1, false);
        assert_eq!(vy2, 0.0, "horizontal push must not scroll vertically");
        assert!(hx2 > 0.0);
    }

    #[test]
    fn scroll_accumulator_emits_sub_pixel_ticks_smoothly() {
        let m = ms();
        let zone = 8689.0;
        let dt = 1.0 / 120.0;
        // Gentle micro-push just above deadzone: produces tiny fractional ticks per frame
        let (_, tiny_tick) = scroll_ticks((0.0, 8900.0), &m, zone, dt, false);
        assert!(tiny_tick > 0.0 && tiny_tick < 1.0, "tiny micro-push must be fractional, got {tiny_tick}");

        let mut accum_y = 0.0f32;
        let mut emitted_ticks = 0;
        for _ in 0..120 {
            accum_y += tiny_tick;
            let step = accum_y.trunc() as i32;
            if step != 0 {
                accum_y -= step as f32;
                emitted_ticks += step;
            }
        }
        assert!(
            emitted_ticks > 0,
            "sub-pixel ticks must accumulate and emit integer wheel steps over 1 second"
        );
    }

    #[test]
    fn diagonal_gate_preserves_direction() {
        // Regression guard: atan2 was called as atan2(x, y), which rotated every
        // snapped direction by 90 degrees — pushing right moved the pointer up.
        let m = ms();
        // Force the gate wide so a moderate push is inside it.
        let d = std::f32::consts::FRAC_1_SQRT_2;
        let cases: [(f32, f32, &str); 8] = [
            (1.0, 0.0, "right"),
            (-1.0, 0.0, "left"),
            (0.0, 1.0, "up"),
            (0.0, -1.0, "down"),
            (d, d, "up-right"),
            (-d, d, "up-left"),
            (d, -d, "down-right"),
            (-d, -d, "down-left"),
        ];
        let push = 5000.0; // inside the default gate
        for (dx, dy, name) in cases {
            let (x, y) = shape_stick(dx * push, dy * push, &m, 100.0);
            let dot = (x * dx + y * dy) / (push.max(1.0));
            assert!(
                dot > 0.99,
                "{name}: snapped to the wrong direction ({x:.0}, {y:.0})"
            );
        }
    }

    #[test]
    fn altsnap_is_continuous_at_its_zone() {
        let mut m = ms();
        m.altsnap_enabled = true;
        m.altsnap_speed = 0.35;
        let z = m.altsnap_zone;
        let below = altsnap_factor(z - 100.0, &m);
        let at = altsnap_factor(z, &m);
        let just_above = altsnap_factor(z + 100.0, &m);
        assert!((below - 1.0).abs() < 1e-6);
        assert!((at - 1.0).abs() < 1e-6, "must be exactly 1.0 at the zone");
        // No visible jump as the stick crosses the threshold.
        assert!(
            (just_above - at).abs() < 0.01,
            "jump of {} at the zone", (just_above - at).abs()
        );
    }

    #[test]
    fn altsnap_never_speeds_the_pointer_up() {
        let mut m = ms();
        m.altsnap_enabled = true;
        m.altsnap_speed = 0.35;
        let mut prev = f32::INFINITY;
        for mag in (0..=32767).step_by(500) {
            let f = altsnap_factor(mag as f32, &m);
            assert!(f <= 1.0 + 1e-6, "AltSnap must only slow down, got {f} at {mag}");
            assert!(f >= m.altsnap_speed - 1e-3, "must not drop below the floor");
            assert!(f <= prev + 1e-6, "curve must be monotonic");
            prev = f;
        }
    }

    #[test]
    fn altsnap_reaches_its_floor_at_full_deflection() {
        let mut m = ms();
        m.altsnap_enabled = true;
        m.altsnap_speed = 0.35;
        let full = altsnap_factor(32767.0, &m);
        assert!(
            (full - m.altsnap_speed).abs() < 1e-3,
            "full deflection should hit the floor, got {full} want {}",
            m.altsnap_speed
        );
    }

    #[test]
    fn altsnap_can_be_switched_off() {
        let mut m = ms();
        m.altsnap_enabled = false;
        assert_eq!(altsnap_factor(32767.0, &m), 1.0);
    }

    #[test]
    fn default_curve_is_monotonic_across_the_whole_range() {
        let m = ms();
        let mut prev_mag = -1.0;
        for mag in (0..=32767).step_by(1000) {
            let (x, y) = shape_stick(mag as f32, 0.0, &m, 7849.0);
            let got = (x * x + y * y).sqrt();
            assert!(got >= prev_mag - 0.01, "response must not go backwards");
            prev_mag = got;
        }
    }

    #[test]
    fn sniper_mode_scaling_reduces_speed() {
        let mut m = ms();
        m.sniper_enabled = true;
        m.sniper_scale = 0.25;
        let lt = 255u8;
        let trigger_ratio = (lt as f32) / 255.0;
        let factor = 1.0 - (1.0 - m.sniper_scale) * trigger_ratio;
        assert!((factor - 0.25).abs() < 1e-4, "at full trigger press factor must be 0.25");

        let lt_half = 128u8;
        let trigger_half_ratio = (lt_half as f32) / 255.0;
        let factor_half = 1.0 - (1.0 - m.sniper_scale) * trigger_half_ratio;
        assert!(factor_half > 0.25 && factor_half < 1.0, "half squeeze must interpolate smoothly");
    }
}

