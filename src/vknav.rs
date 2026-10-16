//! Shared controller-navigation state for the on-screen virtual keyboard.
//!
//! When the keyboard is visible the engine switches into "keyboard mode": it
//! stops the normal mouse/key bindings and instead feeds directional steps and
//! button edges here. The virtual-keyboard overlay drains this each frame to
//! move its cursor and press keys — this is how you drive the keyboard with the
//! controller (the port of the original `GetKeyboardModeBindings` idea).

#[derive(Default, Clone, Copy)]
#[allow(dead_code)]
pub struct VkNav {
    /// Accumulated horizontal grid steps (-1 left, +1 right). Drained by the UI.
    pub dx: i32,
    /// Accumulated vertical grid steps (-1 up, +1 down).
    pub dy: i32,
    /// Whole-row jumps from the shoulder triggers (-1 up, +1 down).
    pub jump_y: i32,
    /// Number of "select key" presses (A).
    pub select: u32,
    /// Number of backspace presses (X).
    pub backspace: u32,
    /// Number of space presses (Y).
    pub space: u32,
    /// Number of close requests (B / O, the ✕ button, or Back).
    pub close: u32,
    /// Move text cursor left in text field (L1 / LB).
    pub caret_left: u32,
    /// Move text cursor right in text field (R1 / RB).
    pub caret_right: u32,
    /// Number of Shift-latch toggles (RightShoulder / RightTrigger).
    pub shift: u32,
    /// Number of AltGr-latch toggles (LeftTrigger).
    pub altgr: u32,
    /// Number of layer toggles (?123 / ABC).
    pub layer: u32,
    /// Number of language toggles (EN / ع).
    pub lang: u32,
    /// Number of Enter presses (Start).
    pub enter: u32,
    /// Number of period presses (B in the original keyboard profile).
    pub period: u32,
    /// Number of hide requests (LeftThumb / L3 in the original).
    pub hide: u32,
    /// Number of "help / hint" requests (RightThumb / R3).
    pub help: u32,
    /// Raw left stick X (-32768..32767) for radial menu steering.
    pub stick_x: f32,
    /// Raw left stick Y (-32768..32767) for radial menu steering.
    pub stick_y: f32,
    /// Accumulated right stick horizontal grid steps (-1 left, +1 right) for Dual-Stick Split mode.
    pub rx: i32,
    /// Accumulated right stick vertical grid steps (-1 up, +1 down) for Dual-Stick Split mode.
    pub ry: i32,
    /// Number of "select left key" presses (Left Trigger in Dual-Stick Split mode).
    pub select_left: u32,
    /// Number of "select right key" presses (Right Trigger in Dual-Stick Split mode).
    pub select_right: u32,
    /// Number of requests to toggle Dual-Stick Split Keyboard mode.
    pub toggle_split: u32,
    /// Number of requests to toggle Keyboard Repositioning / Move Mode (R3).
    pub reposition_toggle: u32,
    /// Raw right stick X (-32768..32767) for keyboard repositioning.
    pub right_stick_x: f32,
    /// Raw right stick Y (-32768..32767) for keyboard repositioning.
    pub right_stick_y: f32,
    /// Whether keyboard repositioning / move mode is currently active.
    pub is_repositioning: bool,
}

impl VkNav {
    /// Read the pending state and reset it (single consumer).
    pub fn drain(&mut self) -> VkNav {
        let is_rep = self.is_repositioning;
        let rx = self.right_stick_x;
        let ry = self.right_stick_y;
        let sx = self.stick_x;
        let sy = self.stick_y;
        let s = *self;
        *self = VkNav::default();
        self.is_repositioning = is_rep;
        self.right_stick_x = rx;
        self.right_stick_y = ry;
        self.stick_x = sx;
        self.stick_y = sy;
        s
    }

    /// Reset any queued close or hide signals so stale cancel requests never kill a newly opened keyboard.
    pub fn clear_close_and_hide(&mut self) {
        self.close = 0;
        self.hide = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_preserves_is_repositioning_and_analog_sticks() {
        let mut nav = VkNav {
            dx: 3,
            select: 1,
            close: 2,
            is_repositioning: true,
            right_stick_x: 12000.0,
            right_stick_y: -8000.0,
            stick_x: 5000.0,
            stick_y: -5000.0,
            ..Default::default()
        };

        let drained = nav.drain();
        assert_eq!(drained.dx, 3);
        assert_eq!(drained.select, 1);
        assert_eq!(drained.close, 2);
        assert!(drained.is_repositioning);
        assert_eq!(drained.right_stick_x, 12000.0);
        assert_eq!(drained.right_stick_y, -8000.0);

        assert_eq!(nav.dx, 0);
        assert_eq!(nav.select, 0);
        assert_eq!(nav.close, 0);
        assert!(nav.is_repositioning, "is_repositioning must persist after drain");
        assert_eq!(nav.right_stick_x, 12000.0, "analog stick X must persist after drain");
        assert_eq!(nav.right_stick_y, -8000.0, "analog stick Y must persist after drain");
        assert_eq!(nav.stick_x, 5000.0, "left stick X must persist after drain");
        assert_eq!(nav.stick_y, -5000.0, "left stick Y must persist after drain");

        nav.close = 1;
        nav.hide = 1;
        nav.clear_close_and_hide();
        assert_eq!(nav.close, 0);
        assert_eq!(nav.hide, 0);
    }
}
