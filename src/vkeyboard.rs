//! On-screen virtual keyboard — Windows 11 Fluent 2 style.
//!
//! Architecture (deliberately simple so nothing silently breaks):
//!   * `Layout` — pure data describing every key. Built once per
//!     (language, layer) change, never inside the paint loop.
//!   * `VKbdApp` — owns the egui window, the current layout and the key state.
//!     Every frame it (1) applies queued controller navigation, (2) computes a
//!     deterministic geometry, (3) paints.
//!   * `VkNav` — the cross-thread queue the engine writes controller input to.
//!
//! Window show/hide is driven through Win32 directly (`ShowWindow`) because
//! eframe's `ViewportCommand::Visible` is a no-op for the native window.
//!
//! Behaviour worth knowing:
//!   * Shift is tri-state: tap = one-shot, double-tap = locked, tap again = off.
//!   * Ctrl / Alt / AltGr are one-shot latches that wrap the *next* key of any
//!     kind (letters, Backspace, Tab, ...), and light up while armed.
//!   * Caps is a real latch handled in software, so the OS Caps Lock LED is
//!     never toggled behind the user's back.
//!   * Backspace / Space / characters repeat while held (mouse or touch).
//!   * Controller up/down moves to the key *visually* above/below, not to the
//!     same column index (rows have different key widths).
//!   * Latches are cleared whenever the keyboard hides, so Ctrl can never get
//!     "stuck" on for the next time it appears.

#![cfg(feature = "gui")]

use crate::keys::{Key as NavKey, KeyPresser};
use crate::platform;
use crate::vknav::VkNav;
use eframe::egui;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
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

/// Toggle the on-screen virtual keyboard.
pub fn toggle_keyboard(flag: Option<Arc<AtomicBool>>) {
    if let Some(f) = flag {
        let cur_vis = f.load(Ordering::SeqCst);
        let cur_mode = OVERLAY_MODE.load(Ordering::SeqCst);
        if cur_vis && cur_mode == 0 {
            f.store(false, Ordering::SeqCst);
            platform::set_window_visible(WINDOW_TITLE, false);
            platform::move_window(WINDOW_TITLE, -4000, -4000);
        } else {
            OVERLAY_MODE.store(0, Ordering::SeqCst);
            platform::push_foreground();
            f.store(true, Ordering::SeqCst);
            let work = platform::work_area();
            let origin = crate::config::KeyboardPrefs::default().anchor(work, None);
            let base_w = (crate::config::KB_BASE_W).round() as i32;
            let base_h = (crate::config::KB_BASE_H).round() as i32;
            platform::place_window(WINDOW_TITLE, origin.0, origin.1, base_w, base_h);
            platform::set_window_visible(WINDOW_TITLE, true);
        }
        request_vkeyboard_repaint();
    }
}

/// Toggle the quick radial / pie menu.
pub fn toggle_radial_menu(flag: Option<Arc<AtomicBool>>) {
    if let Some(f) = flag {
        let cur_vis = f.load(Ordering::SeqCst);
        let cur_mode = OVERLAY_MODE.load(Ordering::SeqCst);
        if cur_vis && cur_mode == 1 {
            f.store(false, Ordering::SeqCst);
            platform::set_window_visible(WINDOW_TITLE, false);
            platform::move_window(WINDOW_TITLE, -4000, -4000);
        } else {
            OVERLAY_MODE.store(1, Ordering::SeqCst);
            platform::push_foreground();
            f.store(true, Ordering::SeqCst);
            let (wx, wy, ww, wh) = platform::work_area();
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

/// `--trace-keys` prints every synthetic key press to stderr. This is a
/// process-wide flag (the overlay runs on its own thread).
static TRACE: AtomicBool = AtomicBool::new(false);
static VKBD_SHOWN: AtomicBool = AtomicBool::new(false);

/// Query whether the virtual keyboard overlay window is actively visible on screen.
#[allow(dead_code)] // public accessor kept for external/debug use
pub fn is_vk_actually_shown() -> bool {
    VKBD_SHOWN.load(Ordering::SeqCst)
}

static LIVE_PREFS: Mutex<Option<crate::config::KeyboardPrefs>> = Mutex::new(None);

/// Dynamically update keyboard preferences (scale, position, start Arabic, hints) live from settings.
pub fn update_prefs(prefs: crate::config::KeyboardPrefs) {
    if let Ok(mut lock) = LIVE_PREFS.lock() {
        *lock = Some(prefs);
    }
}

/// Enable the key-press trace (used by `--trace-keys`).
pub fn set_trace(on: bool) {
    TRACE.store(on, Ordering::Relaxed);
}

// ---- Modern Windows 11 Fluent 2 / Mica Dark Theme ----
const C_PANEL: egui::Color32 = egui::Color32::from_rgba_premultiplied(18, 19, 24, 235);
const C_PANEL_BORDER: egui::Color32 = egui::Color32::from_rgba_premultiplied(35, 35, 45, 35);
const C_CHROME: egui::Color32 = egui::Color32::from_rgb(24, 25, 30);
const C_KEY: egui::Color32 = egui::Color32::from_rgb(44, 46, 52);
const C_KEY_SPECIAL: egui::Color32 = egui::Color32::from_rgb(32, 33, 38);
const C_KEY_HOVER: egui::Color32 = egui::Color32::from_rgb(58, 61, 70);
const C_KEY_DOWN: egui::Color32 = egui::Color32::from_rgb(24, 25, 29);
const C_KEY_TOP_BEVEL: egui::Color32 = egui::Color32::from_rgba_premultiplied(12, 12, 12, 12);
const C_KEY_BOT_SHADOW: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 75);
const C_ACCENT: egui::Color32 = egui::Color32::from_rgb(255, 170, 0);
const C_ACCENT_HOVER: egui::Color32 = egui::Color32::from_rgb(255, 188, 50);
const C_ACCENT_DOWN: egui::Color32 = egui::Color32::from_rgb(224, 145, 0);
const C_ACCENT_GLOW: egui::Color32 = egui::Color32::from_rgba_premultiplied(45, 30, 0, 45);
const C_ACCENT_SUBTLE: egui::Color32 = egui::Color32::from_rgba_premultiplied(25, 16, 0, 25);
const C_ON_ACCENT: egui::Color32 = egui::Color32::from_rgb(16, 16, 18);
const C_TEXT: egui::Color32 = egui::Color32::from_rgb(245, 246, 250);
const C_TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(148, 153, 166);
const C_PILL: egui::Color32 = egui::Color32::from_rgba_premultiplied(55, 55, 55, 55);
const C_SCRIM: egui::Color32 = egui::Color32::from_rgba_premultiplied(11, 12, 15, 245);
const C_RECORDING: egui::Color32 = egui::Color32::from_rgb(235, 55, 55);
const C_SUCCESS: egui::Color32 = egui::Color32::from_rgb(20, 185, 125);
const C_CLOSE_HOVER: egui::Color32 = egui::Color32::from_rgb(215, 38, 38);

// Geometry (px) — every key is sized in "units", so rows stay aligned.
const UNIT: f32 = 52.0;
const KEY_H: f32 = 47.0;
const GAP: f32 = 5.0;
const ROW_GAP: f32 = 6.0;
const PAD: f32 = 10.0;
const CHROME_H: f32 = 28.0;
const TOOLBAR_H: f32 = 36.0;
const ROUNDING: f32 = 7.5;

/// Window size at scale 1.0. Mirrors `config::KB_BASE_W/H`, which the settings
/// window uses to preview the size without depending on the `gui` feature.
const BASE_W: f32 = crate::config::KB_BASE_W;
const BASE_H: f32 = crate::config::KB_BASE_H;

// Timing.
const REPEAT_DELAY: Duration = Duration::from_millis(320);
const REPEAT_RATE: Duration = Duration::from_millis(30);
const DOUBLE_TAP: Duration = Duration::from_millis(350);
const FLASH: Duration = Duration::from_millis(140);
const IDLE_REPAINT: Duration = Duration::from_millis(1500);

/// Controller cheat-sheet shown by the 🎮 key / `?` button / R3.
/// NOTE: keep this in sync with the engine's actual button mapping.
const HINTS: &[(&str, &str)] = &[
    ("D-Pad / Left stick", "Move between keys"),
    ("Up from row 0", "Quick actions & word predictions"),
    ("L1 / R1 (LB / RB)", "Move text cursor ◄ / ►"),
    ("R2 (RT)", "Capitalize letters (Shift)"),
    ("L2 (LT)", "Numbers & symbols (#+=)"),
    ("R3 (Right stick click)", "Move keyboard anywhere"),
    ("A", "Press the highlighted key"),
    ("X / Y", "Backspace / Space"),
    ("Start", "Enter"),
    ("RT + Start", "Shift + Enter (New line)"),
    ("Back (Select)", "Switch language (EN / ع)"),
    ("B / L3", "Close keyboard"),
    ("😊 / Emoji", "Open Native Emoji Picker"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Ar,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Abc,
    Num,
    Emoji,
    Edit,
}

/// Shift is tri-state, like the Windows touch keyboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ShiftState {
    Off,
    /// Applies to the next key only.
    Once,
    /// Stays on until tapped again.
    Locked,
}

impl ShiftState {
    /// `double` = this tap followed the previous one within the double-tap window.
    fn tap(self, double: bool) -> Self {
        match self {
            ShiftState::Off => ShiftState::Once,
            ShiftState::Once if double => ShiftState::Locked,
            ShiftState::Once => ShiftState::Off,
            ShiftState::Locked => ShiftState::Off,
        }
    }
    fn on(self) -> bool {
        self != ShiftState::Off
    }
}

/// How a key face is drawn. Glyphs that no system font renders consistently
/// (Windows logo, gamepad, return arrow, backspace) are vector-drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Glyph {
    Text,
    Win,
    Pad,
    Enter,
    ShiftEnter,
    Backspace,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
enum Act {
    /// Emit a character; the second is produced while Shift is held.
    Char(char, Option<char>),
    /// Emit a multi-codepoint string (Arabic ligature "لا").
    Str(&'static str),
    /// Press a physical/virtual key.
    Key(NavKey),
    /// The Windows logo key, drawn as a glyph rather than typed.
    WinKey,
    Shift,
    Backspace,
    Enter,
    ShiftEnter,
    Space,
    Tab,
    Lang,
    Close,
    /// Toggle the digits/symbols layer.
    ToggleLayer,
    /// Latched modifiers: pressing the key arms it, the next key uses it.
    Ctrl,
    Alt,
    AltGr,
    /// Toggle the on-screen button hints (R3).
    Hints,
    /// Latch caps-lock (software), so whole words can be typed in upper case.
    Caps,
    /// Quick action toolbar items
    Paste,
    Copy,
    Cut,
    Undo,
    Redo,
    DeleteWord,
    EditTools,
    SelectAll,
    VoiceDictation,
    TaskView,
    ShowDesktop,
    CloseActiveWindow,
    ClearTyped,
    AcceptPrediction(&'static str),
    FlipPosition,
    Emoji(&'static str),
    EmojiPicker,
    EmojiCat(usize),
    ToggleSplitMode,
}

/// Keys that auto-repeat while held.
fn repeatable(act: Act) -> bool {
    matches!(act, Act::Char(..) | Act::Backspace | Act::Space)
}

#[derive(Clone, Copy)]
struct KDef {
    label: &'static str,
    /// Small glyph in the upper-right corner (shifted symbol, digit, ...).
    alt: Option<&'static str>,
    /// Label shown while Shift is held.
    shifted: Option<&'static str>,
    act: Act,
    w: f32,
    special: bool,
    glyph: Glyph,
}

/// Intern a string so layouts can be rebuilt any number of times without
/// leaking a fresh allocation per key per rebuild.
fn intern(s: &str) -> &'static str {
    static POOL: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let pool = POOL.get_or_init(|| Mutex::new(HashSet::new()));
    let mut g = pool.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&hit) = g.get(s) {
        return hit;
    }
    let leaked: &'static str = Box::leak(s.to_owned().into_boxed_str());
    g.insert(leaked);
    leaked
}

fn ch_str(c: char) -> &'static str {
    let mut buf = [0u8; 4];
    intern(c.encode_utf8(&mut buf))
}

impl KDef {
    fn new(label: &'static str, act: Act) -> Self {
        Self { label, alt: None, shifted: None, act, w: 1.0, special: false, glyph: Glyph::Text }
    }
    fn wide(label: &'static str, act: Act, w: f32) -> Self {
        Self { w, ..Self::new(label, act) }
    }
    fn spec(label: &'static str, act: Act, w: f32) -> Self {
        Self { special: true, ..Self::wide(label, act, w) }
    }
    fn with_glyph(mut self, glyph: Glyph) -> Self {
        self.glyph = glyph;
        self
    }
    /// A character key with a Shift variant (letters, digits, punctuation).
    fn shiftable(base: char, shifted: char) -> Self {
        Self {
            shifted: Some(ch_str(shifted)),
            ..Self::new(ch_str(base), Act::Char(base, Some(shifted)))
        }
    }
    fn letter(lower: char, upper: char) -> Self {
        Self::shiftable(lower, upper)
    }
    fn digit(d: char, shift: char) -> Self {
        Self::shiftable(d, shift)
    }
    /// A plain symbol with no Shift variant.
    fn sym(c: char) -> Self {
        Self::new(ch_str(c), Act::Char(c, None))
    }
    /// Arabic letter: big Arabic glyph + small Latin/digit in the corner.
    fn ar(arabic: char, corner: char) -> Self {
        Self { alt: Some(ch_str(corner)), ..Self::new(ch_str(arabic), Act::Char(arabic, None)) }
    }
    /// Arabic letter that emits the ligature "لا".
    fn ar_ligature(corner: char) -> Self {
        Self { alt: Some(ch_str(corner)), ..Self::new("لا", Act::Str("لا")) }
    }
    /// A diacritic, shown over a dotted circle so it is visible on its own.
    fn ar_mark(mark: char) -> Self {
        Self::new(intern(&format!("\u{25CC}{mark}")), Act::Char(mark, None))
    }
}

// ============================ Layouts ===================================

/// Row 5 is shared by both languages: language, layer, modifiers, space, hints.
///
/// Every key here does something real — the modifiers latch and apply to the
/// next key instead of firing once and doing nothing, and the arrow cluster is
/// gone because the D-pad already moves the cursor.
fn bottom_row(lang: Lang, on_num: bool) -> Vec<KDef> {
    let (lang_label, space_label) = match lang {
        Lang::En => ("EN / \u{0639}", "Space"),
        Lang::Ar => ("\u{0639} / EN", "\u{0645}\u{0633}\u{0627}\u{0641}\u{0629}"), // مسافة
    };
    let layer_label = if on_num { "abc" } else { "?123" };
    vec![
        KDef::spec(lang_label, Act::Lang, 1.35),
        KDef::spec(layer_label, Act::ToggleLayer, 1.35),
        KDef::spec("Ctrl", Act::Ctrl, 1.35),
        KDef::spec("", Act::WinKey, 1.0).with_glyph(Glyph::Win),
        KDef::spec("Alt", Act::Alt, 1.0),
        KDef::spec(space_label, Act::Space, 4.6),
        KDef::spec("AltGr", Act::AltGr, 1.0),
        KDef::spec("Edit", Act::EditTools, 1.0),
        KDef { shifted: Some("?"), ..KDef::spec(".", Act::Char('.', Some('?')), 1.0) },
        KDef::spec("\u{21E7}\u{23CE}", Act::ShiftEnter, 1.35).with_glyph(Glyph::ShiftEnter),
    ]
}

fn en_abc() -> Vec<Vec<KDef>> {
    let r1 = vec![
        KDef::spec("Esc", Act::Key(NavKey::Escape), 1.35),
        KDef::digit('1', '!'),
        KDef::digit('2', '@'),
        KDef::digit('3', '#'),
        KDef::digit('4', '$'),
        KDef::digit('5', '%'),
        KDef::digit('6', '^'),
        KDef::digit('7', '&'),
        KDef::digit('8', '*'),
        KDef::digit('9', '('),
        KDef::digit('0', ')'),
        KDef::digit('-', '_'),
        KDef::digit('=', '+'),
        // Windows OSK gives backspace a full 2u key on the number row.
        KDef::spec("\u{232B}", Act::Backspace, 2.0).with_glyph(Glyph::Backspace),
    ];
    let r2 = {
        let mut v = vec![KDef::spec("Tab", Act::Tab, 1.5)];
        for c in "qwertyuiop".chars() {
            v.push(KDef::letter(c, c.to_ascii_uppercase()));
        }
        v.push(KDef::shiftable('[', '{'));
        v.push(KDef::shiftable(']', '}'));
        v.push(KDef::shiftable('\\', '|'));
        v
    };
    let r3 = {
        let mut v = vec![KDef::spec("Caps", Act::Caps, 1.75)];
        for c in "asdfghjkl".chars() {
            v.push(KDef::letter(c, c.to_ascii_uppercase()));
        }
        v.push(KDef::shiftable(';', ':'));
        v.push(KDef::shiftable('\'', '"'));
        v.push(KDef::spec("\u{23CE}", Act::Enter, 2.2).with_glyph(Glyph::Enter));
        v
    };
    let r4 = {
        let mut v = vec![KDef::spec("Shift", Act::Shift, 2.25)];
        for c in "zxcvbnm".chars() {
            v.push(KDef::letter(c, c.to_ascii_uppercase()));
        }
        v.push(KDef::shiftable(',', '<'));
        v.push(KDef::shiftable('.', '>'));
        v.push(KDef::shiftable('/', '?'));
        v.push(KDef::spec("Shift", Act::Shift, 2.75));
        v
    };
    vec![r1, r2, r3, r4, bottom_row(Lang::En, false)]
}

/// `?123` — digits row plus a symbols block, mirroring the Windows OSK.
fn num_layer(lang: Lang) -> Vec<Vec<KDef>> {
    let r1: Vec<KDef> = {
        let mut v: Vec<KDef> = "1234567890".chars().map(KDef::sym).collect();
        // The digits layer used to have no Backspace at all.
        v.push(KDef::spec("\u{232B}", Act::Backspace, 1.6).with_glyph(Glyph::Backspace));
        v
    };
    let r2 = {
        let mut v = vec![KDef { shifted: Some("~"), ..KDef::spec("`", Act::Char('`', Some('~')), 1.35) }];
        v.extend("!@#$%^&*()".chars().map(KDef::sym));
        v
    };
    let r3 = {
        let mut v = vec![KDef::spec("-", Act::Char('-', None), 1.35)];
        v.extend("_+=[]{}\\|".chars().map(KDef::sym));
        v
    };
    let r4 = {
        let mut v = vec![KDef::spec("Tab", Act::Tab, 1.35)];
        v.extend("<>?:;',\"/".chars().map(KDef::sym));
        v
    };
    vec![r1, r2, r3, r4, bottom_row(lang, true)]
}

/// Arabic layout (standard Arabic 101 letters, Windows-touch style): digits
/// live in the upper-right corner of the top letter row; the Latin key each
/// letter sits on is shown as a corner hint on the other rows. The last letter
/// row carries the diacritics (tashkeel) and Arabic punctuation, since Shift
/// does nothing in Arabic.
fn ar_abc() -> Vec<Vec<KDef>> {
    let r1 = {
        let mut v = vec![KDef::spec("Esc", Act::Key(NavKey::Escape), 1.35)];
        for (a, d) in [
            ('\u{0636}', '1'), // ض
            ('\u{0635}', '2'), // ص
            ('\u{062B}', '3'), // ث
            ('\u{0642}', '4'), // ق
            ('\u{0641}', '5'), // ف
            ('\u{063A}', '6'), // غ
            ('\u{0639}', '7'), // ع
            ('\u{0647}', '8'), // ه
            ('\u{062E}', '9'), // خ
            ('\u{062D}', '0'), // ح
            ('\u{062C}', '-'), // ج
            ('\u{062F}', '='), // د
        ] {
            v.push(KDef::ar(a, d));
        }
        v.push(KDef::spec("\u{232B}", Act::Backspace, 2.0).with_glyph(Glyph::Backspace)); // ⌫
        v
    };
    let r2 = {
        let mut v = vec![KDef::spec("Tab", Act::Tab, 1.5)];
        for (a, l) in [
            ('\u{0634}', 'a'), // ش
            ('\u{0633}', 's'), // س
            ('\u{064A}', 'd'), // ي
            ('\u{0628}', 'f'), // ب
            ('\u{0644}', 'g'), // ل
            ('\u{0627}', 'h'), // ا
            ('\u{062A}', 'j'), // ت
            ('\u{0646}', 'k'), // ن
            ('\u{0645}', 'l'), // م
            ('\u{0643}', ';'), // ك
            ('\u{0637}', '\''), // ط
            ('\u{0630}', '`'), // ذ
        ] {
            v.push(KDef::ar(a, l));
        }
        v
    };
    let r3 = {
        let mut v = vec![KDef::spec("Caps", Act::Caps, 1.75)];
        v.push(KDef::ar('\u{0626}', 'z')); // ئ
        v.push(KDef::ar('\u{0621}', 'x')); // ء
        v.push(KDef::ar('\u{0624}', 'c')); // ؤ
        v.push(KDef::ar('\u{0631}', 'v')); // ر
        v.push(KDef::ar_ligature('b')); // لا
        v.push(KDef::ar('\u{0649}', 'n')); // ى
        v.push(KDef::ar('\u{0629}', 'm')); // ة
        v.push(KDef::ar('\u{0648}', ',')); // و
        v.push(KDef::ar('\u{0632}', '.')); // ز
        v.push(KDef::ar('\u{0638}', '/')); // ظ
        v.push(KDef::spec("\u{23CE}", Act::Enter, 2.2).with_glyph(Glyph::Enter));
        v
    };
    let r4 = {
        let mut v: Vec<KDef> = [
            '\u{064E}', // فتحة
            '\u{064F}', // ضمة
            '\u{0650}', // كسرة
            '\u{0652}', // سكون
            '\u{0651}', // شدة
            '\u{064B}', // تنوين فتح
            '\u{064C}', // تنوين ضم
            '\u{064D}', // تنوين كسر
            '\u{0670}', // ٰ superscript alef
            '\u{0653}', // همزة فوق
            '\u{0654}', // همزة تحت
            '\u{0655}', // مدّ
        ]
        .into_iter()
        .map(KDef::ar_mark)
        .collect();
        v.push(KDef::sym('\u{0640}')); // ـ tatweel
        v.push(KDef::sym('\u{060C}')); // ،
        v.push(KDef::sym('\u{061B}')); // ؛
        v.push(KDef::sym('\u{061F}')); // ؟
        v
    };
    vec![r1, r2, r3, r4, bottom_row(Lang::Ar, false)]
}

/// Brings every row to exactly `target_units` so the right edge is straight.
/// A row with a Space bar gives the whole deficit to Space (so the labelled
/// keys keep their size); other rows spread it evenly.
fn align_rows(rows: &mut [Vec<KDef>], target_units: f32) {
    for row in rows.iter_mut() {
        let n = row.len() as f32;
        if n == 0.0 {
            continue;
        }
        let have: f32 = row.iter().map(|k| k.w).sum();
        let deficit = target_units - have;
        if deficit <= 0.005 {
            continue;
        }
        let mut space_found = false;
        for k in row.iter_mut() {
            if k.act == Act::Space {
                k.w += deficit;
                space_found = true;
                break;
            }
        }
        if !space_found {
            let extra = deficit / n;
            for k in row.iter_mut() {
                k.w += extra;
            }
        }
    }
}

fn emoji_layer(cat: usize) -> Vec<Vec<KDef>> {
    let tabs = vec![
        KDef::spec("😀", Act::EmojiCat(0), 1.8),
        KDef::spec("👍", Act::EmojiCat(1), 1.8),
        KDef::spec("❤️", Act::EmojiCat(2), 1.8),
        KDef::spec("🎉", Act::EmojiCat(3), 1.8),
        KDef::spec("🍕", Act::EmojiCat(4), 1.8),
        KDef::spec("🚀", Act::EmojiCat(5), 1.8),
        KDef::spec("ABC", Act::ToggleLayer, 2.0),
        KDef::spec("\u{232B}", Act::Backspace, 2.2).with_glyph(Glyph::Backspace),
    ];

    let emojis: &[&[&'static str]] = match cat {
        1 => &[
            &["👍", "👎", "👏", "🙌", "👐", "🤲", "🤝", "👊", "✊", "🤛", "🤜"],
            &["🤞", "✌️", "🤟", "🤘", "👌", "🤏", "👈", "👉", "👆", "👇", "☝️"],
            &["✋", "🤚", "🖐️", "🖖", "👋", "🤙", "💪", "🦾", "🙏", "✍️", "🤳"],
        ],
        2 => &[
            &["❤️", "🧡", "💛", "💚", "💙", "💜", "🖤", "🤍", "🤎", "💔", "❣️"],
            &["💕", "💞", "💓", "💗", "💖", "💘", "💝", "💟", "💯", "🔥", "✨"],
            &["🌟", "⭐", "💥", "💢", "💫", "💬", "💭", "🗯️", "💤", "👑", "💎"],
        ],
        3 => &[
            &["🎉", "🎊", "🎈", "🎁", "🏆", "🥇", "🥈", "🥉", "⚽", "🏀", "🎮"],
            &["🕹️", "🎲", "🎯", "🎳", "🎵", "🎶", "🎤", "🎧", "🍿", "🎬", "🎨"],
            &["🎪", "🎭", "🎰", "🎫", "🎟️", "🎸", "🎹", "🎺", "🥁", "🛹", "🚴"],
        ],
        4 => &[
            &["☕", "🍵", "🥤", "🍕", "🍔", "🍟", "🌭", "🥪", "🌮", "🌯", "🍖"],
            &["🍗", "🥩", "🍣", "🍦", "🍩", "🍰", "🍫", "🍬", "🍭", "🍮", "🍯"],
            &["🍎", "🍌", "🍉", "🍇", "🍓", "🍒", "🍑", "🍍", "🥥", "🥑", "🥕"],
        ],
        5 => &[
            &["🚀", "🚗", "✈️", "🛵", "🚲", "⚡", "💡", "🔔", "📱", "💻", "🖥️"],
            &["⌨️", "🕒", "⏳", "🔑", "🔒", "🔓", "📌", "📍", "🚩", "✅", "❌"],
            &["⚠️", "❓", "❗", "➕", "➖", "✖️", "➗", "💲", "🌍", "🌙", "☀️"],
        ],
        _ => &[
            &["😀", "😃", "😄", "😁", "😆", "😅", "😂", "🤣", "😊", "😇", "🙂"],
            &["🙃", "😉", "😌", "😍", "🥰", "😘", "😋", "😛", "😜", "🤪", "🤨"],
            &["😎", "🥳", "😏", "😒", "😞", "🥺", "😢", "😭", "😤", "😠", "😡"],
        ],
    };

    let mut rows = vec![tabs];
    for r in emojis {
        let mut row_keys = Vec::new();
        for &em in *r {
            row_keys.push(KDef::spec(em, Act::Emoji(em), 1.3));
        }
        rows.push(row_keys);
    }

    let bottom = vec![
        KDef::spec("ABC", Act::ToggleLayer, 2.0),
        KDef::spec("?123", Act::ToggleLayer, 1.8),
        KDef::spec("Space", Act::Space, 6.0),
        KDef::spec(".", Act::Char('.', None), 1.2),
        KDef::spec("\u{23CE}", Act::Enter, 2.4).with_glyph(Glyph::Enter),
    ];
    rows.push(bottom);

    let target = layout_units(&rows);
    align_rows(&mut rows, target);

    rows
}

fn build_layout(lang: Lang, layer: Layer) -> Vec<Vec<KDef>> {
    let mut rows = match (lang, layer) {
        (Lang::En, Layer::Abc) => en_abc(),
        (Lang::Ar, Layer::Abc) => ar_abc(),
        (lang, Layer::Num) => num_layer(lang),
        (_, Layer::Emoji) => emoji_layer(0),
        (lang, Layer::Edit) => editing_layer(lang),
    };
    let target = layout_units(&rows);
    align_rows(&mut rows, target);
    rows
}

fn editing_layer(lang: Lang) -> Vec<Vec<KDef>> {
    let ar = lang == Lang::Ar;
    let key = |en, arabic, act| KDef::spec(if ar { arabic } else { en }, act, 3.5);
    vec![
        vec![key("Copy", "نسخ", Act::Copy), key("Paste", "لصق", Act::Paste), key("Cut", "قص", Act::Cut), key("Select all", "تحديد الكل", Act::SelectAll)],
        vec![key("Undo", "تراجع", Act::Undo), key("Redo", "إعادة", Act::Redo), key("Delete word", "حذف كلمة", Act::DeleteWord), key("Delete", "حذف", Act::Key(NavKey::Delete))],
        vec![key("Home", "بداية السطر", Act::Key(NavKey::Home)), key("Left", "يسار", Act::Key(NavKey::Left)), key("Right", "يمين", Act::Key(NavKey::Right)), key("End", "نهاية السطر", Act::Key(NavKey::End))],
        vec![key("Shift", "Shift", Act::Shift), key("Up", "أعلى", Act::Key(NavKey::Up)), key("Down", "أسفل", Act::Key(NavKey::Down)), key("Backspace", "مسح للخلف", Act::Backspace)],
        vec![key("ABC", "حروف", Act::EditTools), key("Space", "مسافة", Act::Space), key("Help", "مساعدة", Act::Hints).with_glyph(Glyph::Pad), key("Enter", "إدخال", Act::Enter)],
    ]
}

/// Only the trailing word belongs to the prediction context. Arabic punctuation
/// is a boundary; combining marks and the lam-alef sequence remain part of it.
fn trailing_word(text: &str) -> String {
    text.chars().rev().take_while(|c| c.is_alphabetic() || *c == '\'' || ('\u{064B}'..='\u{065F}').contains(c))
        .collect::<String>().chars().rev().collect()
}

/// Width (in units) of the widest row.
fn layout_units(rows: &[Vec<KDef>]) -> f32 {
    rows.iter().map(|r| r.iter().map(|k| k.w).sum::<f32>()).fold(0.0, f32::max)
}

// ---- Single source of truth for key geometry (painting AND navigation) ----

fn key_px_w(w: f32) -> f32 {
    w * UNIT + (w - 1.0).max(0.0) * GAP
}

/// Left edge of key `col`, relative to the row's left edge.
fn key_x(row: &[KDef], col: usize) -> f32 {
    row[..col.min(row.len())].iter().map(|k| key_px_w(k.w) + GAP).sum()
}

fn key_center(row: &[KDef], col: usize) -> f32 {
    match row.get(col) {
        Some(k) => key_x(row, col) + key_px_w(k.w) * 0.5,
        None => 0.0,
    }
}

fn row_px(row: &[KDef]) -> f32 {
    if row.is_empty() {
        return 0.0;
    }
    row.iter().map(|k| key_px_w(k.w)).sum::<f32>() + (row.len() as f32 - 1.0) * GAP
}

/// `(content_w, content_h)` of the whole keyboard, excluding window padding.
fn layout_px(rows: &[Vec<KDef>]) -> (f32, f32) {
    let w = rows.iter().map(|r| row_px(r)).fold(0.0, f32::max);
    let n = rows.len() as f32;
    let h = CHROME_H + TOOLBAR_H + 6.0 + n * KEY_H + (n - 1.0).max(0.0) * ROW_GAP + 6.0;
    (w, h)
}

/// The column in `row` whose centre is closest to horizontal position `goal_x`.
fn nearest_col(row: &[KDef], goal_x: f32) -> usize {
    nearest_col_clamped(row, goal_x, 0, row.len())
}

/// The column in `row` between `min_col` and `max_col` whose centre is closest to `goal_x`.
fn nearest_col_clamped(row: &[KDef], goal_x: f32, min_col: usize, max_col: usize) -> usize {
    if row.is_empty() || min_col >= max_col {
        return min_col;
    }
    let end = max_col.min(row.len());
    (min_col..end)
        .min_by(|&a, &b| {
            let da = (key_center(row, a) - goal_x).abs();
            let db = (key_center(row, b) - goal_x).abs();
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(min_col)
}

fn wrap(i: i32, n: i32) -> i32 {
    ((i % n) + n) % n
}

// ---- Which character a key produces / shows ----

/// Single source of truth for Shift + Caps interaction.
/// Letters: Shift XOR Caps. Everything else: Shift only.
fn resolve_char(base: char, shifted: Option<char>, shift: bool, caps: bool) -> char {
    match shifted {
        Some(s) if base.is_alphabetic() => {
            if shift ^ caps {
                s
            } else {
                base
            }
        }
        Some(s) if shift => s,
        _ => base,
    }
}

/// What a key should *display* right now (must agree with `resolve_char`).
fn shown_label(kd: &KDef, shift: bool, caps: bool) -> &'static str {
    match (kd.act, kd.shifted) {
        (Act::Char(c, Some(_)), Some(s)) if c.is_alphabetic() => {
            if shift ^ caps {
                s
            } else {
                kd.label
            }
        }
        (_, Some(s)) if shift => s,
        _ => kd.label,
    }
}

/// Secondary upper-right corner hint: explicit `alt` (Arabic layout Latin/digit hints).
fn shown_alt(kd: &KDef, _shift: bool) -> Option<&'static str> {
    kd.alt
}

// ============================ Application ==============================

#[derive(Clone, Copy)]
struct Repeat {
    pos: (usize, usize),
    act: Act,
    next: Instant,
}

#[derive(Clone, Copy)]
struct DragAnchor {
    /// Absolute screen position of the pointer when the drag began (px).
    cursor: (f32, f32),
    /// Window origin when the drag began (px).
    origin: (f32, f32),
}

struct VKbdApp {
    visible: Arc<AtomicBool>,
    nav: Arc<Mutex<VkNav>>,
    /// `--demo-kb`: ignore controller input so screenshots are deterministic.
    demo: bool,
    /// Position / size / start-language preferences from the settings window.
    prefs: crate::config::KeyboardPrefs,
    lang: Lang,
    last_sys_lang: Option<Lang>,
    layer: Layer,
    shift: ShiftState,
    last_shift_tap: Option<Instant>,
    /// `(row, column)` of the orange cursor.
    cursor: (usize, usize),
    /// Horizontal position (px) the cursor "wants" — so Up/Down return to the
    /// same visual column even across rows with different key widths.
    goal_x: f32,
    rows: Vec<Vec<KDef>>,
    /// Set whenever lang/layer change so the layout is rebuilt only then.
    dirty: bool,
    key: KeyPresser,
    /// Whether the HWND has been located and shown at least once.
    shown: bool,
    /// Number of frames rendered while visible to warm up GPU buffers before unhiding.
    warm_frames: u32,
    /// Last applied window size; `(0.0, 0.0)` = not sized yet.
    sized: (f32, f32),
    /// Last seen screen size, to re-centre on resolution/DPI change.
    screen: (f32, f32),
    frame: u64,
    /// Current window origin on screen, in pixels. `None` = follow the default
    /// bottom-centre position.
    window_pos: Option<(i32, i32)>,
    drag_anchor: Option<DragAnchor>,
    /// One-shot latches: armed by their key, consumed by the next key.
    ctrl: bool,
    alt: bool,
    altgr: bool,
    /// Software caps-lock latch (stays on until tapped again).
    caps: bool,
    /// Show the on-screen button hints (toggled with R3).
    hints: bool,
    /// Held-key auto-repeat.
    repeat: Option<Repeat>,
    /// Brief highlight for a key activated by the controller.
    flash: Option<((usize, usize), Instant)>,
    initial_hidden: bool,
    mode: OverlayMode,
    typed_word: String,
    typed_buffer: String,
    typing_target: usize,
    toolbar_focused: bool,
    toolbar_col: usize,
    last_predictions_count: usize,
    radial_selected_slice: Option<usize>,
    split_mode: bool,
    left_cursor: (usize, usize),
    right_cursor: (usize, usize),
    goal_x_left: f32,
    goal_x_right: f32,
    last_cursor_moved: u8,
    repositioning: bool,
    reposition_rx: f32,
    reposition_ry: f32,
    emoji_cat: usize,
    last_mouse_down_pos: Option<(usize, usize)>,
}

impl VKbdApp {
    fn new(
        visible: Arc<AtomicBool>,
        nav: Arc<Mutex<VkNav>>,
        lang: Lang,
        demo: bool,
        prefs: crate::config::KeyboardPrefs,
    ) -> Self {
        let rows = build_layout(lang, Layer::Abc);
        let goal_x = key_center(&rows[0], 0);
        let left_cursor = (1, 2);
        let right_col = if rows.len() > 1 && rows[1].len() > 7 { 7 } else { 0 };
        let right_cursor = (1, right_col);
        let goal_x_left = if rows.len() > 1 { key_center(&rows[1], left_cursor.1) } else { 0.0 };
        let goal_x_right = if rows.len() > 1 { key_center(&rows[1], right_cursor.1) } else { 0.0 };
        let split_mode = prefs.split_mode;
        Self {
            visible,
            nav,
            demo,
            prefs,
            lang,
            last_sys_lang: None,
            layer: Layer::Abc,
            shift: ShiftState::Off,
            last_shift_tap: None,
            cursor: (0, 0),
            goal_x,
            rows,
            dirty: false,
            key: KeyPresser::new(),
            shown: false,
            warm_frames: 0,
            sized: (0.0, 0.0),
            screen: (0.0, 0.0),
            frame: 0,
            window_pos: None,
            drag_anchor: None,
            ctrl: false,
            alt: false,
            altgr: false,
            caps: false,
            hints: false,
            repeat: None,
            flash: None,
            initial_hidden: false,
            mode: OverlayMode::Keyboard,
            typed_word: String::new(),
            typed_buffer: String::new(),
            typing_target: 0,
            toolbar_focused: false,
            toolbar_col: 0,
            last_predictions_count: 0,
            radial_selected_slice: None,
            split_mode,
            left_cursor,
            right_cursor,
            goal_x_left,
            goal_x_right,
            last_cursor_moved: 0,
            repositioning: false,
            reposition_rx: 0.0,
            reposition_ry: 0.0,
            emoji_cat: 0,
            last_mouse_down_pos: None,
        }
    }

    fn rebuild(&mut self) {
        self.rows = if self.layer == Layer::Emoji {
            emoji_layer(self.emoji_cat)
        } else {
            build_layout(self.lang, self.layer)
        };
        self.dirty = false;
        self.clamp_cursor();
    }

    /// Keep the cursor inside the grid after a layout change, staying as close
    /// as possible to where it was *visually*.
    fn clamp_cursor(&mut self) {
        let nrows = self.rows.len();
        if nrows == 0 {
            self.cursor = (0, 0);
            self.left_cursor = (0, 0);
            self.right_cursor = (0, 0);
            return;
        }
        let r = self.cursor.0.min(nrows - 1);
        let c = nearest_col(&self.rows[r], self.goal_x);
        self.cursor = (r, c);

        let lr = self.left_cursor.0.min(nrows - 1);
        let l_mid = (self.rows[lr].len() / 2).max(1);
        let lc = nearest_col_clamped(&self.rows[lr], self.goal_x_left, 0, l_mid);
        self.left_cursor = (lr, lc);

        let rr = self.right_cursor.0.min(nrows - 1);
        let r_mid = self.rows[rr].len() / 2;
        let rc = nearest_col_clamped(&self.rows[rr], self.goal_x_right, r_mid, self.rows[rr].len());
        self.right_cursor = (rr, rc);
    }

    fn set_cursor(&mut self, r: usize, c: usize) {
        self.cursor = (r, c);
        if let Some(row) = self.rows.get(r) {
            self.goal_x = key_center(row, c);
        }
    }

    // ---- latches ---------------------------------------------------------

    fn tap_shift(&mut self) {
        let double = self.last_shift_tap.is_some_and(|t| t.elapsed() < DOUBLE_TAP);
        self.shift = self.shift.tap(double);
        self.last_shift_tap = Some(Instant::now());
    }

    /// Modifiers currently armed, in press order.
    fn armed_mods(&self) -> Vec<NavKey> {
        let mut m = Vec::new();
        if self.ctrl {
            m.push(NavKey::LControl);
        }
        if self.alt {
            m.push(NavKey::LMenu);
        }
        if self.altgr {
            m.push(NavKey::RMenu);
        }
        m
    }

    /// One-shot latches are spent after any real key.
    fn release_oneshots(&mut self) {
        self.ctrl = false;
        self.alt = false;
        self.altgr = false;
        if self.shift == ShiftState::Once {
            self.shift = ShiftState::Off;
        }
    }

    /// Forget every latch. Called whenever the keyboard hides so Ctrl/Alt can
    /// never be left armed for the next time it appears.
    fn reset_transient(&mut self) {
        self.ctrl = false;
        self.alt = false;
        self.altgr = false;
        self.shift = ShiftState::Off;
        self.last_shift_tap = None;
        self.repeat = None;
        self.drag_anchor = None;
        self.flash = None;
        self.typed_word.clear();
        self.typed_buffer.clear();
        self.toolbar_focused = false;
        self.toolbar_col = 0;
        self.radial_selected_slice = None;
        self.repositioning = false;
        self.reposition_rx = 0.0;
        self.reposition_ry = 0.0;
        if let Ok(mut g) = self.nav.lock() {
            g.is_repositioning = false;
        }
    }

    /// `(highlighted, locked)` for a key face.
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

    // ---- key output ------------------------------------------------------

    /// Press `k` with every armed modifier around it, then spend the latches.
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

    /// Type one (already resolved) character with armed modifiers applied.
    fn type_char(&mut self, ch: char) {
        crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
        crate::engine::trigger_haptic(7500, 9500, 0.022);
        let mods = self.armed_mods();
        for m in &mods {
            self.key.down(*m);
        }
        self.key.send_char(ch);
        for m in mods.iter().rev() {
            self.key.up(*m);
        }
        self.release_oneshots();
    }

    fn execute_action(&mut self, act: Act) {
        // These actions can change the selection or caret in ways the local
        // preview cannot observe. Never reuse their old prediction prefix.
        if matches!(act, Act::Key(_) | Act::WinKey | Act::Cut | Act::Undo | Act::Redo | Act::DeleteWord | Act::SelectAll | Act::Paste | Act::TaskView | Act::ShowDesktop | Act::CloseActiveWindow)
            || ((self.ctrl || self.alt || self.altgr) && matches!(act, Act::Char(..) | Act::Backspace | Act::Space | Act::Str(_))) {
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
            Act::Str(s) => {
                self.key.send_str(s);
                self.release_oneshots();
                self.typed_buffer.push_str(s);
                self.typed_word = trailing_word(&self.typed_buffer);
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
            Act::Ctrl => self.ctrl = !self.ctrl,
            Act::Alt => self.alt = !self.alt,
            Act::AltGr => self.altgr = !self.altgr,
            Act::Caps => self.caps = !self.caps,
            Act::Hints => self.hints = !self.hints,
            Act::Lang => {
                self.lang = match self.lang {
                    Lang::En => Lang::Ar,
                    Lang::Ar => Lang::En,
                };
                self.last_sys_lang = Some(self.lang);
                self.dirty = true;
                // Synchronize active Windows input layout via Alt+Shift
                self.key.down(NavKey::LMenu);
                self.key.down(NavKey::LShift);
                self.key.up(NavKey::LShift);
                self.key.up(NavKey::LMenu);
            }
            Act::ToggleLayer => {
                self.layer = match self.layer {
                    Layer::Abc => Layer::Num,
                    Layer::Num => Layer::Abc,
                    Layer::Emoji => Layer::Abc,
                    Layer::Edit => Layer::Abc,
                };
                self.dirty = true;
            }
            Act::Close => self.hide(),
            Act::Paste => {
                self.key.down(NavKey::LControl);
                self.key.down(NavKey::V);
                self.key.up(NavKey::V);
                self.key.up(NavKey::LControl);
                self.release_oneshots();
                crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
                crate::engine::trigger_haptic(7500, 9500, 0.022);
            }
            Act::Copy => {
                self.key.down(NavKey::LControl);
                self.key.down(NavKey::C);
                self.key.up(NavKey::C);
                self.key.up(NavKey::LControl);
                self.release_oneshots();
            }
            Act::Cut => {
                self.key.down(NavKey::LControl);
                self.key.down(NavKey::X);
                self.key.up(NavKey::X);
                self.key.up(NavKey::LControl);
                self.release_oneshots();
            }
            Act::Undo => {
                self.key.down(NavKey::LControl);
                self.key.down(NavKey::Z);
                self.key.up(NavKey::Z);
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
                self.repeat = None;
            }
            Act::SelectAll => {
                self.key.down(NavKey::LControl);
                self.key.down(NavKey::A);
                self.key.up(NavKey::A);
                self.key.up(NavKey::LControl);
                self.release_oneshots();
            }
            Act::VoiceDictation => {
                #[cfg(feature = "voice")]
                {
                    crate::voice::toggle_voice_typing(self.lang == Lang::Ar);
                }
                #[cfg(not(feature = "voice"))]
                {
                    self.key.down(NavKey::LWin);
                    self.key.down(NavKey::H);
                    self.key.up(NavKey::H);
                    self.key.up(NavKey::LWin);
                }
                self.release_oneshots();
            }
            Act::TaskView => {
                self.key.down(NavKey::LWin);
                self.key.down(NavKey::Tab);
                self.key.up(NavKey::Tab);
                self.key.up(NavKey::LWin);
                self.release_oneshots();
            }
            Act::ShowDesktop => {
                self.key.down(NavKey::LWin);
                self.key.down(NavKey::D);
                self.key.up(NavKey::D);
                self.key.up(NavKey::LWin);
                self.release_oneshots();
            }
            Act::CloseActiveWindow => {
                self.key.down(NavKey::LMenu);
                self.key.down(NavKey::F4);
                self.key.up(NavKey::F4);
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
                if self.typed_word.is_empty() || !crate::predict::predict(&self.typed_word, self.lang == Lang::Ar, 3).contains(&word) { return; }
                self.release_oneshots();
                let count = self.typed_word.chars().count();
                for _ in 0..count {
                    self.tap_key(NavKey::Backspace);
                    self.typed_buffer.pop();
                }
                self.key.send_str(word);
                self.type_char(' ');
                self.typed_buffer.push_str(word);
                self.typed_buffer.push(' ');
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
            Act::Emoji(em) => {
                self.key.send_str(em);
                self.typed_buffer.push_str(em);
                self.typed_word.clear();
                crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
                crate::engine::trigger_haptic(7500, 9500, 0.022);
            }
            Act::EmojiPicker => {
                crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
                crate::engine::trigger_haptic(8000, 10000, 0.03);
                self.layer = if self.layer == Layer::Emoji { Layer::Abc } else { Layer::Emoji };
                self.dirty = true;
            }
            Act::EmojiCat(cat) => {
                self.emoji_cat = cat;
                self.dirty = true;
                crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
                crate::engine::trigger_haptic(6000, 8000, 0.02);
            }
            Act::ToggleSplitMode => {
                self.split_mode = !self.split_mode;
                crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
                crate::engine::trigger_haptic(10000, 14000, 0.035);
                self.clamp_cursor();
                if self.split_mode {
                    crate::osd::show_info("🎮", "Dual-Stick Split Mode ON", "Left Stick (LT) + Right Stick (RT)");
                } else {
                    crate::osd::show_info("⌨", "Single Cursor Mode ON", "Standard D-Pad & Left Stick");
                }
            }
        }
    }

    fn hide(&mut self) {
        platform::move_window(WINDOW_TITLE, -4000, -4000);
        self.shown = false;
        self.sized = (0.0, 0.0);
        VKBD_SHOWN.store(false, Ordering::SeqCst);
        self.visible.store(false, Ordering::SeqCst);
        self.reset_transient();
    }

    /// Run an action for the user's focused app.
    ///
    /// The overlay carries `WS_EX_NOACTIVATE`, so clicking a key never steals
    /// activation: the user's text box keeps focus and the injected keystroke
    /// lands there.
    fn perform_in_context(&mut self, act: Act) {
        let sends_input = matches!(
            act,
            Act::Char(_, _)
                | Act::Str(_)
                | Act::Key(_)
                | Act::WinKey
                | Act::Backspace
                | Act::Enter
                | Act::ShiftEnter
                | Act::Tab
                | Act::Space
                | Act::Paste
                | Act::Copy
                | Act::Cut
                | Act::Undo
                | Act::Redo
                | Act::DeleteWord
                | Act::ClearTyped
                | Act::SelectAll
                | Act::VoiceDictation
                | Act::TaskView
                | Act::ShowDesktop
                | Act::CloseActiveWindow
                | Act::AcceptPrediction(_)
        );
        if sends_input {
            let cur = platform::foreground_hwnd();
            if cur == 0 || cur == platform::overlay_hwnd() {
                platform::restore_foreground();
            }
            self.sync_typing_target(platform::foreground_hwnd());
        }
        self.execute_action(act);
        if TRACE.load(Ordering::Relaxed) {
            eprintln!("[vkeyboard] key {act:?} -> fg={}", platform::foreground_hwnd());
        }
    }

    fn sync_typing_target(&mut self, target: usize) {
        if target == 0 || target == platform::overlay_hwnd() { return; }
        platform::push_foreground();
        if self.typing_target != target {
            self.typed_word.clear();
            self.typed_buffer.clear();
            self.repeat = None;
            self.typing_target = target;
        }
    }

    fn consume_radial_nav(&mut self) -> bool {
        let n = match self.nav.lock() {
            Ok(mut g) => {
                if self.demo {
                    g.drain();
                    return false;
                }
                g.drain()
            }
            Err(_) => return false,
        };
        if n.close > 0 || n.hide > 0 {
            self.hide();
            return true;
        }

        // Stick steering
        let sx = n.stick_x;
        let sy = n.stick_y;
        let mag = (sx * sx + sy * sy).sqrt();
        if mag > 12000.0 {
            // Note: sy in XInput is positive UP. In screen coords, Y is downwards, so screen_dy = -sy.
            let phi = (-sy).atan2(sx);
            let mut theta = phi + std::f32::consts::FRAC_PI_2;
            let two_pi = std::f32::consts::PI * 2.0;
            while theta < 0.0 {
                theta += two_pi;
            }
            while theta >= two_pi {
                theta -= two_pi;
            }
            let step = two_pi / 8.0;
            let sector = (((theta + step * 0.5) / step).floor() as usize) % 8;
            if self.radial_selected_slice != Some(sector) {
                crate::engine::trigger_haptic(7000, 9000, 0.025);
                crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
            }
            self.radial_selected_slice = Some(sector);
        }

        if n.select > 0 {
            if let Some(slice) = self.radial_selected_slice {
                self.trigger_radial_action(slice);
            }
        }
        !self.visible.load(Ordering::SeqCst)
    }

    fn trigger_radial_action(&mut self, slice: usize) {
        crate::engine::trigger_haptic(10000, 14000, 0.035);
        crate::sound::play_sfx(crate::sound::SoundEffect::KeyClick);
        match slice {
            0 => {
                // Switch to Virtual Keyboard
                OVERLAY_MODE.store(0, Ordering::SeqCst);
                self.mode = OverlayMode::Keyboard;
                self.sized = (0.0, 0.0);
            }
            1 => {
                // Volume Up
                self.tap_key(NavKey::VolumeUp);
                crate::osd::trigger_volume_feedback();
                self.hide();
            }
            2 => {
                // Task View
                self.perform_in_context(Act::TaskView);
                self.hide();
            }
            3 => {
                // Volume Down
                self.tap_key(NavKey::VolumeDown);
                crate::osd::trigger_volume_feedback();
                self.hide();
            }
            4 => {
                // Show Desktop
                self.perform_in_context(Act::ShowDesktop);
                self.hide();
            }
            5 => {
                // Voice Dictation
                self.perform_in_context(Act::VoiceDictation);
                self.hide();
            }
            6 => {
                // Close Window
                self.perform_in_context(Act::CloseActiveWindow);
                self.hide();
            }
            7 => {
                // Screenshot (Win+Shift+S)
                self.key.down(NavKey::LWin);
                self.key.down(NavKey::LShift);
                self.key.down(NavKey::S);
                self.key.up(NavKey::S);
                self.key.up(NavKey::LShift);
                self.key.up(NavKey::LWin);
                self.hide();
            }
            _ => self.hide(),
        }
    }

    fn get_toolbar_actions(&self) -> Vec<Act> {
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
            acts.push(Act::Enter);
        } else if has_clear {
            acts.push(Act::Enter);
            acts.push(Act::VoiceDictation);
            acts.push(Act::Paste);
            acts.push(Act::FlipPosition);
            acts.push(Act::ToggleSplitMode);
            acts.push(Act::Lang);
        } else {
            acts.push(Act::VoiceDictation);
            acts.push(Act::Paste);
            acts.push(Act::FlipPosition);
            acts.push(Act::EmojiPicker);
            acts.push(Act::ToggleSplitMode);
            acts.push(Act::Lang);
        }
        acts
    }

    fn activate_toolbar_focused(&mut self) {
        let acts = self.get_toolbar_actions();
        if let Some(&act) = acts.get(self.toolbar_col) {
            self.perform_in_context(act);
            if matches!(act, Act::AcceptPrediction(_) | Act::Enter | Act::ClearTyped | Act::Emoji(_) | Act::EmojiPicker) {
                self.toolbar_focused = false;
            }
        }
    }

    /// Apply queued controller input. Returns `true` if the keyboard was
    /// closed by the controller (so the engine can drop nav mode immediately).
    fn consume_nav(&mut self) -> bool {
        // A queued controller press would flip the language (Back), the layer
        // (RB) or move the cursor even while the demo is just being screenshotted.
        let mut n = match self.nav.lock() {
            Ok(mut g) => {
                if self.demo {
                    g.drain();
                    return false;
                }
                g.drain()
            }
            Err(_) => return false,
        };
        // If the overlay is newly surfacing, ignore any stale close/hide queued from prior sessions!
        if !self.shown {
            n.close = 0;
            n.hide = 0;
        }
        self.reposition_rx = n.right_stick_x;
        self.reposition_ry = n.right_stick_y;
        n.dx = n.dx.clamp(-1, 1);
        n.dy = n.dy.clamp(-1, 1);
        if self.rows.is_empty() {
            return false;
        }
        let nrows = self.rows.len() as i32;
        let (mut r, mut c) = self.cursor;
        r = r.min((nrows - 1) as usize);
        c = c.min(self.rows[r].len().saturating_sub(1));

        // Shift / AltGr toggle *before* a select, so "shift then letter" works.
        if n.shift > 0 {
            self.tap_shift();
        }
        if n.altgr > 0 {
            self.altgr = !self.altgr;
        }
        if n.layer > 0 {
            self.layer = match self.layer {
                Layer::Abc => Layer::Num,
                Layer::Num => Layer::Abc,
                Layer::Emoji => Layer::Abc,
                Layer::Edit => Layer::Abc,
            };
            self.dirty = true;
        }
        if n.lang > 0 {
            self.lang = if self.lang == Lang::En { Lang::Ar } else { Lang::En };
            self.dirty = true;
        }

        // Handle navigation when toolbar/prediction bar is focused
        if self.toolbar_focused {
            let max_cols = self.get_toolbar_actions().len().max(1);
            if n.dx != 0 {
                let next = (self.toolbar_col as i32 + n.dx).clamp(0, (max_cols as i32) - 1);
                self.toolbar_col = next as usize;
            }
            if n.dy > 0 || n.jump_y > 0 {
                self.toolbar_focused = false;
                self.cursor = (0, nearest_col(&self.rows[0], self.goal_x));
            }
            if n.select > 0 {
                self.activate_toolbar_focused();
            }
            if n.backspace > 0 {
                self.perform_in_context(Act::Backspace);
            }
            if n.space > 0 {
                self.perform_in_context(Act::Space);
            }
            if n.enter > 0 {
                self.perform_in_context(Act::Enter);
            }
            if n.shift_enter > 0 {
                self.perform_in_context(Act::ShiftEnter);
            }
            if n.close > 0 || n.hide > 0 {
                self.hide();
            }
            return !self.visible.load(Ordering::SeqCst);
        }

        // Entering toolbar from top row with Up or LT
        if r == 0 && (n.dy < 0 || n.jump_y < 0) {
            self.toolbar_focused = true;
            let acts = self.get_toolbar_actions();
            self.toolbar_col = if !self.typed_word.is_empty() && !self.typed_buffer.is_empty() && acts.len() > 1 {
                1
            } else {
                0
            };
            return false;
        }

        if n.toggle_split > 0 {
            self.execute_action(Act::ToggleSplitMode);
        }

        // --- Cursor movement ---
        if self.split_mode {
            let (mut lr, mut lc) = self.left_cursor;
            let (mut rr, mut rc) = self.right_cursor;
            lr = lr.min((nrows - 1) as usize);
            rr = rr.min((nrows - 1) as usize);
            let left_mid = (self.rows[lr].len() / 2).max(1);
            let right_mid = self.rows[rr].len() / 2;
            lc = lc.min(left_mid.saturating_sub(1));
            rc = rc.clamp(right_mid, self.rows[rr].len().saturating_sub(1));

            // Left Stick navigates left half
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

            // Right Stick navigates right half
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

            // Left Trigger selects left cursor
            if n.select_left > 0 {
                if let Some(kd) = self.rows.get(lr).and_then(|row| row.get(lc)) {
                    let a = kd.act;
                    self.flash = Some(((lr, lc), Instant::now()));
                    self.perform_in_context(a);
                }
            }
            // Right Trigger selects right cursor
            if n.select_right > 0 {
                if let Some(kd) = self.rows.get(rr).and_then(|row| row.get(rc)) {
                    let a = kd.act;
                    self.flash = Some(((rr, rc), Instant::now()));
                    self.perform_in_context(a);
                }
            }
            // A button commits whichever cursor moved last
            if n.select > 0 {
                let (sr, sc) = if self.last_cursor_moved == 0 { (lr, lc) } else { (rr, rc) };
                if let Some(kd) = self.rows.get(sr).and_then(|row| row.get(sc)) {
                    let a = kd.act;
                    self.flash = Some(((sr, sc), Instant::now()));
                    self.perform_in_context(a);
                }
            }
        } else {
            // Whole-row jumps (triggers) first, then the fine grid steps. Vertical
            // moves land on the key *visually* above/below (nearest centre).
            if n.jump_y != 0 {
                r = wrap(r as i32 + n.jump_y, nrows) as usize;
                c = nearest_col(&self.rows[r], self.goal_x);
            }
            if n.dx != 0 {
                let len = self.rows[r].len() as i32;
                if len > 0 {
                    c = wrap(c as i32 + n.dx, len) as usize;
                    self.goal_x = key_center(&self.rows[r], c);
                }
            }
            if n.dy != 0 {
                r = wrap(r as i32 + n.dy, nrows) as usize;
                c = nearest_col(&self.rows[r], self.goal_x);
            }

            // --- Actions ---
            if n.select > 0 {
                if let Some(kd) = self.rows.get(r).and_then(|row| row.get(c)) {
                    let a = kd.act;
                    self.flash = Some(((r, c), Instant::now()));
                    self.perform_in_context(a);
                }
            }
            self.cursor = (r, c);
        }
        if n.reposition_toggle > 0 {
            self.repositioning = !self.repositioning;
            if let Ok(mut g) = self.nav.lock() {
                g.is_repositioning = self.repositioning;
            }
            if self.repositioning && self.window_pos.is_none() {
                let (wx, wy, ww, wh) = platform::work_area();
                self.window_pos = Some(self.prefs.anchor((wx, wy, ww, wh), None));
            }
            crate::sound::play_sfx(crate::sound::SoundEffect::RadialSnap);
            crate::engine::trigger_haptic(10000, 14000, 0.04);
        }
        for _ in 0..n.caret_left.min(4) {
            self.perform_in_context(Act::Key(NavKey::Left));
        }
        for _ in 0..n.caret_right.min(4) {
            self.perform_in_context(Act::Key(NavKey::Right));
        }
        for _ in 0..n.backspace.min(2) {
            self.perform_in_context(Act::Backspace);
        }
        for _ in 0..n.space.min(2) {
            self.perform_in_context(Act::Space);
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
        // `close` and L3 (`hide`) both dismiss.
        if n.close > 0 || n.hide > 0 {
            self.hide();
        }
        if n.help > 0 {
            self.hints = !self.hints;
        }
        self.cursor = (r, c);
        // Also catches an Act::Close that was *selected* with the controller.
        !self.visible.load(Ordering::SeqCst)
    }
}

impl eframe::App for VKbdApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.sync_typing_target(platform::foreground_hwnd());
        self.frame = self.frame.wrapping_add(1);
        if self.frame <= 3 {
            crate::platform::log_debug(&format!(
                "[vkeyboard] VKbdApp::update frame={} vis={}",
                self.frame,
                self.visible.load(Ordering::SeqCst)
            ));
        }
        // Automatically detect Windows active keyboard layout / input language transitions
        if let Some(sys_lang) = platform::active_input_language() {
            if self.last_sys_lang != Some(sys_lang) {
                self.last_sys_lang = Some(sys_lang);
                if self.lang != sys_lang {
                    self.lang = sys_lang;
                    self.dirty = true;
                }
            }
        }

        // Dynamically apply updated preferences from settings UI
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

        #[cfg(feature = "voice")]
        if let Some(transcribed) = crate::voice::take_last_transcribed() {
            let clean = transcribed.trim();
            if !clean.is_empty() {
                if !self.typed_buffer.is_empty() && !self.typed_buffer.ends_with(' ') {
                    self.typed_buffer.push(' ');
                }
                self.typed_buffer.push_str(clean);
            }
        }

        let cur_mode = match OVERLAY_MODE.load(Ordering::SeqCst) {
            1 => OverlayMode::RadialMenu,
            _ => OverlayMode::Keyboard,
        };
        if self.mode != cur_mode {
            self.mode = cur_mode;
            self.sized = (0.0, 0.0);
        }

        // --- Window show/hide through Win32 ---
        let vis = self.visible.load(Ordering::SeqCst);
        let _ = platform::enforce_overlay_style(WINDOW_TITLE);

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

        let (wx, wy, ww, wh) = platform::work_area();
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
            if !self.shown && platform::set_window_visible(WINDOW_TITLE, true) {
                let _ = platform::enforce_overlay_style(WINDOW_TITLE);
                self.shown = true;
                VKBD_SHOWN.store(true, Ordering::SeqCst);
            }
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
            }

            ctx.request_repaint_after(Duration::from_millis(16));
            return;
        }

        if self.consume_nav() {
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
        if self.dirty {
            self.rebuild();
        }

        // --- Deterministic geometry (compact: window hugs the content) ---
        let (content_w, content_h) = layout_px(&self.rows);
        let win_w = content_w + PAD * 2.0;
        let win_h = content_h + PAD * 2.0;
        let want = (win_w * self.prefs.scale, win_h * self.prefs.scale);

        let origin = match (self.window_pos, self.drag_anchor.is_some()) {
            (Some(p), _) if self.prefs.remember_position || self.repositioning => {
                let min_x = wx as f32;
                let min_y = wy as f32;
                let max_x = (wx as f32 + sw - want.0).max(min_x);
                let max_y = (wy as f32 + sh - want.1).max(min_y);
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
                let max_x = (wx as f32 + sw - want.0).max(min_x);
                let max_y = (wy as f32 + sh - want.1).max(min_y);
                let nx = ((cur.0 as f32 + dx).clamp(min_x, max_x)).round() as i32;
                let ny = ((cur.1 as f32 + dy).clamp(min_y, max_y)).round() as i32;
                if self.window_pos != Some((nx, ny)) {
                    self.window_pos = Some((nx, ny));
                    self.prefs.remember_position = true;
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

        if !self.shown && want.0 > 0.0 {
            if platform::set_window_visible(WINDOW_TITLE, true) {
                let _ = platform::enforce_overlay_style(WINDOW_TITLE);
                self.shown = true;
                VKBD_SHOWN.store(true, Ordering::SeqCst);
            } else {
                ctx.request_repaint();
            }
        }

        let shift_on = self.shift.on();
        let caps_on = self.caps;
        let hints_on = self.hints;

        let ppp = if self.prefs.scale.is_finite() {
            self.prefs.scale.clamp(0.5, 2.0)
        } else {
            1.0
        };
        ctx.set_pixels_per_point(ppp);
        let max_x = (sw - want.0).max(0.0);
        let max_y = (sh - want.1).max(0.0);
        let (cr, cc) = self.cursor;
        let flash_pos = self.flash.filter(|(_, t)| t.elapsed() < FLASH).map(|(p, _)| p);

        let mut pending: Vec<Act> = Vec::new();
        let mut switch_to_radial = false;
        let mut pressed_key: Option<(usize, usize, Act)> = None;
        let mut held_key: Option<(usize, usize)> = None;
        let mut drag_state: Option<egui::Response> = None;

        let chrome_status = ChromeStatus {
            lang: self.lang,
            layer: self.layer,
            shift: self.shift,
            caps: self.caps,
            ctrl: self.ctrl,
            alt: self.alt,
            altgr: self.altgr,
            hints_on,
        };

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(C_PANEL)
                    .stroke(egui::Stroke::new(1.0, C_PANEL_BORDER))
                    .inner_margin(egui::Margin::same(PAD))
                    .rounding(egui::Rounding::same(16.0)),
            )
            .show(ctx, |ui| {
                let area = ui.max_rect();

                // Chrome: ? + status badges left, drag handle centre, radial + ✕ right.
                let chrome = paint_chrome(ui, area, &chrome_status);
                if chrome.close {
                    pending.push(Act::Close);
                }
                if chrome.help {
                    pending.push(Act::Hints);
                }
                if chrome.radial {
                    switch_to_radial = true;
                }
                drag_state = Some(chrome.drag);

                // Quick Actions / Word Prediction Toolbar
                let tb_rect = egui::Rect::from_min_size(
                    egui::pos2(area.left(), area.top() + CHROME_H + 2.0),
                    egui::vec2(area.width(), TOOLBAR_H),
                );
                let (tb_ev, pred_count) = paint_toolbar(
                    ui,
                    tb_rect,
                    &self.typed_buffer,
                    &self.typed_word,
                    self.lang,
                    self.split_mode,
                    self.toolbar_focused,
                    self.toolbar_col,
                );
                self.last_predictions_count = pred_count;
                if let Some(act) = tb_ev.action {
                    pending.push(act);
                }

                // Keys.
                let origin_y = area.top() + CHROME_H + TOOLBAR_H + 6.0;
                for (ri, row) in self.rows.iter().enumerate() {
                    let y = origin_y + ri as f32 * (KEY_H + ROW_GAP);
                    for (ci, kd) in row.iter().enumerate() {
                        let rect = egui::Rect::from_min_size(
                            egui::pos2(area.left() + key_x(row, ci), y),
                            egui::vec2(key_px_w(kd.w), KEY_H),
                        );
                        let (active, locked) = self.key_state(kd.act);
                        let is_split_l = self.split_mode && !self.toolbar_focused && ri == self.left_cursor.0 && ci == self.left_cursor.1;
                        let is_split_r = self.split_mode && !self.toolbar_focused && ri == self.right_cursor.0 && ci == self.right_cursor.1;
                        let is_foc = if self.split_mode {
                            false
                        } else {
                            !self.toolbar_focused && ri == cr && ci == cc
                        };
                        let view = KeyView {
                            label: shown_label(kd, shift_on, caps_on),
                            alt: shown_alt(kd, shift_on),
                            act: kd.act,
                            glyph: kd.glyph,
                            special: kd.special,
                            focused: is_foc,
                            split_left: is_split_l,
                            split_right: is_split_r,
                            active,
                            locked,
                            flash: flash_pos == Some((ri, ci)),
                            enabled: !hints_on,
                        };
                        let ev = paint_key(ui, egui::Id::new(("vk_key", ri, ci)), rect, &view);
                        if ev.down_now {
                            self.last_mouse_down_pos = Some((ri, ci));
                            pressed_key = Some((ri, ci, kd.act));
                        } else if ev.clicked && self.last_mouse_down_pos != Some((ri, ci)) {
                            pressed_key = Some((ri, ci, kd.act));
                        }
                        if ev.held {
                            held_key = Some((ri, ci));
                        }
                    }
                }

                // Vertical glowing separator between Left and Right halves in Split Mode
                if self.split_mode {
                    let mid_x = area.center().x;
                    let top_y = origin_y + 2.0;
                    let bot_y = origin_y + self.rows.len() as f32 * (KEY_H + ROW_GAP) - 4.0;
                    ui.painter().line_segment(
                        [egui::pos2(mid_x, top_y), egui::pos2(mid_x, bot_y)],
                        egui::Stroke::new(1.5, egui::Color32::from_rgba_unmultiplied(120, 150, 220, 90)),
                    );
                }

                if self.repositioning {
                    let banner = egui::Rect::from_min_size(
                        egui::pos2(area.left() + 20.0, area.top() + 6.0),
                        egui::vec2(area.width() - 40.0, 22.0),
                    );
                    let p = ui.painter();
                    p.rect_filled(banner, 6.0, egui::Color32::from_rgb(0, 120, 215));
                    p.rect_stroke(banner, 6.0, egui::Stroke::new(1.5, egui::Color32::WHITE));
                    let banner_text = if self.lang == Lang::Ar {
                        crate::arabic::reshape_ar("✥ وضع تحريك الكيبورد: حرك بالقير الأيمن • اضغط R3 للتثبيت")
                    } else {
                        "✥ Reposition Mode: Move with Right Stick • Press R3 to Lock".to_string()
                    };
                    p.text(
                        banner.center(),
                        egui::Align2::CENTER_CENTER,
                        banner_text,
                        egui::FontId::proportional(11.5),
                        egui::Color32::WHITE,
                    );
                }

                // Hints overlay sits on top; any click on it dismisses it.
                if hints_on && paint_hints(ui, area) {
                    pending.push(Act::Hints);
                }
            });

        if switch_to_radial {
            OVERLAY_MODE.store(1, Ordering::SeqCst);
            self.mode = OverlayMode::RadialMenu;
            self.sized = (0.0, 0.0);
            ctx.request_repaint();
            return;
        }

        // --- Press + hold-to-repeat -------------------------------------
        let now = Instant::now();
        let mouse_down = ctx.input(|i| i.pointer.primary_down());
        if !mouse_down {
            self.repeat = None;
            self.last_mouse_down_pos = None;
        }

        if let Some((r, c, act)) = pressed_key {
            self.set_cursor(r, c);
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

        // --- Drag the window by its top strip ---
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
                    let nx = (a.origin.0 + cur.0 - a.cursor.0).clamp(0.0, max_x).round() as i32;
                    let ny = (a.origin.1 + cur.1 - a.cursor.1).clamp(0.0, max_y).round() as i32;
                    if self.window_pos != Some((nx, ny)) {
                        self.window_pos = Some((nx, ny));
                        platform::move_window(WINDOW_TITLE, nx, ny);
                    }
                }
            } else {
                self.drag_anchor = None;
            }
        } else {
            self.drag_anchor = None;
        }

        // Fast repaints only when something is animating or repeating.
        let busy = self.repeat.is_some() || flash_pos.is_some() || self.drag_anchor.is_some() || self.repositioning;
        ctx.request_repaint_after(Duration::from_millis(if busy { 16 } else { 33 }));
    }
}

// ============================== Painting ================================

struct ToolbarEvent {
    action: Option<Act>,
}

#[allow(clippy::too_many_arguments)]
fn paint_toolbar(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    typed_buffer: &str,
    typed_word: &str,
    lang: Lang,
    split_mode: bool,
    focused: bool,
    focused_col: usize,
) -> (ToolbarEvent, usize) {
    let mut event = ToolbarEvent { action: None };
    let painter = ui.painter_at(rect.expand(2.0));
    painter.rect_filled(rect, 8.0, egui::Color32::from_rgba_premultiplied(0, 0, 0, 45));

    #[cfg(feature = "voice")]
    let (is_rec, is_transcribing, had_speech) = (
        crate::voice::is_recording(),
        crate::voice::is_transcribing(),
        crate::voice::had_speech(),
    );
    #[cfg(not(feature = "voice"))]
    let (is_rec, is_transcribing, had_speech) = (false, false, false);

    if is_rec || is_transcribing {
        ui.ctx().request_repaint_after(Duration::from_millis(40));
    }

    let is_ar = lang == Lang::Ar;
    let has_clear = !typed_buffer.is_empty();

    let voice_label = if is_transcribing {
        if is_ar {
            crate::arabic::reshape_ar("⚡ معالجة...")
        } else {
            "⚡ Sending...".to_string()
        }
    } else if is_rec {
        if had_speech {
            if is_ar {
                crate::arabic::reshape_ar("🗣 مسموع")
            } else {
                "🗣 Speaking".to_string()
            }
        } else {
            if is_ar {
                crate::arabic::reshape_ar("🔴 استماع")
            } else {
                "🔴 Listening".to_string()
            }
        }
    } else {
        if is_ar {
            crate::arabic::reshape_ar("🎙 إملاء")
        } else {
            "🎙 Dictate".to_string()
        }
    };

    // --- Build right contextual buttons ---
    struct RightBtn {
        label: String,
        act: Act,
        is_primary: bool,
        col_idx: usize,
    }

    let mut right_btns = Vec::new();
    let start_col = if has_clear { 1 } else { 0 };

    if !typed_word.is_empty() {
        let preds = crate::predict::predict(typed_word, is_ar, 3);
        for (i, p) in preds.iter().enumerate() {
            right_btns.push(RightBtn {
                label: if is_ar {
                    crate::arabic::reshape_ar(p)
                } else {
                    p.to_string()
                },
                act: Act::AcceptPrediction(intern(p)),
                is_primary: false,
                col_idx: start_col + i,
            });
        }
        let enter_col = start_col + preds.len();
        right_btns.push(RightBtn {
            label: if is_ar {
                crate::arabic::reshape_ar("⏎ إرسال")
            } else {
                "⏎ Send".to_string()
            },
            act: Act::Enter,
            is_primary: true,
            col_idx: enter_col,
        });
    } else if has_clear {
        let send_lbl = if is_ar {
            crate::arabic::reshape_ar("⏎ إرسال")
        } else {
            "⏎ Send".to_string()
        };
        let paste_lbl = if is_ar {
            crate::arabic::reshape_ar("📋 لصق")
        } else {
            "📋 Paste".to_string()
        };
        let flip_lbl = if is_ar {
            crate::arabic::reshape_ar("↕ رفع")
        } else {
            "↕ Flip".to_string()
        };
        let lang_lbl = if lang == Lang::En {
            crate::arabic::reshape_ar("🌐 العربية")
        } else {
            "🌐 English".to_string()
        };

        right_btns.push(RightBtn {
            label: send_lbl,
            act: Act::Enter,
            is_primary: true,
            col_idx: start_col,
        });
        right_btns.push(RightBtn {
            label: voice_label,
            act: Act::VoiceDictation,
            is_primary: false,
            col_idx: start_col + 1,
        });
        right_btns.push(RightBtn {
            label: paste_lbl,
            act: Act::Paste,
            is_primary: false,
            col_idx: start_col + 2,
        });
        let split_lbl = if split_mode {
            if is_ar {
                crate::arabic::reshape_ar("🎮 منقسم")
            } else {
                "🎮 Split".to_string()
            }
        } else {
            if is_ar {
                crate::arabic::reshape_ar("⌨ فردي")
            } else {
                "⌨ Single".to_string()
            }
        };

        right_btns.push(RightBtn {
            label: flip_lbl,
            act: Act::FlipPosition,
            is_primary: false,
            col_idx: start_col + 3,
        });
        right_btns.push(RightBtn {
            label: split_lbl.clone(),
            act: Act::ToggleSplitMode,
            is_primary: false,
            col_idx: start_col + 4,
        });
        right_btns.push(RightBtn {
            label: lang_lbl,
            act: Act::Lang,
            is_primary: false,
            col_idx: start_col + 5,
        });
    } else {
        let paste_lbl = if is_ar {
            crate::arabic::reshape_ar("📋 لصق")
        } else {
            "📋 Paste".to_string()
        };
        let flip_lbl = if is_ar {
            crate::arabic::reshape_ar("↕ رفع")
        } else {
            "↕ Flip".to_string()
        };
        let lang_lbl = if lang == Lang::En {
            crate::arabic::reshape_ar("🌐 العربية")
        } else {
            "🌐 English".to_string()
        };
        let split_lbl = if split_mode {
            if is_ar {
                crate::arabic::reshape_ar("🎮 منقسم")
            } else {
                "🎮 Split".to_string()
            }
        } else {
            if is_ar {
                crate::arabic::reshape_ar("⌨ فردي")
            } else {
                "⌨ Single".to_string()
            }
        };

        right_btns.push(RightBtn {
            label: voice_label,
            act: Act::VoiceDictation,
            is_primary: false,
            col_idx: start_col,
        });
        right_btns.push(RightBtn {
            label: paste_lbl,
            act: Act::Paste,
            is_primary: false,
            col_idx: start_col + 1,
        });
        right_btns.push(RightBtn {
            label: flip_lbl,
            act: Act::FlipPosition,
            is_primary: false,
            col_idx: start_col + 2,
        });
        right_btns.push(RightBtn {
            label: "😊 Emoji".to_string(),
            act: Act::EmojiPicker,
            is_primary: false,
            col_idx: start_col + 3,
        });
        right_btns.push(RightBtn {
            label: split_lbl,
            act: Act::ToggleSplitMode,
            is_primary: false,
            col_idx: start_col + 4,
        });
        right_btns.push(RightBtn {
            label: lang_lbl,
            act: Act::Lang,
            is_primary: false,
            col_idx: start_col + 5,
        });
    }

    // --- Left Section: Live Search & Typing Bar Capsule ---
    let search_w = (rect.width() * 0.52).round();
    let search_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 2.0, rect.top() + 2.0),
        egui::vec2(search_w - 4.0, rect.height() - 4.0),
    );

    let search_fill = egui::Color32::from_rgb(18, 19, 23);
    let search_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgba_premultiplied(35, 35, 42, 35));
    painter.rect_filled(search_rect, 7.0, search_fill);
    painter.rect_stroke(search_rect, 7.0, search_stroke);

    // Search icon on left
    let icon_col = if is_rec {
        C_RECORDING
    } else if !typed_buffer.is_empty() {
        C_ACCENT
    } else {
        C_TEXT_DIM
    };
    painter.text(
        egui::pos2(search_rect.left() + 14.0, search_rect.center().y),
        egui::Align2::CENTER_CENTER,
        "🔍",
        egui::FontId::proportional(11.5),
        icon_col,
    );

    // Clear button inside search capsule on right if buffer has text
    if has_clear {
        let clear_rect = egui::Rect::from_center_size(
            egui::pos2(search_rect.right() - 15.0, search_rect.center().y),
            egui::vec2(18.0, 18.0),
        );
        let is_clear_focus = focused && focused_col == 0;
        let resp = ui.interact(clear_rect, egui::Id::new("vk_tb_clear_btn"), egui::Sense::click());
        let hovered = resp.hovered();
        let p = ui.painter_at(clear_rect.expand(2.0));

        if is_clear_focus {
            p.rect_filled(clear_rect.expand(2.0), 9.0, C_ACCENT_GLOW);
            p.rect_filled(clear_rect, 9.0, C_ACCENT);
            p.text(
                clear_rect.center(),
                egui::Align2::CENTER_CENTER,
                "✕",
                egui::FontId::proportional(11.0),
                C_ON_ACCENT,
            );
        } else if hovered {
            p.rect_filled(clear_rect, 9.0, C_CLOSE_HOVER);
            p.text(
                clear_rect.center(),
                egui::Align2::CENTER_CENTER,
                "✕",
                egui::FontId::proportional(11.0),
                egui::Color32::WHITE,
            );
        } else {
            p.rect_filled(
                clear_rect,
                9.0,
                egui::Color32::from_rgba_premultiplied(28, 28, 34, 28),
            );
            p.text(
                clear_rect.center(),
                egui::Align2::CENTER_CENTER,
                "✕",
                egui::FontId::proportional(11.0),
                C_TEXT_DIM,
            );
        }

        if resp.clicked() {
            event.action = Some(Act::ClearTyped);
        }
    }

    // Live Typed Text or Placeholder
    let right_margin = if has_clear { 28.0 } else { 12.0 };
    let left_margin = 28.0;
    let text_clip = egui::Rect::from_min_max(
        egui::pos2(search_rect.left() + left_margin, search_rect.top()),
        egui::pos2(search_rect.right() - right_margin, search_rect.bottom()),
    );
    let text_painter = ui.painter_at(text_clip);
    let font_id = egui::FontId::proportional(13.5);

    let is_ar_content = is_ar
        || typed_buffer
            .chars()
            .any(|c| ('\u{0600}'..='\u{06FF}').contains(&c));

    if typed_buffer.is_empty() {
        let placeholder = if is_ar {
            crate::arabic::reshape_ar("ابحث أو اكتب هنا...")
        } else {
            "Search or type here...".to_string()
        };
        let (align, pos) = if is_ar_content {
            (
                egui::Align2::RIGHT_CENTER,
                egui::pos2(text_clip.right() - 2.0, text_clip.center().y),
            )
        } else {
            (
                egui::Align2::LEFT_CENTER,
                egui::pos2(text_clip.left() + 2.0, text_clip.center().y),
            )
        };
        text_painter.text(
            pos,
            align,
            placeholder,
            font_id,
            egui::Color32::from_rgb(115, 120, 132),
        );
    } else {
        // Blinking cursor
        let show_cursor = ui.input(|i| i.time * 2.0).fract() < 0.5;
        ui.ctx().request_repaint_after(Duration::from_millis(250));
        let cursor_str = if show_cursor { "|" } else { " " };
        let display_raw = format!("{}{}", typed_buffer, cursor_str);
        let display_text = if is_ar_content {
            crate::arabic::reshape_ar(&display_raw)
        } else {
            display_raw
        };

        let text_w = ui.fonts(|f| {
            f.layout_no_wrap(display_text.clone(), font_id.clone(), C_TEXT)
                .size()
                .x
        });
        let avail_w = text_clip.width();
        let (align, pos) = if is_ar_content || text_w > avail_w {
            (
                egui::Align2::RIGHT_CENTER,
                egui::pos2(text_clip.right() - 2.0, text_clip.center().y),
            )
        } else {
            (
                egui::Align2::LEFT_CENTER,
                egui::pos2(text_clip.left() + 2.0, text_clip.center().y),
            )
        };

        text_painter.text(pos, align, display_text, font_id, C_TEXT);
    }

    // --- Right Section: Quick Actions / Word Predictions ---
    let right_rect = egui::Rect::from_min_max(
        egui::pos2(search_rect.right() + 6.0, rect.top() + 2.0),
        egui::pos2(rect.right() - 2.0, rect.bottom() - 2.0),
    );
    let n = right_btns.len() as f32;
    let btn_gap = 5.0;
    let btn_w = (right_rect.width() - (n - 1.0) * btn_gap) / n;

    for (i, btn) in right_btns.iter().enumerate() {
        let ir = egui::Rect::from_min_size(
            egui::pos2(
                right_rect.left() + i as f32 * (btn_w + btn_gap),
                right_rect.top() + 1.0,
            ),
            egui::vec2(btn_w, right_rect.height() - 2.0),
        );
        let is_focus = focused && focused_col == btn.col_idx;
        let resp = ui.interact(ir, egui::Id::new(("vk_tb_btn", btn.col_idx)), egui::Sense::click());
        let hovered = resp.hovered();
        let p = ui.painter_at(ir.expand(2.5));

        let is_voice_active = btn.act == Act::VoiceDictation && (is_rec || is_transcribing);

        let (fill, stroke, text_col) = if is_focus {
            (C_ACCENT, egui::Stroke::new(1.5, C_ACCENT_HOVER), C_ON_ACCENT)
        } else if is_voice_active {
            let bg = if is_transcribing {
                C_ACCENT
            } else if had_speech {
                C_SUCCESS
            } else {
                C_RECORDING
            };
            (bg, egui::Stroke::NONE, C_ON_ACCENT)
        } else if btn.is_primary {
            if hovered {
                (C_ACCENT, egui::Stroke::NONE, C_ON_ACCENT)
            } else {
                (
                    C_ACCENT_SUBTLE,
                    egui::Stroke::new(1.0, C_ACCENT),
                    C_ACCENT_HOVER,
                )
            }
        } else if hovered {
            (C_KEY_HOVER, egui::Stroke::NONE, C_TEXT)
        } else {
            (
                egui::Color32::from_rgba_premultiplied(18, 18, 22, 18),
                egui::Stroke::NONE,
                C_TEXT,
            )
        };

        if is_focus {
            p.rect_filled(ir.expand(2.5), 7.5, C_ACCENT_GLOW);
            p.rect_filled(ir, 6.0, fill);
            p.rect_stroke(ir, 6.0, stroke);
        } else {
            p.rect_filled(ir, 6.0, fill);
            if stroke.width > 0.0 {
                p.rect_stroke(ir, 6.0, stroke);
            }
        }

        p.text(
            ir.center(),
            egui::Align2::CENTER_CENTER,
            &btn.label,
            egui::FontId::proportional(12.5),
            text_col,
        );

        if resp.clicked() {
            event.action = Some(btn.act);
        }
    }

    let total_count = (if has_clear { 1 } else { 0 }) + right_btns.len();
    (event, total_count)
}

fn paint_radial_menu(
    ui: &mut egui::Ui,
    area: egui::Rect,
    selected_slice: Option<usize>,
) -> Option<usize> {
    let center = area.center();
    let painter = ui.painter_at(area);
    let mut chosen_slice = None;

    let slices: &[(&str, &str)] = &[
        ("⌨ Keyboard", "الكيبورد"),
        ("🔊 Vol Up", "رفع الصوت"),
        ("🗔 Task View", "المهام"),
        ("🔉 Vol Down", "خفض الصوت"),
        ("🖥 Desktop", "سطح المكتب"),
        ("🎙 Dictate", "إملاء صوتي"),
        ("✕ Close App", "إغلاق النافذة"),
        ("📸 Capture", "لقطة شاشة"),
    ];

    // Outer ambient backdrop disc
    painter.circle_filled(
        center,
        202.0,
        egui::Color32::from_rgba_premultiplied(18, 19, 23, 246),
    );
    painter.circle_stroke(center, 202.0, egui::Stroke::new(1.5, C_PANEL_BORDER));

    // Pointer detection
    let ptr = ui.input(|i| i.pointer.hover_pos());
    let mut hovered_slice = None;
    let mut hovered_center = false;
    if let Some(pos) = ptr {
        let d = (pos - center).length();
        if d < 52.0 {
            hovered_center = true;
        } else if (52.0..=196.0).contains(&d) {
            let phi = (pos.y - center.y).atan2(pos.x - center.x);
            let mut theta = phi + std::f32::consts::FRAC_PI_2;
            let two_pi = std::f32::consts::PI * 2.0;
            while theta < 0.0 {
                theta += two_pi;
            }
            while theta >= two_pi {
                theta -= two_pi;
            }
            let step = two_pi / 8.0;
            let s = (((theta + step * 0.5) / step).floor() as usize) % 8;
            hovered_slice = Some(s);
        }
    }

    let clicked = ui.input(|i| i.pointer.primary_clicked());
    if clicked {
        if hovered_center {
            chosen_slice = Some(8); // Cancel
        } else if let Some(s) = hovered_slice {
            chosen_slice = Some(s);
        }
    }

    let active_slice = hovered_slice.or(selected_slice);

    // Draw the 8 modern segmented wedges
    let step = std::f32::consts::PI * 2.0 / 8.0;
    let gap_angle = 0.025; // clean separation gap between wedges
    for (i, &(title, sub)) in slices.iter().enumerate().take(8) {
        let mid_angle = -std::f32::consts::FRAC_PI_2 + (i as f32) * step;
        let start_angle = mid_angle - step * 0.5 + gap_angle;
        let end_angle = mid_angle + step * 0.5 - gap_angle;

        let is_active = active_slice == Some(i);
        let outer_r = if is_active { 193.0 } else { 188.0 };
        let inner_r = 56.0;

        let fill = if is_active {
            C_ACCENT
        } else {
            egui::Color32::from_rgb(38, 40, 47)
        };
        let stroke = if is_active {
            egui::Stroke::new(2.0, C_ACCENT_HOVER)
        } else {
            egui::Stroke::new(1.0, C_PANEL_BORDER)
        };
        let text_col = if is_active { C_ON_ACCENT } else { C_TEXT };
        let sub_col = if is_active { C_ON_ACCENT } else { C_TEXT_DIM };

        let mut pts = Vec::with_capacity(22);
        let mut outer_arc = Vec::with_capacity(11);
        for step_idx in 0..=10 {
            let a = start_angle + (step_idx as f32 / 10.0) * (end_angle - start_angle);
            let p = center + egui::vec2(a.cos() * outer_r, a.sin() * outer_r);
            pts.push(p);
            outer_arc.push(p);
        }
        for step_idx in (0..=10).rev() {
            let a = start_angle + (step_idx as f32 / 10.0) * (end_angle - start_angle);
            pts.push(center + egui::vec2(a.cos() * inner_r, a.sin() * inner_r));
        }
        painter.add(egui::Shape::convex_polygon(pts, fill, stroke));

        if is_active {
            painter.add(egui::Shape::line(
                outer_arc,
                egui::Stroke::new(3.5, C_TEXT),
            ));
        }

        // Text & icon
        let text_pos = center + egui::vec2(mid_angle.cos() * 124.0, mid_angle.sin() * 124.0);
        let reshaped_sub = crate::arabic::reshape_ar(sub);
        painter.text(
            text_pos - egui::vec2(0.0, 7.5),
            egui::Align2::CENTER_CENTER,
            title,
            egui::FontId::proportional(if is_active { 13.5 } else { 12.5 }),
            text_col,
        );
        painter.text(
            text_pos + egui::vec2(0.0, 9.0),
            egui::Align2::CENTER_CENTER,
            reshaped_sub,
            egui::FontId::proportional(11.0),
            sub_col,
        );
    }

    // Center hub with active sector indicator
    let center_fill = if hovered_center { C_CLOSE_HOVER } else { C_CHROME };
    painter.circle_filled(center, 49.0, center_fill);
    painter.circle_stroke(
        center,
        49.0,
        egui::Stroke::new(2.0, if active_slice.is_some() { C_ACCENT } else { C_PANEL_BORDER }),
    );

    if let Some(active_idx) = active_slice {
        let mid_angle = -std::f32::consts::FRAC_PI_2 + (active_idx as f32) * step;
        let tip = center + egui::vec2(mid_angle.cos() * 54.0, mid_angle.sin() * 54.0);
        painter.circle_filled(tip, 4.0, C_ACCENT_HOVER);
    }

    let center_label = if let Some(s) = active_slice {
        slices[s].0
    } else {
        "✕\nClose"
    };
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        center_label,
        egui::FontId::proportional(11.5),
        C_TEXT,
    );

    chosen_slice
}

struct ChromeStatus {
    lang: Lang,
    layer: Layer,
    shift: ShiftState,
    caps: bool,
    ctrl: bool,
    alt: bool,
    altgr: bool,
    hints_on: bool,
}

struct Chrome {
    close: bool,
    help: bool,
    radial: bool,
    drag: egui::Response,
}

/// Top Fluent chrome strip:
/// Left: `?` help button + live status badges (`EN · ABC`, active modifier pills).
/// Centre: sleek grabber handle pill.
/// Right: `◎` Quick Radial Menu switcher + `✕` crimson-hover close button.
fn paint_chrome(ui: &mut egui::Ui, area: egui::Rect, st: &ChromeStatus) -> Chrome {
    let painter = ui.painter_at(area.expand(PAD));
    let strip = egui::Rect::from_min_size(
        egui::pos2(area.left() - PAD, area.top() - PAD),
        egui::vec2(area.width() + PAD * 2.0, CHROME_H + PAD),
    );
    painter.rect_filled(
        strip,
        egui::Rounding { nw: 14.0, ne: 14.0, sw: 0.0, se: 0.0 },
        C_CHROME,
    );

    let cy = area.top() + (CHROME_H - PAD) * 0.5 + 2.0;

    let icon = || egui::FontId::proportional(13.0);
    let button = |ui: &mut egui::Ui,
                  cx: f32,
                  id: &'static str,
                  glyph: &str,
                  lit: bool,
                  hover_fill: egui::Color32| {
        let r = egui::Rect::from_center_size(egui::pos2(cx, cy), egui::vec2(24.0, 22.0));
        let resp = ui.interact(r, egui::Id::new(id), egui::Sense::click());
        let bp = ui.painter_at(r);
        let hovered = resp.hovered();
        let pressed = hovered && ui.input(|i| i.pointer.primary_pressed());
        if hovered {
            bp.rect_filled(r, 6.0, hover_fill);
        } else if lit {
            bp.rect_filled(r, 6.0, C_ACCENT_SUBTLE);
        }
        let col = if hovered {
            if hover_fill == C_CLOSE_HOVER { egui::Color32::WHITE } else { C_TEXT }
        } else if lit {
            C_ACCENT
        } else {
            C_TEXT_DIM
        };
        bp.text(r.center(), egui::Align2::CENTER_CENTER, glyph, icon(), col);
        resp.clicked() || pressed
    };

    let help = button(ui, area.left() + 14.0, "vk_help", "?", st.hints_on, C_KEY_HOVER);

    // Status pills next to `?`
    let mut badge_x = area.left() + 34.0;
    let draw_badge = |x: &mut f32, text: &str, active: bool| {
        let w = (text.chars().count() as f32 * 6.5 + 14.0).max(36.0);
        let r = egui::Rect::from_min_size(egui::pos2(*x, cy - 9.5), egui::vec2(w, 19.0));
        let fill = if active { C_ACCENT } else { egui::Color32::from_rgba_unmultiplied(255, 255, 255, 12) };
        let fg = if active { C_ON_ACCENT } else { C_TEXT_DIM };
        painter.rect_filled(r, 5.0, fill);
        painter.text(r.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(11.0), fg);
        *x += w + 5.0;
    };

    let mode_tag = match (st.lang, st.layer) {
        (Lang::En, Layer::Abc) => "EN · ABC",
        (Lang::Ar, Layer::Abc) => "ع · أبجد",
        (Lang::En, Layer::Num) => "EN · 123",
        (Lang::Ar, Layer::Num) => "ع · 123",
        (_, Layer::Emoji) => "😊 EMOJI",
        (_, Layer::Edit) => "EDIT · تحرير",
    };
    let reshaped_mode = if st.lang == Lang::Ar || mode_tag.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)) {
        crate::arabic::reshape_ar(mode_tag)
    } else {
        mode_tag.to_string()
    };
    draw_badge(&mut badge_x, &reshaped_mode, false);

    if st.caps {
        draw_badge(&mut badge_x, "CAPS", true);
    }
    if st.shift == ShiftState::Locked {
        draw_badge(&mut badge_x, "SHIFT LOCK", true);
    } else if st.shift == ShiftState::Once {
        draw_badge(&mut badge_x, "SHIFT", true);
    }
    if st.ctrl {
        draw_badge(&mut badge_x, "CTRL", true);
    }
    if st.alt {
        draw_badge(&mut badge_x, "ALT", true);
    }
    if st.altgr {
        draw_badge(&mut badge_x, "ALTGR", true);
    }

    // Right buttons: Radial Menu switch (`◎`) and Close (`✕`)
    let radial = button(ui, area.right() - 44.0, "vk_radial_btn", "◎", false, C_KEY_HOVER);
    let close = button(ui, area.right() - 14.0, "vk_close", "\u{2715}", false, C_CLOSE_HOVER);

    // Drag handle covers the middle region between the buttons
    let drag_rect = egui::Rect::from_min_max(
        egui::pos2(area.left() + 32.0, strip.top()),
        egui::pos2(area.right() - 62.0, strip.bottom()),
    );
    let drag = ui.interact(drag_rect, egui::Id::new("vk_drag"), egui::Sense::click_and_drag());
    let pill_col = if drag.hovered() || drag.dragged() { C_ACCENT } else { C_PILL };
    let pill_w = if drag.hovered() || drag.dragged() { 48.0 } else { 38.0 };
    painter.rect_filled(
        egui::Rect::from_center_size(egui::pos2(area.center().x, cy), egui::vec2(pill_w, 4.0)),
        2.0,
        pill_col,
    );

    Chrome { close, help, radial, drag }
}

struct KeyView<'a> {
    label: &'a str,
    alt: Option<&'a str>,
    act: Act,
    glyph: Glyph,
    special: bool,
    focused: bool,
    split_left: bool,
    split_right: bool,
    /// Armed latch (Shift/Ctrl/Alt/AltGr/Caps/Hints) — drawn in the accent colour.
    active: bool,
    /// Shift/Caps lock — adds an underline bar or status LED.
    locked: bool,
    /// Controller activation flash.
    flash: bool,
    /// False while the hints overlay covers the keys.
    enabled: bool,
}

struct KeyEvent {
    /// The pointer went down on this key this frame (fires immediately, so
    /// typing feels instant and hold-to-repeat can start).
    down_now: bool,
    /// Physical mouse click completed (pressed & released on this key).
    clicked: bool,
    /// The pointer is still held down on this key.
    held: bool,
}

/// Paint one Fluent 2 tactile keycap at an explicit rect.
fn paint_key(ui: &mut egui::Ui, id: egui::Id, rect: egui::Rect, v: &KeyView) -> KeyEvent {
    let sense = if v.enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let resp = ui.interact(rect, id, sense);
    let painter = ui.painter_at(rect.expand(4.0));

    let hovered = v.enabled && resp.hovered();
    let down_now = hovered && ui.input(|i| i.pointer.primary_pressed());
    let clicked = v.enabled && resp.clicked();
    let held = v.enabled && resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down());
    let is_down = held || v.flash;
    let is_focused = v.focused || v.split_left || v.split_right;

    // Slight physical downward shift when depressed
    let draw_rect = if is_down {
        rect.translate(egui::vec2(0.0, 1.0))
    } else {
        rect
    };

    // Controller Focus transforms the key into a vibrant solid glowing tile!
    let fill = if v.split_left {
        if is_down {
            egui::Color32::from_rgb(0, 130, 180)
        } else if hovered {
            egui::Color32::from_rgb(0, 205, 255)
        } else {
            egui::Color32::from_rgb(0, 160, 220)
        }
    } else if v.split_right {
        if is_down {
            egui::Color32::from_rgb(150, 40, 175)
        } else if hovered {
            egui::Color32::from_rgb(235, 95, 255)
        } else {
            egui::Color32::from_rgb(190, 60, 215)
        }
    } else if is_focused || v.active {
        if is_down {
            C_ACCENT_DOWN
        } else if hovered {
            C_ACCENT_HOVER
        } else {
            C_ACCENT
        }
    } else if is_down {
        C_KEY_DOWN
    } else if hovered {
        C_KEY_HOVER
    } else if v.special {
        C_KEY_SPECIAL
    } else {
        C_KEY
    };

    let fg = if is_focused || v.active {
        C_ON_ACCENT
    } else {
        C_TEXT
    };

    let dim = if is_focused || v.active {
        C_ON_ACCENT
    } else {
        C_TEXT_DIM
    };

    // Ambient controller focus glow halo behind the focused key
    if v.split_left {
        painter.rect_filled(
            draw_rect.expand(3.5),
            ROUNDING + 2.5,
            egui::Color32::from_rgba_unmultiplied(0, 195, 255, 65),
        );
    } else if v.split_right {
        painter.rect_filled(
            draw_rect.expand(3.5),
            ROUNDING + 2.5,
            egui::Color32::from_rgba_unmultiplied(220, 80, 255, 65),
        );
    } else if is_focused {
        painter.rect_filled(
            draw_rect.expand(3.5),
            ROUNDING + 2.5,
            C_ACCENT_GLOW,
        );
    }

    // Bottom physical shadow rim (hidden when pressed down or focused)
    if !is_down && !is_focused {
        let shadow_rect = egui::Rect::from_min_max(
            egui::pos2(draw_rect.left() + 1.0, draw_rect.bottom() - 2.0),
            egui::pos2(draw_rect.right() - 1.0, draw_rect.bottom() + 1.0),
        );
        painter.rect_filled(shadow_rect, ROUNDING, C_KEY_BOT_SHADOW);
    }

    // Main keycap surface (Solid tile, clean rounded rectangle)
    painter.rect_filled(draw_rect, ROUNDING, fill);

    // Subtle top specular highlight edge (gives soft tactile elevation)
    if !is_down && !is_focused && !v.active {
        painter.line_segment(
            [
                egui::pos2(draw_rect.left() + ROUNDING, draw_rect.top() + 0.8),
                egui::pos2(draw_rect.right() - ROUNDING, draw_rect.top() + 0.8),
            ],
            egui::Stroke::new(1.0, C_KEY_TOP_BEVEL),
        );
    }

    // Fine border: ONLY on the focused key!
    if v.split_left {
        painter.rect_stroke(
            draw_rect,
            ROUNDING,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 220, 255)),
        );
        painter.text(
            egui::pos2(draw_rect.left() + 5.0, draw_rect.top() + 3.0),
            egui::Align2::LEFT_TOP,
            "LT",
            egui::FontId::proportional(9.0),
            egui::Color32::WHITE,
        );
    } else if v.split_right {
        painter.rect_stroke(
            draw_rect,
            ROUNDING,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(240, 100, 255)),
        );
        painter.text(
            egui::pos2(draw_rect.left() + 5.0, draw_rect.top() + 3.0),
            egui::Align2::LEFT_TOP,
            "RT",
            egui::FontId::proportional(9.0),
            egui::Color32::WHITE,
        );
    } else if is_focused {
        painter.rect_stroke(
            draw_rect,
            ROUNDING,
            egui::Stroke::new(1.5, C_ACCENT_HOVER),
        );
    }

    match v.glyph {
        Glyph::Win => draw_win(&painter, draw_rect, fg),
        Glyph::Pad => draw_pad(&painter, draw_rect, fg),
        Glyph::Enter => draw_enter(&painter, draw_rect, fg),
        Glyph::ShiftEnter => draw_shift_enter(&painter, draw_rect, fg),
        Glyph::Backspace => draw_backspace(&painter, draw_rect, fg),
        Glyph::Text => {
            if v.act == Act::Space {
                draw_spacebar(&painter, draw_rect, v.label, dim);
            } else {
                let n = v.label.chars().count();
                let arabic = v
                    .label
                    .chars()
                    .next()
                    .is_some_and(|c| ('\u{0600}'..='\u{06FF}').contains(&c));
                let k = KEY_H / 40.0;
                let size = if arabic && n <= 2 {
                    20.0 * k
                } else if n > 2 {
                    13.0 * k
                } else {
                    17.0 * k
                };
                let display_text = if arabic && n > 1 {
                    crate::arabic::reshape_ar(v.label)
                } else {
                    v.label.to_string()
                };
                let text_center = if v.alt.is_some() && n == 1 {
                    egui::pos2(draw_rect.center().x - 1.0, draw_rect.center().y + 1.0)
                } else {
                    draw_rect.center()
                };
                painter.text(
                    text_center,
                    egui::Align2::CENTER_CENTER,
                    display_text,
                    egui::FontId::proportional(size),
                    fg,
                );
            }
        }
    }

    // Top-right corner secondary glyph (digits, shifted symbols, Latin hints)
    if matches!(v.glyph, Glyph::Text | Glyph::Win) {
        if let Some(a) = v.alt {
            let k = (KEY_H / 40.0).clamp(1.0, 1.4);
            let alt_col = if v.active {
                C_ON_ACCENT
            } else if is_focused {
                C_ACCENT_HOVER
            } else {
                egui::Color32::from_rgba_unmultiplied(165, 172, 186, 210)
            };
            painter.text(
                egui::pos2(draw_rect.right() - 7.5, draw_rect.top() + 4.5),
                egui::Align2::RIGHT_TOP,
                a,
                egui::FontId::proportional(10.5 * k),
                alt_col,
            );
        }
    }

    // Caps Lock LED dot in top-left corner
    if v.act == Act::Caps {
        let led_col = if v.active { C_ON_ACCENT } else { C_PILL };
        painter.circle_filled(
            egui::pos2(draw_rect.left() + 7.5, draw_rect.top() + 7.5),
            2.5,
            led_col,
        );
    }

    // Locked underline indicator (Shift Lock / Caps Lock)
    if v.locked {
        painter.rect_filled(
            egui::Rect::from_center_size(
                egui::pos2(draw_rect.center().x, draw_rect.bottom() - 4.5),
                egui::vec2(draw_rect.width() * 0.36, 2.5),
            ),
            1.5,
            C_ON_ACCENT,
        );
    }

    KeyEvent { down_now, clicked, held }
}

/// Spacebar: subtle Fluent pill indicator + dim label.
fn draw_spacebar(p: &egui::Painter, rect: egui::Rect, label: &str, col: egui::Color32) {
    let pill_w = (rect.width() * 0.36).clamp(40.0, 110.0);
    p.rect_filled(
        egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.bottom() - 7.5),
            egui::vec2(pill_w, 3.0),
        ),
        1.5,
        col.gamma_multiply(0.7),
    );
    let display_label = if label.contains('\u{0645}') {
        crate::arabic::reshape_ar(label)
    } else {
        label.to_string()
    };
    p.text(
        egui::pos2(rect.center().x, rect.center().y - 2.5),
        egui::Align2::CENTER_CENTER,
        display_label,
        egui::FontId::proportional(12.5),
        col,
    );
}

/// Windows 11 logo: four symmetric squares.
fn draw_win(p: &egui::Painter, rect: egui::Rect, col: egui::Color32) {
    let s = rect.height() * 0.17;
    let gap = 1.8;
    let c = rect.center();
    for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let center_sq = egui::pos2(c.x + dx * (s * 0.5 + gap * 0.5), c.y + dy * (s * 0.5 + gap * 0.5));
        p.rect_filled(egui::Rect::from_center_size(center_sq, egui::vec2(s, s)), 1.5, col);
    }
}

/// Controller glyph: a rounded body with two sticks and four buttons.
fn draw_pad(p: &egui::Painter, rect: egui::Rect, col: egui::Color32) {
    let s = rect.height() * 0.26;
    let c = rect.center();
    let pt = |dx: f32, dy: f32| egui::pos2(c.x + dx * s, c.y + dy * s);
    let r = rect.height() * 0.30;
    p.rect_stroke(
        egui::Rect::from_center_size(c, egui::vec2(r * 1.9, r)),
        r * 0.45,
        egui::Stroke::new(1.6, col),
    );
    let d = r * 0.30;
    p.circle_filled(pt(-0.75, 0.15), d, col);
    p.circle_filled(pt(0.75, 0.15), d, col);
    let q = r * 0.17;
    for (dx, dy) in [(0.35, -0.30), (0.75, -0.30), (0.35, -0.60), (0.75, -0.60)] {
        p.circle_filled(pt(dx, dy), q, col.gamma_multiply(0.75));
    }
}

/// Return arrow: vector-drawn, because ↵/⏎ are missing from Segoe UI.
fn draw_enter(p: &egui::Painter, rect: egui::Rect, col: egui::Color32) {
    let c = rect.center();
    let w = rect.height() * 0.36;
    let h = rect.height() * 0.36;
    let stroke = egui::Stroke::new(2.0, col);
    let pt = |x: f32, y: f32| egui::pos2(c.x + x, c.y + y);
    p.line_segment([pt(w * 0.45, -h * 0.40), pt(w * 0.45, h * 0.30)], stroke);
    p.line_segment([pt(w * 0.45, h * 0.30), pt(-w * 0.45, h * 0.30)], stroke);
    p.line_segment([pt(-w * 0.45, h * 0.30), pt(-w * 0.12, h * 0.0)], stroke);
    p.line_segment([pt(-w * 0.45, h * 0.30), pt(-w * 0.12, h * 0.60)], stroke);
}

/// Shift+Enter glyph: vector-drawn upward shift arrow paired with return arrow.
fn draw_shift_enter(p: &egui::Painter, rect: egui::Rect, col: egui::Color32) {
    let c = rect.center();
    let h = rect.height() * 0.36;
    let stroke = egui::Stroke::new(2.0, col);

    // Left side: Upward Shift arrow (at c.x - 12.0)
    let sx = c.x - 12.0;
    let sy = c.y;
    p.line_segment([egui::pos2(sx, sy + h * 0.45), egui::pos2(sx, sy - h * 0.45)], stroke);
    p.line_segment([egui::pos2(sx, sy - h * 0.45), egui::pos2(sx - 4.5, sy - h * 0.05)], stroke);
    p.line_segment([egui::pos2(sx, sy - h * 0.45), egui::pos2(sx + 4.5, sy - h * 0.05)], stroke);

    // Right side: Classic return arrow (at c.x + 9.0)
    let ex = c.x + 9.0;
    let w = rect.height() * 0.28;
    let pt = |x: f32, y: f32| egui::pos2(ex + x, c.y + y);
    p.line_segment([pt(w * 0.45, -h * 0.40), pt(w * 0.45, h * 0.30)], stroke);
    p.line_segment([pt(w * 0.45, h * 0.30), pt(-w * 0.45, h * 0.30)], stroke);
    p.line_segment([pt(-w * 0.45, h * 0.30), pt(-w * 0.12, 0.0)], stroke);
    p.line_segment([pt(-w * 0.45, h * 0.30), pt(-w * 0.12, h * 0.60)], stroke);
}

/// Backspace glyph: crisp vector pentagon tag with an inner `×`.
fn draw_backspace(p: &egui::Painter, rect: egui::Rect, col: egui::Color32) {
    let c = rect.center();
    let w = (rect.height() * 0.48).min(rect.width() * 0.35);
    let h = rect.height() * 0.32;
    let stroke = egui::Stroke::new(1.8, col);
    let p_tip = egui::pos2(c.x - w * 0.55, c.y);
    let p_tl = egui::pos2(c.x - w * 0.18, c.y - h * 0.5);
    let p_tr = egui::pos2(c.x + w * 0.50, c.y - h * 0.5);
    let p_br = egui::pos2(c.x + w * 0.50, c.y + h * 0.5);
    let p_bl = egui::pos2(c.x - w * 0.18, c.y + h * 0.5);

    p.line_segment([p_tip, p_tl], stroke);
    p.line_segment([p_tl, p_tr], stroke);
    p.line_segment([p_tr, p_br], stroke);
    p.line_segment([p_br, p_bl], stroke);
    p.line_segment([p_bl, p_tip], stroke);

    let xc = c.x + w * 0.15;
    let r = h * 0.22;
    p.line_segment([egui::pos2(xc - r, c.y - r), egui::pos2(xc + r, c.y + r)], stroke);
    p.line_segment([egui::pos2(xc - r, c.y + r), egui::pos2(xc + r, c.y - r)], stroke);
}

/// Controller cheat-sheet drawn over the keys. Returns true if it was clicked
/// (= dismiss).
fn paint_hints(ui: &mut egui::Ui, area: egui::Rect) -> bool {
    let panel = egui::Rect::from_min_max(
        egui::pos2(area.left() + 4.0, area.top() + CHROME_H + 2.0),
        egui::pos2(area.right() - 4.0, area.bottom() - 2.0),
    );
    let resp = ui.interact(panel, egui::Id::new("vk_hints"), egui::Sense::click());
    let p = ui.painter();
    p.rect_filled(panel, 10.0, C_SCRIM);
    p.rect_stroke(panel, 10.0, egui::Stroke::new(1.5, C_ACCENT));
    p.text(
        egui::pos2(panel.left() + 18.0, panel.top() + 12.0),
        egui::Align2::LEFT_TOP,
        "🎮 Controller Shortcuts",
        egui::FontId::proportional(15.0),
        C_ACCENT,
    );

    let per_col = HINTS.len().div_ceil(2);
    let col_w = panel.width() * 0.5;
    let line_h = 24.0;
    for (i, (button, what)) in HINTS.iter().enumerate() {
        let (col, row) = (i / per_col, i % per_col);
        let x = panel.left() + 18.0 + col as f32 * col_w;
        let y = panel.top() + 42.0 + row as f32 * line_h;

        let badge_rect = egui::Rect::from_min_size(egui::pos2(x, y - 1.0), egui::vec2(126.0, 19.0));
        p.rect_filled(badge_rect, 4.0, C_KEY_SPECIAL);
        p.rect_stroke(badge_rect, 4.0, egui::Stroke::new(1.0, C_PANEL_BORDER));
        p.text(
            badge_rect.center(),
            egui::Align2::CENTER_CENTER,
            *button,
            egui::FontId::proportional(12.0),
            C_TEXT,
        );
        p.text(
            egui::pos2(x + 136.0, y),
            egui::Align2::LEFT_TOP,
            *what,
            egui::FontId::proportional(13.0),
            C_TEXT_DIM,
        );
    }
    p.text(
        egui::pos2(panel.center().x, panel.bottom() - 10.0),
        egui::Align2::CENTER_BOTTOM,
        "Click anywhere or press R3 to dismiss",
        egui::FontId::proportional(11.5),
        C_TEXT_DIM,
    );
    resp.clicked()
}

// ============================ Bootstrap ================================

/// Spawn the overlay thread. The keyboard stays hidden until a text box is
/// clicked or the tray/LeftThumb toggles it, unless `start_visible` is set.
///
/// `demo` (the `--demo-kb` / `--demo-kb-ar` flags) makes the overlay ignore
/// controller input entirely, so a screenshot always shows the requested
/// language instead of whatever button the pad happens to be reporting.
pub fn spawn(
    nav: Arc<Mutex<VkNav>>,
    prefs: crate::config::KeyboardPrefs,
    start_visible: bool,
    demo: bool,
) -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(start_visible));
    let arabic = prefs.start_arabic;
    crate::platform::log_debug(&format!(
        "[vkeyboard] spawn() called: start_visible={start_visible} demo={demo}"
    ));
    {
        let flag = flag.clone();
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
                    Box::new(VKbdApp::new(
                        flag.clone(),
                        nav.clone(),
                        lang_of(arabic),
                        demo,
                        prefs.clone(),
                    ))
                }),
            );
            crate::platform::log_debug(&format!("[vkeyboard] eframe::run_native finished: {res:?}"));
            if let Err(e) = res {
                eprintln!("[vkeyboard] eframe error: {e}");
            }
        });
    }
    flag
}

fn lang_of(arabic: bool) -> Lang {
    if arabic {
        Lang::Ar
    } else {
        Lang::En
    }
}

/// Load Windows system fonts (Latin + Arabic) and symbols so Arabic text and arrow/enter glyphs
/// render beautifully instead of tofu boxes.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut loaded: Vec<String> = Vec::new();
    for (name, path) in [
        ("segoe-ui", "C:\\Windows\\Fonts\\segoeui.ttf"),
        ("arial", "C:\\Windows\\Fonts\\arial.ttf"),
        ("tahoma", "C:\\Windows\\Fonts\\tahoma.ttf"),
        ("segoe-sym", "C:\\Windows\\Fonts\\seguisym.ttf"),
        ("segoe-emoji", "C:\\Windows\\Fonts\\seguiemj.ttf"),
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
            let sizes = widths(&rows);
            assert!(sizes.iter().all(|size| (*size - sizes[0]).abs() < 0.001));
            assert!(!repeatable(Act::DeleteWord));
            assert!(build_layout(lang, Layer::Abc).iter().flatten().any(|key| key.act == Act::EditTools));
        }
    }

    #[test]
    fn switching_target_discards_old_predictions_and_preview() {
        let mut app = VKbdApp::new(Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(VkNav::default())), Lang::En, false, crate::config::KeyboardPrefs::default());
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

    fn widths(rows: &[Vec<KDef>]) -> Vec<f32> {
        rows.iter().map(|r| r.iter().map(|k| k.w).sum::<f32>()).collect()
    }

    #[test]
    fn every_row_has_the_same_width() {
        for (lang, layer) in ALL {
            let rows = build_layout(lang, layer);
            let w = widths(&rows);
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
    fn arabic_tashkeel_row_is_not_visibly_gappy() {
        let rows = ar_abc();
        let r4 = &rows[3];
        assert!(r4.len() >= 12, "tashkeel row has only {} keys", r4.len());
        let total: f32 = r4.iter().map(|k| k.w).sum();
        let widest = r4.iter().map(|k| k.w).fold(0.0f32, f32::max);
        assert!(
            widest <= total / 3.0,
            "one tashkeel key covers {widest} of {total} units"
        );
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
        let rows = en_abc();
        let bottom = rows.last().unwrap();
        let acts: Vec<Act> = bottom.iter().map(|k| k.act).collect();
        for needed in [Act::Lang, Act::ToggleLayer, Act::Space, Act::ShiftEnter, Act::WinKey, Act::AltGr] {
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
            assert!(rows[4].iter().any(|k| k.act == Act::ShiftEnter));
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
    fn arabic_layout_has_no_digit_row() {
        let rows = ar_abc();
        let first = &rows[0];
        assert!(!first.is_empty());
        let plain_digits = first
            .iter()
            .all(|k| matches!(k.label, "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"));
        assert!(!plain_digits, "Arabic layout should not have a digit row");
    }

    #[test]
    fn arabic_ligature_is_a_string_action() {
        let rows = ar_abc();
        assert!(
            rows.iter().flatten().any(|k| matches!(k.act, Act::Str("لا"))),
            "the لا ligature should be emitted as a string"
        );
    }

    #[test]
    fn arabic_layout_has_the_full_alphabet_with_no_duplicates() {
        let rows = ar_abc();
        let mut seen = HashSet::new();
        for k in rows.iter().flatten() {
            if let Act::Char(c, _) = k.act {
                assert!(seen.insert(c), "duplicate key for {c:?} in the Arabic layout");
            }
        }
        for c in "ضصثقفغعهخحجدشسيبلاتنمكطذئءؤرىةوزظ".chars() {
            assert!(seen.contains(&c), "missing Arabic letter {c}");
        }
    }

    #[test]
    fn shift_swaps_every_letter_label() {
        let rows = en_abc();
        let b = rows[1].iter().find(|k| k.label == "q").unwrap();
        assert_eq!(b.shifted, Some("Q"));
        assert_eq!(shown_label(b, true, false), "Q");
        assert_eq!(shown_label(b, false, true), "Q");
        assert_eq!(shown_label(b, true, true), "q");
    }

    #[test]
    fn shift_also_swaps_punctuation_labels() {
        let rows = en_abc();
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
        let rows = en_abc();
        let r1 = &rows[1];
        let mid = r1.len() / 2;
        let left_idx = nearest_col_clamped(r1, 5000.0, 0, mid);
        assert!(left_idx < mid);
        let right_idx = nearest_col_clamped(r1, 0.0, mid, r1.len());
        assert!(right_idx >= mid);
    }

    #[test]
    fn test_split_mode_toggle_action_exists() {
        let app = VKbdApp::new(
            Arc::new(AtomicBool::new(false)),
            Arc::new(Mutex::new(VkNav::default())),
            Lang::En,
            false,
            crate::config::KeyboardPrefs::default(),
        );
        assert!(!app.split_mode);
        let acts = app.get_toolbar_actions();
        assert!(acts.contains(&Act::ToggleSplitMode));
    }

    #[test]
    fn emoji_layer_has_uniform_row_widths_and_reposition_flag() {
        for cat in 0..6 {
            let rows = emoji_layer(cat);
            let target = layout_units(&rows);
            for (idx, r) in rows.iter().enumerate() {
                let units: f32 = r.iter().map(|k| k.w).sum();
                assert!(
                    (units - target).abs() < 0.01,
                    "cat {cat} row {idx} width {units} != target {target}"
                );
            }
        }
    }
}
