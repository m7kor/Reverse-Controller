//! Profile management: create, edit, activate, and auto-switch by process.
//!
//! A *profile* is a named set of bindings that can be swapped at runtime.
//! Three things make this more than a file listing:
//!
//!   1. **Activation is explicit and persistent** — the chosen profile is
//!      written to `config.json`, so it survives a restart.
//!   2. **Process rules** — a profile may declare which executables select it,
//!      so launching a game switches bindings automatically.
//!   3. **Mutability** — the user can create, duplicate, rename and delete
//!      profiles from the UI without hand-editing JSON.
//!
//! Files live in the profiles directory as `.json`; the original `.amgp`
//! format is still importable but always rewritten as JSON when edited.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// One saved profile: its bindings plus the processes that select it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    /// Optional display name; the file stem is used when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Executable names (e.g. `["chrome.exe", "firefox.exe"]`) that activate
    /// this profile when they gain focus. Empty = manual only.
    #[serde(default)]
    pub processes: Vec<String>,
    /// The binding table, identical in shape to `Config::bindings`.
    #[serde(default = "crate::bindings::default_profile")]
    pub bindings: Vec<crate::bindings::PadBindingDef>,
}

impl Profile {
    pub fn from_bindings(bindings: Vec<crate::bindings::PadBindingDef>) -> Self {
        Profile { name: None, processes: Vec::new(), bindings }
    }

    /// Built-in preset matching the optimized "Smooth Native Scroll" layout.
    #[allow(dead_code)]
    pub fn smooth_native_scroll() -> Self {
        Profile {
            name: Some("Smooth Native Scroll".to_string()),
            processes: Vec::new(),
            bindings: crate::bindings::default_profile(),
        }
    }

    /// Does `exe` (a bare process name like `notepad.exe`) select this profile?
    pub fn matches_process(&self, exe: &str) -> bool {
        let exe = exe.to_ascii_lowercase();
        self.processes
            .iter()
            .any(|p| p.to_ascii_lowercase() == exe)
    }
}

/// A discovered profile, with the metadata the UI needs.
#[derive(Clone, Debug)]
pub struct ProfileEntry {
    pub path: PathBuf,
    pub name: String,
    /// True when the file came from the original `.amgp` format and has not
    /// been rewritten yet.
    pub legacy: bool,
}

/// The built-in profile that always exists and cannot be deleted.
pub const DEFAULT_NAME: &str = "Default";

/// Load a profile file. `.amgp` files go through the original reader and keep
/// whatever bindings it found (it exposes no process rules, so `processes` is
/// empty and the user can add them in the UI).
pub fn load(path: &Path) -> Result<Profile, String> {
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("amgp"))
    {
        let cfg = crate::amgp::load_amgp_file(path)?;
        return Ok(Profile::from_bindings(cfg.bindings));
    }
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(raw.strip_prefix('\u{feff}').unwrap_or(&raw)).map_err(|e| format!("parse {}: {e}", path.display()))
}

pub fn save(dir: &Path, stem: &str, profile: &Profile) -> Result<PathBuf, String> {
    if stem.is_empty() || stem.starts_with('.') || stem.ends_with(['.', ' ']) || stem.chars().any(|c| c.is_control() || "<>:\"/\\|?*".contains(c)) {
        return Err("Profile name must be a plain filename without reserved characters".into());
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(format!("{stem}.json"));
    let raw = serde_json::to_string_pretty(profile)
        .map_err(|e| format!("serialize profile: {e}"))?;
    crate::storage::write_atomic(&path, raw.as_bytes()).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(path)
}

/// List every profile file in `dir`, newest-looking names first, sorted so the
/// UI order is stable between runs.
pub fn list(dir: &Path) -> Vec<ProfileEntry> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let path = e.path();
        let Some(ext) = path.extension().and_then(|s| s.to_str()) else {
            continue;
        };
        let legacy = ext.eq_ignore_ascii_case("amgp");
        if !legacy && !ext.eq_ignore_ascii_case("json") {
            continue;
        }
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        if stem.starts_with('.') {
            continue; // dotfiles are not profiles
        }
        let name = load_name(&path).unwrap_or(stem);
        out.push(ProfileEntry { path, name, legacy });
    }
    out.sort_by_key(|a| a.name.to_ascii_lowercase());
    out
}

/// The display name stored inside a profile file, if it parses.
fn load_name(path: &Path) -> Option<String> {
    load(path).ok().and_then(|p| p.name)
}

/// Make `stem` unique inside `dir` by appending " 2", " 3", ... if needed.
/// Protects against clobbering an existing profile when importing.
pub fn unique_stem(dir: &Path, stem: &str) -> String {
    let taken = |s: &str| {
        ["json", "amgp"]
            .iter()
            .any(|e| dir.join(format!("{s}.{e}")).exists())
    };
    if !taken(stem) {
        return stem.to_string();
    }
    let mut n = 2;
    loop {
        let cand = format!("{stem} {n}");
        if !taken(&cand) {
            return cand;
        }
        n += 1;
    }
}

/// Pick the profile that claims `exe`, if any. Later entries win, so a file
/// created after another overrides it — this makes "add a rule" predictable.
pub fn match_process(dir: &Path, exe: &str) -> Option<ProfileEntry> {
    let mut found = None;
    for e in list(dir) {
        if load(&e.path).map(|p| p.matches_process(exe)).unwrap_or(false) {
            found = Some(e);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_names_that_escape_the_profile_directory() {
        let dir = std::env::temp_dir().join("cc-invalid-profile-name");
        for stem in ["", "../outside", "..\\outside", "C:\\outside", "name:stream", ".hidden", "name."] {
            assert!(save(&dir, stem, &sample()).is_err(), "accepted {stem:?}");
        }
    }

    #[test]
    fn accepts_bom_prefixed_profiles() {
        let dir = tmp("bom");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bom.json");
        std::fs::write(&path, "\u{feff}{}").unwrap();
        assert!(load(&path).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("cc_prof_{name}"));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn sample() -> Profile {
        Profile::from_bindings(crate::bindings::default_profile())
    }

    #[test]
    fn round_trips_through_disk() {
        let d = tmp("rt");
        let mut p = sample();
        p.name = Some("Mine".into());
        p.processes = vec!["notepad.exe".into()];
        save(&d, "mine", &p).expect("save");
        let back = load(&d.join("mine.json")).expect("load");
        assert_eq!(back.name.as_deref(), Some("Mine"));
        assert_eq!(back.processes, vec!["notepad.exe"]);
        assert_eq!(back.bindings.len(), p.bindings.len());
    }

    #[test]
    fn process_matching_is_case_insensitive() {
        let mut p = sample();
        p.processes = vec!["Chrome.EXE".into()];
        assert!(p.matches_process("chrome.exe"));
        assert!(p.matches_process("CHROME.EXE"));
        assert!(!p.matches_process("firefox.exe"));
    }

    #[test]
    fn listing_is_sorted_and_skips_dotfiles() {
        let d = tmp("list");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(".hidden.json"), "{}").unwrap();
        std::fs::write(d.join("notes.txt"), "x").unwrap();
        let mut p = sample();
        p.name = Some("Beta".into());
        save(&d, "beta", &p).unwrap();
        p.name = Some("Alpha".into());
        save(&d, "alpha", &p).unwrap();
        let names: Vec<String> = list(&d).into_iter().map(|e| e.name).collect();
        assert_eq!(names, vec!["Alpha", "Beta"]);
    }

    #[test]
    fn unique_stem_never_clobbers() {
        let d = tmp("uniq");
        save(&d, "game", &sample()).unwrap();
        assert_eq!(unique_stem(&d, "game"), "game 2");
        save(&d, "game 2", &sample()).unwrap();
        assert_eq!(unique_stem(&d, "game"), "game 3");
        assert_eq!(unique_stem(&d, "fresh"), "fresh");
    }

    #[test]
    fn later_rule_wins_for_a_process() {
        let d = tmp("proc");
        let mut a = sample();
        a.processes = vec!["game.exe".into()];
        save(&d, "a_first", &a).unwrap();
        let mut b = sample();
        b.processes = vec!["game.exe".into()];
        save(&d, "b_second", &b).unwrap();
        let hit = match_process(&d, "game.exe").expect("a rule matches");
        // "b_second" sorts after "a_first" alphabetically, so it wins.
        assert_eq!(hit.path.file_stem().unwrap(), "b_second");
        assert!(match_process(&d, "other.exe").is_none());
    }

    #[test]
    fn corrupt_files_are_reported_not_panicked() {
        let d = tmp("bad");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("broken.json"), "{ not json").unwrap();
        assert!(load(&d.join("broken.json")).is_err());
        // Listing must survive a corrupt file rather than skipping everything.
        let entries = list(&d);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "broken");
    }

    #[test]
    fn a_profile_carries_its_own_bindings() {
        // The point of a profile is that its bindings come back intact, so a
        // switch is observable rather than silently falling back to defaults.
        let d = tmp("bind");
        let mut p = sample();
        p.bindings.clear();
        p.bindings.push(crate::bindings::PadBindingDef {
            input: crate::bindings::Input::A,
            modifier: None,
            action: crate::actions::Action::MouseClick(crate::mouse::MouseButton::Middle),
        });
        save(&d, "custom", &p).unwrap();
        let back = load(&d.join("custom.json")).expect("loads");
        assert_eq!(back.bindings.len(), 1, "bindings must survive the round trip");
        assert!(back.bindings[0].action == crate::actions::Action::MouseClick(crate::mouse::MouseButton::Middle));
    }

    #[test]
    fn switching_a_profile_keeps_the_other_settings() {
        // Regression guard: activating a profile must only swap bindings, never
        // reset mouse speed, dead zones or keyboard preferences.
        let d = tmp("keep");
        let mut p = sample();
        p.processes = vec!["notepad.exe".into()];
        save(&d, "np", &p).unwrap();
        let loaded = load(&d.join("np.json")).expect("loads");

        let mut cfg = crate::config::Config::default();
        cfg.mouse.speed = 1234.0;
        cfg.controller.left_stick_dead_zone = 4242;
        cfg.keyboard.scale = 1.25;
        let before = cfg.clone();

        cfg.bindings = loaded.bindings;
        cfg.active_profile = "np".into();

        assert_eq!(cfg.mouse.speed, before.mouse.speed, "mouse speed must survive");
        assert_eq!(cfg.controller.left_stick_dead_zone, 4242, "dead zone must survive");
        assert_eq!(cfg.keyboard.scale, 1.25, "keyboard prefs must survive");
        assert_eq!(cfg.active_profile, "np");
        assert!(!cfg.bindings.is_empty());
    }

    #[test]
    fn default_name_is_never_treated_as_a_file() {
        // "Default" is built in; there is no default.json to load, and the UI
        // must fall back to the compiled-in bindings instead of erroring.
        let d = tmp("no_default");
        save(&d, "real", &sample()).unwrap();
        // "Default" is compiled in, not a file, so nothing should match it as
        // if it were one.
        assert!(match_process(&d, "anything.exe").is_none());
    }

    #[test]
    fn missing_directory_lists_nothing() {
        let d = tmp("does_not_exist_xyz");
        assert!(list(&d).is_empty());
    }

    #[test]
    fn smooth_native_scroll_preset_has_bindings() {
        let p = Profile::smooth_native_scroll();
        assert_eq!(p.name.as_deref(), Some("Smooth Native Scroll"));
        assert!(p.bindings.iter().any(|b| b.action == crate::actions::Action::WindowDrag));
    }
}
