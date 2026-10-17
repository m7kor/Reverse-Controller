//! `--doctor`: a plain-text troubleshooting report.
//!
//! Collects everything that commonly explains "the controller does nothing" —
//! configuration errors, binding conflicts, broken profiles, missing XInput,
//! no controller, a fullscreen pause — into one report the user can read or
//! attach to a bug report. Nothing here changes any setting.

use crate::config::Config;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

pub struct Report {
    pub text: String,
    /// Findings that block normal use (marked ✗ in the text).
    pub problems: usize,
}

/// Builds the report line by line, counting problems as it goes.
struct Builder {
    text: String,
    problems: usize,
}

impl Builder {
    fn section(&mut self, title: &str) {
        let _ = writeln!(self.text, "\n== {title} ==");
    }
    fn ok(&mut self, line: impl AsRef<str>) {
        let _ = writeln!(self.text, "  ✓ {}", line.as_ref());
    }
    fn info(&mut self, line: impl AsRef<str>) {
        let _ = writeln!(self.text, "  · {}", line.as_ref());
    }
    fn warn(&mut self, line: impl AsRef<str>) {
        let _ = writeln!(self.text, "  ! {}", line.as_ref());
    }
    fn problem(&mut self, line: impl AsRef<str>) {
        self.problems += 1;
        let _ = writeln!(self.text, "  ✗ {}", line.as_ref());
    }
}

/// Configuration and binding findings; pure, so it is unit-tested.
fn check_config(b: &mut Builder, path: &Path, loaded: Result<Config, String>) -> Option<Config> {
    b.section("Settings");
    b.info(format!("File: {}", path.display()));
    let config = match loaded {
        Ok(config) => {
            b.ok("Settings file is valid");
            config
        }
        Err(error) if !path.exists() => {
            b.info(format!("No settings file yet; built-in defaults are used ({error})"));
            Config::default()
        }
        Err(error) => {
            b.problem(format!("Settings file cannot be used: {error}"));
            return None;
        }
    };
    if config.enabled {
        b.ok("Desktop control is on");
    } else {
        b.warn("Desktop control is off — press Back + Start on the controller to turn it on");
    }
    let profile = if config.active_profile.is_empty() { crate::profiles::DEFAULT_NAME } else { &config.active_profile };
    b.info(format!("Active profile: {profile}"));
    b.info(format!("Controller slot: {}", config.controller.controller_index));
    b.info(format!(
        "Dead zones: left {} · right {} · trigger threshold {}",
        config.controller.left_stick_dead_zone, config.controller.right_stick_dead_zone, config.controller.trigger_threshold
    ));
    if config.controller.left_stick_dead_zone < 1500 {
        b.warn("Left stick dead zone is very small; a worn stick may drift the pointer");
    }
    if config.toggle_hotkey.trim().is_empty() {
        b.info("Keyboard toggle shortcut: off");
    } else {
        b.info(format!("Keyboard toggle shortcut: {}", config.toggle_hotkey));
    }
    let conflicts = crate::config::binding_conflicts(&config.bindings);
    if conflicts.is_empty() {
        b.ok(format!("{} bindings, no conflicts", config.bindings.len()));
    } else {
        for (input, modifier) in conflicts {
            let layer = if modifier.is_some() { "Back + " } else { "" };
            b.problem(format!("{layer}{input:?} is bound more than once; only the first binding works"));
        }
    }
    let has_pointer = config
        .bindings
        .iter()
        .any(|x| matches!(x.action, crate::actions::Action::MouseMove { .. }));
    if !has_pointer {
        b.problem("No stick moves the mouse pointer");
    }
    Some(config)
}

fn check_profiles(b: &mut Builder, dir: &Path) {
    b.section("Profiles");
    b.info(format!("Folder: {}", dir.display()));
    let entries = crate::profiles::list(dir);
    if entries.is_empty() {
        b.info("No saved profiles (the Default profile is always available)");
    }
    for entry in entries {
        match crate::profiles::load(&entry.path) {
            Ok(profile) => {
                let rules = if profile.processes.is_empty() {
                    "manual only".to_string()
                } else {
                    format!("auto for {}", profile.processes.join(", "))
                };
                let conflicts = crate::config::binding_conflicts(&profile.bindings).len();
                if conflicts == 0 {
                    b.ok(format!("{} — {rules}", entry.name));
                } else {
                    b.problem(format!("{} — {conflicts} conflicting binding(s)", entry.name));
                }
            }
            Err(error) => b.problem(format!("{} cannot be loaded: {error}", entry.name)),
        }
    }
}

fn check_system(b: &mut Builder, config: Option<&Config>) {
    b.section("Controller");
    match crate::win32::XInputApi::load() {
        Ok(xinput) => {
            b.ok(format!("XInput loaded ({})", xinput.dll_name));
            let mut found = false;
            for slot in 0..4 {
                if xinput.get_state(slot).is_some() {
                    found = true;
                    let battery = xinput.battery(slot).map(|bat| match (bat.BatteryType, bat.BatteryLevel) {
                        (0x01, _) => "wired".to_string(),
                        (kind, _) if !matches!(kind, 0x02 | 0x03) => "battery level not reported".to_string(),
                        (_, 0) => "battery empty".to_string(),
                        (_, 1) => "battery low".to_string(),
                        (_, 2) => "battery medium".to_string(),
                        _ => "battery full".to_string(),
                    });
                    b.ok(format!("Slot {slot}: connected{}", battery.map(|s| format!(" ({s})")).unwrap_or_default()));
                }
            }
            let others = crate::gamepads::list_devices();
            for dev in &others {
                if found {
                    // Xbox-mode pads from other makers also appear in the
                    // joystick API; XInput is used while one is connected.
                    b.info(format!(
                        "Also visible to the joystick API: {} (joystick {}, vendor {:04X}, product {:04X}) — usually the same controller; used only when no XInput controller is connected",
                        dev.display_name(),
                        dev.id,
                        dev.vendor,
                        dev.product
                    ));
                } else {
                    b.ok(format!(
                        "{} connected (joystick {}, vendor {:04X}, product {:04X}); no vibration or battery level",
                        dev.display_name(),
                        dev.id,
                        dev.vendor,
                        dev.product
                    ));
                }
            }
            let found = found || !others.is_empty();
            if !found {
                b.problem("No controller connected. Plug it in or pair it (Xbox, PlayStation, Switch Pro and generic gamepads work).");
            }
        }
        Err(error) => b.problem(format!("XInput is unavailable: {error}")),
    }

    b.section("Windows");
    b.info(format!("Running as administrator: {}", if crate::platform::is_elevated() { "yes" } else { "no" }));
    if !crate::platform::is_elevated() {
        b.info("Without administrator rights, the pointer cannot control windows that run as administrator");
    }
    let monitors = crate::platform::monitor_work_areas();
    b.info(format!("Monitors: {}", monitors.len().max(1)));
    let fullscreen = crate::platform::is_fullscreen_cached();
    let foreground = crate::platform::foreground_process_name().unwrap_or_else(|| "unknown".into());
    b.info(format!("Focused program: {foreground}"));
    if let Some(config) = config {
        if fullscreen && config.disable_in_fullscreen {
            if config.is_fullscreen_exception(&foreground) {
                b.ok("Focused program is fullscreen but listed as an exception; control stays on");
            } else {
                b.warn("Focused program is fullscreen, so desktop control is paused for it");
            }
        }
        b.info(format!("Start with Windows: {}", if config.autostart { "on" } else { "off" }));
    }
    let log = log_path();
    if let Ok(meta) = std::fs::metadata(&log) {
        b.info(format!("Debug log: {} ({} KB)", log.display(), meta.len() / 1024));
    }
}

fn log_path() -> PathBuf {
    crate::config::default_config_path()
        .parent()
        .map(|d| d.join("debug.log"))
        .unwrap_or_else(|| PathBuf::from("debug.log"))
}

/// Build the full report.
pub fn run(config_path: &Path) -> Report {
    let mut b = Builder { text: String::new(), problems: 0 };
    let _ = writeln!(
        b.text,
        "Reverse Controller {} — diagnostics report",
        env!("CARGO_PKG_VERSION")
    );
    let loaded = if config_path.exists() {
        Config::try_load(config_path)
    } else {
        Err("file not found".into())
    };
    let config = check_config(&mut b, config_path, loaded);
    check_profiles(&mut b, &crate::config::profiles_dir());
    check_system(&mut b, config.as_ref());
    b.section("Summary");
    if b.problems == 0 {
        b.ok("No problems found");
    } else {
        let n = b.problems;
        let _ = writeln!(b.text, "  {n} problem(s) found — see the ✗ lines above");
    }
    Report { text: b.text, problems: b.problems }
}

/// Save the report next to the settings file and return its path.
pub fn save(config_path: &Path, text: &str) -> std::io::Result<PathBuf> {
    let path = config_path
        .parent()
        .map(|d| d.join("diagnostics.txt"))
        .unwrap_or_else(|| PathBuf::from("diagnostics.txt"));
    // A byte-order mark makes Notepad pick UTF-8 for the ✓ / ✗ marks.
    let mut data = vec![0xEF, 0xBB, 0xBF];
    data.extend_from_slice(text.replace('\n', "\r\n").as_bytes());
    crate::storage::write_atomic(&path, &data)?;
    Ok(path)
}

/// Build, save and open the report in Notepad (tray and Settings use this).
pub fn open_report(config_path: &Path) {
    let report = run(config_path);
    match save(config_path, &report.text) {
        Ok(path) => {
            let _ = std::process::Command::new("notepad.exe").arg(path).spawn();
        }
        Err(error) => crate::platform::log_debug(&format!("[doctor] could not save report: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Action;
    use crate::bindings::{Input, PadBindingDef};

    fn builder() -> Builder {
        Builder { text: String::new(), problems: 0 }
    }

    #[test]
    fn default_settings_report_no_config_problems() {
        let mut b = builder();
        let config = check_config(&mut b, Path::new("config.json"), Ok(Config::default()));
        assert!(config.is_some());
        assert_eq!(b.problems, 0, "{}", b.text);
        assert!(b.text.contains("no conflicts"));
    }

    #[test]
    fn conflicts_missing_pointer_and_broken_files_are_problems() {
        let mut cfg = Config::default();
        cfg.bindings.retain(|x| !matches!(x.action, Action::MouseMove { .. }));
        cfg.bindings.push(PadBindingDef { input: Input::A, modifier: None, action: Action::DoNothing });
        let mut b = builder();
        check_config(&mut b, Path::new("config.json"), Ok(cfg));
        assert_eq!(b.problems, 2, "{}", b.text);
        assert!(b.text.contains("A is bound more than once"));

        let dir = std::env::temp_dir().join(format!("cc-doctor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, "{").unwrap();
        let mut b = builder();
        assert!(check_config(&mut b, &path, Err("parse error".into())).is_none());
        assert_eq!(b.problems, 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_settings_file_uses_defaults_without_a_problem() {
        let mut b = builder();
        let missing = Path::new("Z:\\does-not-exist\\config.json");
        assert!(check_config(&mut b, missing, Err("file not found".into())).is_some());
        assert_eq!(b.problems, 0);
    }
}
