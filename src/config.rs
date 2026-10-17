//! Application settings and profile persistence (JSON).
//!
//! Defaults are taken directly from the decompiled `MouseSettings`,
//! `ControllerSettings` (dead zones from `XInput.Gamepad`), and the default
//! binding table.

use crate::bindings::{default_profile, PadBindingDef};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MouseSettings {
    #[serde(default = "d_speed")]
    pub speed: f32,
    #[serde(default = "d_accel")]
    pub acceleration: f32,
    #[serde(default = "d_accel_threshold")]
    pub acceleration_threshold: f32,
    #[serde(default = "d_wheel_speed")]
    pub wheel_speed: f32,
    /// Horizontal / vertical pointer speed. Zero means "mirror `speed`", so
    /// older configs keep working.
    #[serde(default)]
    pub speed_x: f32,
    #[serde(default)]
    pub speed_y: f32,
    /// Magnitude above which the radial "cruise" curve engages.
    #[serde(default = "d_modifier_zone")]
    pub modifier_zone: f32,
    /// Saturate the stick response at this magnitude instead of 32767, which
    /// is what makes a full push reach max speed without a hard stop.
    #[serde(default = "d_max_zone")]
    pub max_zone: f32,
    /// Inner angle (percent) treated as a pure 8-way direction. Kills the
    /// diagonal drift you get when a stick is nudged slightly off-axis.
    #[serde(default = "d_diagonal_range")]
    pub diagonal_range: f32,
    // ---- AltSnap: fine positioning ----
    /// Magnitude above which the stick is treated as "hugging" its maximum
    /// deflection. This is what the reference profile calls a
    /// `stickmodifierbutton` (AltSnap): the outer ring becomes precise
    /// movement so a pixel can be placed exactly, while the inner region
    /// stays fast.
    ///
    /// This is *separate* from `modifier_zone`, which drives the cruise
    /// speed-up curve: AltSnap is about precision, cruise is about pace.
    #[serde(default = "d_altsnap_zone")]
    pub altsnap_zone: f32,
    /// Enable AltSnap. Disabled by default so stick deflection provides full, responsive speed across modern displays.
    #[serde(default)]
    pub altsnap_enabled: bool,
    /// Speed multiplier applied inside the AltSnap ring. The reference uses
    /// 55 against a base of 68 — roughly 0.8×.
    #[serde(default = "d_altsnap_speed")]
    pub altsnap_speed: f32,
    /// Quantise the right (scroll) stick to the four cardinal directions.
    #[serde(default = "d_true")]
    pub scroll_four_way: bool,
    /// Override: invert the scroll stick's Y. Needed for pads whose driver
    /// already reports that axis inverted, so "push up" scrolls up.
    #[serde(default)]
    pub scroll_flip_y: bool,
    /// Separate horizontal and vertical scroll rates.
    #[serde(default)]
    pub wheel_speed_x: f32,
    #[serde(default)]
    pub wheel_speed_y: f32,
    /// Smooth sniper precision scaling when Left Trigger (LT) is squeezed.
    #[serde(default = "d_true")]
    pub sniper_enabled: bool,
    /// Minimum mouse speed multiplier at full trigger press (0.25 = 4x finer precision).
    #[serde(default = "d_sniper_scale")]
    pub sniper_scale: f32,
}
fn d_sniper_scale() -> f32 {
    0.25
}
fn d_speed() -> f32 {
    1100.0
}
fn d_modifier_zone() -> f32 {
    10000.0
}
fn d_max_zone() -> f32 {
    32700.0
}
fn d_diagonal_range() -> f32 {
    // 0.0 = True 360-degree analog freedom with zero quantization or glitching.
    0.0
}
fn d_altsnap_zone() -> f32 {
    24000.0
}
fn d_altsnap_speed() -> f32 {
    1.0
}
fn d_accel() -> f32 {
    800.0
}
fn d_accel_threshold() -> f32 {
    0.95
}
fn d_wheel_speed() -> f32 {
    3600.0
}
impl Default for MouseSettings {
    fn default() -> Self {
        MouseSettings {
            speed: d_speed(),
            acceleration: d_accel(),
            acceleration_threshold: d_accel_threshold(),
            wheel_speed: d_wheel_speed(),
            speed_x: 0.0,
            speed_y: 0.0,
            modifier_zone: d_modifier_zone(),
            max_zone: d_max_zone(),
            diagonal_range: d_diagonal_range(),
            altsnap_zone: d_altsnap_zone(),
            altsnap_enabled: false,
            altsnap_speed: d_altsnap_speed(),
            scroll_four_way: true,
            scroll_flip_y: false,
            wheel_speed_x: 0.0,
            wheel_speed_y: 0.0,
            sniper_enabled: true,
            sniper_scale: d_sniper_scale(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ControllerSettings {
    #[serde(default = "d_lz")]
    pub left_stick_dead_zone: u16,
    #[serde(default = "d_rz")]
    pub right_stick_dead_zone: u16,
    /// Trigger value (0..255) at which a trigger counts as "pressed".
    #[serde(default = "d_trigger")]
    pub trigger_threshold: u8,
    /// Which XInput user slot (0..3) to read.
    #[serde(default)]
    pub controller_index: u32,
    /// Enable subtle tactile rumble haptic feedback on clicks and shortcuts.
    #[serde(default = "d_true")]
    pub haptic_feedback: bool,
}
fn d_lz() -> u16 {
    7849
}
fn d_rz() -> u16 {
    8689
}
fn d_trigger() -> u8 {
    30
}
impl Default for ControllerSettings {
    fn default() -> Self {
        ControllerSettings {
            left_stick_dead_zone: d_lz(),
            right_stick_dead_zone: d_rz(),
            trigger_threshold: d_trigger(),
            controller_index: 0,
            haptic_feedback: true,
        }
    }
}

/// Persisted on-screen-keyboard preferences.
///
/// Lives here rather than in `vkeyboard` because the whole point is that the
/// layout survives a restart, and the config is compiled even without the GUI.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyboardPrefs {
    /// Remember the on-screen position so the keyboard reopens where the user
    /// dragged it. When false it reopens bottom-centre.
    #[serde(default = "d_true")]
    pub remember_position: bool,
    /// Start in the Arabic layout instead of English.
    #[serde(default)]
    pub start_arabic: bool,
    /// Where the keyboard appears on screen.
    #[serde(default = "d_pos_bottom")]
    pub position: KeyboardPosition,
    /// Scale multiplier applied on top of the base geometry (0.75 - 1.5).
    #[serde(default = "d_one")]
    pub scale: f32,
    /// Show the controller cheat-sheet when the keyboard opens.
    #[serde(default)]
    pub show_hints_on_open: bool,
    /// Start in Steam Deck-style Dual-Stick Split Mode by default.
    #[serde(default)]
    pub split_mode: bool,
    /// Automatically move keyboard opposite the text caret to avoid covering text input.
    #[serde(default = "d_true")]
    pub smart_caret_avoidance: bool,
    /// Subtle tactile/mechanical audio feedback on keystrokes.
    #[serde(default = "d_true")]
    pub mechanical_sfx: bool,
}

/// Persisted voice typing and Whisper AI preferences.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VoiceSettings {
    /// Preferred model filename: "ggml-tiny.bin", "ggml-base.bin", etc.
    #[serde(default = "d_voice_model")]
    pub model: String,
    /// Language code: "ar" for Arabic, "en" for English, or "auto"
    #[serde(default = "d_voice_lang")]
    pub language: String,
    /// VAD silence auto-stop pause duration in milliseconds (default 1200ms)
    #[serde(default = "d_voice_vad_ms")]
    pub vad_silence_ms: u64,
    /// Natural voice macros ("ارسل" -> send Enter, "سطر جديد" -> newline, etc.)
    #[serde(default = "d_true")]
    pub enable_macros: bool,
    /// Custom words, player tags, slang, and terms biased in Whisper prompt
    #[serde(default)]
    pub custom_vocabulary: Vec<String>,
}

fn d_voice_model() -> String {
    "ggml-tiny.bin".to_string()
}
fn d_voice_lang() -> String {
    "ar".to_string()
}
fn d_voice_vad_ms() -> u64 {
    1200
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            model: d_voice_model(),
            language: d_voice_lang(),
            vad_silence_ms: d_voice_vad_ms(),
            enable_macros: true,
            custom_vocabulary: Vec::new(),
        }
    }
}

fn d_pos_bottom() -> KeyboardPosition {
    KeyboardPosition::Bottom
}
fn d_one() -> f32 {
    1.0
}
fn d_default_hotkey() -> String {
    "Ctrl+K".to_string()
}

/// Parse a `"Ctrl+Shift+K"`-style chord into (modifier flags, virtual key).
///
/// Lives next to the config so the settings window and the Win32 hook agree on
/// one spelling, and so an unparseable chord is rejected when it is saved
/// rather than silently never firing.
pub fn parse_hotkey(text: &str) -> Option<(u32, u32)> {
    let mut mods = 0u32;
    let mut key = None;
    for part in text.split(['+', '-']).map(str::trim).filter(|p| !p.is_empty()) {
        let lower = part.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => mods |= crate::platform::MOD_CONTROL,
            "shift" => mods |= crate::platform::MOD_SHIFT,
            "alt" | "menu" => mods |= crate::platform::MOD_MENU,
            "win" | "windows" | "lwin" => mods |= crate::platform::MOD_WIN,
            // A single character is the trigger key. Virtual-key codes for
            // letters are the *uppercase* ASCII value, so normalise here.
            other if other.chars().count() == 1 => {
                let c = other.chars().next().expect("len checked");
                key = Some(c.to_ascii_uppercase() as u32);
            }
            // Named function keys F1..F12.
            _ if lower.starts_with('f') && lower.len() >= 2 && lower.len() <= 3 => {
                let num_str = &lower[1..];
                let n: u32 = match num_str.parse::<u32>() {
                    Ok(v) if (1..=12).contains(&v) => v,
                    _ => return None,
                };
                // VK_F1 = 0x70, VK_F2 = 0x71, ..., VK_F12 = 0x7B
                key = Some(0x70 + n - 1);
            }
            _ => return None,
        }
    }
    key.map(|vk| (mods, vk))
}

/// Base keyboard geometry at scale 1.0, matching `vkeyboard::BASE_W/H`.
/// Kept here so the settings window can preview the real size without the
/// `gui` feature pulling in egui.
pub const KB_BASE_W: f32 = 870.0;
pub const KB_BASE_H: f32 = 354.0;

impl KeyboardPrefs {
    /// Window size at the configured scale.
    pub fn window_size(&self) -> (f32, f32) {
        let s = if self.scale > 0.1 { self.scale } else { 1.0 };
        (KB_BASE_W * s, KB_BASE_H * s)
    }

    /// Where the keyboard should sit for a given work area, honouring the
    /// configured edge and a remembered custom origin.
    ///
    /// `remembered` is the last origin the user dragged the window to; it is
    /// only consulted for `Custom` (and as the fallback when the chosen edge
    /// would place the window off-screen).
    pub fn anchor(&self, area: (i32, i32, i32, i32), remembered: Option<(i32, i32)>) -> (i32, i32) {
        let (ax, ay, aw, ah) = area;
        let (w, h) = self.window_size();
        let w = w as i32;
        let h = h as i32;
        let margin = 12;

        // A remembered position wins only when the user asked to keep it.
        if self.remember_position && self.position == KeyboardPosition::Custom {
            if let Some((x, y)) = remembered {
                return clamp_to_area(x, y, w, h, area);
            }
        }

        let fallback = || {
            let x = ax + (aw - w) / 2;
            let y = ay + ah - h - margin;
            clamp_to_area(x, y, w, h, area)
        };
        match self.position {
            KeyboardPosition::Bottom => {
                // The margin is a preference, not a hard offset: clamping still
                // applies, so an oversized window lands where it can.
                let x = ax + (aw - w) / 2;
                let y = (ay + ah - h - margin).max(ay);
                clamp_to_area(x, y, w, h, area)
            }
            KeyboardPosition::FloatBottom => {
                let x = ax + (aw - w) / 2;
                let float_margin = 110;
                let y = (ay + ah - h - float_margin).max(ay);
                clamp_to_area(x, y, w, h, area)
            }
            KeyboardPosition::Top => {
                let x = ax + (aw - w) / 2;
                clamp_to_area(x, ay + margin, w, h, area)
            }
            KeyboardPosition::Left => {
                let y = ay + (ah - h) / 2;
                clamp_to_area(ax + margin, y, w, h, area)
            }
            KeyboardPosition::Right => {
                let y = ay + (ah - h) / 2;
                clamp_to_area(ax + aw - w - margin, y, w, h, area)
            }
            // Custom without a usable remembered origin degrades to the default
            // edge rather than to whatever the caller happened to pass in.
            KeyboardPosition::Custom => match (self.remember_position, remembered) {
                (true, Some(p)) => clamp_to_area(p.0, p.1, w, h, area),
                _ => fallback(),
            },
        }
    }
}

/// Keep a window of `w`×`h` at `(x, y)` fully inside the work area. A window
/// larger than the area is pinned to its top-left rather than pushed negative.
fn clamp_to_area(x: i32, y: i32, w: i32, h: i32, area: (i32, i32, i32, i32)) -> (i32, i32) {
    let (ax, ay, aw, ah) = area;
    let max_x = ax + aw - w;
    let max_y = ay + ah - h;
    let cx = if max_x <= ax { ax } else { x.clamp(ax, max_x) };
    let cy = if max_y <= ay { ay } else { y.clamp(ay, max_y) };
    (cx, cy)
}

impl Default for KeyboardPrefs {
    fn default() -> Self {
        KeyboardPrefs {
            remember_position: true,
            start_arabic: false,
            position: KeyboardPosition::Bottom,
            scale: 1.0,
            show_hints_on_open: false,
            split_mode: false,
            smart_caret_avoidance: true,
            mechanical_sfx: true,
        }
    }
}

/// Preferred edge for the on-screen keyboard. `Custom` means "wherever the
/// user last dragged it", and is only valid with `remember_position`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyboardPosition {
    Bottom,
    Top,
    Left,
    Right,
    Custom,
    #[serde(rename = "floatbottom")]
    FloatBottom,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "d_true")]
    pub enabled: bool,
    /// Temporarily disable input while a borderless-fullscreen app has focus
    /// (port of `FullScreenLoopDisabler`).
    #[serde(default)]
    pub disable_in_fullscreen: bool,
    /// Start automatically with Windows (HKCU Run key).
    #[serde(default)]
    pub autostart: bool,
    /// Auto-show the on-screen virtual keyboard when a text field/search box is clicked.
    #[serde(default = "d_true")]
    pub vk_auto_show: bool,
    /// File stem of the profile to activate at startup. Empty = built-in
    /// default bindings from `Config::bindings`.
    #[serde(default)]
    pub active_profile: String,
    /// Automatically switch profiles when a matching process gains focus.
    #[serde(default = "d_true")]
    pub auto_profile_switch: bool,
    /// System-wide hotkey that toggles input on and off. Empty means "off".
    ///
    /// The default is Ctrl+K, which nothing else in the Windows shell claims.
    #[serde(default = "d_default_hotkey")]
    pub toggle_hotkey: String,
    /// Minimalist floating OSD / HUD overlay for volume, battery, profiles, and snapping.
    #[serde(default = "d_true")]
    pub osd_enabled: bool,
    /// Keep the on-screen keyboard geometry between sessions.
    #[serde(default)]
    pub keyboard: KeyboardPrefs,
    #[serde(default)]
    pub mouse: MouseSettings,
    #[serde(default)]
    pub controller: ControllerSettings,
    /// Local Whisper AI voice dictation and speech-to-text preferences.
    #[serde(default)]
    pub voice: VoiceSettings,
    /// Settings GUI language ("en" or "ar", default "en").
    #[serde(default = "d_gui_language")]
    pub gui_language: String,
    /// The binding table. Missing => built-in default profile.
    #[serde(default = "default_profile")]
    pub bindings: Vec<PadBindingDef>,
}
fn d_true() -> bool {
    true
}
fn d_gui_language() -> String {
    "en".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            disable_in_fullscreen: false,
            autostart: false,
            vk_auto_show: true,
            active_profile: String::new(),
            auto_profile_switch: true,
            toggle_hotkey: d_default_hotkey(),
            osd_enabled: true,
            keyboard: KeyboardPrefs::default(),
            mouse: MouseSettings::default(),
            controller: ControllerSettings::default(),
            voice: VoiceSettings::default(),
            gui_language: d_gui_language(),
            bindings: default_profile(),
        }
    }
}

impl Config {
    pub fn try_load(path: &std::path::Path) -> Result<Config, String> {
        if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("amgp")) {
            let config = crate::amgp::load_amgp_file(path)?;
            config.validate()?;
            return Ok(config);
        }
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        let clean_text = text.strip_prefix('\u{feff}').unwrap_or(&text);
        match serde_json::from_str(clean_text) {
            Ok(config) => {
                let config: Config = config;
                config.validate()?;
                Ok(config)
            },
            Err(error) => {
                // Older imports can contain the original XML under a .json name.
                if text.contains("<gamecontroller") {
                    if let Ok(config) = crate::amgp::parse_amgp_xml(&text) {
                        config.validate()?;
                        return Ok(config);
                    }
                }
                Err(format!("parse {}: {error}", path.display()))
            }
        }
    }

    pub fn load(path: &std::path::Path) -> Config {
        if !path.exists() {
            return Config::default();
        }
        Self::try_load(path).unwrap_or_else(|error| {
            eprintln!("[config] {error}; using defaults");
            Config::default()
        })
    }

    pub fn save(&self, path: &std::path::Path) -> std::io::Result<()> {
        self.validate().map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let text = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::storage::write_atomic(path, &text)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !self.toggle_hotkey.trim().is_empty() && parse_hotkey(&self.toggle_hotkey).is_none() {
            return Err("toggle_hotkey must be empty or a valid chord such as Ctrl+K".into());
        }
        let m = &self.mouse;
        for (name, value, max) in [
            ("speed", m.speed, 100000.0), ("acceleration", m.acceleration, 100000.0),
            ("speed_x", m.speed_x, 100000.0), ("speed_y", m.speed_y, 100000.0),
            ("wheel_speed", m.wheel_speed, 100000.0), ("wheel_speed_x", m.wheel_speed_x, 100000.0),
            ("wheel_speed_y", m.wheel_speed_y, 100000.0),
            ("acceleration_threshold", m.acceleration_threshold, 1.0),
            ("modifier_zone", m.modifier_zone, 32767.0), ("max_zone", m.max_zone, 32767.0),
            ("altsnap_zone", m.altsnap_zone, 32767.0), ("diagonal_range", m.diagonal_range, 100.0),
            ("altsnap_speed", m.altsnap_speed, 1.0), ("sniper_scale", m.sniper_scale, 1.0),
        ] {
            if !value.is_finite() || !(0.0..=max).contains(&value) {
                return Err(format!("mouse.{name} must be finite and between 0 and {max}"));
            }
        }
        if !self.keyboard.scale.is_finite() || !(0.5..=3.0).contains(&self.keyboard.scale) {
            return Err("keyboard.scale must be between 0.5 and 3".into());
        }
        if self.controller.controller_index > 3 {
            return Err("controller.controller_index must be between 0 and 3".into());
        }
        if self.controller.left_stick_dead_zone > 32767 || self.controller.right_stick_dead_zone > 32767 {
            return Err("controller dead zones must be between 0 and 32767".into());
        }
        if self.voice.vad_silence_ms < 200 || self.voice.vad_silence_ms > 10000 {
            return Err("voice.vad_silence_ms must be between 200 and 10000 milliseconds".into());
        }
        Ok(())
    }
}

/// Default config file location: `%APPDATA%\ControllerCompanionRust\config.json`,
/// falling back to the executable directory.
pub fn default_config_path() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        return PathBuf::from(appdata)
            .join("ControllerCompanionRust")
            .join("config.json");
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("config.json")))
        .unwrap_or_else(|| PathBuf::from("config.json"))
}

/// Profiles folder location: `%APPDATA%\ControllerCompanionRust\profiles`
pub fn profiles_dir() -> PathBuf {
    if let Ok(appdata) = std::env::var("APPDATA") {
        let p = PathBuf::from(appdata)
            .join("ControllerCompanionRust")
            .join("profiles");
        let _ = std::fs::create_dir_all(&p);
        return p;
    }
    let p = PathBuf::from("profiles");
    let _ = std::fs::create_dir_all(&p);
    p
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Action;
    use crate::bindings::Input;

    #[test]
    fn rejects_invalid_numeric_settings_before_saving() {
        let mut cfg = Config::default();
        assert!(cfg.validate().is_ok());
        cfg.mouse.speed = f32::NAN;
        assert!(cfg.validate().unwrap_err().contains("speed"));
        cfg.mouse.speed = -1.0;
        assert!(cfg.validate().is_err());
        cfg.mouse.speed = 1100.0;
        cfg.controller.controller_index = 4;
        assert!(cfg.validate().unwrap_err().contains("controller_index"));
        cfg.controller.controller_index = 0;
        cfg.keyboard.scale = 0.0;
        assert!(cfg.validate().unwrap_err().contains("scale"));
    }

    #[test]
    fn configuration_rejects_invalid_recovery_hotkey_but_allows_disabling_it() {
        let mut cfg = Config { toggle_hotkey: "Hyper+K".into(), ..Config::default() };
        assert!(cfg.validate().unwrap_err().contains("toggle_hotkey"));
        cfg.toggle_hotkey.clear();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn config_json_roundtrip() {
        let cfg = Config::default();
        let s = serde_json::to_string(&cfg).expect("serialize");
        let back: Config = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(back.mouse.speed, cfg.mouse.speed);
        assert_eq!(back.controller.left_stick_dead_zone, 7849);
        assert_eq!(back.bindings.len(), cfg.bindings.len());
        assert!(back.enabled);
        assert!(back.vk_auto_show);
    }

    #[test]
    fn corrupt_config_is_rejected_for_live_reload() {
        let path = std::env::temp_dir().join(format!(
            "cc-corrupt-config-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"{\"enabled\":").unwrap();
        assert!(Config::try_load(&path).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn partial_json_falls_back_to_defaults() {
        // A minimal/older config must still parse with sensible defaults.
        let back: Config = serde_json::from_str(r#"{"mouse":{"speed":123.0}}"#).expect("parse");
        assert_eq!(back.mouse.speed, 123.0);
        assert_eq!(back.mouse.acceleration, 800.0); // default
        assert_eq!(back.controller.right_stick_dead_zone, 8689); // default
        assert!(!back.bindings.is_empty()); // default profile
    }

    #[test]
    fn the_real_shipped_config_shape_still_parses() {
        // This is the exact spelling an older build wrote: `MouseScroll` was a
        // unit variant, so it saved as a bare string. If that stops parsing,
        // the *whole* binding list is discarded and the mouse goes dead with
        // no error shown.
        let json = r#"{
          "enabled": true,
          "mouse": { "speed": 600.0, "acceleration": 1500.0,
                     "acceleration_threshold": 0.95, "wheel_speed": 4800.0 },
          "controller": { "left_stick_dead_zone": 7849, "right_stick_dead_zone": 8689,
                          "trigger_threshold": 30, "controller_index": 0 },
          "bindings": [
            { "input": "LeftStick",  "action": { "MouseMove": { "inverted": false } } },
            { "input": "RightStick", "action": "MouseScroll" },
            { "input": "A",          "action": { "MouseClick": "Left" } },
            { "input": "B",          "action": { "Key": { "key": "Backspace", "repeat": true } } },
            { "input": "Guide",      "action": "TurnOffController" },
            { "input": "RightThumb", "action": { "Excluded": "help overlay (not ported)" } }
          ]
        }"#;
        let cfg: Config = serde_json::from_str(json).expect("shipped config must parse");
        assert_eq!(cfg.bindings.len(), 6, "every binding must survive");
        // The two that matter for the reported "mouse does not work" symptom.
        let move_b = cfg
            .bindings
            .iter()
            .find(|b| b.input == Input::LeftStick)
            .expect("LeftStick binding");
        assert!(matches!(move_b.action, Action::MouseMove { inverted: false }));
        let scroll_b = cfg
            .bindings
            .iter()
            .find(|b| b.input == Input::RightStick)
            .expect("RightStick binding");
        assert_eq!(scroll_b.action, Action::MouseScroll { flip_y: false });
    }

    #[test]
    fn hotkey_chords_parse_into_modifier_flags_and_a_key() {
        use crate::platform::{MOD_CONTROL, MOD_MENU, MOD_SHIFT, MOD_WIN};
        assert_eq!(parse_hotkey("Ctrl+K"), Some((MOD_CONTROL, 'K' as u32)));
        assert_eq!(parse_hotkey("ctrl+k"), Some((MOD_CONTROL, 'K' as u32)));
        assert_eq!(
            parse_hotkey("Ctrl+Shift+A"),
            Some((MOD_CONTROL | MOD_SHIFT, 'A' as u32))
        );
        assert_eq!(parse_hotkey("Win+F5"), Some((MOD_WIN, 0x74)));
        assert_eq!(parse_hotkey("Alt-F1"), Some((MOD_MENU, 0x70)));
        assert_eq!(parse_hotkey("Ctrl+Alt+Shift+Win+Q"), Some((MOD_CONTROL | MOD_MENU | MOD_SHIFT | MOD_WIN, 'Q' as u32)));
    }

    #[test]
    fn an_empty_hotkey_disables_it() {
        assert_eq!(parse_hotkey(""), None);
        assert_eq!(parse_hotkey("   "), None);
    }

    #[test]
    fn nonsense_hotkeys_are_rejected_rather_than_half_parsed() {
        // A typo must not silently register a different key.
        assert_eq!(parse_hotkey("Ctrl"), None, "no trigger key");
        assert_eq!(parse_hotkey("Hyper+K"), None, "unknown modifier");
        assert_eq!(parse_hotkey("Ctrl+F13"), None, "no F13");
        assert_eq!(parse_hotkey("Ctrl+KK"), None, "two trigger keys");
    }

    #[test]
    fn the_default_hotkey_is_usable() {
        let cfg = Config::default();
        assert_eq!(parse_hotkey(&cfg.toggle_hotkey), Some((crate::platform::MOD_CONTROL, 'K' as u32)));
    }

    #[test]
    fn the_live_config_file_on_disk_still_parses() {
        // Proves the fix against the user's actual saved config rather than a
        // hand-written fixture. Skipped when the file is absent (CI, fresh
        // install).
        let path = crate::config::default_config_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            eprintln!("skipping: no config at {}", path.display());
            return;
        };
        match serde_json::from_str::<Config>(&text) {
            Ok(c) => {
                assert!(!c.bindings.is_empty(), "live config lost every binding");
                assert!(
                    c.bindings.iter().any(|b| b.input == Input::LeftStick),
                    "live config has no LeftStick binding — the mouse cannot move"
                );
            }
            Err(e) => panic!("live config {} failed to parse: {e}", path.display()),
        }
    }

    /// A 1920×1080 monitor with no taskbar inset.
    const AREA: (i32, i32, i32, i32) = (0, 0, 1920, 1080);

    #[test]
    fn keyboard_default_sits_centred_along_the_bottom() {
        let p = KeyboardPrefs::default();
        let (w, h) = p.window_size();
        let (x, y) = p.anchor(AREA, None);
        assert_eq!(x, (1920 - w as i32) / 2, "bottom means centred horizontally");
        assert!(y + h as i32 <= 1080, "must stay on screen");
        assert_eq!(y, 1080 - h as i32 - 12);
    }

    #[test]
    fn keyboard_honours_every_configured_edge() {
        let (w, h) = KeyboardPrefs::default().window_size();
        let w = w as i32;
        let h = h as i32;
        // Top and Left still centre on the perpendicular axis, so a keyboard
        // never sticks out awkwardly.
        for (pos, want) in [
            (KeyboardPosition::Top, ((1920 - w) / 2, 12)),
            (KeyboardPosition::FloatBottom, ((1920 - w) / 2, 1080 - h - 110)),
            (KeyboardPosition::Left, (12, (1080 - h) / 2)),
            (KeyboardPosition::Right, (1920 - w - 12, (1080 - h) / 2)),
        ] {
            let p = KeyboardPrefs { position: pos, ..KeyboardPrefs::default() };
            let (x, y) = p.anchor(AREA, None);
            assert_eq!((x, y), want, "{pos:?} anchored wrong");
        }
    }

    #[test]
    fn keyboard_respects_a_multi_monitor_work_area_origin() {
        // A second monitor at x=1920 must not place the keyboard at 0.
        let area = (1920, 0, 1280, 1024);
        let p = KeyboardPrefs::default();
        let (_w, _) = p.window_size();
        let (x, _) = p.anchor(area, None);
        assert!(x >= 1920, "keyboard escaped onto the primary monitor: {x}");
    }

    #[test]
    fn keyboard_never_lands_off_screen_when_remembered_position_is_stale() {
        // A position saved on a monitor that is no longer attached.
        let p = KeyboardPrefs {
            remember_position: true,
            position: KeyboardPosition::Custom,
            ..KeyboardPrefs::default()
        };
        let (w, h) = p.window_size();
        let (x, y) = p.anchor(AREA, Some((-5000, -5000)));
        assert!(x >= 0 && y >= 0, "stale position was not clamped: {x},{y}");
        assert!(x + w as i32 <= 1920 && y + h as i32 <= 1080);
    }

    #[test]
    fn keyboard_custom_position_is_ignored_when_not_remembering() {
        let p = KeyboardPrefs {
            remember_position: false,
            position: KeyboardPosition::Custom,
            ..KeyboardPrefs::default()
        };
        let (x, y) = p.anchor(AREA, Some((10, 10)));
        assert_ne!((x, y), (10, 10), "must fall back to the default edge");
    }

    #[test]
    fn keyboard_scale_multiplies_the_base_geometry() {
        let p = KeyboardPrefs { scale: 1.25, ..KeyboardPrefs::default() };
        let (w, h) = p.window_size();
        assert!((w - KB_BASE_W * 1.25).abs() < 0.01);
        assert!((h - KB_BASE_H * 1.25).abs() < 0.01);
        // A degenerate scale must not collapse the window to nothing.
        let bad = KeyboardPrefs { scale: 0.0, ..KeyboardPrefs::default() };
        assert_eq!(bad.window_size(), (KB_BASE_W, KB_BASE_H));
    }

    #[test]
    fn keyboard_too_large_for_the_screen_pins_to_the_origin() {
        // Wider than an 800×600 display: the window cannot be centred
        // horizontally, so it must start at the left edge instead of going
        // negative.
        let small = (0, 0, 800, 600);
        let p = KeyboardPrefs { scale: 1.5, position: KeyboardPosition::Bottom, ..KeyboardPrefs::default() };
        let (w, h) = p.window_size();
        assert!(w > 800.0, "precondition: keyboard must overflow the width");
        let (x, y) = p.anchor(small, None);
        assert_eq!(x, 0, "overflowing width should pin to the left edge");
        // Height still fits, so bottom anchoring keeps its 12px margin.
        assert_eq!(y, 600 - h as i32 - 12);
    }

    #[test]
    fn voice_settings_roundtrip_and_validation() {
        let mut cfg = Config::default();
        assert_eq!(cfg.voice.vad_silence_ms, 1200);
        assert_eq!(cfg.voice.language, "ar");
        assert!(cfg.voice.enable_macros);
        assert!(cfg.voice.custom_vocabulary.is_empty());

        cfg.voice.custom_vocabulary.push("فيفا".into());
        cfg.voice.custom_vocabulary.push("فورتنايت".into());
        let s = serde_json::to_string(&cfg).expect("serialize");
        let back: Config = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(back.voice.custom_vocabulary.len(), 2);
        assert_eq!(back.voice.custom_vocabulary[0], "فيفا");

        // Invalid vad_silence_ms rejected
        cfg.voice.vad_silence_ms = 50;
        assert!(cfg.validate().is_err());
        cfg.voice.vad_silence_ms = 1200;
        assert!(cfg.validate().is_ok());
    }
}
