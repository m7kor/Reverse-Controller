//! Keyboard layout representations, key definitions, and language builders.

use crate::keys::Key as NavKey;
use crate::platform;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

pub const UNIT: f32 = 52.0;
pub const KEY_H: f32 = 47.0;
pub const GAP: f32 = 5.0;
pub const ROW_GAP: f32 = 6.0;
pub const PAD: f32 = 10.0;
pub const CHROME_H: f32 = 28.0;
pub const TOOLBAR_H: f32 = 36.0;
pub const ROUNDING: f32 = 7.5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Ar,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Abc,
    Num,
    Edit,
}

/// Shift is tri-state, like the Windows touch keyboard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShiftState {
    Off,
    /// Applies to the next key only.
    Once,
    /// Stays on until tapped again.
    Locked,
}

impl ShiftState {
    /// `double` = this tap followed the previous one within the double-tap window.
    pub fn tap(self, double: bool) -> Self {
        match self {
            ShiftState::Off => ShiftState::Once,
            ShiftState::Once if double => ShiftState::Locked,
            ShiftState::Once => ShiftState::Off,
            ShiftState::Locked => ShiftState::Off,
        }
    }
    pub fn on(self) -> bool {
        self != ShiftState::Off
    }
}

/// How a key face is drawn. Glyphs that no system font renders consistently
/// (Windows logo, gamepad, return arrow, backspace) are vector-drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Glyph {
    Text,
    Win,
    Pad,
    Enter,
    ShiftEnter,
    Backspace,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
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
    #[allow(dead_code)]
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
    TaskView,
    ShowDesktop,
    CloseActiveWindow,
    ClearTyped,
    AcceptPrediction(&'static str),
    FlipPosition,
    ToggleSplitMode,
}

/// Keys that auto-repeat while held.
pub fn repeatable(act: Act) -> bool {
    matches!(act, Act::Char(..) | Act::Backspace | Act::Space)
}

#[derive(Clone, Copy)]
pub struct KDef {
    pub label: &'static str,
    /// Small glyph in the upper-right corner (shifted symbol, digit, ...).
    pub alt: Option<&'static str>,
    /// Label shown while Shift is held.
    pub shifted: Option<&'static str>,
    pub act: Act,
    pub w: f32,
    pub special: bool,
    pub glyph: Glyph,
}

/// Intern a string so layouts can be rebuilt any number of times without
/// leaking a fresh allocation per key per rebuild.
pub fn intern(s: &str) -> &'static str {
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

pub fn ch_str(c: char) -> &'static str {
    let mut buf = [0u8; 4];
    intern(c.encode_utf8(&mut buf))
}

impl KDef {
    pub fn new(label: &'static str, act: Act) -> Self {
        Self { label, alt: None, shifted: None, act, w: 1.0, special: false, glyph: Glyph::Text }
    }
    pub fn wide(label: &'static str, act: Act, w: f32) -> Self {
        Self { w, ..Self::new(label, act) }
    }
    pub fn spec(label: &'static str, act: Act, w: f32) -> Self {
        Self { special: true, ..Self::wide(label, act, w) }
    }
    pub fn with_glyph(mut self, glyph: Glyph) -> Self {
        self.glyph = glyph;
        self
    }
    /// A character key with a Shift variant (letters, digits, punctuation).
    pub fn shiftable(base: char, shifted: char) -> Self {
        Self {
            shifted: Some(ch_str(shifted)),
            ..Self::new(ch_str(base), Act::Char(base, Some(shifted)))
        }
    }
    pub fn letter(lower: char, upper: char) -> Self {
        Self::shiftable(lower, upper)
    }
    pub fn digit(d: char, shift: char) -> Self {
        Self::shiftable(d, shift)
    }
    /// A plain symbol with no Shift variant.
    pub fn sym(c: char) -> Self {
        Self::new(ch_str(c), Act::Char(c, None))
    }
    /// Arabic letter: big Arabic glyph + small Latin/digit in the corner.
    pub fn ar(arabic: char, corner: char) -> Self {
        Self { alt: Some(ch_str(corner)), ..Self::new(ch_str(arabic), Act::Char(arabic, None)) }
    }
    /// Arabic letter that emits the ligature "لا".
    pub fn ar_ligature(corner: char) -> Self {
        Self { alt: Some(ch_str(corner)), ..Self::new("لا", Act::Str("لا")) }
    }
    /// A diacritic, shown over a dotted circle so it is visible on its own.
    pub fn ar_mark(mark: char) -> Self {
        Self::new(intern(&format!("\u{25CC}{mark}")), Act::Char(mark, None))
    }
}

// ============================ Layout Builders =============================

/// Row 5 is shared by both languages: language, layer, modifiers, space, hints.
pub fn bottom_row(lang: Lang, on_num: bool) -> Vec<KDef> {
    let (lang_label, space_label) = match lang {
        Lang::En => ("EN / \u{0639}", "Space"),
        Lang::Ar => ("\u{0639} / EN", "\u{0645}\u{0633}\u{0627}\u{0641}\u{0629}"), // مسافة
    };
    let layer_label = if on_num { "abc" } else { "?123" };
    let (copy_lbl, paste_lbl) = match lang {
        Lang::En => ("Copy", "Paste"),
        Lang::Ar => ("\u{0646}\u{0633}\u{062e}", "\u{0644}\u{0635}\u{0642}"), // نسخ, لصق
    };
    vec![
        KDef::spec(lang_label, Act::Lang, 1.0),
        KDef::spec(layer_label, Act::ToggleLayer, 1.0),
        KDef::spec("Ctrl", Act::Ctrl, 1.0),
        KDef::spec("", Act::WinKey, 1.0).with_glyph(Glyph::Win),
        KDef::spec("Alt", Act::Alt, 1.0),
        KDef::spec(copy_lbl, Act::Copy, 1.0),
        KDef::spec(paste_lbl, Act::Paste, 1.0),
        KDef::spec(space_label, Act::Space, 4.0),
        KDef::spec("AltGr", Act::AltGr, 1.0),
        KDef::spec("Edit", Act::EditTools, 1.0),
        KDef { shifted: Some("?"), ..KDef::spec(".", Act::Char('.', Some('?')), 1.0) },
        KDef::spec("\u{21E7}\u{23CE}", Act::ShiftEnter, 1.35).with_glyph(Glyph::ShiftEnter),
    ]
}

/// Dynamically query Windows for the active keyboard layout (HKL) via ToUnicodeEx and MapVirtualKeyExW.
/// This adapts the virtual keyboard automatically to any layout installed on the user's Windows system
/// (e.g. German QWERTZ, French AZERTY, Russian, Spanish, Turkish, etc.) without downloading any files.
pub fn windows_abc(hkl: usize) -> Vec<Vec<KDef>> {
    let get_char = |scan: u32, shift: bool| crate::win32::scancode_to_char(scan, shift, hkl);

    let r1 = {
        let mut v = vec![KDef::spec("Esc", Act::Key(NavKey::Escape), 1.35)];
        for (scan, def_base, def_shift) in [
            (0x02, '1', '!'),
            (0x03, '2', '@'),
            (0x04, '3', '#'),
            (0x05, '4', '$'),
            (0x06, '5', '%'),
            (0x07, '6', '^'),
            (0x08, '7', '&'),
            (0x09, '8', '*'),
            (0x0A, '9', '('),
            (0x0B, '0', ')'),
            (0x0C, '-', '_'),
            (0x0D, '=', '+'),
        ] {
            let base = get_char(scan, false).unwrap_or(def_base);
            let shifted = get_char(scan, true).unwrap_or(def_shift);
            v.push(KDef::digit(base, shifted));
        }
        v.push(KDef::spec("\u{232B}", Act::Backspace, 2.0).with_glyph(Glyph::Backspace));
        v
    };

    let r2 = {
        let mut v = vec![KDef::spec("Tab", Act::Tab, 1.5)];
        for (scan, def_base, def_shift) in [
            (0x10, 'q', 'Q'),
            (0x11, 'w', 'W'),
            (0x12, 'e', 'E'),
            (0x13, 'r', 'R'),
            (0x14, 't', 'T'),
            (0x15, 'y', 'Y'),
            (0x16, 'u', 'U'),
            (0x17, 'i', 'I'),
            (0x18, 'o', 'O'),
            (0x19, 'p', 'P'),
            (0x1A, '[', '{'),
            (0x1B, ']', '}'),
            (0x2B, '\\', '|'),
        ] {
            let base = get_char(scan, false).unwrap_or(def_base);
            let shifted = get_char(scan, true).unwrap_or(def_shift);
            v.push(KDef::shiftable(base, shifted));
        }
        v
    };

    let r3 = {
        let mut v = vec![KDef::spec("Caps", Act::Caps, 1.75)];
        for (scan, def_base, def_shift) in [
            (0x1E, 'a', 'A'),
            (0x1F, 's', 'S'),
            (0x20, 'd', 'D'),
            (0x21, 'f', 'F'),
            (0x22, 'g', 'G'),
            (0x23, 'h', 'H'),
            (0x24, 'j', 'J'),
            (0x25, 'k', 'K'),
            (0x26, 'l', 'L'),
            (0x27, ';', ':'),
            (0x28, '\'', '"'),
        ] {
            let base = get_char(scan, false).unwrap_or(def_base);
            let shifted = get_char(scan, true).unwrap_or(def_shift);
            v.push(KDef::shiftable(base, shifted));
        }
        v.push(KDef::spec("\u{23CE}", Act::Enter, 2.2).with_glyph(Glyph::Enter));
        v
    };

    let r4 = {
        let mut v = vec![KDef::spec("Shift", Act::Shift, 2.25)];
        for (scan, def_base, def_shift) in [
            (0x2C, 'z', 'Z'),
            (0x2D, 'x', 'X'),
            (0x2E, 'c', 'C'),
            (0x2F, 'v', 'V'),
            (0x30, 'b', 'B'),
            (0x31, 'n', 'N'),
            (0x32, 'm', 'M'),
            (0x33, ',', '<'),
            (0x34, '.', '>'),
            (0x35, '/', '?'),
        ] {
            let base = get_char(scan, false).unwrap_or(def_base);
            let shifted = get_char(scan, true).unwrap_or(def_shift);
            v.push(KDef::shiftable(base, shifted));
        }
        v.push(KDef::spec("Shift", Act::Shift, 2.75));
        v
    };

    vec![r1, r2, r3, r4, bottom_row(Lang::En, false)]
}

pub fn en_abc() -> Vec<Vec<KDef>> {
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
pub fn num_layer(lang: Lang) -> Vec<Vec<KDef>> {
    let r1: Vec<KDef> = {
        let mut v: Vec<KDef> = "1234567890".chars().map(KDef::sym).collect();
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

/// Arabic layout (standard Arabic 101 letters, Windows-touch style)
pub fn ar_abc() -> Vec<Vec<KDef>> {
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
        v.push(KDef::spec("\u{232B}", Act::Backspace, 2.0).with_glyph(Glyph::Backspace));
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

pub fn editing_layer(lang: Lang) -> Vec<Vec<KDef>> {
    let ar = lang == Lang::Ar;
    let key = |en, arabic, act| KDef::spec(if ar { arabic } else { en }, act, 3.5);
    vec![
        vec![key("Copy", "نسخ", Act::Copy), key("Paste", "لصق", Act::Paste), key("Cut", "قص", Act::Cut), key("Select all", "تحديد الكل", Act::SelectAll)],
        vec![key("Undo", "تراجع", Act::Undo), key("Redo", "إعادة", Act::Redo), key("Delete word", "حذف كلمة", Act::DeleteWord), key("Delete", "حذف", Act::Key(NavKey::Delete))],
        vec![key("Home", "بداية السطر", Act::Key(NavKey::Home)), key("Left", "يسار", Act::Key(NavKey::Left)), key("Right", "يمين", Act::Key(NavKey::Right)), key("End", "نهاية السطر", Act::Key(NavKey::End))],
        vec![key("Shift", "Shift", Act::Shift), key("Up", "أعلى", Act::Key(NavKey::Up)), key("Down", "أسفل", Act::Key(NavKey::Down)), key("Backspace", "مسح للخلف", Act::Backspace)],
        vec![key("ABC", "أبجد", Act::EditTools), key("Space", "مسافة", Act::Space), key("Help", "تعليمات", Act::Hints).with_glyph(Glyph::Pad), key("Enter", "إدخال", Act::Enter)],
    ]
}

/// Brings every row to exactly `target_units` so the right edge is straight.
pub fn align_rows(rows: &mut [Vec<KDef>], target_units: f32) {
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

pub fn build_layout(lang: Lang, layer: Layer) -> Vec<Vec<KDef>> {
    let mut rows = match (lang, layer) {
        (Lang::En, Layer::Abc) => {
            let hkl = platform::active_input_hkl();
            if hkl != 0 && (hkl & 0x03FF) != 0x01 {
                windows_abc(hkl)
            } else {
                en_abc()
            }
        }
        (Lang::Ar, Layer::Abc) => ar_abc(),
        (lang, Layer::Num) => num_layer(lang),
        (lang, Layer::Edit) => editing_layer(lang),
    };
    let target = layout_units(&rows);
    align_rows(&mut rows, target);
    rows
}

/// Only the trailing word belongs to the prediction context.
pub fn trailing_word(text: &str) -> String {
    text.chars().rev().take_while(|c| c.is_alphabetic() || *c == '\'' || ('\u{064B}'..='\u{065F}').contains(c))
        .collect::<String>().chars().rev().collect()
}

pub fn layout_units(rows: &[Vec<KDef>]) -> f32 {
    rows.iter().map(|r| r.iter().map(|k| k.w).sum::<f32>()).fold(0.0, f32::max)
}

pub fn key_px_w(w: f32) -> f32 {
    w * UNIT + (w - 1.0).max(0.0) * GAP
}

pub fn row_px(row: &[KDef]) -> f32 {
    let n = row.len() as f32;
    row.iter().map(|k| key_px_w(k.w)).sum::<f32>() + (n - 1.0).max(0.0) * GAP
}

pub fn layout_px(rows: &[Vec<KDef>]) -> (f32, f32) {
    let w = rows.iter().map(|r| row_px(r)).fold(0.0, f32::max);
    let n = rows.len() as f32;
    let h = CHROME_H + TOOLBAR_H + 6.0 + n * KEY_H + (n - 1.0).max(0.0) * ROW_GAP + 6.0;
    (w, h)
}

#[allow(dead_code)]
pub fn widths(rows: &[Vec<KDef>]) -> Vec<f32> {
    rows.iter().map(|r| r.iter().map(|k| k.w).sum::<f32>()).collect()
}

pub fn resolve_char(base: char, shifted: Option<char>, shift: bool, caps: bool) -> char {
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

pub fn shown_label(kd: &KDef, shift: bool, caps: bool) -> &'static str {
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

pub fn shown_alt(kd: &KDef, _shift: bool) -> Option<&'static str> {
    kd.alt
}
