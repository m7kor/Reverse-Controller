//! Virtual-key abstraction and the `SendInput`-based key presser.
//!
//! `KeyPresser::key_event` reproduces the original `SendInputKeyPresser.KeyEvent`:
//! set the VK, resolve the scan code via `MapVirtualKeyW`, add `SCANCODE`, and
//! add `EXTENDEDKEY` (with the 0xE000 scan prefix) for extended keys.

use crate::win32 as w32;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum Key {
    // Letters
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    // Digits
    D0,
    D1,
    D2,
    D3,
    D4,
    D5,
    D6,
    D7,
    D8,
    D9,
    // Function keys
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    // Navigation / editing
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    // Whitespace / control
    Enter,
    Escape,
    Space,
    Tab,
    Backspace,
    // Locks
    CapsLock,
    NumLock,
    ScrollLock,
    PrintScreen,
    Pause,
    // Modifiers (left variants, as used by the original defaults)
    LShift,
    RShift,
    LControl,
    RControl,
    LMenu,
    RMenu,
    LWin,
    RWin,
    Apps,
    // OEM
    OemPeriod,
    OemComma,
    OemMinus,
    OemPlus,
    OemSemicolon,
    OemQuestion,
    Oemtilde,
    OemOpenBrackets,
    OemCloseBrackets,
    OemPipe,
    OemQuotes,
    // Volume / media
    VolumeUp,
    VolumeDown,
    VolumeMute,
    MediaNextTrack,
    MediaPreviousTrack,
    MediaStop,
    MediaPlayPause,
    // Launch / browser
    LaunchMail,
    LaunchApp1,
    LaunchApp2,
    BrowserBack,
    BrowserForward,
    BrowserHome,
    BrowserRefresh,
    BrowserStop,
    BrowserSearch,
    BrowserFavorites,
    Sleep,
    Zoom,
}

impl Key {
    /// Windows virtual-key code.
    pub fn vk(self) -> u16 {
        use Key::*;
        match self {
            A => 0x41,
            B => 0x42,
            C => 0x43,
            D => 0x44,
            E => 0x45,
            F => 0x46,
            G => 0x47,
            H => 0x48,
            I => 0x49,
            J => 0x4A,
            K => 0x4B,
            L => 0x4C,
            M => 0x4D,
            N => 0x4E,
            O => 0x4F,
            P => 0x50,
            Q => 0x51,
            R => 0x52,
            S => 0x53,
            T => 0x54,
            U => 0x55,
            V => 0x56,
            W => 0x57,
            X => 0x58,
            Y => 0x59,
            Z => 0x5A,
            D0 => 0x30,
            D1 => 0x31,
            D2 => 0x32,
            D3 => 0x33,
            D4 => 0x34,
            D5 => 0x35,
            D6 => 0x36,
            D7 => 0x37,
            D8 => 0x38,
            D9 => 0x39,
            F1 => 0x70,
            F2 => 0x71,
            F3 => 0x72,
            F4 => 0x73,
            F5 => 0x74,
            F6 => 0x75,
            F7 => 0x76,
            F8 => 0x77,
            F9 => 0x78,
            F10 => 0x79,
            F11 => 0x7A,
            F12 => 0x7B,
            Left => 0x25,
            Right => 0x27,
            Up => 0x26,
            Down => 0x28,
            Home => 0x24,
            End => 0x23,
            PageUp => 0x21,
            PageDown => 0x22,
            Insert => 0x2D,
            Delete => 0x2E,
            Enter => 0x0D,
            Escape => 0x1B,
            Space => 0x20,
            Tab => 0x09,
            Backspace => 0x08,
            CapsLock => 0x14,
            NumLock => 0x90,
            ScrollLock => 0x91,
            PrintScreen => 0x2C,
            Pause => 0x13,
            LShift => 0xA0,
            RShift => 0xA1,
            LControl => 0xA2,
            RControl => 0xA3,
            LMenu => 0xA4,
            RMenu => 0xA5,
            LWin => 0x5B,
            RWin => 0x5C,
            Apps => 0x5D,
            OemPeriod => 0xBE,
            OemComma => 0xBC,
            OemMinus => 0xBD,
            OemPlus => 0xBB,
            OemSemicolon => 0xBA,
            OemQuestion => 0xBF,
            Oemtilde => 0xC0,
            OemOpenBrackets => 0xDB,
            OemCloseBrackets => 0xDD,
            OemPipe => 0xDC,
            OemQuotes => 0xDE,
            VolumeUp => 0xAF,
            VolumeDown => 0xAE,
            VolumeMute => 0xAD,
            MediaNextTrack => 0xB0,
            MediaPreviousTrack => 0xB1,
            MediaStop => 0xB2,
            MediaPlayPause => 0xB3,
            LaunchMail => 0xB4,
            LaunchApp1 => 0xB6,
            LaunchApp2 => 0xB7,
            BrowserBack => 0xA6,
            BrowserForward => 0xA7,
            BrowserStop => 0xA8,
            BrowserRefresh => 0xA9,
            BrowserFavorites => 0xAA,
            BrowserHome => 0xAB,
            BrowserSearch => 0xAC,
            Sleep => 0x5F,
            Zoom => 0xFB,
        }
    }

    /// Whether the VK needs the EXTENDEDKEY flag (mirrors
    /// `SendInputKeyPresser.IsExtendedKey`).
    pub fn is_extended(self) -> bool {
        use Key::*;
        matches!(
            self,
            Left | Up | Right | Down
                | Home | End
                | PageUp | PageDown
                | Insert | Delete
                | PrintScreen
                | LWin | RWin | Apps
                | RControl | RMenu
                | VolumeUp | VolumeDown | VolumeMute
                | MediaNextTrack | MediaPreviousTrack | MediaStop | MediaPlayPause
                | BrowserBack | BrowserForward | BrowserStop | BrowserRefresh
                | BrowserSearch | BrowserFavorites | BrowserHome
                | LaunchMail | LaunchApp1 | LaunchApp2
                | Sleep | Zoom | Pause
        )
    }

    #[allow(dead_code)] // used by the virtual-keyboard / chatpad layers
    pub fn is_modifier(self) -> bool {
        use Key::*;
        matches!(self, LShift | RShift | LControl | RControl | LMenu | RMenu | LWin | RWin)
    }
}

/// Tracks which virtual keys this process currently holds down, so a release
/// only fires for keys we actually pressed (mirrors `isPressed` in
/// `ButtonKeyBinding`).
pub struct KeyPresser {
    down: HashSet<u16>,
}

impl KeyPresser {
    pub fn new() -> Self {
        KeyPresser { down: HashSet::new() }
    }

    fn send_key(&self, key: Key, up: bool) {
        let vk = key.vk();
        let mut flags = if up { w32::KEYEVENTF_KEYUP } else { 0 };
        let scan = w32::map_vk_to_scancode(vk);
        if key.is_extended() {
            flags |= w32::KEYEVENTF_EXTENDEDKEY;
        }
        w32::send_inputs(&[w32::key_input(vk, scan, flags)]);
    }

    pub fn down(&mut self, key: Key) {
        if self.down.insert(key.vk()) {
            self.send_key(key, false);
        }
    }

    pub fn up(&mut self, key: Key) {
        if self.down.remove(&key.vk()) {
            self.send_key(key, true);
        }
    }

    /// A single tap (down then up) — used for combos' final key and for
    /// one-shot commands.
    #[allow(dead_code)] // used by the virtual-keyboard / chatpad layers
    pub fn tap(&mut self, key: Key) {
        self.send_key(key, false);
        self.send_key(key, true);
    }

    /// A repeat tick: up+down cycle so applications observe auto-repeat
    /// (mirrors `RepeatingButtonKeyBinding.ButtonHeld`'s Released+Pressed).
    pub fn repeat_tick(&mut self, key: Key) {
        self.send_key(key, true);
        self.send_key(key, false);
    }

    /// Inject a single character via `KEYEVENTF_UNICODE` (layout-independent).
    /// Used by the on-screen virtual keyboard for letters, digits, punctuation
    /// and non-Latin scripts such as Arabic.
    pub fn send_char(&self, c: char) {
        let mut buf = [0u16; 2];
        let units = c.encode_utf16(&mut buf).len();
        let mut inputs: Vec<w32::INPUT> = Vec::with_capacity(units * 2);
        for &u in buf.iter().take(units) {
            inputs.push(w32::key_input(0, u, w32::KEYEVENTF_UNICODE));
            inputs.push(w32::key_input(0, u, w32::KEYEVENTF_UNICODE | w32::KEYEVENTF_KEYUP));
        }
        w32::send_inputs(&inputs);
    }

    /// Inject a string atomically in a single SendInput batch (layout-independent Unicode).
    #[allow(dead_code)]
    pub fn send_str(&self, s: &str) {
        let wide: Vec<u16> = s.encode_utf16().collect();
        if wide.is_empty() {
            return;
        }
        let mut inputs: Vec<w32::INPUT> = Vec::with_capacity(wide.len() * 2);
        for &u in &wide {
            inputs.push(w32::key_input(0, u, w32::KEYEVENTF_UNICODE));
            inputs.push(w32::key_input(0, u, w32::KEYEVENTF_UNICODE | w32::KEYEVENTF_KEYUP));
        }
        w32::send_inputs(&inputs);
    }

    /// Release everything we are holding (used on disconnect / disable).
    pub fn release_all(&mut self) {
        let held: Vec<u16> = self.down.iter().copied().collect();
        for vk in held {
            if let Some(key) = key_from_vk(vk) {
                self.send_key(key, true);
            }
        }
        self.down.clear();
    }
}

fn key_from_vk(vk: u16) -> Option<Key> {
    // Small reverse lookup for release_all.
    use Key::*;
    const ALL: &[Key] = &[
        A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
        D0, D1, D2, D3, D4, D5, D6, D7, D8, D9,
        F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
        Left, Right, Up, Down, Home, End, PageUp, PageDown, Insert, Delete,
        Enter, Escape, Space, Tab, Backspace,
        CapsLock, NumLock, ScrollLock, PrintScreen, Pause,
        LShift, RShift, LControl, RControl, LMenu, RMenu, LWin, RWin, Apps,
        OemPeriod, OemComma, OemMinus, OemPlus, OemSemicolon, OemQuestion,
        Oemtilde, OemOpenBrackets, OemCloseBrackets, OemPipe, OemQuotes,
        VolumeUp, VolumeDown, VolumeMute,
        MediaNextTrack, MediaPreviousTrack, MediaStop, MediaPlayPause,
        LaunchMail, LaunchApp1, LaunchApp2,
        BrowserBack, BrowserForward, BrowserStop, BrowserRefresh,
        BrowserSearch, BrowserFavorites, BrowserHome,
        Sleep, Zoom,
    ];
    ALL.iter().copied().find(|k| k.vk() == vk)
}
