//! Mouse injection via `SendInput` (mirrors `SendInputMouseMover` /
//! `SendInputMouseClicker`).

use crate::win32 as w32;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

pub fn move_relative(dx: i32, dy: i32) {
    if dx == 0 && dy == 0 {
        return;
    }
    let sent = w32::send_inputs(&[w32::mouse_input(dx, dy, 0, w32::MOUSEEVENTF_MOVE)]);
    if sent == 0 {
        // Fallback: If SendInput was blocked by UIPI (e.g. elevated window focused),
        // displace cursor directly via SetCursorPos so mouse control is never lost!
        if let Some((cx, cy)) = w32::get_cursor_pos() {
            w32::set_cursor_pos(cx + dx, cy + dy);
        }
    }
}

/// Vertical wheel scroll. `ticks` is passed straight through as `mouseData`,
/// exactly like the original `MouseMove(0, 0, ticks, absolute: false)`.
pub fn scroll_vertical(ticks: i32) {
    if ticks == 0 {
        return;
    }
    w32::send_inputs(&[w32::mouse_input(0, 0, ticks as u32, w32::MOUSEEVENTF_WHEEL)]);
}

/// Horizontal wheel scroll. The reference profile drives horizontal scroll from
/// the right stick's X axis, which needs `MOUSEEVENTF_HWHEEL`; positive values
/// scroll right, matching Windows' default direction.
pub fn scroll_horizontal(ticks: i32) {
    if ticks == 0 {
        return;
    }
    w32::send_inputs(&[w32::mouse_input(0, 0, ticks as u32, w32::MOUSEEVENTF_HWHEEL)]);
}

fn button_flags(btn: MouseButton, up: bool) -> (u32, u32) {
    match (btn, up) {
        (MouseButton::Left, false) => (w32::MOUSEEVENTF_LEFTDOWN, 0),
        (MouseButton::Left, true) => (w32::MOUSEEVENTF_LEFTUP, 0),
        (MouseButton::Right, false) => (w32::MOUSEEVENTF_RIGHTDOWN, 0),
        (MouseButton::Right, true) => (w32::MOUSEEVENTF_RIGHTUP, 0),
        (MouseButton::Middle, false) => (w32::MOUSEEVENTF_MIDDLEDOWN, 0),
        (MouseButton::Middle, true) => (w32::MOUSEEVENTF_MIDDLEUP, 0),
        (MouseButton::X1, false) => (w32::MOUSEEVENTF_XDOWN, w32::XBUTTON1),
        (MouseButton::X1, true) => (w32::MOUSEEVENTF_XUP, w32::XBUTTON1),
        (MouseButton::X2, false) => (w32::MOUSEEVENTF_XDOWN, w32::XBUTTON2),
        (MouseButton::X2, true) => (w32::MOUSEEVENTF_XUP, w32::XBUTTON2),
    }
}

pub fn button_down(btn: MouseButton) {
    let (flags, data) = button_flags(btn, false);
    w32::send_inputs(&[w32::mouse_input(0, 0, data, flags)]);
}

pub fn button_up(btn: MouseButton) {
    let (flags, data) = button_flags(btn, true);
    w32::send_inputs(&[w32::mouse_input(0, 0, data, flags)]);
}
