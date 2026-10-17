//! A short-lived snapshot of the background engine for the separate settings process.
//!
//! The settings window has its own `Engine`; that copy cannot report a toggle
//! made by the controller or an automatic profile switch in the main process.
//! A small, replace-on-write file gives it a truthful view without polling the
//! entire config or making the input loop depend on the GUI.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::os::windows::ffi::OsStrExt;
use std::time::{Duration, SystemTime};

#[cfg_attr(not(feature = "gui"), allow(dead_code))]
const MAX_AGE: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    Disconnected,
    Disabled,
    PausedFullscreen,
    Active,
}

impl RuntimeMode {
    pub fn from_state(connected: bool, enabled: bool, fullscreen_paused: bool) -> Self {
        if !connected {
            Self::Disconnected
        } else if !enabled {
            Self::Disabled
        } else if fullscreen_paused {
            Self::PausedFullscreen
        } else {
            Self::Active
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Disconnected => "Waiting for controller",
            Self::Disabled => "Desktop control off",
            Self::PausedFullscreen => "Paused for fullscreen app",
            Self::Active => "Desktop control active",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RuntimeStatus {
    pub mode: RuntimeMode,
    pub enabled: bool,
    pub connected: bool,
    pub controller_index: u32,
    pub active_profile: String,
    pub battery_level: Option<u8>,
    /// The controller is on a cable, so its battery level means nothing.
    #[serde(default)]
    pub battery_wired: bool,
    /// Which controller is in use ("Xbox controller (XInput)",
    /// "PlayStation 5 (DualSense)", …); empty when none.
    #[serde(default)]
    pub controller_name: String,
}

/// Rewrite the snapshot at most this often while nothing changes. Readers
/// treat anything older than `MAX_AGE` as "background app not running".
const HEARTBEAT: Duration = Duration::from_millis(1500);

/// Publish when the state changed or the heartbeat is due.
pub fn should_publish(last: Option<&(RuntimeStatus, std::time::Instant)>, current: &RuntimeStatus) -> bool {
    match last {
        None => true,
        Some((previous, at)) => previous != current || at.elapsed() >= HEARTBEAT,
    }
}

pub fn path_for_config(config_path: &Path) -> PathBuf {
    config_path.with_extension("runtime.json")
}

pub fn publish(path: &Path, status: &RuntimeStatus) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("runtime-{}.tmp", std::process::id()));
    let data = serde_json::to_vec(status).map_err(std::io::Error::other)?;
    std::fs::write(&temporary, &data)?;
    let from: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // std::fs::rename does not replace an existing file on Windows.
    // MoveFileExW replaces the complete snapshot in one operation, so the GUI
    // never sees a partially written JSON document.
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    // No write-through: this snapshot is disposable and rewritten constantly,
    // so forcing it to the disk would only wear the drive.
    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    let result = unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_REPLACE_EXISTING) };
    if result == 0 {
        // Some managed workspaces deny replace-by-rename even though writing
        // the existing file is allowed. Readers reject an incomplete JSON
        // snapshot and will retry on the next refresh.
        let fallback = std::fs::write(path, &data);
        let _ = std::fs::remove_file(&temporary);
        fallback
    } else {
        Ok(())
    }
}

#[cfg_attr(not(feature = "gui"), allow(dead_code))]
pub fn read_recent(path: &Path) -> Option<RuntimeStatus> {
    let metadata = std::fs::metadata(path).ok()?;
    if SystemTime::now().duration_since(metadata.modified().ok()?).ok()? > MAX_AGE {
        return None;
    }
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_precedence_explains_why_input_is_blocked() {
        assert_eq!(RuntimeMode::from_state(false, false, true), RuntimeMode::Disconnected);
        assert_eq!(RuntimeMode::from_state(true, false, true), RuntimeMode::Disabled);
        assert_eq!(RuntimeMode::from_state(true, true, true), RuntimeMode::PausedFullscreen);
        assert_eq!(RuntimeMode::from_state(true, true, false), RuntimeMode::Active);
    }

    #[test]
    fn publishes_on_change_and_on_heartbeat_only() {
        let status = RuntimeStatus {
            mode: RuntimeMode::Active,
            enabled: true,
            connected: true,
            controller_index: 0,
            active_profile: "Default".into(),
            battery_level: Some(3),
            battery_wired: false,
            controller_name: String::new(),
        };
        assert!(should_publish(None, &status));
        let fresh = (status.clone(), std::time::Instant::now());
        assert!(!should_publish(Some(&fresh), &status), "unchanged state is not rewritten");
        let changed = RuntimeStatus { enabled: false, ..status.clone() };
        assert!(should_publish(Some(&fresh), &changed));
        let old = (status.clone(), std::time::Instant::now() - HEARTBEAT);
        assert!(should_publish(Some(&old), &status), "heartbeat keeps the file fresh");
        assert!(HEARTBEAT * 2 <= MAX_AGE, "readers must see at least one heartbeat per window");
    }

    #[test]
    fn snapshot_can_be_replaced_and_read() {
        let path = std::env::temp_dir().join(format!(
            "cc-runtime-status-{}-{}.json",
            std::process::id(),
            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()
        ));
        let status = RuntimeStatus {
            mode: RuntimeMode::Active,
            enabled: true,
            connected: true,
            controller_index: 0,
            active_profile: "Default".into(),
            battery_level: Some(3),
            battery_wired: false,
            controller_name: String::new(),
        };
        publish(&path, &status).unwrap();
        assert_eq!(read_recent(&path).unwrap().mode, RuntimeMode::Active);
        let changed = RuntimeStatus { mode: RuntimeMode::Disabled, enabled: false, ..status };
        publish(&path, &changed).unwrap();
        assert_eq!(read_recent(&path).unwrap().mode, RuntimeMode::Disabled);
        let _ = std::fs::remove_file(path);
    }
}
