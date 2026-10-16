//! Controller inputs, binding definitions, and the default (mouse-mode) profile.
//!
//! The default table is a 1:1 port of `DefaultBindings.GetMouseModeBindings()`
//! with Steam commands removed (per the user's request) and the virtual-keyboard
//! / help-overlay actions marked `Excluded` (not ported).

use crate::actions::{Action, DisplayMode, Modifier};
use crate::keys::Key;
use crate::mouse::MouseButton;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub enum Input {
    // Buttons
    A,
    B,
    X,
    Y,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    Start,
    Back,
    LeftShoulder,
    RightShoulder,
    LeftThumb,
    RightThumb,
    Guide,
    // Triggers
    LeftTrigger,
    RightTrigger,
    // Sticks (as whole inputs)
    LeftStick,
    RightStick,
}

impl Input {
    #[allow(dead_code)] // used by tooling / future per-input filtering
    pub fn is_button(self) -> bool {
        !self.is_trigger() && !self.is_stick()
    }
    #[allow(dead_code)]
    pub fn is_trigger(self) -> bool {
        matches!(self, Input::LeftTrigger | Input::RightTrigger)
    }
    pub fn is_stick(self) -> bool {
        matches!(self, Input::LeftStick | Input::RightStick)
    }
}

/// One binding: an input, an optional active modifier, and the action to run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PadBindingDef {
    pub input: Input,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modifier: Option<Modifier>,
    pub action: Action,
}

pub fn default_profile() -> Vec<PadBindingDef> {
    use Action as A;
    use Input as I;
    let d = |input: I, action: A| PadBindingDef { input, modifier: None, action };
    let dm = |input: I, m: Modifier, action: A| PadBindingDef { input, modifier: Some(m), action };
    let back = Modifier::Back;

    let mut v = vec![
        // --- Base (no modifier) ---
        d(I::A, A::MouseClick(MouseButton::Left)),
        d(I::B, A::Key { key: Key::Backspace, repeat: true }),
        d(I::X, A::MouseClick(MouseButton::Right)),
        d(I::Y, A::Key { key: Key::MediaPlayPause, repeat: false }),
        d(I::DPadLeft, A::Key { key: Key::Left, repeat: true }),
        d(I::DPadRight, A::Key { key: Key::Right, repeat: true }),
        d(I::DPadUp, A::Key { key: Key::Up, repeat: true }),
        d(I::DPadDown, A::Key { key: Key::Down, repeat: true }),
        d(I::LeftShoulder, A::Key { key: Key::MediaPreviousTrack, repeat: false }),
        d(I::RightShoulder, A::Key { key: Key::MediaNextTrack, repeat: false }),
        // LeftThumb (stick click) toggles the on-screen virtual keyboard.
        d(I::LeftThumb, A::ToggleVirtualKeyboard),
        d(I::RightThumb, A::MouseClick(MouseButton::Middle)),
        // Triggers act as held modifier keys.
        d(I::LeftTrigger, A::Key { key: Key::LControl, repeat: false }),
        d(I::RightTrigger, A::Key { key: Key::LShift, repeat: false }),
        // Sticks: mouse movement + scroll.
        d(I::LeftStick, A::MouseMove { inverted: false }),
        d(I::RightStick, A::MouseScroll { flip_y: false }),
        d(I::Start, A::Key { key: Key::Enter, repeat: false }),
        d(I::Back, A::Modifier(Modifier::Back)),
        // Steam removed: Guide always opens the Windows Start menu.
        d(I::Guide, A::Key { key: Key::LWin, repeat: false }),
        // --- Back-modified layer ---
        dm(I::Guide, back, A::TurnOffController),
        dm(I::A, back, A::DisplaySwitch(DisplayMode::External)),
        dm(I::Y, back, A::DisplaySwitch(DisplayMode::Internal)),
        dm(I::X, back, A::Combo(vec![Key::LWin, Key::M])),
        dm(I::RightShoulder, back, A::Combo(vec![Key::LControl, Key::LMenu, Key::Tab])),
        dm(I::LeftShoulder, back, A::VoiceDictation),
        dm(I::Start, back, A::ToggleEnabled),
        dm(I::LeftThumb, back, A::ToggleDragLock),
        dm(I::DPadLeft, back, A::SnapWindowLeft),
        dm(I::DPadRight, back, A::SnapWindowRight),
        dm(I::DPadUp, back, A::Key { key: Key::VolumeUp, repeat: false }),
        dm(I::DPadDown, back, A::Key { key: Key::VolumeDown, repeat: false }),
        dm(I::B, back, A::Key { key: Key::Escape, repeat: false }),
        dm(I::RightThumb, back, A::RadialMenu),
    ];
    // Deterministic ordering: modifier-specific bindings first for a given input
    // (matches `orderby Modifiers descending` in the original runtime).
    v.sort_by_key(|b| std::cmp::Reverse(b.modifier.is_some() as u8));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::Action;

    #[test]
    fn default_profile_has_expected_core_bindings() {
        let p = default_profile();
        // Base layer must map A -> left mouse click.
        assert!(p.iter().any(|b| {
            b.input == Input::A
                && b.modifier.is_none()
                && matches!(b.action, Action::MouseClick(MouseButton::Left))
        }));
        // Back must act as the modifier producer.
        assert!(p.iter().any(|b| {
            b.input == Input::Back && b.modifier.is_none() && matches!(b.action, Action::Modifier(Modifier::Back))
        }));
        // Left stick drives mouse movement, right stick scrolls.
        assert!(p
            .iter()
            .any(|b| b.input == Input::LeftStick && matches!(b.action, Action::MouseMove { .. })));
        assert!(p
            .iter()
            .any(|b| b.input == Input::RightStick && matches!(b.action, Action::MouseScroll { .. })));
        // A healthy default profile is non-trivial.
        assert!(p.len() >= 20, "expected a full default profile, got {}", p.len());
    }

    #[test]
    fn default_profile_contains_no_steam_features() {
        let p = default_profile();
        let dump = format!("{:?}", p);
        let lower = dump.to_lowercase();
        assert!(
            !lower.contains("steam") && !lower.contains("workshop") && !lower.contains("bigpicture"),
            "Steam features must be fully removed"
        );
    }

    #[test]
    fn default_profile_covers_back_modifier_layer() {
        let p = default_profile();
        // There must be Back-modified entries (the second-function row).
        let back_entries = p.iter().filter(|b| b.modifier == Some(Modifier::Back)).count();
        assert!(back_entries >= 5, "expected a Back modifier layer, got {back_entries}");
    }
}
