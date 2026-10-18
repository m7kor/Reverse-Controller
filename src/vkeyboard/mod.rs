//! On-screen virtual keyboard — Windows 11 Fluent 2 style.
//!
//! Submodules:
//!   * `layout`: Key definitions, layer structures, language builders (Windows dynamic & Arabic).
//!   * `nav`: Grid navigation algorithms, cursor clamping, visual column tracking.
//!   * `render`: Fluent 2 keycaps, colors, vector glyphs, and chrome headers.
//!   * `toolbar`: Live typing capsule and word prediction suggestions.
//!   * `radial`: Rapid 8-slice circular pie menu for controller actions.

#![cfg(feature = "gui")]

pub mod layout;
pub mod nav;
pub mod radial;
pub mod render;
pub mod toolbar;

#[allow(unused_imports)]
pub use layout::{
    align_rows, build_layout, ch_str, intern, key_px_w, layout_px, layout_units, repeatable,
    resolve_char, row_px, shown_alt, shown_label, trailing_word, Act, Glyph, KDef, Lang, Layer,
    ShiftState, CHROME_H, GAP, KEY_H, PAD, ROUNDING, ROW_GAP, TOOLBAR_H, UNIT,
};
pub use nav::*;
pub use radial::*;
pub use render::*;
pub use toolbar::*;

use crate::keys::{Key as NavKey, KeyPresser};
use crate::platform;
use crate::vknav::VkNav;
use eframe::egui;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const WINDOW_TITLE: &str = "CCVKbd";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OverlayMode {
    Keyboard,
    RadialMenu,
}

static OVERLAY_MODE: AtomicU8 = AtomicU8::new(0); // 0 = Keyboard, 1 = RadialMenu
static EGUI_CTX: Mutex<Option<egui::Context>> = Mutex::new(None);

/// Wake up the virtual keyboard UI immediately from any thread.
pub fn request_vkeyboard_repaint() {
    if let Ok(guard) = EGUI_CTX.lock() {
        if let Some(ctx) = &*guard {
            ctx.request_repaint();
        }
    }
}

/// The latest keyboard preferences, for code outside the overlay thread.
static CURRENT_PREFS: Mutex<Option<crate::config::KeyboardPrefs>> = Mutex::new(None);
/// Where the user last dragged or moved the keyboard, saved on exit.
static LAST_POSITION: Mutex<Option<(i32, i32)>> = Mutex::new(None);

fn current_prefs() -> crate::config::KeyboardPrefs {
    CURRENT_PREFS.lock().ok().and_then(|g| g.clone()).unwrap_or_default()
}

/// The position the user last moved the keyboard to in this session.
pub fn last_position() -> Option<(i32, i32)> {
    LAST_POSITION.lock().ok().and_then(|g| *g)
}

fn note_last_position(pos: (i32, i32)) {
    if let Ok(mut g) = LAST_POSITION.lock() {
        *g = Some(pos);
    }
}

/// Where the keyboard opens: the remembered spot (kept on its own monitor) or
/// the configured edge of the monitor the user is working on.
pub fn opening_origin(prefs: &crate::config::KeyboardPrefs, remembered: Option<(i32, i32)>) -> (i32, i32) {
    match remembered.filter(|_| prefs.remember_position) {
        Some((x, y)) => {
            let (w, h) = prefs.window_size();
            let area = platform::work_area_at(x + w as i32 / 2, y + h as i32 / 2);
            crate::config::KeyboardPrefs { position: crate::config::KeyboardPosition::Custom, ..prefs.clone() }
                .anchor(area, Some((x, y)))
        }
        None => prefs.anchor(platform::work_area_active(), None),
    }
}

/// Show the on-screen keyboard at its preferred size and place.
pub fn show_keyboard(flag: &Arc<AtomicBool>) {
    OVERLAY_MODE.store(0, Ordering::SeqCst);
    platform::push_foreground();
    flag.store(true, Ordering::SeqCst);
    let prefs = current_prefs();
    let remembered = last_position().or(prefs.last_position);
    let (x, y) = opening_origin(&prefs, remembered);
    let (w, h) = prefs.window_size();
    platform::place_window(WINDOW_TITLE, x, y, w.round() as i32, h.round() as i32);
    platform::set_window_visible(WINDOW_TITLE, true);
    request_vkeyboard_repaint();
}

/// Hide the overlay (keyboard or radial menu).
pub fn hide_keyboard(flag: &Arc<AtomicBool>) {
    flag.store(false, Ordering::SeqCst);
    platform::set_window_visible(WINDOW_TITLE, false);
    platform::move_window(WINDOW_TITLE, -4000, -4000);
    request_vkeyboard_repaint();
}

/// Toggle the on-screen virtual keyboard.
pub fn toggle_keyboard(flag: Option<Arc<AtomicBool>>) {
    if let Some(f) = flag {
        let cur_vis = f.load(Ordering::SeqCst);
        let cur_mode = OVERLAY_MODE.load(Ordering::SeqCst);
        if cur_vis && cur_mode == 0 {
            hide_keyboard(&f);
        } else {
            show_keyboard(&f);
        }
    }
}

/// Toggle the quick radial / pie menu.
pub fn toggle_radial_menu(flag: Option<Arc<AtomicBool>>) {
    if let Some(f) = flag {
        let cur_vis = f.load(Ordering::SeqCst);
        let cur_mode = OVERLAY_MODE.load(Ordering::SeqCst);
        if cur_vis && cur_mode == 1 {
            hide_keyboard(&f);
            return;
        } else {
            OVERLAY_MODE.store(1, Ordering::SeqCst);
            platform::push_foreground();
            f.store(true, Ordering::SeqCst);
            let (wx, wy, ww, wh) = platform::work_area_active();
            let want = (430.0f32, 430.0f32);
            let origin = (
                (wx as f32 + (ww as f32 - want.0) / 2.0).round() as i32,
                (wy as f32 + (wh as f32 - want.1) / 2.0).round() as i32,
            );
            platform::place_window(WINDOW_TITLE, origin.0, origin.1, want.0 as i32, want.1 as i32);
            platform::set_window_visible(WINDOW_TITLE, true);
        }
        request_vkeyboard_repaint();
    }
}

static TRACE: AtomicBool = AtomicBool::new(false);
static VKBD_SHOWN: AtomicBool = AtomicBool::new(false);

#[allow(dead_code)]
pub fn is_vk_actually_shown() -> bool {
    VKBD_SHOWN.load(Ordering::SeqCst)
}

static LIVE_PREFS: Mutex<Option<crate::config::KeyboardPrefs>> = Mutex::new(None);

pub fn update_prefs(prefs: crate::config::KeyboardPrefs) {
    if let Ok(mut lock) = CURRENT_PREFS.lock() {
        *lock = Some(prefs.clone());
    }
    if let Ok(mut lock) = LIVE_PREFS.lock() {
        *lock = Some(prefs);
    }
}

pub fn set_trace(on: bool) {
    TRACE.store(on, Ordering::Relaxed);
}

// Window size at scale 1.0
pub const BASE_W: f32 = crate::config::KB_BASE_W;
pub const BASE_H: f32 = crate::config::KB_BASE_H;

// Timing
pub const REPEAT_DELAY: Duration = Duration::from_millis(320);
pub const REPEAT_RATE: Duration = Duration::from_millis(30);
pub const DOUBLE_TAP: Duration = Duration::from_millis(350);
pub const FLASH: Duration = Duration::from_millis(140);
pub const IDLE_REPAINT: Duration = Duration::from_millis(1500);

#[derive(Clone, Copy)]
struct Repeat {
    pos: (usize, usize),
    act: Act,
    next: Instant,
}

#[derive(Clone, Copy)]
struct DragAnchor {
    cursor: (f32, f32),
    origin: (f32, f32),
}

pub struct VKbdApp {
    visible: Arc<AtomicBool>,
    nav: Arc<Mutex<VkNav>>,
    demo: bool,
    prefs: crate::config::KeyboardPrefs,
    lang: Lang,
    last_sys_lang: Option<Lang>,
    last_sys_hkl: usize,
    layer: Layer,
    shift: ShiftState,
    last_shift_tap: Option<Instant>,
    cursor: (usize, usize),
    goal_x: f32,
    rows: Vec<Vec<KDef>>,
    dirty: bool,
    key: KeyPresser,
    shown: bool,
    warm_frames: u8,
    sized: (f32, f32),
    screen: (f32, f32),
    ctrl: bool,
    alt: bool,
    altgr: bool,
    caps: bool,
    hints: bool,
    repeat: Option<Repeat>,
    flash: Option<((usize, usize), Instant)>,
    drag_anchor: Option<DragAnchor>,
    initial_hidden: bool,
    frame: u32,
    mode: OverlayMode,
    radial_selected_slice: Option<usize>,
    window_pos: Option<(i32, i32)>,
    typing_target: usize,
    typed_buffer: String,
    typed_word: String,
    toolbar_focused: bool,
    toolbar_col: usize,
    split_mode: bool,
    left_cursor: (usize, usize),
    right_cursor: (usize, usize),
    goal_x_left: f32,
    goal_x_right: f32,
    last_cursor_moved: u8,
    repositioning: bool,
    reposition_rx: f32,
    reposition_ry: f32,
    last_mouse_down_pos: Option<egui::Pos2>,
    grid_nav_active: bool,
    running: Arc<AtomicBool>,
}

impl VKbdApp {
    pub fn new(
        visible: Arc<AtomicBool>,
        nav: Arc<Mutex<VkNav>>,
        lang: Lang,
        demo: bool,
        prefs: crate::config::KeyboardPrefs,
        running: Arc<AtomicBool>,
    ) -> Self {
        let (screen_w, screen_h) = platform::screen_size();
        let rows = build_layout(lang, Layer::Abc);
        let goal_x = if rows.len() > 1 { key_center(&rows[1], 0) } else { 0.0 };
        let left_cursor = (1, 0);
        let right_col = if rows.len() > 1 && rows[1].len() > 7 { 7 } else { 0 };
        let right_cursor = (1, right_col);
        let goal_x_left = if rows.len() > 1 { key_center(&rows[1], left_cursor.1) } else { 0.0 };
        let goal_x_right = if rows.len() > 1 { key_center(&rows[1], right_cursor.1) } else { 0.0 };
        let split_mode = prefs.split_mode;
        let window_pos = prefs.last_position.filter(|_| prefs.remember_position);
        Self {
            visible,
            nav,
            demo,
            prefs,
            lang,
            last_sys_lang: None,
            last_sys_hkl: 0,
            layer: Layer::Abc,
            shift: ShiftState::Off,
            last_shift_tap: None,
            cursor: (2, 4),
            goal_x,
            rows,
            dirty: false,
            key: KeyPresser::new(),
            shown: false,
            warm_frames: 0,
            sized: (0.0, 0.0),
            screen: (screen_w as f32, screen_h as f32),
            ctrl: false,
            alt: false,
            altgr: false,
            caps: false,
            hints: false,
            repeat: None,
            flash: None,
            drag_anchor: None,
            initial_hidden: false,
            frame: 0,
            mode: OverlayMode::Keyboard,
            radial_selected_slice: None,
            window_pos,
            typing_target: 0,
            typed_buffer: String::new(),
            typed_word: String::new(),
            toolbar_focused: false,
            toolbar_col: 0,
            split_mode,
            left_cursor,
            right_cursor,
            goal_x_left,
            goal_x_right,
            last_cursor_moved: 0,
            repositioning: false,
            reposition_rx: 0.0,
            reposition_ry: 0.0,
            last_mouse_down_pos: None,
            grid_nav_active: false,
            running,
        }
    }

    fn rebuild(&mut self) {
        self.rows = build_layout(self.lang, self.layer);
        self.dirty = false;
        self.clamp_cursor();
    }

    fn clamp_cursor(&mut self) {
        let nrows = self.rows.len();
        if nrows == 0 {
            self.cursor = (0, 0);
            return;
        }
        let r = self.cursor.0.min(nrows - 1);
        let c = nearest_col(&self.rows[r], self.goal_x);
        self.cursor = (r, c);
    }

    pub fn sync_typing_target(&mut self, hwnd: usize) {
        if hwnd != 0 && hwnd != platform::overlay_hwnd() && hwnd != self.typing_target {
            self.typing_target = hwnd;
            platform::set_foreground_target(hwnd);
            self.typed_buffer.clear();
            self.typed_word.clear();
        }
    }

    fn set_cursor(&mut self, r: usize, c: usize) {
        if let Some(row) = self.rows.get(r) {
            if c < row.len() {
                self.cursor = (r, c);
                self.goal_x = key_center(row, c);
            }
        }
    }

    fn hide(&mut self) {
        self.visible.store(false, Ordering::SeqCst);
        self.reset_transient();
    }

    fn reset_transient(&mut self) {
        self.ctrl = false;
        self.alt = false;
        self.altgr = false;
        self.shift = ShiftState::Off;
        self.hints = false;
        self.repeat = None;
        self.flash = None;
        self.drag_anchor = None;
        self.toolbar_focused = false;
        self.toolbar_col = 0;
        self.cursor = (2, 4);
        self.key.release_all();
        if self.layer != Layer::Abc {
            self.layer = Layer::Abc;
            self.dirty = true;
        }
    }

    fn key_state(&self, act: Act) -> (bool, bool) {
        match act {
            Act::Shift => (self.shift.on(), self.shift == ShiftState::Locked),
            Act::Ctrl => (self.ctrl, false),
            Act::Alt => (self.alt, false),
            Act::AltGr => (self.altgr, false),
            Act::Caps => (self.caps, self.caps),
            Act::Hints => (self.hints, false),
            _ => (false, false),
        }
    }

    fn tap_key(&mut self, k: NavKey) {
        crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
        crate::engine::trigger_haptic(7500, 9500, 0.022);
        let mods = self.armed_mods();
        for m in &mods {
            self.key.down(*m);
        }
        self.key.down(k);
        self.key.up(k);
        for m in mods.iter().rev() {
            self.key.up(*m);
        }
        self.release_oneshots();
    }

    fn type_char(&mut self, ch: char) {
        crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
        crate::engine::trigger_haptic(7500, 9500, 0.022);
        let mods = self.armed_mods();
        if !mods.is_empty() {
            if let Some(vk) = layout::char_to_nav_key(ch) {
                for m in &mods {
                    self.key.down(*m);
                }
                self.key.down(vk);
                self.key.up(vk);
                for m in mods.iter().rev() {
                    self.key.up(*m);
                }
                self.release_oneshots();
                return;
            }
        }
        for m in &mods {
            self.key.down(*m);
        }
        self.key.send_char(ch);
        for m in mods.iter().rev() {
            self.key.up(*m);
        }
        self.release_oneshots();
    }

    /// Remember the word just finished, if the user allows learning.
    fn commit_word(&mut self) {
        if self.prefs.learn_words && !self.typed_word.is_empty() {
            crate::predict::learn(&self.typed_word);
        }
    }

    pub fn execute_action(&mut self, act: Act) {
        let ends_word = match act {
            Act::Space | Act::Enter | Act::ShiftEnter | Act::Tab => true,
            Act::Char(c, _) => !c.is_alphabetic() && c != '\'',
            _ => false,
        };
        if ends_word {
            self.commit_word();
        }
        if matches!(
            act,
            Act::Key(_)
                | Act::WinKey
                | Act::Copy
                | Act::Cut
                | Act::Undo
                | Act::Redo
                | Act::DeleteWord
                | Act::SelectAll
                | Act::Paste
                | Act::TaskView
                | Act::ShowDesktop
                | Act::CloseActiveWindow
        ) || ((self.ctrl || self.alt || self.altgr)
            && matches!(act, Act::Char(..) | Act::CharOrStr(..) | Act::Backspace | Act::Space | Act::Str(_)))
        {
            self.typed_word.clear();
            self.typed_buffer.clear();
        }
        let shortcut = self.ctrl || self.alt || self.altgr;
        match act {
            Act::Char(c, shifted) => {
                let ch = resolve_char(c, shifted, self.shift.on(), self.caps);
                self.type_char(ch);
                if !shortcut {
                    self.typed_buffer.push(ch);
                    self.typed_word = trailing_word(&self.typed_buffer);
                }
            }
            Act::CharOrStr(c, shifted) => {
                if self.shift.on() && !shortcut {
                    self.key.send_str(shifted);
                    self.release_oneshots();
                    self.typed_buffer.push_str(shifted);
                } else {
                    self.type_char(c);
                    if !shortcut {
                        self.typed_buffer.push(c);
                    }
                }
                self.typed_word = trailing_word(&self.typed_buffer);
            }
            Act::Str(s) => {
                let mods = self.armed_mods();
                if !mods.is_empty() && (s == "لا" || s == "لآ") {
                    self.tap_key(NavKey::B);
                } else {
                    let out = if self.shift.on() && s == "لا" { "لآ" } else { s };
                    self.key.send_str(out);
                    self.release_oneshots();
                    self.typed_buffer.push_str(out);
                    self.typed_word = trailing_word(&self.typed_buffer);
                }
            }
            Act::Key(k) => self.tap_key(k),
            Act::WinKey => self.tap_key(NavKey::LWin),
            Act::Backspace => {
                self.tap_key(NavKey::Backspace);
                self.typed_buffer.pop();
                self.typed_word = trailing_word(&self.typed_buffer);
            }
            Act::Enter => {
                self.tap_key(NavKey::Enter);
                self.typed_buffer.clear();
                self.typed_word.clear();
            }
            Act::ShiftEnter => {
                crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
                crate::engine::trigger_haptic(7500, 9500, 0.022);
                let mods = self.armed_mods();
                for m in &mods {
                    self.key.down(*m);
                }
                self.key.down(NavKey::LShift);
                self.key.down(NavKey::Enter);
                self.key.up(NavKey::Enter);
                self.key.up(NavKey::LShift);
                for m in mods.iter().rev() {
                    self.key.up(*m);
                }
                self.release_oneshots();
                self.typed_buffer.push('\n');
                self.typed_word.clear();
            }
            Act::Tab => {
                self.tap_key(NavKey::Tab);
                self.typed_buffer.push('\t');
                self.typed_word.clear();
            }
            Act::Space => {
                self.type_char(' ');
                self.typed_buffer.push(' ');
                self.typed_word.clear();
            }
            Act::Shift => self.tap_shift(),
            Act::Ctrl => {
                self.ctrl = !self.ctrl;
                self.dirty = true;
            }
            Act::Alt => {
                self.alt = !self.alt;
                self.dirty = true;
            }
            Act::AltGr => {
                self.altgr = !self.altgr;
                self.dirty = true;
            }
            Act::Caps => {
                self.caps = !self.caps;
                self.dirty = true;
            }
            Act::Hints => {
                self.hints = !self.hints;
                self.dirty = true;
            }
            Act::Lang => {
                self.lang = match self.lang {
                    Lang::En => Lang::Ar,
                    Lang::Ar => Lang::En,
                };
                self.last_sys_lang = Some(self.lang);
                self.dirty = true;
                self.key.down(NavKey::LWin);
                self.key.tap(NavKey::Space);
                self.key.up(NavKey::LWin);
            }
            Act::ToggleLayer => {
                self.layer = match self.layer {
                    Layer::Abc => Layer::Num,
                    Layer::Num => Layer::Abc,
                    Layer::Edit => Layer::Abc,
                };
                self.dirty = true;
            }
            Act::Close => self.hide(),
            Act::Paste => {
                crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
                crate::engine::trigger_haptic(9000, 12000, 0.03);
                self.key.down(NavKey::LControl);
                self.key.tap(NavKey::V);
                self.key.up(NavKey::LControl);
                self.release_oneshots();
                self.typed_word.clear();
            }
            Act::Copy => {
                crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
                crate::engine::trigger_haptic(7500, 9500, 0.022);
                self.key.down(NavKey::LControl);
                self.key.tap(NavKey::C);
                self.key.up(NavKey::LControl);
                self.release_oneshots();
            }
            Act::Cut => {
                crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
                crate::engine::trigger_haptic(7500, 9500, 0.022);
                self.key.down(NavKey::LControl);
                self.key.tap(NavKey::X);
                self.key.up(NavKey::LControl);
                self.release_oneshots();
            }
            Act::Undo => {
                crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
                crate::engine::trigger_haptic(7500, 9500, 0.022);
                self.key.down(NavKey::LControl);
                self.key.tap(NavKey::Z);
                self.key.up(NavKey::LControl);
                self.release_oneshots();
            }
            Act::Redo | Act::DeleteWord => {
                let key = if act == Act::Redo { NavKey::Y } else { NavKey::Backspace };
                self.key.down(NavKey::LControl);
                self.key.tap(key);
                self.key.up(NavKey::LControl);
                self.release_oneshots();
            }
            Act::EditTools => {
                self.layer = if self.layer == Layer::Edit { Layer::Abc } else { Layer::Edit };
                self.dirty = true;
            }
            Act::SelectAll => {
                crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
                self.key.down(NavKey::LControl);
                self.key.tap(NavKey::A);
                self.key.up(NavKey::LControl);
                self.release_oneshots();
            }
            Act::TaskView => {
                crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
                self.key.down(NavKey::LWin);
                self.key.tap(NavKey::Tab);
                self.key.up(NavKey::LWin);
                self.release_oneshots();
            }
            Act::ShowDesktop => {
                crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
                self.key.down(NavKey::LWin);
                self.key.tap(NavKey::D);
                self.key.up(NavKey::LWin);
                self.release_oneshots();
            }
            Act::CloseActiveWindow => {
                crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
                self.key.down(NavKey::LMenu);
                self.key.tap(NavKey::F4);
                self.key.up(NavKey::LMenu);
                self.release_oneshots();
            }
            Act::ClearTyped => {
                let count = self.typed_buffer.chars().count();
                for _ in 0..count {
                    self.tap_key(NavKey::Backspace);
                }
                self.typed_buffer.clear();
                self.typed_word.clear();
            }
            Act::AcceptPrediction(word) => {
                if self.typed_word.is_empty()
                    || !crate::predict::predict(&self.typed_word, self.lang == Lang::Ar, 3).contains(&word)
                {
                    return;
                }
                self.release_oneshots();
                if self.prefs.learn_words {
                    crate::predict::learn(word);
                }
                let count = self.typed_word.chars().count();
                for _ in 0..count {
                    self.tap_key(NavKey::Backspace);
                }
                self.key.send_str(word);
                for _ in 0..count {
                    self.typed_buffer.pop();
                }
                self.typed_buffer.push_str(word);
                self.typed_word.clear();
            }
            Act::FlipPosition => {
                self.prefs.position = match self.prefs.position {
                    crate::config::KeyboardPosition::FloatBottom => crate::config::KeyboardPosition::Top,
                    crate::config::KeyboardPosition::Top => crate::config::KeyboardPosition::Bottom,
                    _ => crate::config::KeyboardPosition::FloatBottom,
                };
                self.window_pos = None;
                self.sized = (0.0, 0.0);
            }
            Act::ToggleSplitMode => {
                self.split_mode = !self.split_mode;
                self.prefs.split_mode = self.split_mode;
                crate::vkeyboard::update_prefs(self.prefs.clone());
                crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
                crate::engine::trigger_haptic(10000, 14000, 0.04);
            }
        }
    }

    fn perform_in_context(&mut self, act: Act) {
        let sends_input = matches!(
            act,
            Act::Char(_, _)
                | Act::Str(_)
                | Act::CharOrStr(..)
                | Act::Key(_)
                | Act::WinKey
                | Act::Backspace
                | Act::Enter
                | Act::ShiftEnter
                | Act::Space
                | Act::Tab
                | Act::Paste
                | Act::Copy
                | Act::Cut
                | Act::Undo
                | Act::Redo
                | Act::DeleteWord
                | Act::ClearTyped
                | Act::SelectAll
                | Act::TaskView
                | Act::ShowDesktop
                | Act::CloseActiveWindow
                | Act::AcceptPrediction(_)
                | Act::Lang
        );
        if sends_input {
            let cur = platform::foreground_hwnd();
            let ov = platform::overlay_hwnd();
            if cur == 0 || (ov != 0 && cur == ov) || (self.typing_target != 0 && cur != self.typing_target) {
                platform::restore_foreground();
            }
        }
        self.execute_action(act);
        if TRACE.load(Ordering::Relaxed) {
            eprintln!("[vkeyboard] key {act:?} -> fg={}", platform::foreground_hwnd());
        }
    }

    fn release_oneshots(&mut self) {
        if self.shift == ShiftState::Once {
            self.shift = ShiftState::Off;
        }
        self.ctrl = false;
        self.alt = false;
        self.altgr = false;
    }

    fn armed_mods(&self) -> Vec<NavKey> {
        let mut v = Vec::with_capacity(3);
        if self.ctrl {
            v.push(NavKey::LControl);
        }
        if self.alt {
            v.push(NavKey::LMenu);
        }
        if self.altgr {
            v.push(NavKey::RMenu);
        }
        v
    }

    fn tap_shift(&mut self) {
        let now = Instant::now();
        let double = self
            .last_shift_tap
            .map(|t| now.duration_since(t) <= DOUBLE_TAP)
            .unwrap_or(false);
        self.shift = self.shift.tap(double);
        self.last_shift_tap = Some(now);
        self.dirty = true;
    }

    pub fn get_toolbar_actions(&self) -> Vec<Act> {
        let mut acts = Vec::new();
        let has_clear = !self.typed_buffer.is_empty();
        if has_clear {
            acts.push(Act::ClearTyped);
        }
        if !self.typed_word.is_empty() {
            let is_ar = self.lang == Lang::Ar;
            let preds = crate::predict::predict(&self.typed_word, is_ar, 3);
            for p in preds {
                acts.push(Act::AcceptPrediction(intern(p)));
            }
        }
        acts.push(Act::Copy);
        acts.push(Act::Paste);
        acts.push(Act::ShiftEnter);
        acts
    }

    fn activate_toolbar_focused(&mut self) {
        let acts = self.get_toolbar_actions();
        if let Some(&act) = acts.get(self.toolbar_col) {
            self.perform_in_context(act);
            if matches!(act, Act::AcceptPrediction(_) | Act::Enter | Act::ClearTyped) {
                self.toolbar_focused = false;
            }
        }
    }

    fn accept_top_prediction(&mut self) {
        if !self.typed_word.is_empty() {
            let is_ar = self.lang == Lang::Ar;
            let preds = crate::predict::predict(&self.typed_word, is_ar, 1);
            if let Some(top) = preds.first() {
                let leaked = intern(top);
                self.perform_in_context(Act::AcceptPrediction(leaked));
                self.perform_in_context(Act::Space);
            }
        }
    }

    fn consume_radial_nav(&mut self) -> bool {
        let nav = match self.nav.lock() {
            Ok(mut g) => g.drain(),
            Err(_) => return false,
        };
        let (sx, sy) = (nav.stick_x, nav.stick_y);
        let d = (sx * sx + sy * sy).sqrt();

        if d > 12000.0 {
            let two_pi = std::f32::consts::PI * 2.0;
            let step = two_pi / 8.0;
            let phi = (-sy).atan2(sx);
            let mut theta = phi + std::f32::consts::FRAC_PI_2;
            while theta < 0.0 {
                theta += two_pi;
            }
            while theta >= two_pi {
                theta -= two_pi;
            }
            let s = (((theta + step * 0.5) / step).floor() as usize) % 8;
            self.radial_selected_slice = Some(s);
        } else {
            self.radial_selected_slice = None;
        }

        if nav.select > 0 {
            if let Some(s) = self.radial_selected_slice {
                self.trigger_radial_action(s);
                // Slice 0 switches the overlay to the keyboard; every other
                // slice is a one-shot action that closes the menu.
                return s != 0;
            }
        }
        if nav.close > 0 || nav.hide > 0 {
            return true;
        }
        false
    }

    fn trigger_radial_action(&mut self, slice: usize) {
        match slice {
            0 => {
                OVERLAY_MODE.store(0, Ordering::SeqCst);
                self.mode = OverlayMode::Keyboard;
                self.sized = (0.0, 0.0);
            }
            1 => {
                crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
                self.tap_key(NavKey::VolumeUp);
            }
            2 => {
                self.perform_in_context(Act::TaskView);
            }
            3 => {
                crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
                self.tap_key(NavKey::VolumeDown);
            }
            4 => {
                self.perform_in_context(Act::ShowDesktop);
            }
            5 => {
                self.perform_in_context(Act::Paste);
            }
            6 => {
                self.perform_in_context(Act::CloseActiveWindow);
            }
            7 => {
                crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
                self.key.down(NavKey::LWin);
                self.key.down(NavKey::LShift);
                self.key.tap(NavKey::S);
                self.key.up(NavKey::LShift);
                self.key.up(NavKey::LWin);
                self.release_oneshots();
            }
            _ => {}
        }
    }

    fn consume_controller_nav(&mut self) -> bool {
        let n = match self.nav.lock() {
            Ok(mut g) => g.drain(),
            Err(_) => return false,
        };
        if n.dx != 0 || n.dy != 0 || n.jump_y != 0 || n.rx != 0 || n.ry != 0 {
            self.grid_nav_active = true;
        } else if !n.grid_nav_active {
            self.grid_nav_active = false;
        }

        if self.demo {
            return false;
        }

        let nrows = self.rows.len() as i32;
        if nrows == 0 {
            return false;
        }
        let (mut r, mut c) = self.cursor;

        if n.reposition_toggle > 0 {
            self.repositioning = !self.repositioning;
            crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
            crate::engine::trigger_haptic(8000, 11000, 0.03);
            if let Ok(mut g) = self.nav.lock() {
                g.is_repositioning = self.repositioning;
            }
        }

        if self.repositioning {
            // The window itself is moved once per frame in `update`.
            self.reposition_rx = n.right_stick_x;
            self.reposition_ry = n.right_stick_y;
            if n.select > 0 || n.close > 0 || n.hide > 0 {
                self.repositioning = false;
                if let Ok(mut g) = self.nav.lock() {
                    g.is_repositioning = false;
                }
                crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
            }
            return !self.visible.load(Ordering::SeqCst);
        }

        if self.toolbar_focused {
            let max_cols = self.get_toolbar_actions().len();
            if n.dx != 0 {
                let next = (self.toolbar_col as i32 + n.dx).clamp(0, (max_cols as i32) - 1);
                self.toolbar_col = next as usize;
            }
            if n.dy > 0 || n.jump_y > 0 {
                self.toolbar_focused = false;
                self.cursor = (0, nearest_col(&self.rows[0], self.goal_x));
            }
            if n.select > 0 && self.grid_nav_active {
                self.activate_toolbar_focused();
            }
        } else if r == 0 && (n.dy < 0 || n.jump_y < 0) {
            self.toolbar_focused = true;
            let acts = self.get_toolbar_actions();
            self.toolbar_col = if !self.typed_word.is_empty() && !self.typed_buffer.is_empty() && acts.len() > 1 {
                1
            } else {
                0
            };
        } else {
            if n.toggle_split > 0 {
                self.execute_action(Act::ToggleSplitMode);
            }

            if self.split_mode {
                let (mut lr, mut lc) = self.left_cursor;
                let (mut rr, mut rc) = self.right_cursor;
                lr = lr.min((nrows - 1) as usize);
                rr = rr.min((nrows - 1) as usize);
                let left_mid = (self.rows[lr].len() / 2).max(1);
                let right_mid = self.rows[rr].len() / 2;
                lc = lc.min(left_mid.saturating_sub(1));
                rc = rc.clamp(right_mid, self.rows[rr].len().saturating_sub(1));

                if n.dx != 0 {
                    let span = left_mid as i32;
                    if span > 0 {
                        lc = wrap(lc as i32 + n.dx, span) as usize;
                        self.goal_x_left = key_center(&self.rows[lr], lc);
                        self.last_cursor_moved = 0;
                    }
                }
                if n.dy != 0 {
                    lr = wrap(lr as i32 + n.dy, nrows) as usize;
                    let mid = (self.rows[lr].len() / 2).max(1);
                    lc = nearest_col_clamped(&self.rows[lr], self.goal_x_left, 0, mid);
                    self.last_cursor_moved = 0;
                }

                if n.rx != 0 {
                    let span = (self.rows[rr].len() - right_mid) as i32;
                    if span > 0 {
                        rc = right_mid + wrap((rc - right_mid) as i32 + n.rx, span) as usize;
                        self.goal_x_right = key_center(&self.rows[rr], rc);
                        self.last_cursor_moved = 1;
                    }
                }
                if n.ry != 0 {
                    rr = wrap(rr as i32 + n.ry, nrows) as usize;
                    let mid = self.rows[rr].len() / 2;
                    rc = nearest_col_clamped(&self.rows[rr], self.goal_x_right, mid, self.rows[rr].len());
                    self.last_cursor_moved = 1;
                }

                self.left_cursor = (lr, lc);
                self.right_cursor = (rr, rc);

                let select_left = n.select_left > 0 || (n.select > 0 && self.last_cursor_moved == 0);
                let select_right = n.select_right > 0 || (n.select > 0 && self.last_cursor_moved == 1);
                if select_left {
                    if let Some(kd) = self.rows.get(lr).and_then(|row| row.get(lc)) {
                        let a = kd.act;
                        self.flash = Some(((lr, lc), Instant::now()));
                        self.perform_in_context(a);
                    }
                }
                if select_right {
                    if let Some(kd) = self.rows.get(rr).and_then(|row| row.get(rc)) {
                        let a = kd.act;
                        self.flash = Some(((rr, rc), Instant::now()));
                        self.perform_in_context(a);
                    }
                }
            } else {
                let row_len = self.rows[r].len() as i32;
                if n.dx != 0 && row_len > 0 {
                    c = wrap(c as i32 + n.dx, row_len) as usize;
                    self.goal_x = key_center(&self.rows[r], c);
                }
                if n.dy != 0 || n.jump_y != 0 {
                    let delta = n.dy + n.jump_y;
                    r = wrap(r as i32 + delta, nrows) as usize;
                    c = nearest_col(&self.rows[r], self.goal_x);
                }

                if n.select > 0 && self.grid_nav_active {
                    let (fr, fc) = (r, c);
                    if let Some(kd) = self.rows.get(fr).and_then(|row| row.get(fc)) {
                        let a = kd.act;
                        self.flash = Some(((fr, fc), Instant::now()));
                        self.perform_in_context(a);
                    }
                }
            }
        }

        // Global controller shortcuts — active everywhere on the keyboard:
        if n.layer > 0 {
            crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
            crate::engine::trigger_haptic(8000, 11000, 0.03);
            self.layer = match self.layer {
                Layer::Abc => Layer::Num,
                Layer::Num => Layer::Abc,
                Layer::Edit => Layer::Abc,
            };
            self.dirty = true;
        }
        for _ in 0..n.shift {
            crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
            crate::engine::trigger_haptic(7500, 9500, 0.025);
            self.tap_shift();
        }
        for _ in 0..n.altgr {
            self.altgr = !self.altgr;
        }
        if n.lang > 0 {
            self.perform_in_context(Act::Lang);
            crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
            crate::engine::trigger_haptic(10000, 14000, 0.04);
        }
        for _ in 0..n.caret_left.min(4) {
            crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
            crate::engine::trigger_haptic(6000, 8000, 0.02);
            self.perform_in_context(Act::Key(NavKey::Left));
        }
        for _ in 0..n.caret_right.min(4) {
            crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
            crate::engine::trigger_haptic(6000, 8000, 0.02);
            self.perform_in_context(Act::Key(NavKey::Right));
        }
        for _ in 0..n.backspace.min(2) {
            self.perform_in_context(Act::Backspace);
        }
        for _ in 0..n.delete_word.min(2) {
            self.perform_in_context(Act::DeleteWord);
        }
        for _ in 0..n.space.min(2) {
            self.perform_in_context(Act::Space);
        }
        for _ in 0..n.accept_prediction.min(2) {
            self.accept_top_prediction();
        }
        for _ in 0..n.enter {
            self.perform_in_context(Act::Enter);
        }
        for _ in 0..n.shift_enter {
            self.perform_in_context(Act::ShiftEnter);
        }
        for _ in 0..n.period {
            self.perform_in_context(Act::Char('.', None));
        }
        if n.close > 0 || n.hide > 0 {
            self.hide();
        }
        if n.help > 0 {
            self.hints = !self.hints;
        }
        self.cursor = (r, c);
        !self.visible.load(Ordering::SeqCst)
    }
}

impl eframe::App for VKbdApp {
    /// Fully transparent, so only the painted panel or circle is visible
    /// (eframe's default is a translucent dark grey that fills the window).
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.running.load(Ordering::Relaxed) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        self.sync_typing_target(platform::foreground_hwnd());
        let pointer_delta = ctx.input(|i| i.pointer.delta().length());
        let pointer_down = ctx.input(|i| i.pointer.primary_pressed() || i.pointer.primary_down() || i.pointer.any_pressed() || i.pointer.any_down());
        if pointer_delta > 0.2 || pointer_down {
            self.grid_nav_active = false;
            if let Ok(mut g) = self.nav.lock() {
                g.grid_nav_active = false;
            }
        }
        self.frame = self.frame.wrapping_add(1);
        if self.frame <= 3 {
            crate::platform::log_debug(&format!(
                "[vkeyboard] VKbdApp::update frame={} vis={}",
                self.frame,
                self.visible.load(Ordering::SeqCst)
            ));
        }

        let current_hkl = platform::active_input_hkl();
        if self.last_sys_hkl != current_hkl {
            self.last_sys_hkl = current_hkl;
            let sys_lang = if (current_hkl & 0x03FF) == 0x01 {
                Lang::Ar
            } else {
                Lang::En
            };
            self.last_sys_lang = Some(sys_lang);
            if self.lang != sys_lang {
                self.lang = sys_lang;
            }
            self.dirty = true;
        }

        if let Ok(mut lock) = LIVE_PREFS.lock() {
            if let Some(new_prefs) = lock.take() {
                self.prefs = new_prefs;
                self.dirty = true;
                self.sized = (0.0, 0.0);
            }
        }

        if self.dirty {
            self.rebuild();
        }

        let cur_mode = match OVERLAY_MODE.load(Ordering::SeqCst) {
            1 => OverlayMode::RadialMenu,
            _ => OverlayMode::Keyboard,
        };
        if self.mode != cur_mode {
            self.mode = cur_mode;
            self.sized = (0.0, 0.0);
        }

        let vis = self.visible.load(Ordering::SeqCst);
        let _ = platform::enforce_overlay_style(WINDOW_TITLE, self.mode == OverlayMode::Keyboard);
        if let Ok(mut g) = self.nav.lock() {
            g.radial_active = vis && self.mode == OverlayMode::RadialMenu;
            g.split_mode = vis && self.mode == OverlayMode::Keyboard && self.split_mode;
        }

        if !vis {
            if self.shown || !self.initial_hidden {
                if platform::move_window(WINDOW_TITLE, -4000, -4000) {
                    self.initial_hidden = true;
                }
                self.shown = false;
                self.sized = (0.0, 0.0);
                VKBD_SHOWN.store(false, Ordering::SeqCst);
                self.warm_frames = 0;
                self.reset_transient();
                crate::platform::trim_working_set();
                if let Ok(mut g) = self.nav.lock() {
                    g.clear_close_and_hide();
                }
            }
            ctx.request_repaint_after(IDLE_REPAINT);
            return;
        }

        // A placed keyboard stays on its own monitor; otherwise it follows the
        // monitor of the window being typed into.
        let (wx, wy, ww, wh) = match self.window_pos {
            Some((x, y)) => {
                let (w, h) = self.prefs.window_size();
                platform::work_area_at(x + w as i32 / 2, y + h as i32 / 2)
            }
            None => platform::work_area_active(),
        };
        let (sw, sh) = (ww as f32, wh as f32);
        if (sw, sh) != self.screen {
            self.screen = (sw, sh);
            self.sized = (0.0, 0.0);
        }

        if self.mode == OverlayMode::RadialMenu {
            if self.consume_radial_nav() {
                platform::move_window(WINDOW_TITLE, -4000, -4000);
                self.shown = false;
                self.sized = (0.0, 0.0);
                VKBD_SHOWN.store(false, Ordering::SeqCst);
                self.warm_frames = 0;
                self.visible.store(false, Ordering::SeqCst);
                self.reset_transient();
                crate::platform::trim_working_set();
                ctx.request_repaint_after(IDLE_REPAINT);
                return;
            }

            let want = (430.0, 430.0);
            let origin = (
                (wx as f32 + (sw - want.0) / 2.0).round() as i32,
                (wy as f32 + (sh - want.1) / 2.0).round() as i32,
            );
            if (!self.shown || self.sized != want || self.frame.is_multiple_of(90))
                && platform::place_window(
                    WINDOW_TITLE,
                    origin.0,
                    origin.1,
                    want.0 as i32,
                    want.1 as i32,
                )
            {
                self.sized = want;
            }
            self.shown = true;
            VKBD_SHOWN.store(true, Ordering::SeqCst);

            let mut radial_choice = None;
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::none()
                        .fill(egui::Color32::TRANSPARENT)
                        .inner_margin(egui::Margin::same(0.0)),
                )
                .show(ctx, |ui| {
                    radial_choice =
                        paint_radial_menu(ui, ui.max_rect(), self.radial_selected_slice);
                });

            if let Some(slice) = radial_choice {
                self.trigger_radial_action(slice);
                if slice != 0 {
                    // One-shot actions and the centre "cancel" close the menu.
                    self.visible.store(false, Ordering::SeqCst);
                    ctx.request_repaint();
                }
            }
            ctx.request_repaint_after(Duration::from_millis(30));
            return;
        }

        if self.consume_controller_nav() {
            platform::move_window(WINDOW_TITLE, -4000, -4000);
            self.shown = false;
            self.sized = (0.0, 0.0);
            VKBD_SHOWN.store(false, Ordering::SeqCst);
            self.warm_frames = 0;
            self.visible.store(false, Ordering::SeqCst);
            self.reset_transient();
            crate::platform::trim_working_set();
            ctx.request_repaint_after(IDLE_REPAINT);
            return;
        }

        let (content_w, content_h) = layout_px(&self.rows);
        let win_w = content_w + PAD * 2.0;
        let win_h = content_h + PAD * 2.0;
        let want = (win_w * self.prefs.scale, win_h * self.prefs.scale);

        let max_x = (wx as f32 + sw - want.0).max(wx as f32);
        let max_y = (wy as f32 + sh - want.1).max(wy as f32);

        let origin = match (self.window_pos, self.drag_anchor.is_some()) {
            (Some(p), _) if self.prefs.remember_position || self.repositioning => {
                let min_x = wx as f32;
                let min_y = wy as f32;
                (
                    (p.0 as f32).clamp(min_x, max_x).round() as i32,
                    (p.1 as f32).clamp(min_y, max_y).round() as i32,
                )
            }
            _ => {
                // Smart Caret Collision Avoidance:
                // Automatically place keyboard opposite the active text cursor so it never covers it!
                let auto_pos = if self.prefs.smart_caret_avoidance {
                    if let Some((_cx, cy)) = platform::caret_screen_pos() {
                        let mid_y = wy + wh / 2;
                        if cy > mid_y - 30 {
                            crate::config::KeyboardPosition::Top
                        } else {
                            self.prefs.position
                        }
                    } else {
                        self.prefs.position
                    }
                } else {
                    self.prefs.position
                };
                let mut dynamic_prefs = self.prefs.clone();
                dynamic_prefs.position = auto_pos;
                dynamic_prefs.anchor((wx, wy, ww, wh), None)
            }
        };

        if self.repositioning {
            let rx = self.reposition_rx;
            let ry = self.reposition_ry;
            let deadzone = 4000.0;
            if rx.abs() > deadzone || ry.abs() > deadzone {
                let speed = 18.0;
                let dx = (rx / 32768.0) * speed;
                let dy = (-ry / 32768.0) * speed;
                let cur = self.window_pos.unwrap_or(origin);
                let min_x = wx as f32;
                let min_y = wy as f32;
                let nx = ((cur.0 as f32 + dx).clamp(min_x, max_x)).round() as i32;
                let ny = ((cur.1 as f32 + dy).clamp(min_y, max_y)).round() as i32;
                if self.window_pos != Some((nx, ny)) {
                    self.window_pos = Some((nx, ny));
                    self.prefs.remember_position = true;
                    note_last_position((nx, ny));
                    platform::place_window(WINDOW_TITLE, nx, ny, want.0.round() as i32, want.1.round() as i32);
                    ctx.request_repaint();
                }
            }
        }

        if self.drag_anchor.is_none() && !self.repositioning
            && (!self.shown || self.sized != want || self.frame.is_multiple_of(90))
        {
            let ok = platform::place_window(
                WINDOW_TITLE,
                origin.0,
                origin.1,
                want.0.round() as i32,
                want.1.round() as i32,
            );
            if ok {
                self.sized = want;
            }
        }

        if !self.shown && want.0 > 0.0 && platform::set_window_visible(WINDOW_TITLE, true) {
            self.shown = true;
            VKBD_SHOWN.store(true, Ordering::SeqCst);
            // Each time the keyboard appears, optionally start on the cheat sheet.
            self.hints = self.prefs.show_hints_on_open;
        }

        let now = Instant::now();
        let flash_pos = self
            .flash
            .filter(|(_, t)| now.duration_since(*t) < FLASH)
            .map(|(p, _)| p);
        if flash_pos.is_none() {
            self.flash = None;
        }

        let mut pressed_key = None;
        let mut held_key = None;
        let mut switch_to_radial = false;
        let mut drag_state = None;

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(C_PANEL)
                    .stroke(egui::Stroke::new(1.0, C_PANEL_BORDER))
                    .rounding(egui::Rounding::same(14.0))
                    .inner_margin(egui::Margin::same(PAD)),
            )
            .show(ctx, |ui| {
                let area = ui.max_rect();
                let chrome = paint_chrome(
                    ui,
                    area,
                    &ChromeStatus {
                        lang: self.lang,
                        layer: self.layer,
                        shift: self.shift,
                        caps: self.caps,
                        ctrl: self.ctrl,
                        alt: self.alt,
                        altgr: self.altgr,
                        hints_on: self.hints,
                    },
                );

                if chrome.close {
                    self.hide();
                    return;
                }
                if chrome.help || chrome.radial || chrome.drag.dragged() || chrome.drag.clicked() {
                    self.grid_nav_active = false;
                    if let Ok(mut g) = self.nav.lock() {
                        g.grid_nav_active = false;
                        g.select = 0;
                    }
                }
                if chrome.help {
                    self.hints = !self.hints;
                }
                if chrome.radial {
                    switch_to_radial = true;
                }
                drag_state = Some(chrome.drag);

                let toolbar_rect = egui::Rect::from_min_size(
                    egui::pos2(area.left(), area.top() + CHROME_H + 2.0),
                    egui::vec2(area.width(), TOOLBAR_H),
                );
                let (tb_event, _) = paint_toolbar(
                    ui,
                    toolbar_rect,
                    &self.typed_buffer,
                    &self.typed_word,
                    self.lang,
                    self.split_mode,
                    self.toolbar_focused,
                    self.toolbar_col,
                );
                if let Some(action) = tb_event.action {
                    self.grid_nav_active = false;
                    if let Ok(mut g) = self.nav.lock() {
                        g.grid_nav_active = false;
                        g.select = 0;
                    }
                    self.perform_in_context(action);
                }

                let keys_top = area.top() + CHROME_H + TOOLBAR_H + 6.0;
                let enabled = !self.hints;
                let mut y = keys_top;

                for (ri, row) in self.rows.iter().enumerate() {
                    let mut x = area.left();
                    for (ci, kd) in row.iter().enumerate() {
                        let w = key_px_w(kd.w);
                        let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, KEY_H));
                        let (active, locked) = self.key_state(kd.act);
                        let is_cursor = self.grid_nav_active && !self.split_mode && self.cursor == (ri, ci);
                        let split_left = self.grid_nav_active && self.split_mode && self.left_cursor == (ri, ci);
                        let split_right = self.grid_nav_active && self.split_mode && self.right_cursor == (ri, ci);

                        let kv = KeyView {
                            label: shown_label(kd, self.shift.on(), self.caps),
                            alt: shown_alt(kd, self.shift.on()),
                            act: kd.act,
                            glyph: kd.glyph,
                            special: kd.special,
                            focused: is_cursor,
                            split_left,
                            split_right,
                            active,
                            locked,
                            flash: flash_pos == Some((ri, ci)),
                            enabled,
                        };
                        let ev = paint_key(ui, egui::Id::new((ri, ci)), rect, &kv);

                        if ev.hovered && !self.grid_nav_active {
                            self.cursor = (ri, ci);
                            self.goal_x = key_center(&self.rows[ri], ci);
                        }
                        if ev.down_now {
                            pressed_key = Some((ri, ci, kd.act));
                        }
                        if ev.held {
                            held_key = Some((ri, ci));
                        }
                        x += w + GAP;
                    }
                    y += KEY_H + ROW_GAP;
                }

                if self.hints && paint_hints(ui, area, self.lang) {
                    self.hints = false;
                }
            });

        if switch_to_radial {
            OVERLAY_MODE.store(1, Ordering::SeqCst);
            self.mode = OverlayMode::RadialMenu;
            self.sized = (0.0, 0.0);
            ctx.request_repaint();
            return;
        }

        let mut pending = Vec::new();
        let mouse_down = ctx.input(|i| i.pointer.primary_down());
        let mouse_pos = ctx.input(|i| i.pointer.hover_pos());

        if mouse_down {
            if let Some(pos) = mouse_pos {
                if let Some(last_pos) = self.last_mouse_down_pos {
                    if (pos - last_pos).length() > 6.0 {
                        self.repeat = None;
                    }
                } else {
                    self.last_mouse_down_pos = Some(pos);
                }
            }
        } else {
            self.last_mouse_down_pos = None;
        }

        if let Some((r, c, act)) = pressed_key {
            self.grid_nav_active = false;
            if let Ok(mut g) = self.nav.lock() {
                g.grid_nav_active = false;
                g.select = 0;
            }
            self.set_cursor(r, c);
            self.flash = Some(((r, c), now));
            pending.push(act);
            self.repeat = if repeatable(act) && mouse_down {
                Some(Repeat { pos: (r, c), act, next: now + REPEAT_DELAY })
            } else {
                None
            };
        } else if let Some(mut rep) = self.repeat {
            if held_key == Some(rep.pos) && mouse_down {
                if now >= rep.next {
                    pending.push(rep.act);
                    rep.next = now + REPEAT_RATE;
                }
                self.repeat = Some(rep);
            } else {
                self.repeat = None;
            }
        }
        for a in pending {
            self.perform_in_context(a);
        }
        if !self.visible.load(Ordering::SeqCst) {
            ctx.request_repaint_after(IDLE_REPAINT);
            return;
        }

        let ppp = ctx.pixels_per_point();
        let win_origin = ctx
            .input(|i| i.viewport().outer_rect)
            .map(|r| ((r.min.x * ppp).round(), (r.min.y * ppp).round()))
            .unwrap_or((origin.0 as f32, origin.1 as f32));
        let ptr = ctx.input(|i| i.pointer.interact_pos());
        if let Some(drag) = drag_state {
            if drag.drag_started() {
                if let Some(p) = ptr {
                    self.drag_anchor = Some(DragAnchor {
                        cursor: (win_origin.0 + p.x * ppp, win_origin.1 + p.y * ppp),
                        origin: win_origin,
                    });
                }
            }
            if drag.dragged() {
                if let (Some(a), Some(p)) = (self.drag_anchor, ptr) {
                    let cur = (win_origin.0 + p.x * ppp, win_origin.1 + p.y * ppp);
                    let nx = (a.origin.0 + cur.0 - a.cursor.0).clamp(wx as f32, max_x).round() as i32;
                    let ny = (a.origin.1 + cur.1 - a.cursor.1).clamp(wy as f32, max_y).round() as i32;
                    if self.window_pos != Some((nx, ny)) {
                        self.window_pos = Some((nx, ny));
                        note_last_position((nx, ny));
                        platform::move_window(WINDOW_TITLE, nx, ny);
                    }
                }
            } else {
                self.drag_anchor = None;
            }
        } else {
            self.drag_anchor = None;
        }

        let busy = self.repeat.is_some() || flash_pos.is_some() || self.drag_anchor.is_some() || self.repositioning;
        ctx.request_repaint_after(Duration::from_millis(if busy { 16 } else { 33 }));
    }
}

pub fn spawn(
    nav: Arc<Mutex<VkNav>>,
    prefs: crate::config::KeyboardPrefs,
    start_visible: bool,
    demo: bool,
    running: Arc<AtomicBool>,
) -> (Arc<AtomicBool>, std::thread::JoinHandle<()>) {
    let flag = Arc::new(AtomicBool::new(start_visible));
    let arabic = prefs.start_arabic;
    if let Ok(mut g) = CURRENT_PREFS.lock() {
        *g = Some(prefs.clone());
    }
    crate::platform::log_debug(&format!(
        "[vkeyboard] spawn() called: start_visible={start_visible} demo={demo}"
    ));
    let handle = {
        let flag = flag.clone();
        let run_clone = running.clone();
        std::thread::spawn(move || {
            crate::platform::attach_to_interactive_desktop();
            crate::platform::log_debug("[vkeyboard] thread started, configuring eframe...");
            let options = eframe::NativeOptions {
                renderer: eframe::Renderer::Wgpu,
                viewport: egui::ViewportBuilder::default()
                    .with_title(WINDOW_TITLE)
                    .with_inner_size([BASE_W * prefs.scale, BASE_H * prefs.scale])
                    .with_position([-4000.0, -4000.0])
                    .with_always_on_top()
                    .with_visible(start_visible)
                    .with_active(false)
                    .with_decorations(false)
                    .with_transparent(true),
                event_loop_builder: Some(Box::new(|builder| {
                    use winit::platform::windows::EventLoopBuilderExtWindows;
                    builder.with_any_thread(true);
                })),
                ..Default::default()
            };
            crate::platform::log_debug("[vkeyboard] calling eframe::run_native...");
            let res = eframe::run_native(
                WINDOW_TITLE,
                options,
                Box::new(move |cc| {
                    crate::platform::log_debug("[vkeyboard] eframe creator closure invoked!");
                    if let Ok(mut g) = EGUI_CTX.lock() {
                        *g = Some(cc.egui_ctx.clone());
                    }
                    install_fonts(&cc.egui_ctx);
                    let _ = crate::platform::enforce_overlay_style(WINDOW_TITLE, false);
                    Box::new(VKbdApp::new(
                        flag.clone(),
                        nav.clone(),
                        lang_of(arabic),
                        demo,
                        prefs.clone(),
                        run_clone,
                    ))
                }),
            );
            crate::platform::log_debug(&format!("[vkeyboard] eframe::run_native finished: {res:?}"));
            if let Err(e) = res {
                eprintln!("[vkeyboard] eframe error: {e}");
            }
        })
    };
    (flag, handle)
}

fn lang_of(arabic: bool) -> Lang {
    if arabic {
        Lang::Ar
    } else {
        Lang::En
    }
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut loaded: Vec<String> = Vec::new();
    for (name, path) in [
        ("segoe-ui", "C:\\Windows\\Fonts\\segoeui.ttf"),
        ("tahoma", "C:\\Windows\\Fonts\\tahoma.ttf"),
        ("segoe-sym", "C:\\Windows\\Fonts\\seguisym.ttf"),
        ("arial", "C:\\Windows\\Fonts\\arial.ttf"),
    ] {
        if let Ok(data) = std::fs::read(path) {
            fonts
                .font_data
                .insert(name.to_string(), egui::FontData::from_owned(data));
            loaded.push(name.to_string());
        }
    }
    if loaded.is_empty() {
        return;
    }
    if let Some(fam) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        let mut next: Vec<String> = loaded;
        next.extend(fam.iter().cloned());
        *fam = next;
    }
    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn arabic_prediction_context_handles_ligatures_marks_and_punctuation() {
        assert_eq!(trailing_word("مرحبا لا"), "لا");
        assert_eq!(trailing_word("كَتَبَ"), "كَتَبَ");
        assert_eq!(trailing_word("مرحبا،"), "");
        assert_eq!(trailing_word("مرحبا؟"), "");
        assert_eq!(trailing_word("hello wor"), "wor");
        assert_eq!(trailing_word("hello "), "");
        let mut text = "hello ".to_string();
        text.pop();
        assert_eq!(trailing_word(&text), "hello");
    }

    #[test]
    fn editing_layout_exposes_tools_in_both_languages_and_fits() {
        for lang in [Lang::En, Lang::Ar] {
            let rows = build_layout(lang, Layer::Edit);
            assert_eq!(rows.len(), 5);
            let actions: Vec<_> = rows.iter().flatten().map(|key| key.act).collect();
            for act in [Act::Copy, Act::Paste, Act::Cut, Act::Undo, Act::Redo, Act::DeleteWord, Act::SelectAll, Act::EditTools] {
                assert!(actions.contains(&act), "missing {act:?}");
            }
            let (width, height) = layout_px(&rows);
            assert!(width <= BASE_W && height <= BASE_H);
            let sizes = layout::widths(&rows);
            assert!(sizes.iter().all(|size| (*size - sizes[0]).abs() < 0.001));
            assert!(!repeatable(Act::DeleteWord));
        }
    }

    #[test]
    fn switching_target_discards_old_predictions_and_preview() {
        let mut app = VKbdApp::new(
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(VkNav::default())),
            Lang::En,
            false,
            crate::config::KeyboardPrefs::default(),
            Arc::new(AtomicBool::new(true)),
        );
        app.typing_target = 10;
        app.typed_word = "hel".into();
        app.typed_buffer = "hel".into();
        app.sync_typing_target(10);
        assert_eq!(app.typed_word, "hel");
        app.sync_typing_target(11);
        assert!(app.typed_word.is_empty());
        assert!(app.typed_buffer.is_empty());
        app.execute_action(Act::EditTools);
        assert_eq!(app.layer, Layer::Edit);
        app.execute_action(Act::EditTools);
        assert_eq!(app.layer, Layer::Abc);
    }

    const ALL: [(Lang, Layer); 4] = [
        (Lang::En, Layer::Abc),
        (Lang::En, Layer::Num),
        (Lang::Ar, Layer::Abc),
        (Lang::Ar, Layer::Num),
    ];

    #[test]
    fn every_row_has_the_same_width() {
        for (lang, layer) in ALL {
            let rows = build_layout(lang, layer);
            let w = layout::widths(&rows);
            let first = w[0];
            for x in w {
                assert!((x - first).abs() < 0.001, "{lang:?}/{layer:?}: row width {x} != {first}");
            }
        }
    }

    #[test]
    fn every_row_has_the_same_pixel_width() {
        for (lang, layer) in ALL {
            let rows = build_layout(lang, layer);
            let first = row_px(&rows[0]);
            for r in &rows {
                assert!((row_px(r) - first).abs() < 0.01, "{lang:?}/{layer:?} px width mismatch");
            }
            assert!((layout_px(&rows).0 - first).abs() < 0.01);
        }
    }

    #[test]
    fn layout_has_five_rows_and_no_empty_keys() {
        for (lang, layer) in ALL {
            let rows = build_layout(lang, layer);
            assert_eq!(rows.len(), 5, "{lang:?}/{layer:?} should have 5 rows");
            for (ri, row) in rows.iter().enumerate() {
                assert!(!row.is_empty(), "{lang:?} row {ri} is empty");
                for k in row {
                    assert!(k.w > 0.0, "{lang:?} row {ri} has a zero-width key");
                }
            }
        }
    }

    #[test]
    fn arabic_row4_has_balanced_letter_keys() {
        let rows = layout::ar_abc();
        let r4 = &rows[3];
        assert_eq!(r4.len(), 13, "Arabic row 4 should have 13 keys (Shift + 11 letters + Shift)");
    }

    #[test]
    fn every_row_of_every_layout_has_the_same_total_width() {
        for (lang, layer) in ALL {
            let rows = build_layout(lang, layer);
            let target = layout_units(&rows);
            for (ri, row) in rows.iter().enumerate() {
                let have: f32 = row.iter().map(|k| k.w).sum();
                assert!(
                    (have - target).abs() < 0.01,
                    "{lang:?}/{layer:?} row {ri} is {have} units, expected {target}"
                );
            }
        }
    }

    #[test]
    fn bottom_row_has_the_toggle_keys_and_space() {
        let rows = layout::en_abc();
        let bottom = rows.last().unwrap();
        let acts: Vec<Act> = bottom.iter().map(|k| k.act).collect();
        for needed in [Act::Lang, Act::ToggleLayer, Act::Space, Act::Ctrl, Act::WinKey, Act::AltGr] {
            assert!(acts.contains(&needed), "bottom row is missing {needed:?}");
        }
    }

    #[test]
    fn digits_layer_can_backspace() {
        for lang in [Lang::En, Lang::Ar] {
            let rows = build_layout(lang, Layer::Num);
            assert!(
                rows.iter().flatten().any(|k| k.act == Act::Backspace),
                "{lang:?} ?123 layer has no Backspace"
            );
        }
    }

    #[test]
    fn symbols_layer_is_reachable_and_complete() {
        for lang in [Lang::En, Lang::Ar] {
            let rows = build_layout(lang, Layer::Num);
            assert!(
                rows[4].iter().any(|k| k.act == Act::ToggleLayer),
                "{lang:?}: no layer toggle"
            );
            let top: String = rows[0]
                .iter()
                .filter_map(|k| match k.act {
                    Act::Char(c, _) => Some(c),
                    _ => None,
                })
                .collect();
            for d in "0123456789".chars() {
                assert!(top.contains(d), "{lang:?} ?123 row is missing {d}");
            }
            assert!(rows[4].iter().any(|k| k.act == Act::Space));
            assert!(rows[4].iter().any(|k| k.act == Act::Ctrl));
        }
    }

    #[test]
    fn every_layout_fits_a_reasonable_screen() {
        let (w, h) = layout_px(&build_layout(Lang::En, Layer::Abc));
        let (sw, sh) = platform::screen_size();
        assert!(w + PAD * 2.0 <= sw as f32, "keyboard {w} wider than screen {sw}");
        assert!(h + PAD * 2.0 <= sh as f32, "keyboard {h} taller than screen {sh}");
    }

    #[test]
    fn layer_key_label_reflects_the_current_layer() {
        let abc = build_layout(Lang::En, Layer::Abc);
        let num = build_layout(Lang::En, Layer::Num);
        let label = |rows: &Vec<Vec<KDef>>| {
            rows[4].iter().find(|k| k.act == Act::ToggleLayer).unwrap().label
        };
        assert_eq!(label(&abc), "?123");
        assert_eq!(label(&num), "abc");
    }

    #[test]
    fn arabic_layout_has_digit_row() {
        let rows = layout::ar_abc();
        let first = &rows[0];
        assert!(!first.is_empty());
        let digits: String = first
            .iter()
            .filter_map(|k| match k.act {
                Act::Char(c, _) if c.is_ascii_digit() => Some(c),
                _ => None,
            })
            .collect();
        for d in "1234567890".chars() {
            assert!(digits.contains(d), "Arabic layout missing digit {d}");
        }
    }

    #[test]
    fn arabic_shift_lam_alef_sends_real_letters_not_presentation_forms() {
        let rows = layout::ar_abc();
        for (base, want) in [('\u{0641}', "\u{0644}\u{0625}"), ('\u{0644}', "\u{0644}\u{0623}")] {
            let k = rows.iter().flatten().find(|k| matches!(k.act, Act::CharOrStr(c, _) if c == base)).unwrap();
            assert_eq!(k.act, Act::CharOrStr(base, want));
            assert_eq!(k.shifted, Some(want));
        }
        for k in rows.iter().flatten() {
            for s in [k.label].into_iter().chain(k.shifted) {
                assert!(!s.chars().any(|c| ('\u{FB50}'..='\u{FEFF}').contains(&c)), "presentation form in {s:?}");
            }
        }
    }

    #[test]
    fn toolbar_offers_only_the_editing_shortcuts() {
        let app = VKbdApp::new(
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(VkNav::default())),
            Lang::En,
            true,
            crate::config::KeyboardPrefs::default(),
            Arc::new(AtomicBool::new(true)),
        );
        assert_eq!(
            app.get_toolbar_actions(),
            vec![Act::Copy, Act::Paste, Act::ShiftEnter]
        );
    }

    #[test]
    fn edit_tools_are_reachable_from_every_letter_layout() {
        for lang in [Lang::En, Lang::Ar] {
            for layer in [Layer::Abc, Layer::Num] {
                let rows = build_layout(lang, layer);
                assert!(rows.iter().flatten().any(|k| k.act == Act::EditTools), "{lang:?}/{layer:?} has no Edit key");
            }
        }
    }

    #[test]
    fn arabic_ligature_is_a_string_action() {
        let rows = layout::ar_abc();
        assert!(
            rows.iter().flatten().any(|k| matches!(k.act, Act::Str("لا"))),
            "the لا ligature should be emitted as a string"
        );
    }

    #[test]
    fn arabic_layout_has_the_full_alphabet_with_no_duplicates() {
        let rows = layout::ar_abc();
        let mut seen = HashSet::new();
        for k in rows.iter().flatten() {
            if let Act::Char(c, _) | Act::CharOrStr(c, _) = k.act {
                assert!(seen.insert(c), "duplicate key for {c:?} in the Arabic layout");
            }
        }
        for c in "ضصثقفغعهخحجدشسيبلاتنمكطذئءؤرىةوزظ".chars() {
            assert!(seen.contains(&c), "missing Arabic letter {c}");
        }
    }

    #[test]
    fn shift_swaps_every_letter_label() {
        let rows = layout::en_abc();
        let b = rows[1].iter().find(|k| k.label == "q").unwrap();
        assert_eq!(b.shifted, Some("Q"));
        assert_eq!(shown_label(b, true, false), "Q");
        assert_eq!(shown_label(b, false, true), "Q");
        assert_eq!(shown_label(b, true, true), "q");
    }

    #[test]
    fn shift_also_swaps_punctuation_labels() {
        let rows = layout::en_abc();
        let k = rows[1].iter().find(|k| k.label == "[").unwrap();
        assert_eq!(shown_label(k, true, false), "{");
        assert_eq!(shown_label(k, false, true), "[", "Caps must not affect punctuation");
    }

    #[test]
    fn displayed_label_always_matches_the_emitted_char() {
        for (lang, layer) in ALL {
            for k in build_layout(lang, layer).iter().flatten() {
                if let Act::Char(c, s) = k.act {
                    for (shift, caps) in [(false, false), (true, false), (false, true), (true, true)] {
                        let out = resolve_char(c, s, shift, caps);
                        if k.shifted.is_some() {
                            assert_eq!(shown_label(k, shift, caps), out.to_string());
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn caps_and_shift_combine_like_a_real_keyboard() {
        assert_eq!(resolve_char('a', Some('A'), false, false), 'a');
        assert_eq!(resolve_char('a', Some('A'), true, false), 'A');
        assert_eq!(resolve_char('a', Some('A'), false, true), 'A');
        assert_eq!(resolve_char('a', Some('A'), true, true), 'a');
        assert_eq!(resolve_char('1', Some('!'), false, true), '1');
        assert_eq!(resolve_char('1', Some('!'), true, false), '!');
        assert_eq!(resolve_char('\u{0628}', None, true, true), '\u{0628}');
    }

    #[test]
    fn shift_state_machine() {
        assert_eq!(ShiftState::Off.tap(false), ShiftState::Once);
        assert_eq!(ShiftState::Once.tap(true), ShiftState::Locked);
        assert_eq!(ShiftState::Once.tap(false), ShiftState::Off);
        assert_eq!(ShiftState::Locked.tap(false), ShiftState::Off);
        assert_eq!(ShiftState::Locked.tap(true), ShiftState::Off);
        assert!(!ShiftState::Off.on());
        assert!(ShiftState::Once.on() && ShiftState::Locked.on());
    }

    #[test]
    fn vertical_navigation_follows_the_visual_column() {
        let rows = build_layout(Lang::En, Layer::Abc);
        let right = key_center(&rows[0], rows[0].len() - 1);
        assert_eq!(nearest_col(&rows[4], right), rows[4].len() - 1);
        let left = key_center(&rows[0], 0);
        assert_eq!(nearest_col(&rows[4], left), 0);
        for (ci, _) in rows[1].iter().enumerate() {
            let x = key_center(&rows[1], ci);
            let below = nearest_col(&rows[2], x);
            let dist = (key_center(&rows[2], below) - x).abs();
            assert!(dist <= UNIT * 1.5, "col {ci}: landed {dist}px away");
        }
    }

    #[test]
    fn key_geometry_fits_inside_the_row() {
        for (lang, layer) in ALL {
            let rows = build_layout(lang, layer);
            for row in &rows {
                let last = row.len() - 1;
                let right = key_x(row, last) + key_px_w(row[last].w);
                assert!((right - row_px(row)).abs() < 0.01);
            }
        }
    }

    #[test]
    fn only_typing_keys_repeat() {
        assert!(repeatable(Act::Backspace));
        assert!(repeatable(Act::Space));
        assert!(repeatable(Act::Char('a', None)));
        for act in [
            Act::Shift,
            Act::Ctrl,
            Act::Alt,
            Act::AltGr,
            Act::Caps,
            Act::Enter,
            Act::ShiftEnter,
            Act::Lang,
            Act::ToggleLayer,
            Act::Close,
        ] {
            assert!(!repeatable(act), "{act:?} must not auto-repeat");
        }
    }

    #[test]
    fn align_rows_gives_a_synthetic_deficit_to_the_space_bar() {
        let mut rows = vec![
            vec![KDef::new("a", Act::Char('a', None)), KDef::new("b", Act::Char('b', None))],
            vec![
                KDef::spec("Ctrl", Act::Ctrl, 1.0),
                KDef::spec("Space", Act::Space, 2.0),
                KDef::spec("Alt", Act::Alt, 1.0),
            ],
        ];
        align_rows(&mut rows, 8.0);
        assert!((rows[1].iter().find(|k| k.act == Act::Space).unwrap().w - 6.0).abs() < 0.001);
        assert!((rows[1].iter().find(|k| k.act == Act::Ctrl).unwrap().w - 1.0).abs() < 0.001);
        assert!((rows[0][0].w - 4.0).abs() < 0.001);
        assert!((rows[0][1].w - 4.0).abs() < 0.001);
    }

    #[test]
    fn space_is_the_widest_key_of_every_layout() {
        for (lang, layer) in ALL {
            let rows = build_layout(lang, layer);
            let bottom = rows.last().unwrap();
            let space = bottom.iter().find(|k| k.act == Act::Space).unwrap();
            let widest = bottom.iter().map(|k| k.w).fold(0.0f32, f32::max);
            assert!(
                (space.w - widest).abs() < 0.001,
                "{lang:?}/{layer:?}: Space {:?} is not the widest key ({widest})",
                space.w
            );
            assert!(space.w >= 4.0, "{lang:?}/{layer:?}: Space got squeezed to {}", space.w);
        }
    }

    #[test]
    fn interning_does_not_leak_on_rebuild() {
        let a = intern("unique-test-label");
        let b = intern("unique-test-label");
        assert!(std::ptr::eq(a, b), "same text must reuse the same allocation");
        let first = build_layout(Lang::En, Layer::Abc)[1][1].label;
        for _ in 0..50 {
            let again = build_layout(Lang::En, Layer::Abc)[1][1].label;
            assert!(std::ptr::eq(first, again));
        }
    }

    #[test]
    fn radial_menu_sector_math_covers_all_cardinal_and_diagonal_directions() {
        let two_pi = std::f32::consts::PI * 2.0;
        let step = two_pi / 8.0;
        let sector_of = |sx: f32, sy: f32| -> usize {
            let phi = (-sy).atan2(sx);
            let mut theta = phi + std::f32::consts::FRAC_PI_2;
            while theta < 0.0 {
                theta += two_pi;
            }
            while theta >= two_pi {
                theta -= two_pi;
            }
            (((theta + step * 0.5) / step).floor() as usize) % 8
        };

        assert_eq!(sector_of(0.0, 30000.0), 0);
        assert_eq!(sector_of(20000.0, 20000.0), 1);
        assert_eq!(sector_of(30000.0, 0.0), 2);
        assert_eq!(sector_of(20000.0, -20000.0), 3);
        assert_eq!(sector_of(0.0, -30000.0), 4);
        assert_eq!(sector_of(-20000.0, -20000.0), 5);
        assert_eq!(sector_of(-30000.0, 0.0), 6);
        assert_eq!(sector_of(-20000.0, 20000.0), 7);
    }

    #[test]
    fn test_nearest_col_clamped_left_and_right() {
        let rows = layout::en_abc();
        let r1 = &rows[1];
        let mid = r1.len() / 2;
        let left_idx = nearest_col_clamped(r1, 5000.0, 0, mid);
        assert!(left_idx < mid);
        let right_idx = nearest_col_clamped(r1, 0.0, mid, r1.len());
        assert!(right_idx >= mid);
    }

    #[test]
    fn test_essential_toolbar_actions_exist() {
        let app = VKbdApp::new(
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(VkNav::default())),
            Lang::En,
            false,
            crate::config::KeyboardPrefs::default(),
            Arc::new(AtomicBool::new(true)),
        );
        let acts = app.get_toolbar_actions();
        assert!(acts.contains(&Act::Copy));
        assert!(acts.contains(&Act::Paste));
        assert!(acts.contains(&Act::ShiftEnter));
    }

    #[test]
    fn test_accept_top_prediction() {
        let mut app = VKbdApp::new(
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(VkNav::default())),
            Lang::En,
            false,
            crate::config::KeyboardPrefs::default(),
            Arc::new(AtomicBool::new(true)),
        );
        app.typed_word = "hel".to_string();
        app.accept_top_prediction();
        assert!(app.typed_word.is_empty());
        assert!(app.typed_buffer.starts_with("help"));
        assert!(app.typed_buffer.ends_with(' '));
    }
}
