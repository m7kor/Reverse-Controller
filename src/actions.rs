//! The set of actions a controller input can be bound to.
//!
//! These mirror the command/action vocabulary recovered from the decompiled
//! `DefaultBindings` / binding classes. Steam-related commands are intentionally
//! absent (removed at the user's request); their slots are `Excluded` stubs that
//! are documented in the port notes.

use crate::keys::Key;
use crate::mouse::MouseButton;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum DisplayMode {
    External,
    Internal,
    Extend,
    Clone,
}

impl DisplayMode {
    pub fn arg(self) -> &'static str {
        match self {
            DisplayMode::External => "/external",
            DisplayMode::Internal => "/internal",
            DisplayMode::Extend => "/extend",
            DisplayMode::Clone => "/clone",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Modifier {
    Back,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Action {
    /// No operation.
    DoNothing,
    /// Press a keyboard key. `repeat` enables OS-style auto-repeat while held
    /// (true for arrows/space/backspace, false for "norepeat" keys).
    Key { key: Key, repeat: bool },
    /// Hold a mouse button while the input is active.
    MouseClick(MouseButton),
    /// Continuous mouse movement driven by a stick.
    MouseMove { inverted: bool },
    /// Continuous scroll driven by a stick.
    MouseScroll {
        /// Some pads (and several driver/Steam configurations) report the
        /// scroll stick's Y axis already inverted, so "push up" arrives as a
        /// negative value. This flips it so up always scrolls up.
        #[serde(default)]
        flip_y: bool,
    },
    /// One-shot key combo (e.g. Win+M, Ctrl+Alt+Tab). Fired on press edge.
    Combo(Vec<Key>),
    /// Change display configuration via `displayswitch.exe`.
    DisplaySwitch(DisplayMode),
    /// Best-effort controller power-off (hard rumble fallback).
    TurnOffController,
    /// While held, activates this modifier so Back-modified bindings apply.
    Modifier(Modifier),
    /// Show / hide the on-screen virtual keyboard overlay.
    ToggleVirtualKeyboard,
    /// `Win+D` — show the desktop / minimise every window. Present in the
    /// reference "Smooth Native Scroll" profile as `0x1000022`+`0x4d`.
    ShowDesktop,
    /// Toggle the whole engine on or off (the reference profile does this with
    /// Select+Start so it works even while disabled).
    ToggleEnabled,
    /// Activate a named profile at runtime.
    ActivateProfile(String),
    /// Switch to the next saved profile (Default first, then by file name).
    NextProfile,
    /// Switch to the previous saved profile.
    PrevProfile,
    /// Move the pointer to the centre of the next monitor.
    CursorNextMonitor,
    /// Move the pointer to the centre of the active window.
    CenterCursor,
    /// Left double-click at the pointer.
    DoubleClick,
    /// Toggle mouse left-click drag-lock (holds down Left Click until clicked again).
    ToggleDragLock,
    /// Win+H: Start Windows Voice Typing / Speech Dictation.
    VoiceDictation,
    /// Win+Tab: Open Windows Task View.
    TaskView,
    /// Win+Left: Snap active window to the left half.
    SnapWindowLeft,
    /// Win+Right: Snap active window to the right half.
    SnapWindowRight,
    /// Win+Up: Maximize active window.
    MaximizeWindow,
    /// Win+Down: Minimize active window.
    MinimizeWindow,
    /// Alt+F4: Close the active application window.
    CloseActiveWindow,
    /// Win+Ctrl+Right: Switch to next virtual desktop.
    NextVirtualDesktop,
    /// Win+Ctrl+Left: Switch to previous virtual desktop.
    PrevVirtualDesktop,
    /// Summon the 8-slice quick Radial / Pie menu around the cursor.
    RadialMenu,
    /// Continuous window drag / move driven by stick (native AltSnap replacement).
    WindowDrag,
    /// Continuous window resize driven by stick from closest quadrant (AltSnap).
    WindowResize,
    /// Toggle Always-on-Top (Pin window) for window under cursor (AltSnap).
    WindowToggleTopmost,
    /// Roll up / Shade window to titlebar or restore height (AltSnap).
    WindowToggleRoll,
    /// Adjust / Cycle window transparency (100% -> 80% -> 60% -> 40%) (AltSnap).
    WindowCycleTransparency,
    /// Lower window under cursor to bottom of desktop stack (AltSnap Send to Back).
    WindowLower,
    /// Close the window sitting directly under the cursor (AltSnap).
    WindowCloseUnderCursor,
    /// Toggle maximize or restore window under cursor (AltSnap).
    WindowToggleMaximizeUnderCursor,
    /// Minimize window under cursor (AltSnap).
    WindowMinimizeUnderCursor,
    /// Snap window under cursor to top-left quadrant.
    WindowSnapTopLeft,
    /// Snap window under cursor to top-right quadrant.
    WindowSnapTopRight,
    /// Snap window under cursor to bottom-left quadrant.
    WindowSnapBottomLeft,
    /// Snap window under cursor to bottom-right quadrant.
    WindowSnapBottomRight,
    /// A feature that was removed (Steam) or is not ported (help overlay).
    /// Explicitly a no-op and documented.
    Excluded(String),
}

/// Accepts the pre-`flip_y` bare string `"MouseScroll"` in addition to the
/// current `{"MouseScroll": {"flip_y": false}}` form.
///
/// This matters more than it looks: `Action` appears inside `PadBindingDef`,
/// so a single unrecognised spelling fails the *entire* binding list and the
/// mouse silently stops responding.
impl<'de> Deserialize<'de> for Action {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        match v {
            // Legacy bare-string spelling for the variant that used to be a
            // unit variant before it gained `flip_y`.
            serde_json::Value::String(s) => match s.as_str() {
                "MouseScroll" => Ok(Action::MouseScroll { flip_y: false }),
                "DoNothing" => Ok(Action::DoNothing),
                "TurnOffController" => Ok(Action::TurnOffController),
                "ToggleVirtualKeyboard" => Ok(Action::ToggleVirtualKeyboard),
                "ShowDesktop" => Ok(Action::ShowDesktop),
                "ToggleEnabled" => Ok(Action::ToggleEnabled),
                "ToggleDragLock" => Ok(Action::ToggleDragLock),
                "VoiceDictation" => Ok(Action::VoiceDictation),
                "TaskView" => Ok(Action::TaskView),
                "SnapWindowLeft" => Ok(Action::SnapWindowLeft),
                "SnapWindowRight" => Ok(Action::SnapWindowRight),
                "MaximizeWindow" => Ok(Action::MaximizeWindow),
                "MinimizeWindow" => Ok(Action::MinimizeWindow),
                "CloseActiveWindow" => Ok(Action::CloseActiveWindow),
                "NextVirtualDesktop" => Ok(Action::NextVirtualDesktop),
                "PrevVirtualDesktop" => Ok(Action::PrevVirtualDesktop),
                "RadialMenu" => Ok(Action::RadialMenu),
                "WindowDrag" => Ok(Action::WindowDrag),
                "WindowResize" => Ok(Action::WindowResize),
                "WindowToggleTopmost" => Ok(Action::WindowToggleTopmost),
                "WindowToggleRoll" => Ok(Action::WindowToggleRoll),
                "WindowCycleTransparency" => Ok(Action::WindowCycleTransparency),
                "WindowLower" => Ok(Action::WindowLower),
                "WindowCloseUnderCursor" => Ok(Action::WindowCloseUnderCursor),
                "WindowToggleMaximizeUnderCursor" => Ok(Action::WindowToggleMaximizeUnderCursor),
                "WindowMinimizeUnderCursor" => Ok(Action::WindowMinimizeUnderCursor),
                "WindowSnapTopLeft" => Ok(Action::WindowSnapTopLeft),
                "WindowSnapTopRight" => Ok(Action::WindowSnapTopRight),
                "WindowSnapBottomLeft" => Ok(Action::WindowSnapBottomLeft),
                "WindowSnapBottomRight" => Ok(Action::WindowSnapBottomRight),
                "NextProfile" => Ok(Action::NextProfile),
                "PrevProfile" => Ok(Action::PrevProfile),
                "CursorNextMonitor" => Ok(Action::CursorNextMonitor),
                "CenterCursor" => Ok(Action::CenterCursor),
                "DoubleClick" => Ok(Action::DoubleClick),
                other => Err(serde::de::Error::custom(format!("unknown action {other:?}"))),
            },
            serde_json::Value::Object(m) => {
                // Every action is a single-key object: `{"Key": {...}}`.
                let mut it = m.iter();
                let (k, val) = match (it.next(), it.next()) {
                    (Some(kv), None) => kv,
                    _ => return Err(serde::de::Error::custom("an action must have exactly one key")),
                };
                let bad = |what: &str| serde::de::Error::custom(format!("bad {what} payload"));
                match k.as_str() {
                    "Key" => {
                        #[derive(Deserialize)]
                        struct KeyArg {
                            key: Key,
                            #[serde(default)]
                            repeat: bool,
                        }
                        let a: KeyArg = serde_json::from_value(val.clone()).map_err(|_| bad("Key"))?;
                        Ok(Action::Key { key: a.key, repeat: a.repeat })
                    }
                    "MouseMove" => {
                        #[derive(Deserialize)]
                        struct MoveArg {
                            #[serde(default)]
                            inverted: bool,
                        }
                        let a: MoveArg = serde_json::from_value(val.clone()).map_err(|_| bad("MouseMove"))?;
                        Ok(Action::MouseMove { inverted: a.inverted })
                    }
                    "MouseScroll" => {
                        #[derive(Deserialize)]
                        struct ScrollArg {
                            #[serde(default)]
                            flip_y: bool,
                        }
                        let a: ScrollArg = serde_json::from_value(val.clone()).map_err(|_| bad("MouseScroll"))?;
                        Ok(Action::MouseScroll { flip_y: a.flip_y })
                    }
                    "MouseClick" => serde_json::from_value(val.clone())
                        .map(Action::MouseClick)
                        .map_err(|_| bad("MouseClick")),
                    "Combo" => serde_json::from_value(val.clone())
                        .map(Action::Combo)
                        .map_err(|_| bad("Combo")),
                    "DisplaySwitch" => serde_json::from_value(val.clone())
                        .map(Action::DisplaySwitch)
                        .map_err(|_| bad("DisplaySwitch")),
                    "Modifier" => serde_json::from_value(val.clone())
                        .map(Action::Modifier)
                        .map_err(|_| bad("Modifier")),
                    "ActivateProfile" => serde_json::from_value(val.clone())
                        .map(Action::ActivateProfile)
                        .map_err(|_| bad("ActivateProfile")),
                    "Excluded" => serde_json::from_value(val.clone())
                        .map(Action::Excluded)
                        .map_err(|_| bad("Excluded")),
                    "DoNothing" => Ok(Action::DoNothing),
                    "TurnOffController" => Ok(Action::TurnOffController),
                    "ToggleVirtualKeyboard" => Ok(Action::ToggleVirtualKeyboard),
                    "ShowDesktop" => Ok(Action::ShowDesktop),
                    "ToggleEnabled" => Ok(Action::ToggleEnabled),
                    "ToggleDragLock" => Ok(Action::ToggleDragLock),
                    "VoiceDictation" => Ok(Action::VoiceDictation),
                    "TaskView" => Ok(Action::TaskView),
                    "SnapWindowLeft" => Ok(Action::SnapWindowLeft),
                    "SnapWindowRight" => Ok(Action::SnapWindowRight),
                    "MaximizeWindow" => Ok(Action::MaximizeWindow),
                    "MinimizeWindow" => Ok(Action::MinimizeWindow),
                    "CloseActiveWindow" => Ok(Action::CloseActiveWindow),
                    "NextVirtualDesktop" => Ok(Action::NextVirtualDesktop),
                    "PrevVirtualDesktop" => Ok(Action::PrevVirtualDesktop),
                    "RadialMenu" => Ok(Action::RadialMenu),
                    "WindowDrag" => Ok(Action::WindowDrag),
                    "NextProfile" => Ok(Action::NextProfile),
                    "PrevProfile" => Ok(Action::PrevProfile),
                    "CursorNextMonitor" => Ok(Action::CursorNextMonitor),
                    "CenterCursor" => Ok(Action::CenterCursor),
                    "DoubleClick" => Ok(Action::DoubleClick),
                    other => Err(serde::de::Error::custom(format!("unknown action {other:?}"))),
                }
            }
            other => Err(serde::de::Error::custom(format!(
                "an action must be a string or a single-key object, got {other}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_bare_string_mouse_scroll_still_loads() {
        // Configs written before `flip_y` stored this as a plain string. If it
        // stops parsing, the whole binding list is dropped and the mouse dies.
        let a: Action = serde_json::from_str(r#""MouseScroll""#).expect("legacy form");
        assert_eq!(a, Action::MouseScroll { flip_y: false });
    }

    #[test]
    fn current_object_form_round_trips() {
        let a = Action::MouseScroll { flip_y: true };
        let s = serde_json::to_string(&a).expect("serialize");
        let back: Action = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(back, a);
    }

    #[test]
    fn missing_flip_y_defaults_to_false() {
        let a: Action =
            serde_json::from_str(r#"{"MouseScroll":{}}"#).expect("missing flip_y");
        assert_eq!(a, Action::MouseScroll { flip_y: false });
    }

    #[test]
    fn other_unit_actions_keep_their_legacy_string_form() {
        for (json, want) in [
            (r#""DoNothing""#, Action::DoNothing),
            (r#""TurnOffController""#, Action::TurnOffController),
            (r#""ToggleVirtualKeyboard""#, Action::ToggleVirtualKeyboard),
            (r#""ShowDesktop""#, Action::ShowDesktop),
            (r#""ToggleEnabled""#, Action::ToggleEnabled),
            (r#""ToggleDragLock""#, Action::ToggleDragLock),
            (r#""VoiceDictation""#, Action::VoiceDictation),
            (r#""TaskView""#, Action::TaskView),
            (r#""SnapWindowLeft""#, Action::SnapWindowLeft),
            (r#""SnapWindowRight""#, Action::SnapWindowRight),
            (r#""MaximizeWindow""#, Action::MaximizeWindow),
            (r#""MinimizeWindow""#, Action::MinimizeWindow),
            (r#""CloseActiveWindow""#, Action::CloseActiveWindow),
            (r#""NextVirtualDesktop""#, Action::NextVirtualDesktop),
            (r#""PrevVirtualDesktop""#, Action::PrevVirtualDesktop),
            (r#""RadialMenu""#, Action::RadialMenu),
        ] {
            assert_eq!(serde_json::from_str::<Action>(json).expect(json), want);
        }
    }

    #[test]
    fn every_unit_action_loads_in_string_and_object_form() {
        for want in [
            Action::WindowDrag,
            Action::RadialMenu,
            Action::NextProfile,
            Action::PrevProfile,
            Action::NextVirtualDesktop,
            Action::VoiceDictation,
            Action::CursorNextMonitor,
            Action::CenterCursor,
            Action::DoubleClick,
        ] {
            let text = serde_json::to_string(&want).expect("serialize");
            assert_eq!(serde_json::from_str::<Action>(&text).expect(&text), want);
            let object = format!("{{{text}:null}}");
            assert_eq!(serde_json::from_str::<Action>(&object).expect(&object), want);
        }
    }

    #[test]
    fn object_actions_are_unaffected() {
        let a: Action = serde_json::from_str(r#"{"MouseMove":{"inverted":true}}"#).expect("move");
        assert_eq!(a, Action::MouseMove { inverted: true });
        let b: Action = serde_json::from_str(r#"{"MouseClick":"Left"}"#).expect("click");
        assert_eq!(b, Action::MouseClick(MouseButton::Left));
        let c: Action = serde_json::from_str(r#"{"Key":{"key":"Escape","repeat":false}}"#).expect("key");
        assert_eq!(c, Action::Key { key: Key::Escape, repeat: false });
    }

    #[test]
    fn an_unknown_action_is_reported_rather_than_silently_dropped() {
        let err = serde_json::from_str::<Action>(r#""SomethingElse""#).expect_err("must fail");
        assert!(!err.to_string().is_empty());
        let err2 = serde_json::from_str::<Action>(r#"{"NotAnAction":1}"#).expect_err("must fail");
        assert!(err2.to_string().contains("NotAnAction"), "unhelpful error: {err2}");
    }
}
