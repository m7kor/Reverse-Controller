//! Native settings window built with egui/eframe.
//!
//! Lets you edit every tunable (mouse speed/acceleration/wheel, stick dead
//! zones, trigger threshold, controller slot), toggle enable, view controller
//! status/battery, and view the full binding table. Save applies live to the
//! running engine and persists to disk.

#![cfg(feature = "gui")]

use crate::actions::{Action, DisplayMode, Modifier};
use crate::bindings::{Input, PadBindingDef};
use crate::config::Config;
use crate::engine::Engine;
use crate::keys::Key;
use crate::mouse::MouseButton;
use eframe::egui;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Helper to reshape Arabic text so egui renders cursive Arabic correctly from right to left.
fn ar(text: &str) -> String {
    crate::arabic::reshape_ar(text)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum GuiLang {
    #[default]
    En,
    Ar,
}

/// Localized string helper: returns `en` when in English mode, or reshaped `ar_str` when in Arabic mode.
fn t(en: &str, ar_str: &str, lang: GuiLang) -> String {
    match lang {
        GuiLang::En => en.to_string(),
        GuiLang::Ar => ar(ar_str),
    }
}

/// Keep settings changed by the background process while this window was open.
/// A whole binding array is one edit; nested settings objects merge by field.
fn merge_edits(
    baseline: &serde_json::Value,
    edited: &serde_json::Value,
    current: &serde_json::Value,
) -> serde_json::Value {
    if edited == baseline {
        return current.clone();
    }
    match (baseline, edited, current) {
        (
            serde_json::Value::Object(before),
            serde_json::Value::Object(changes),
            serde_json::Value::Object(latest),
        ) => {
            let mut result = latest.clone();
            for (key, value) in changes {
                let old = before.get(key).unwrap_or(&serde_json::Value::Null);
                let now = latest.get(key).unwrap_or(&serde_json::Value::Null);
                result.insert(key.clone(), merge_edits(old, value, now));
            }
            serde_json::Value::Object(result)
        }
        _ => edited.clone(),
    }
}

fn stick_preview(ui: &mut egui::Ui, title: &str, x: i16, y: i16, dead_zone: u16, lang: GuiLang) {
    ui.vertical(|ui| {
        ui.set_min_width(170.0);
        ui.label(egui::RichText::new(title).strong());
        let size = egui::vec2(132.0, 132.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let center = rect.center();
        let radius = 58.0;
        let painter = ui.painter_at(rect);
        painter.circle_filled(center, radius, egui::Color32::from_rgb(24, 31, 42));
        painter.circle_stroke(center, radius, egui::Stroke::new(1.5, egui::Color32::from_rgb(95, 111, 130)));
        let threshold = radius * (dead_zone as f32 / 32767.0);
        if threshold > 1.0 {
            painter.circle_stroke(center, threshold, egui::Stroke::new(1.5, egui::Color32::from_rgb(225, 169, 78)));
        }
        painter.line_segment(
            [egui::pos2(center.x - radius, center.y), egui::pos2(center.x + radius, center.y)],
            egui::Stroke::new(1.0, egui::Color32::from_rgb(55, 68, 82)),
        );
        painter.line_segment(
            [egui::pos2(center.x, center.y - radius), egui::pos2(center.x, center.y + radius)],
            egui::Stroke::new(1.0, egui::Color32::from_rgb(55, 68, 82)),
        );
        let nx = (x as f32 / 32767.0).clamp(-1.0, 1.0);
        let ny = (y as f32 / 32767.0).clamp(-1.0, 1.0);
        let dot = center + egui::vec2(nx * radius, -ny * radius);
        let in_dead_zone = ((x as f32).hypot(y as f32)) < dead_zone as f32;
        let color = if in_dead_zone {
            egui::Color32::from_rgb(225, 169, 78)
        } else {
            egui::Color32::from_rgb(91, 217, 167)
        };
        painter.circle_filled(dot, 6.0, color);
        ui.label(if in_dead_zone {
            t("Inside dead zone", "داخل منطقة الخمول", lang)
        } else {
            t("Active input", "إدخال نشط", lang)
        });
    });
}

fn format_input(input: Input, lang: GuiLang) -> String {
    match lang {
        GuiLang::En => match input {
            Input::A => "A".into(),
            Input::B => "B".into(),
            Input::X => "X".into(),
            Input::Y => "Y".into(),
            Input::DPadUp => "D-Pad Up".into(),
            Input::DPadDown => "D-Pad Down".into(),
            Input::DPadLeft => "D-Pad Left".into(),
            Input::DPadRight => "D-Pad Right".into(),
            Input::LeftShoulder => "LB (Left Bumper)".into(),
            Input::RightShoulder => "RB (Right Bumper)".into(),
            Input::LeftTrigger => "LT (Left Trigger)".into(),
            Input::RightTrigger => "RT (Right Trigger)".into(),
            Input::LeftThumb => "L3 (Left Stick Click)".into(),
            Input::RightThumb => "R3 (Right Stick Click)".into(),
            Input::LeftStick => "Left Stick".into(),
            Input::RightStick => "Right Stick".into(),
            Input::Start => "Start".into(),
            Input::Back => "Back".into(),
            Input::Guide => "Guide (Xbox)".into(),
        },
        GuiLang::Ar => match input {
            Input::A => "A".into(),
            Input::B => "B".into(),
            Input::X => "X".into(),
            Input::Y => "Y".into(),
            Input::DPadUp => ar("سهم لأعلى (D-Pad)"),
            Input::DPadDown => ar("سهم لأسفل (D-Pad)"),
            Input::DPadLeft => ar("سهم يسار (D-Pad)"),
            Input::DPadRight => ar("سهم يمين (D-Pad)"),
            Input::LeftShoulder => ar("زر الكتف الأيسر (LB)"),
            Input::RightShoulder => ar("زر الكتف الأيمن (RB)"),
            Input::LeftTrigger => ar("الزناد الأيسر (LT)"),
            Input::RightTrigger => ar("الزناد الأيمن (RT)"),
            Input::LeftThumb => ar("ضغط العصا اليسرى (L3)"),
            Input::RightThumb => ar("ضغط العصا اليمنى (R3)"),
            Input::LeftStick => ar("عصا التحكم اليسرى"),
            Input::RightStick => ar("عصا التحكم اليمنى"),
            Input::Start => ar("زر Start"),
            Input::Back => ar("زر Back (الوظائف)"),
            Input::Guide => ar("زر Guide (Xbox)"),
        },
    }
}

/// Groups shown as headings in the action picker.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum ActionGroup {
    Mouse,
    Typing,
    Browser,
    Windows,
    System,
    Media,
    App,
}

impl ActionGroup {
    const ALL: [ActionGroup; 7] = [
        ActionGroup::Mouse,
        ActionGroup::Typing,
        ActionGroup::Browser,
        ActionGroup::Windows,
        ActionGroup::System,
        ActionGroup::Media,
        ActionGroup::App,
    ];

    fn title(self, lang: GuiLang) -> String {
        let (en, ar_str) = match self {
            ActionGroup::Mouse => ("Mouse & pointer", "الماوس والمؤشر"),
            ActionGroup::Typing => ("Typing & editing", "الكتابة والتحرير"),
            ActionGroup::Browser => ("Browser", "المتصفح"),
            ActionGroup::Windows => ("Windows & desktops", "النوافذ وأسطح المكتب"),
            ActionGroup::System => ("System", "النظام"),
            ActionGroup::Media => ("Media & volume", "الوسائط والصوت"),
            ActionGroup::App => ("Reverse Controller", "Reverse Controller"),
        };
        t(en, ar_str, lang)
    }
}

/// One entry of the action library: group, English and Arabic label, action.
type CatalogEntry = (ActionGroup, &'static str, &'static str, Action);

/// Every action a button can be given in Settings — the shortcuts that make a
/// controller a complete replacement for the mouse and keyboard.
fn action_catalog() -> Vec<CatalogEntry> {
    use ActionGroup as G;
    use Key as K;
    let key = |k: Key| Action::Key { key: k, repeat: false };
    let held = |k: Key| Action::Key { key: k, repeat: true };
    let combo = |keys: &[Key]| Action::Combo(keys.to_vec());
    vec![
        (G::Mouse, "Left Click", "النقر الأيسر بالماوس", Action::MouseClick(MouseButton::Left)),
        (G::Mouse, "Right Click", "النقر الأيمن بالماوس", Action::MouseClick(MouseButton::Right)),
        (G::Mouse, "Middle Click", "النقر الأوسط بالماوس", Action::MouseClick(MouseButton::Middle)),
        (G::Mouse, "Double Click", "نقر مزدوج", Action::DoubleClick),
        (G::Mouse, "Mouse Back (X1)", "زر الماوس الجانبي: رجوع", Action::MouseClick(MouseButton::X1)),
        (G::Mouse, "Mouse Forward (X2)", "زر الماوس الجانبي: تقدم", Action::MouseClick(MouseButton::X2)),
        (G::Mouse, "Toggle Drag-Lock (hold left button)", "قفل السحب (تثبيت النقر الأيسر)", Action::ToggleDragLock),
        (G::Mouse, "Cursor to next monitor", "نقل المؤشر للشاشة التالية", Action::CursorNextMonitor),
        (G::Mouse, "Center cursor on window", "توسيط المؤشر في النافذة", Action::CenterCursor),
        (G::Typing, "Toggle On-screen Keyboard", "فتح/إغلاق الكيبورد الافتراضي", Action::ToggleVirtualKeyboard),
        (G::Typing, "Enter", "إدخال (Enter)", key(K::Enter)),
        (G::Typing, "Escape", "إلغاء / خروج (Esc)", key(K::Escape)),
        (G::Typing, "Backspace", "مسح للخلف (Backspace)", held(K::Backspace)),
        (G::Typing, "Delete", "حذف (Delete)", held(K::Delete)),
        (G::Typing, "Tab", "Tab", held(K::Tab)),
        (G::Typing, "Space", "مسافة", held(K::Space)),
        (G::Typing, "Arrow Left", "سهم يسار", held(K::Left)),
        (G::Typing, "Arrow Right", "سهم يمين", held(K::Right)),
        (G::Typing, "Arrow Up", "سهم لأعلى", held(K::Up)),
        (G::Typing, "Arrow Down", "سهم لأسفل", held(K::Down)),
        (G::Typing, "Ctrl Modifier", "زر التحكم (Ctrl)", key(K::LControl)),
        (G::Typing, "Shift Modifier", "زر التبديل (Shift)", key(K::LShift)),
        (G::Typing, "Alt Modifier", "زر Alt", key(K::LMenu)),
        (G::Typing, "Copy (Ctrl+C)", "نسخ (Ctrl+C)", combo(&[K::LControl, K::C])),
        (G::Typing, "Paste (Ctrl+V)", "لصق (Ctrl+V)", combo(&[K::LControl, K::V])),
        (G::Typing, "Cut (Ctrl+X)", "قص (Ctrl+X)", combo(&[K::LControl, K::X])),
        (G::Typing, "Undo (Ctrl+Z)", "تراجع (Ctrl+Z)", combo(&[K::LControl, K::Z])),
        (G::Typing, "Redo (Ctrl+Y)", "إعادة (Ctrl+Y)", combo(&[K::LControl, K::Y])),
        (G::Typing, "Select All (Ctrl+A)", "تحديد الكل (Ctrl+A)", combo(&[K::LControl, K::A])),
        (G::Typing, "Save (Ctrl+S)", "حفظ (Ctrl+S)", combo(&[K::LControl, K::S])),
        (G::Typing, "Find (Ctrl+F)", "بحث في الصفحة (Ctrl+F)", combo(&[K::LControl, K::F])),
        (G::Typing, "Voice Typing (Win+H)", "الكتابة الصوتية (Win+H)", Action::VoiceDictation),
        (G::Typing, "Emoji Panel (Win+.)", "لوحة الإيموجي (Win+.)", combo(&[K::LWin, K::OemPeriod])),
        (G::Typing, "Clipboard History (Win+V)", "سجل الحافظة (Win+V)", combo(&[K::LWin, K::V])),
        (G::Browser, "Back (Alt+Left)", "رجوع (Alt+Left)", combo(&[K::LMenu, K::Left])),
        (G::Browser, "Forward (Alt+Right)", "تقدم (Alt+Right)", combo(&[K::LMenu, K::Right])),
        (G::Browser, "New Tab (Ctrl+T)", "تبويب جديد (Ctrl+T)", combo(&[K::LControl, K::T])),
        (G::Browser, "Close Tab (Ctrl+W)", "إغلاق التبويب (Ctrl+W)", combo(&[K::LControl, K::W])),
        (G::Browser, "Reopen Closed Tab (Ctrl+Shift+T)", "استعادة تبويب مغلق (Ctrl+Shift+T)", combo(&[K::LControl, K::LShift, K::T])),
        (G::Browser, "Next Tab (Ctrl+Tab)", "التبويب التالي (Ctrl+Tab)", combo(&[K::LControl, K::Tab])),
        (G::Browser, "Previous Tab (Ctrl+Shift+Tab)", "التبويب السابق (Ctrl+Shift+Tab)", combo(&[K::LControl, K::LShift, K::Tab])),
        (G::Browser, "Refresh (F5)", "تحديث الصفحة (F5)", key(K::F5)),
        (G::Browser, "Fullscreen (F11)", "ملء الشاشة (F11)", key(K::F11)),
        (G::Browser, "Zoom In (Ctrl+=)", "تكبير (Ctrl+=)", combo(&[K::LControl, K::OemPlus])),
        (G::Browser, "Zoom Out (Ctrl+-)", "تصغير (Ctrl+-)", combo(&[K::LControl, K::OemMinus])),
        (G::Browser, "Reset Zoom (Ctrl+0)", "حجم افتراضي (Ctrl+0)", combo(&[K::LControl, K::D0])),
        (G::Windows, "Task View (Win+Tab)", "عرض المهام (Win+Tab)", Action::TaskView),
        (G::Windows, "Window Switcher (Ctrl+Alt+Tab)", "مبدل النوافذ (Ctrl+Alt+Tab)", combo(&[K::LControl, K::LMenu, K::Tab])),
        (G::Windows, "Show Desktop (Win+D)", "عرض سطح المكتب (Win+D)", Action::ShowDesktop),
        (G::Windows, "Minimize All (Win+M)", "تصغير الكل (Win+M)", combo(&[K::LWin, K::M])),
        (G::Windows, "Close Window (Alt+F4)", "إغلاق النافذة الحالية (Alt+F4)", Action::CloseActiveWindow),
        (G::Windows, "Snap Window Left (Win+Left)", "محاذاة النافذة لليسار (Win+Left)", Action::SnapWindowLeft),
        (G::Windows, "Snap Window Right (Win+Right)", "محاذاة النافذة لليمين (Win+Right)", Action::SnapWindowRight),
        (G::Windows, "Maximize Window (Win+Up)", "تكبير النافذة (Win+Up)", Action::MaximizeWindow),
        (G::Windows, "Minimize Window (Win+Down)", "تصغير النافذة (Win+Down)", Action::MinimizeWindow),
        (G::Windows, "Next Desktop (Win+Ctrl+Right)", "سطح المكتب التالي (Win+Ctrl+Right)", Action::NextVirtualDesktop),
        (G::Windows, "Previous Desktop (Win+Ctrl+Left)", "سطح المكتب السابق (Win+Ctrl+Left)", Action::PrevVirtualDesktop),
        (G::System, "Windows Start Menu", "قائمة ابدأ (Windows)", key(K::LWin)),
        (G::System, "Search (Win+S)", "بحث ويندوز (Win+S)", combo(&[K::LWin, K::S])),
        (G::System, "Run (Win+R)", "تشغيل (Win+R)", combo(&[K::LWin, K::R])),
        (G::System, "File Explorer (Win+E)", "مستكشف الملفات (Win+E)", combo(&[K::LWin, K::E])),
        (G::System, "Settings (Win+I)", "إعدادات ويندوز (Win+I)", combo(&[K::LWin, K::I])),
        (G::System, "Quick Settings (Win+A)", "الإعدادات السريعة (Win+A)", combo(&[K::LWin, K::A])),
        (G::System, "Notifications (Win+N)", "الإشعارات (Win+N)", combo(&[K::LWin, K::N])),
        (G::System, "Screenshot Snip (Win+Shift+S)", "لقطة شاشة (Win+Shift+S)", combo(&[K::LWin, K::LShift, K::S])),
        (G::System, "Magnifier Zoom In (Win+=)", "المكبر: تكبير (Win+=)", combo(&[K::LWin, K::OemPlus])),
        (G::System, "Magnifier Zoom Out (Win+-)", "المكبر: تصغير (Win+-)", combo(&[K::LWin, K::OemMinus])),
        (G::System, "Task Manager (Ctrl+Shift+Esc)", "مدير المهام (Ctrl+Shift+Esc)", combo(&[K::LControl, K::LShift, K::Escape])),
        (G::System, "Lock PC (Win+L)", "قفل الجهاز (Win+L)", combo(&[K::LWin, K::L])),
        (G::System, "Display: External Only", "الشاشة الخارجية فقط", Action::DisplaySwitch(DisplayMode::External)),
        (G::System, "Display: PC Screen Only", "شاشة الكمبيوتر فقط", Action::DisplaySwitch(DisplayMode::Internal)),
        (G::System, "Display: Extend", "توسيع الشاشات", Action::DisplaySwitch(DisplayMode::Extend)),
        (G::System, "Display: Duplicate", "نسخ الشاشات", Action::DisplaySwitch(DisplayMode::Clone)),
        (G::Media, "Volume Up", "رفع الصوت", key(K::VolumeUp)),
        (G::Media, "Volume Down", "خفض الصوت", key(K::VolumeDown)),
        (G::Media, "Mute", "كتم الصوت", key(K::VolumeMute)),
        (G::Media, "Media Play/Pause", "تشغيل/إيقاف الوسائط", key(K::MediaPlayPause)),
        (G::Media, "Next Track", "المقطع التالي", key(K::MediaNextTrack)),
        (G::Media, "Previous Track", "المقطع السابق", key(K::MediaPreviousTrack)),
        (G::Media, "Stop Media", "إيقاف الوسائط", key(K::MediaStop)),
        (G::App, "Radial Menu", "القائمة الدائرية السريعة", Action::RadialMenu),
        (G::App, "Toggle Desktop Control", "تشغيل/إيقاف التحكم", Action::ToggleEnabled),
        (G::App, "Next Profile", "البروفايل التالي", Action::NextProfile),
        (G::App, "Previous Profile", "البروفايل السابق", Action::PrevProfile),
        (G::App, "Turn Off Controller", "إيقاف تشغيل يد التحكم", Action::TurnOffController),
        (G::App, "Disabled", "معطل", Action::DoNothing),
    ]
}

/// Do two actions mean the same shortcut? Auto-repeat is a detail of a key
/// binding, not a different action, so it does not change the label.
fn same_shortcut(a: &Action, b: &Action) -> bool {
    match (a, b) {
        (Action::Key { key: x, .. }, Action::Key { key: y, .. }) => x == y,
        _ => a == b,
    }
}

/// Short, readable key names for labels such as "Ctrl+Shift+T".
fn key_name(k: Key) -> String {
    match k {
        Key::LControl | Key::RControl => "Ctrl".into(),
        Key::LShift | Key::RShift => "Shift".into(),
        Key::LMenu | Key::RMenu => "Alt".into(),
        Key::LWin | Key::RWin => "Win".into(),
        Key::OemPlus => "=".into(),
        Key::OemMinus => "-".into(),
        Key::OemPeriod => ".".into(),
        Key::OemComma => ",".into(),
        other => {
            let name = format!("{other:?}");
            match name.strip_prefix('D') {
                Some(digit) if digit.len() == 1 && digit.chars().all(|c| c.is_ascii_digit()) => digit.to_string(),
                _ => name,
            }
        }
    }
}

fn format_action(action: &Action, lang: GuiLang) -> String {
    if let Some((_, en, ar_str, _)) = action_catalog().into_iter().find(|(_, _, _, a)| same_shortcut(a, action)) {
        return t(en, ar_str, lang);
    }
    match action {
        Action::MouseMove { inverted: false } => t("Mouse Pointer", "تحريك مؤشر الفأرة", lang),
        Action::MouseMove { inverted: true } => t("Mouse Pointer (Inverted Y)", "تحريك المؤشر (محور Y معكوس)", lang),
        Action::MouseScroll { flip_y: false } => t("Scroll Wheel", "التمرير بالبكرة", lang),
        Action::MouseScroll { flip_y: true } => t("Scroll Wheel (Inverted Y)", "التمرير معكوس عمودياً", lang),
        Action::WindowDrag => t("Window Drag & Move", "تحريك وسحب النوافذ", lang),
        Action::Modifier(Modifier::Back) => t("Hold Modifier (Back)", "زر الوظائف الإضافية (Back)", lang),
        Action::Key { key, .. } => format!("{}: {}", t("Key", "زر", lang), key_name(*key)),
        Action::Combo(keys) => keys.iter().map(|k| key_name(*k)).collect::<Vec<_>>().join("+"),
        Action::ActivateProfile(n) => format!("{}: {n}", t("Activate Profile", "تفعيل بروفايل", lang)),
        Action::Excluded(s) => format!("Excluded ({s})"),
        other => format!("{other:?}"),
    }
}

/// What a stick can do. Swapping these is how a left-handed user moves the
/// pointer to the right stick.
fn stick_actions(lang: GuiLang) -> Vec<(String, Action)> {
    vec![
        (t("Mouse Pointer", "تحريك مؤشر الفأرة", lang), Action::MouseMove { inverted: false }),
        (t("Scroll Wheel", "التمرير بالبكرة", lang), Action::MouseScroll { flip_y: false }),
        (t("Window Drag & Move", "تحريك وسحب النوافذ", lang), Action::WindowDrag),
        (t("Disabled", "معطل", lang), Action::DoNothing),
    ]
}

fn action_picker(ui: &mut egui::Ui, id: impl std::hash::Hash, current: &mut Action, lang: GuiLang) {
    let current_label = format_action(current, lang);
    let catalog = action_catalog();
    egui::ComboBox::from_id_source(id)
        .selected_text(current_label)
        .width(300.0)
        .height(420.0)
        .show_ui(ui, |ui| {
            for group in ActionGroup::ALL {
                ui.label(egui::RichText::new(group.title(lang)).strong().color(egui::Color32::from_rgb(255, 180, 80)));
                for (_, en, ar_str, act) in catalog.iter().filter(|(g, ..)| *g == group) {
                    let is_sel = same_shortcut(current, act);
                    if ui.selectable_label(is_sel, t(en, ar_str, lang)).clicked() {
                        *current = act.clone();
                    }
                }
                ui.separator();
            }
        });
}

fn stick_picker(ui: &mut egui::Ui, id: impl std::hash::Hash, current: &mut Action, lang: GuiLang) {
    egui::ComboBox::from_id_source(id)
        .selected_text(format_action(current, lang))
        .width(300.0)
        .show_ui(ui, |ui| {
            for (label, act) in stick_actions(lang) {
                let is_sel = std::mem::discriminant(current) == std::mem::discriminant(&act);
                if ui.selectable_label(is_sel, label).clicked() && !is_sel {
                    *current = act;
                }
            }
        });
}

static SETTINGS_OPEN: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Visual system: one palette shared with the on-screen keyboard.
// ---------------------------------------------------------------------------

const C_ACCENT: egui::Color32 = egui::Color32::from_rgb(255, 170, 0);
const C_ON_ACCENT: egui::Color32 = egui::Color32::from_rgb(20, 20, 22);
const C_BG: egui::Color32 = egui::Color32::from_rgb(16, 17, 21);
const C_SIDEBAR: egui::Color32 = egui::Color32::from_rgb(21, 22, 27);
const C_CARD: egui::Color32 = egui::Color32::from_rgb(28, 30, 36);
const C_CARD_BORDER: egui::Color32 = egui::Color32::from_rgb(44, 47, 56);
const C_TEXT: egui::Color32 = egui::Color32::from_rgb(236, 238, 243);
const C_DIM: egui::Color32 = egui::Color32::from_rgb(150, 156, 168);
const C_GOOD: egui::Color32 = egui::Color32::from_rgb(46, 204, 113);
const C_WARN: egui::Color32 = egui::Color32::from_rgb(243, 156, 18);
const C_BAD: egui::Color32 = egui::Color32::from_rgb(231, 76, 60);
const C_INFO: egui::Color32 = egui::Color32::from_rgb(52, 152, 219);

fn apply_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    use egui::{FontFamily, FontId, TextStyle};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(22.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(15.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(15.0, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(12.5, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(14.0, 7.0);
    style.spacing.interact_size.y = 32.0;
    style.spacing.slider_width = 230.0;
    let v = &mut style.visuals;
    *v = egui::Visuals::dark();
    v.panel_fill = C_BG;
    v.window_fill = C_CARD;
    v.extreme_bg_color = egui::Color32::from_rgb(12, 13, 16);
    v.faint_bg_color = egui::Color32::from_rgb(34, 36, 43);
    v.override_text_color = Some(C_TEXT);
    v.selection.bg_fill = egui::Color32::from_rgb(120, 82, 0);
    v.selection.stroke = egui::Stroke::new(1.0, C_ACCENT);
    v.hyperlink_color = C_ACCENT;
    v.window_rounding = egui::Rounding::same(12.0);
    for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.rounding = egui::Rounding::same(8.0);
    }
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, C_CARD_BORDER);
    v.widgets.inactive.weak_bg_fill = egui::Color32::from_rgb(40, 43, 51);
    v.widgets.inactive.bg_fill = egui::Color32::from_rgb(40, 43, 51);
    v.widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(52, 56, 66);
    v.widgets.hovered.bg_fill = egui::Color32::from_rgb(52, 56, 66);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, C_ACCENT);
    v.widgets.active.weak_bg_fill = egui::Color32::from_rgb(70, 52, 10);
    v.widgets.active.bg_fill = C_ACCENT;
    ctx.set_style(style);
}

/// Resolve the saved language setting ("auto", "en", "ar").
fn resolve_lang(setting: &str) -> GuiLang {
    if setting.eq_ignore_ascii_case("ar") {
        GuiLang::Ar
    } else if setting.eq_ignore_ascii_case("en") {
        GuiLang::En
    } else if crate::win32::user_ui_primary_language() == 0x01 {
        GuiLang::Ar
    } else {
        GuiLang::En
    }
}

/// A rounded card with a title and an optional one-line description.
fn card(ui: &mut egui::Ui, lang: GuiLang, title: &str, subtitle: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(C_CARD)
        .stroke(egui::Stroke::new(1.0, C_CARD_BORDER))
        .rounding(egui::Rounding::same(12.0))
        .inner_margin(egui::Margin::same(16.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            let align = if lang == GuiLang::Ar { egui::Align::Max } else { egui::Align::Min };
            ui.with_layout(egui::Layout::top_down(align), |ui| {
                ui.label(egui::RichText::new(title).size(17.0).strong());
                if !subtitle.is_empty() {
                    ui.label(egui::RichText::new(subtitle).color(C_DIM).small());
                }
                ui.add_space(6.0);
                add(ui);
            });
        });
    ui.add_space(10.0);
}

/// One setting: label (and help text) on the reading side, control on the other.
fn row(ui: &mut egui::Ui, lang: GuiLang, label: &str, help: &str, control: impl FnOnce(&mut egui::Ui)) {
    let rtl = lang == GuiLang::Ar;
    let (outer, inner, text_align) = if rtl {
        (egui::Layout::right_to_left(egui::Align::Center), egui::Layout::left_to_right(egui::Align::Center), egui::Align::Max)
    } else {
        (egui::Layout::left_to_right(egui::Align::Center), egui::Layout::right_to_left(egui::Align::Center), egui::Align::Min)
    };
    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 0.0), outer, |ui| {
        ui.with_layout(egui::Layout::top_down(text_align), |ui| {
            ui.set_max_width((ui.available_width() * 0.55).max(180.0));
            ui.label(label);
            if !help.is_empty() {
                ui.label(egui::RichText::new(help).color(C_DIM).small());
            }
        });
        ui.with_layout(inner, control);
    });
    ui.add_space(2.0);
}

/// iOS-style switch; returns the response so callers can react to changes.
fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let size = egui::vec2(44.0, 24.0);
    let (rect, mut response) = ui.allocate_exact_size(size, egui::Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui.ctx().animate_bool(response.id, *on);
    let track = if *on { C_ACCENT } else { egui::Color32::from_rgb(64, 68, 80) };
    let p = ui.painter();
    p.rect_filled(rect, rect.height() / 2.0, track);
    let x = egui::lerp(rect.left() + 12.0..=rect.right() - 12.0, t);
    p.circle_filled(egui::pos2(x, rect.center().y), 9.0, egui::Color32::WHITE);
    if response.hovered() {
        p.rect_stroke(rect, rect.height() / 2.0, egui::Stroke::new(1.0, egui::Color32::from_white_alpha(60)));
    }
    response
}

/// Small coloured badge.
fn pill(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    egui::Frame::none()
        .fill(color.gamma_multiply(0.22))
        .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.7)))
        .rounding(egui::Rounding::same(20.0))
        .inner_margin(egui::Margin::symmetric(10.0, 3.0))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).color(color).small().strong());
        });
}

fn accent_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.into()).color(C_ON_ACCENT).strong()).fill(C_ACCENT)
}

fn battery_text(level: u8, lang: GuiLang) -> String {
    match level {
        0 => t("Empty", "فارغة", lang),
        1 => t("Low", "منخفضة", lang),
        2 => t("Medium", "متوسطة", lang),
        _ => t("Full", "ممتلئة", lang),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum SettingsPage {
    Home,
    Tutorial,
    Pointer,
    Controller,
    Keyboard,
    Buttons,
    Profiles,
    General,
    About,
}

impl SettingsPage {
    const ALL: [SettingsPage; 9] = [
        SettingsPage::Home,
        SettingsPage::Tutorial,
        SettingsPage::Pointer,
        SettingsPage::Controller,
        SettingsPage::Keyboard,
        SettingsPage::Buttons,
        SettingsPage::Profiles,
        SettingsPage::General,
        SettingsPage::About,
    ];

    fn icon(self) -> &'static str {
        match self {
            SettingsPage::Home => "🏠",
            SettingsPage::Tutorial => "🎓",
            SettingsPage::Pointer => "🖱",
            SettingsPage::Controller => "🎮",
            SettingsPage::Keyboard => "⌨",
            SettingsPage::Buttons => "🔘",
            SettingsPage::Profiles => "🗂",
            SettingsPage::General => "⚙",
            SettingsPage::About => "ℹ",
        }
    }

    fn title(self, lang: GuiLang) -> String {
        let (en, ar_str) = match self {
            SettingsPage::Home => ("Home", "الرئيسية"),
            SettingsPage::Pointer => ("Pointer", "المؤشر"),
            SettingsPage::Controller => ("Controller", "يد التحكم"),
            SettingsPage::Keyboard => ("Keyboard", "الكيبورد"),
            SettingsPage::Buttons => ("Buttons", "الأزرار"),
            SettingsPage::Profiles => ("Profiles", "البروفايلات"),
            SettingsPage::General => ("General", "عام"),
            SettingsPage::About => ("About", "حول البرنامج"),
            SettingsPage::Tutorial => ("Getting started", "البداية السريعة"),
        };
        t(en, ar_str, lang)
    }

    fn subtitle(self, lang: GuiLang) -> String {
        let (en, ar_str) = match self {
            SettingsPage::Home => ("Status and your controller at a glance", "الحالة ويد التحكم بنظرة سريعة"),
            SettingsPage::Pointer => ("Speed, precision and scrolling", "السرعة والدقة والتمرير"),
            SettingsPage::Controller => ("Live input, calibration, dead zones and vibration", "الإدخال المباشر والمعايرة ومناطق الخمول والاهتزاز"),
            SettingsPage::Keyboard => ("On-screen keyboard placement and typing", "مكان الكيبورد على الشاشة والكتابة"),
            SettingsPage::Buttons => ("What every button and stick does", "وظيفة كل زر وعصا"),
            SettingsPage::Profiles => ("Button sets per app, switched automatically", "مجموعات أزرار لكل برنامج، تتبدل تلقائياً"),
            SettingsPage::General => ("Language, startup, games and troubleshooting", "اللغة والتشغيل والألعاب وحل المشكلات"),
            SettingsPage::About => ("Version and privacy", "الإصدار والخصوصية"),
            SettingsPage::Tutorial => ("Learn the basics in a minute, with your controller", "تعلّم الأساسيات في دقيقة بيد التحكم"),
        };
        t(en, ar_str, lang)
    }
}

/// What the background app reports this frame (or the local view of it).
struct Live {
    connected: bool,
    enabled: bool,
    battery: Option<u8>,
    wired: bool,
    pad: Option<crate::win32::XINPUT_GAMEPAD>,
    /// The controller in use, e.g. "PlayStation 5 (DualSense)".
    name: String,
    profile: String,
    mode: Option<crate::runtime_status::RuntimeMode>,
    slot: u32,
}

/// A confirmation the user must answer before something destructive happens.
#[derive(Clone, Debug, PartialEq)]
enum Confirm {
    DeleteProfile(PathBuf),
    RestoreDefaults,
    ForgetWords,
    CloseUnsaved,
}

/// Order in which buttons are listed on the Buttons and Home pages.
const INPUT_ORDER: [Input; 17] = [
    Input::A,
    Input::B,
    Input::X,
    Input::Y,
    Input::LeftShoulder,
    Input::RightShoulder,
    Input::LeftTrigger,
    Input::RightTrigger,
    Input::LeftThumb,
    Input::RightThumb,
    Input::DPadUp,
    Input::DPadDown,
    Input::DPadLeft,
    Input::DPadRight,
    Input::Start,
    Input::Back,
    Input::Guide,
];

struct SettingsApp {
    engine: Arc<Mutex<Engine>>,
    config_path: PathBuf,
    working: Config,
    baseline: Config,
    status: String,
    status_error: bool,
    active_profile: String,
    profiles_dir: PathBuf,
    profile_list: Vec<crate::profiles::ProfileEntry>,
    new_profile_name: String,
    new_profile_processes: String,
    hotkey_text: String,
    hotkey_error: bool,
    page: SettingsPage,
    calibration: Option<(Instant, crate::calibration::RestCalibration)>,
    calibration_result: Option<(u32, [u16; 2])>,
    runtime_status: Option<crate::runtime_status::RuntimeStatus>,
    last_status_check: Instant,
    last_status_seen: Instant,
    lang: GuiLang,
    frame_count: u32,
    confirm: Option<Confirm>,
    exception_input: String,
    amgp_candidates: Vec<PathBuf>,
    learned_words: usize,
    add_back_input: Option<Input>,
    allow_close: bool,
    /// Which tutorial steps the user has completed on the controller.
    tutorial: [bool; TUTORIAL_STEPS],
    /// Running button-mapping wizard and the joystick it watches.
    wizard: Option<(u32, crate::gamepads::MapWizard)>,
    /// Non-XInput controllers found by the last "find controllers" click.
    other_pads: Vec<crate::gamepads::Device>,
}

const TUTORIAL_STEPS: usize = 6;

/// Tick off tutorial steps from one controller reading. Steps can be done in
/// any order; once done they stay done.
fn tutorial_progress(pad: &crate::win32::XINPUT_GAMEPAD, done: &mut [bool; TUTORIAL_STEPS]) {
    let tilt = |x: i16, y: i16| (x as f32).hypot(y as f32) / 32767.0;
    let left = tilt(pad.sThumbLX, pad.sThumbLY);
    let b = pad.wButtons;
    let hits = [
        left > 0.6,                                  // move the pointer
        b & 0x1000 != 0,                             // click with A
        tilt(pad.sThumbRX, pad.sThumbRY) > 0.6,      // scroll
        pad.bLeftTrigger > 128 && left > 0.3,        // precision with LT
        b & 0x0040 != 0,                             // open the keyboard with L3
        b & 0x0020 != 0 && b & 0x0080 != 0,          // quick menu: Back + R3
    ];
    for (d, hit) in done.iter_mut().zip(hits) {
        *d |= hit;
    }
}

impl SettingsApp {
    fn new(engine: Arc<Mutex<Engine>>, config_path: PathBuf) -> Self {
        let working = engine.lock().map(|e| e.config().clone()).unwrap_or_default();
        let dir = crate::config::profiles_dir();
        let list = crate::profiles::list(&dir);
        let active_profile = if working.active_profile.is_empty() {
            crate::profiles::DEFAULT_NAME.to_string()
        } else {
            working.active_profile.clone()
        };
        let lang = resolve_lang(&working.gui_language);
        let learned_words = crate::predict::learned_count(&crate::predict::memory_path_for(&config_path));
        // `CC_SETTINGS_PAGE=keyboard` opens a page directly (screenshots, docs).
        let page = std::env::var("CC_SETTINGS_PAGE")
            .ok()
            .and_then(|want| {
                SettingsPage::ALL
                    .into_iter()
                    .find(|p| format!("{p:?}").eq_ignore_ascii_case(want.trim()))
            })
            .unwrap_or(SettingsPage::Home);
        SettingsApp {
            engine,
            hotkey_text: working.toggle_hotkey.clone(),
            baseline: working.clone(),
            working,
            config_path,
            status: String::new(),
            status_error: false,
            active_profile,
            profiles_dir: dir,
            profile_list: list,
            new_profile_name: String::new(),
            new_profile_processes: String::new(),
            hotkey_error: false,
            page,
            calibration: None,
            calibration_result: None,
            runtime_status: None,
            last_status_check: Instant::now() - Duration::from_secs(1),
            last_status_seen: Instant::now() - Duration::from_secs(10),
            lang,
            frame_count: 0,
            confirm: None,
            exception_input: String::new(),
            amgp_candidates: Vec::new(),
            learned_words,
            add_back_input: None,
            allow_close: false,
            tutorial: [false; TUTORIAL_STEPS],
            wizard: None,
            other_pads: crate::gamepads::list_models(),
        }
    }

    fn tr(&self, en: &str, ar_str: &str) -> String {
        t(en, ar_str, self.lang)
    }

    fn say(&mut self, en: &str, ar_str: &str) {
        self.status = t(en, ar_str, self.lang);
        self.status_error = false;
    }

    fn fail(&mut self, en: &str, ar_str: &str, error: impl std::fmt::Display) {
        self.status = format!("{}: {error}", t(en, ar_str, self.lang));
        self.status_error = true;
    }

    fn is_dirty(&self) -> bool {
        serde_json::to_value(&self.working).ok() != serde_json::to_value(&self.baseline).ok()
    }

    fn refresh_profiles(&mut self) {
        self.profile_list = crate::profiles::list(&self.profiles_dir);
    }

    /// Apply one change straight to config.json (merged with whatever the
    /// background app wrote) without saving the rest of the pending edits.
    fn persist_now(&mut self, edit: impl Fn(&mut Config)) -> Result<(), String> {
        let mut latest = if self.config_path.exists() {
            Config::try_load(&self.config_path)?
        } else {
            Config::default()
        };
        edit(&mut latest);
        latest.save(&self.config_path).map_err(|e| e.to_string())?;
        edit(&mut self.working);
        edit(&mut self.baseline);
        if let Ok(mut engine) = self.engine.lock() {
            engine.apply_config(latest);
        }
        Ok(())
    }

    fn toggle_desktop_control(&mut self, enabled: bool) {
        match self.persist_now(|c| c.enabled = enabled) {
            Ok(()) => {
                if let Some(snapshot) = self.runtime_status.as_mut() {
                    snapshot.enabled = enabled;
                    snapshot.mode = crate::runtime_status::RuntimeMode::from_state(
                        snapshot.connected,
                        enabled,
                        snapshot.mode == crate::runtime_status::RuntimeMode::PausedFullscreen,
                    );
                }
                if enabled {
                    self.say("Desktop control turned on", "تم تشغيل التحكم المكتبي");
                } else {
                    self.say("Desktop control turned off", "تم إيقاف التحكم المكتبي");
                }
            }
            Err(error) => self.fail("Could not change desktop control", "فشل تغيير حالة التحكم المكتبي", error),
        }
    }

    fn set_language(&mut self, setting: &str) {
        let setting = setting.to_string();
        let result = self.persist_now(|c| c.gui_language = setting.clone());
        self.lang = resolve_lang(&setting);
        if let Err(error) = result {
            self.fail("Could not save the language", "تعذر حفظ اللغة", error);
        }
    }

    fn set_autostart(&mut self, enable: bool) {
        let exe = std::env::current_exe().map(|p| format!("\"{}\"", p.display())).unwrap_or_default();
        if let Err(error) = crate::platform::set_autostart(enable, &exe) {
            self.fail("Autostart error", "خطأ في التشغيل التلقائي", error);
            return;
        }
        if !enable {
            let _ = crate::platform::remove_scheduled_task();
        }
        match self.persist_now(|c| c.autostart = enable) {
            Ok(()) if enable => self.say("Starts with Windows", "سيعمل مع بدء تشغيل ويندوز"),
            Ok(()) => self.say("No longer starts with Windows", "لن يعمل تلقائياً مع ويندوز"),
            Err(error) => self.fail("Autostart error", "خطأ في التشغيل التلقائي", error),
        }
    }

    /// Persist the working config and push it into the running engine.
    fn commit(&mut self) -> bool {
        let latest = if self.config_path.exists() {
            match Config::try_load(&self.config_path) {
                Ok(config) => config,
                Err(error) => {
                    self.fail("Save stopped", "توقف الحفظ", error);
                    return false;
                }
            }
        } else {
            Config::default()
        };
        let (Ok(before), Ok(edited), Ok(current)) = (
            serde_json::to_value(&self.baseline),
            serde_json::to_value(&self.working),
            serde_json::to_value(&latest),
        ) else {
            self.say("Could not prepare settings for saving", "تعذر تحضير الإعدادات للحفظ");
            self.status_error = true;
            return false;
        };
        let mut merged: Config = match serde_json::from_value(merge_edits(&before, &edited, &current)) {
            Ok(config) => config,
            Err(error) => {
                self.fail("Could not merge settings", "تعذر دمج الإعدادات", error);
                return false;
            }
        };
        // A controller shortcut can toggle the live engine without changing
        // config.json until shutdown. Preserve that state when this window
        // saves an unrelated field.
        if self.working.enabled == self.baseline.enabled {
            if let Some(snapshot) = &self.runtime_status {
                merged.enabled = snapshot.enabled;
            }
        }
        if self.working.controller.controller_index == self.baseline.controller.controller_index {
            if let Some(snapshot) = &self.runtime_status {
                merged.controller.controller_index = snapshot.controller_index;
            }
        }
        match merged.save(&self.config_path) {
            Ok(()) => {
                if let Ok(mut e) = self.engine.lock() {
                    e.apply_config(merged.clone());
                }
                crate::osd::set_osd_enabled(merged.osd_enabled);
                crate::sound::set_sfx_enabled(merged.keyboard.mechanical_sfx);
                self.working = merged.clone();
                self.baseline = merged;
                self.hotkey_text = self.working.toggle_hotkey.clone();
                self.active_profile = if self.working.active_profile.is_empty() {
                    crate::profiles::DEFAULT_NAME.to_string()
                } else {
                    self.working.active_profile.clone()
                };
                self.say("Saved and applied", "تم الحفظ والتطبيق");
                true
            }
            Err(e) => {
                self.fail("Save failed", "فشل الحفظ", e);
                false
            }
        }
    }

    /// Switch to `stem`, loading its bindings into the working config.
    fn activate_profile(&mut self, stem: &str) {
        let (bindings, clear_name) = if stem == crate::profiles::DEFAULT_NAME {
            (crate::bindings::default_profile(), true)
        } else {
            let path = self.profiles_dir.join(format!("{stem}.json"));
            match crate::profiles::load(&path) {
                Ok(p) => (p.bindings, false),
                Err(e) => {
                    self.fail(&format!("Could not load {stem}"), &format!("تعذر تحميل {stem}"), e);
                    return;
                }
            }
        };
        self.working.bindings = bindings;
        if clear_name {
            self.working.active_profile.clear();
        } else {
            self.working.active_profile = stem.to_string();
        }
        self.active_profile = stem.to_string();
        if self.commit() {
            let (en, ar_str) = (format!("Profile '{stem}' activated"), format!("تم تفعيل البروفايل '{stem}'"));
            self.say(&en, &ar_str);
        }
    }

    fn live(&mut self) -> Live {
        if self.last_status_check.elapsed() >= Duration::from_millis(250) {
            let path = crate::runtime_status::path_for_config(&self.config_path);
            if let Some(snapshot) = crate::runtime_status::read_recent(&path) {
                self.runtime_status = Some(snapshot);
                self.last_status_seen = Instant::now();
            } else if self.last_status_seen.elapsed() >= Duration::from_secs(3) {
                self.runtime_status = None;
            }
            self.last_status_check = Instant::now();
        }
        let snapshot = self.runtime_status.clone();
        let slot = snapshot.as_ref().map_or(self.working.controller.controller_index, |s| s.controller_index);
        let (connected, enabled, battery, preview, profile) = self
            .engine
            .lock()
            .map(|mut e| {
                let name = e.config().active_profile.clone();
                let preview = e.preview_pad(slot);
                (
                    e.is_connected() || preview.is_some(),
                    e.is_enabled(),
                    e.battery_info(),
                    preview,
                    if name.is_empty() { crate::profiles::DEFAULT_NAME.to_string() } else { name },
                )
            })
            .unwrap_or((false, false, None, None, crate::profiles::DEFAULT_NAME.to_string()));
        let (pad, local_name) = match preview {
            Some((pad, name)) => (Some(pad), name),
            None => (None, String::new()),
        };
        Live {
            connected: snapshot.as_ref().map_or(connected, |s| s.connected),
            enabled: snapshot.as_ref().map_or(enabled, |s| s.enabled),
            battery: match snapshot.as_ref() {
                Some(s) => s.battery_level,
                None => battery.filter(|(kind, _)| matches!(kind, 0x02 | 0x03)).map(|(_, level)| level),
            },
            wired: snapshot.as_ref().map_or(battery.is_some_and(|(kind, _)| kind == 0x01), |s| s.battery_wired),
            pad,
            name: snapshot
                .as_ref()
                .map(|s| s.controller_name.clone())
                .filter(|n| !n.is_empty())
                .unwrap_or(local_name),
            profile: snapshot.as_ref().map_or(profile, |s| s.active_profile.clone()),
            mode: snapshot.as_ref().map(|s| s.mode),
            slot,
        }
    }

    fn mode_text(&self, live: &Live) -> (String, egui::Color32) {
        use crate::runtime_status::RuntimeMode as M;
        match live.mode {
            Some(M::Active) => (self.tr("Desktop control active", "التحكم المكتبي نشط"), C_GOOD),
            Some(M::PausedFullscreen) => (self.tr("Paused for a fullscreen game", "متوقف مؤقتاً للعبة بملء الشاشة"), C_INFO),
            Some(M::Disabled) => (self.tr("Desktop control off", "التحكم المكتبي متوقف"), C_BAD),
            Some(M::Disconnected) => (self.tr("Waiting for a controller", "بانتظار توصيل يد التحكم"), C_DIM),
            None => (self.tr("Background app not running", "البرنامج لا يعمل في الخلفية"), C_WARN),
        }
    }

    // ------------------------------------------------------------------ chrome

    fn sidebar(&mut self, ui: &mut egui::Ui, live: &Live) {
        // A right-hand side panel draws its content up to the window edge, so
        // the Arabic (mirrored) sidebar adds its own padding on that side.
        let pad = if self.lang == GuiLang::Ar {
            egui::Margin { left: 0.0, right: 16.0, top: 0.0, bottom: 0.0 }
        } else {
            egui::Margin::ZERO
        };
        egui::Frame::none().inner_margin(pad).show(ui, |ui| self.sidebar_contents(ui, live));
    }

    fn sidebar_contents(&mut self, ui: &mut egui::Ui, live: &Live) {
        let align = if self.lang == GuiLang::Ar { egui::Align::Max } else { egui::Align::Min };
        ui.with_layout(egui::Layout::top_down(align), |ui| {
            ui.add_space(14.0);
            ui.label(egui::RichText::new("🎮 Reverse Controller").size(18.0).strong().color(C_ACCENT));
            ui.label(egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).small().color(C_DIM));
            ui.add_space(14.0);
            for page in SettingsPage::ALL {
                let selected = self.page == page;
                let text = if self.lang == GuiLang::Ar {
                    format!("{}  {}", page.title(self.lang), page.icon())
                } else {
                    format!("{}  {}", page.icon(), page.title(self.lang))
                };
                let label = egui::RichText::new(text).size(15.5).color(if selected { C_ON_ACCENT } else { C_TEXT });
                let button = egui::Button::new(label)
                    .fill(if selected { C_ACCENT } else { egui::Color32::TRANSPARENT })
                    .stroke(egui::Stroke::NONE)
                    .min_size(egui::vec2(ui.available_width(), 36.0));
                if ui.add(button).clicked() {
                    self.page = page;
                    if page == SettingsPage::Keyboard {
                        self.learned_words = crate::predict::learned_count(&crate::predict::memory_path_for(&self.config_path));
                    }
                }
            }
            ui.add_space(18.0);
            let (text, color) = self.mode_text(live);
            pill(ui, &text, color);
            if live.connected && !live.name.is_empty() {
                ui.label(egui::RichText::new(format!("🎮 {}", live.name)).small().color(C_DIM));
            }
            if live.connected && live.wired {
                ui.label(egui::RichText::new(format!("🔌 {}", self.tr("Wired", "سلكية"))).small().color(C_DIM));
            } else if let Some(level) = live.battery.filter(|_| live.connected) {
                ui.label(egui::RichText::new(format!("🔋 {}", battery_text(level, self.lang))).small().color(C_DIM));
            }
            ui.add_space(14.0);
            ui.label(egui::RichText::new(self.tr("Language", "اللغة")).small().color(C_DIM));
            let mut chosen = None;
            ui.horizontal(|ui| {
                for (setting, label) in [("en", "English"), ("ar", "العربية")] {
                    let label = if setting == "ar" { ar(label) } else { label.to_string() };
                    let on = self.working.gui_language.eq_ignore_ascii_case(setting);
                    if ui.selectable_label(on, label).clicked() {
                        chosen = Some(setting);
                    }
                }
            });
            if let Some(setting) = chosen {
                self.set_language(setting);
            }
        });
    }

    fn header(&mut self, ui: &mut egui::Ui, live: &Live) {
        let rtl = self.lang == GuiLang::Ar;
        let outer = if rtl { egui::Layout::right_to_left(egui::Align::Center) } else { egui::Layout::left_to_right(egui::Align::Center) };
        let inner = if rtl { egui::Layout::left_to_right(egui::Align::Center) } else { egui::Layout::right_to_left(egui::Align::Center) };
        let align = if rtl { egui::Align::Max } else { egui::Align::Min };
        ui.add_space(10.0);
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 52.0), outer, |ui| {
            ui.with_layout(egui::Layout::top_down(align), |ui| {
                ui.heading(self.page.title(self.lang));
                ui.label(egui::RichText::new(self.page.subtitle(self.lang)).color(C_DIM));
            });
            ui.with_layout(inner, |ui| {
                let (label, color) = if live.enabled {
                    (self.tr("● Desktop control ON", "● التحكم المكتبي يعمل"), C_GOOD)
                } else {
                    (self.tr("○ Desktop control OFF", "○ التحكم المكتبي متوقف"), C_BAD)
                };
                let button = egui::Button::new(egui::RichText::new(label).strong().color(egui::Color32::WHITE))
                    .fill(color.gamma_multiply(0.75));
                if ui.add(button).on_hover_text(self.tr("Also: Back + Start on the controller", "أو: Back + Start على يد التحكم")).clicked() {
                    self.toggle_desktop_control(!live.enabled);
                }
            });
        });
        ui.add_space(8.0);
    }

    fn footer(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let rtl = self.lang == GuiLang::Ar;
        let outer = if rtl { egui::Layout::right_to_left(egui::Align::Center) } else { egui::Layout::left_to_right(egui::Align::Center) };
        let inner = if rtl { egui::Layout::left_to_right(egui::Align::Center) } else { egui::Layout::right_to_left(egui::Align::Center) };
        let dirty = self.is_dirty();
        ui.add_space(6.0);
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 36.0), outer, |ui| {
            if dirty {
                ui.label(egui::RichText::new(self.tr("● Unsaved changes", "● تغييرات غير محفوظة")).color(C_ACCENT).strong());
            } else if !self.status.is_empty() {
                let color = if self.status_error { C_BAD } else { C_GOOD };
                ui.label(egui::RichText::new(&self.status).color(color));
            } else {
                ui.label(egui::RichText::new(self.tr("All changes saved", "كل التغييرات محفوظة")).color(C_DIM));
            }
            ui.with_layout(inner, |ui| {
                if ui.button(self.tr("Close", "إغلاق")).clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if dirty {
                    if ui.button(self.tr("Discard", "تجاهل التغييرات")).clicked() {
                        self.working = self.baseline.clone();
                        self.hotkey_text = self.working.toggle_hotkey.clone();
                        self.lang = resolve_lang(&self.working.gui_language);
                        self.say("Changes discarded", "تم تجاهل التغييرات");
                    }
                    if ui.add(accent_button(self.tr("Save changes", "حفظ التغييرات"))).clicked() {
                        self.commit();
                    }
                }
            });
        });
        ui.add_space(6.0);
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(confirm) = self.confirm.clone() else { return };
        let (title, body) = match &confirm {
            Confirm::DeleteProfile(path) => {
                let name = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                (self.tr("Delete profile?", "حذف البروفايل؟"), self.tr(&format!("'{name}' will be removed permanently."), &format!("سيتم حذف '{name}' نهائياً.")))
            }
            Confirm::RestoreDefaults => (
                self.tr("Restore all defaults?", "استعادة كل الإعدادات الافتراضية؟"),
                self.tr("Every setting returns to its default. Save to apply.", "ستعود كل الإعدادات لقيمها الافتراضية. اضغط حفظ للتطبيق."),
            ),
            Confirm::ForgetWords => (
                self.tr("Forget learned words?", "حذف الكلمات المحفوظة؟"),
                self.tr("Suggestions start again from the built-in word list.", "ستبدأ الاقتراحات من جديد من القائمة المدمجة."),
            ),
            Confirm::CloseUnsaved => (
                self.tr("Save your changes?", "حفظ التغييرات؟"),
                self.tr("You have changes that are not saved yet.", "لديك تغييرات لم تُحفظ بعد."),
            ),
        };
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(body);
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    let primary = match confirm {
                        Confirm::CloseUnsaved => self.tr("Save", "حفظ"),
                        Confirm::DeleteProfile(_) | Confirm::ForgetWords => self.tr("Delete", "حذف"),
                        Confirm::RestoreDefaults => self.tr("Restore", "استعادة"),
                    };
                    if ui.add(accent_button(primary)).clicked() {
                        self.confirm = None;
                        match &confirm {
                            Confirm::DeleteProfile(path) => match std::fs::remove_file(path) {
                                Ok(()) => {
                                    self.refresh_profiles();
                                    self.say("Profile deleted", "تم حذف البروفايل");
                                }
                                Err(e) => self.fail("Delete failed", "فشل الحذف", e),
                            },
                            Confirm::RestoreDefaults => {
                                let language = self.working.gui_language.clone();
                                self.working = Config { gui_language: language, ..Config::default() };
                                self.hotkey_text = self.working.toggle_hotkey.clone();
                                self.say("Defaults are ready. Save to apply them.", "الإعدادات الافتراضية جاهزة. اضغط حفظ للتطبيق.");
                            }
                            Confirm::ForgetWords => {
                                match crate::predict::forget_all(&crate::predict::memory_path_for(&self.config_path)) {
                                    Ok(()) => {
                                        self.learned_words = 0;
                                        self.say("Learned words forgotten", "تم حذف الكلمات المحفوظة");
                                    }
                                    Err(e) => self.fail("Could not delete words", "تعذر حذف الكلمات", e),
                                }
                            }
                            Confirm::CloseUnsaved => {
                                if self.commit() {
                                    self.allow_close = true;
                                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                }
                            }
                        }
                    }
                    if confirm == Confirm::CloseUnsaved && ui.button(self.tr("Don't save", "عدم الحفظ")).clicked() {
                        self.confirm = None;
                        self.allow_close = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button(self.tr("Cancel", "إلغاء")).clicked() {
                        self.confirm = None;
                    }
                });
            });
    }

    // ------------------------------------------------------------------- pages

    fn page_home(&mut self, ui: &mut egui::Ui, live: &Live) {
        let lang = self.lang;
        let (mode, color) = self.mode_text(live);
        card(ui, lang, &mode, &self.tr("Live state of the background app", "الحالة المباشرة للبرنامج في الخلفية"), |ui| {
            ui.horizontal_wrapped(|ui| {
                let connection = if !live.connected {
                    t("No controller", "لا توجد يد تحكم", lang)
                } else if live.name.is_empty() {
                    t("Controller connected", "يد التحكم متصلة", lang)
                } else {
                    format!("🎮 {}", live.name)
                };
                pill(ui, &connection, if live.connected { C_GOOD } else { C_DIM });
                pill(ui, &format!("{}: {}", t("Profile", "البروفايل", lang), live.profile), C_ACCENT);
                pill(ui, &format!("{} {}", t("Slot", "المنفذ", lang), live.slot), C_INFO);
                if live.connected && live.wired {
                    pill(ui, &format!("🔌 {}", t("Wired", "سلكية", lang)), C_GOOD);
                } else if let Some(level) = live.battery.filter(|_| live.connected) {
                    let c = if level <= 1 { C_WARN } else { C_GOOD };
                    pill(ui, &format!("🔋 {}", battery_text(level, lang)), c);
                }
                let _ = color;
            });
        });

        let conflicts = crate::config::binding_conflicts(&self.working.bindings);
        if !conflicts.is_empty() {
            card(ui, lang, &t("⚠ Button conflicts", "⚠ تعارض في الأزرار", lang), &t("Only the first of these bindings works. Fix them on the Buttons page.", "يعمل أول تعيين فقط من هذه. أصلحها من صفحة الأزرار.", lang), |ui| {
                for (input, modifier) in &conflicts {
                    let prefix = if modifier.is_some() { "Back + " } else { "" };
                    ui.label(egui::RichText::new(format!("{prefix}{}", format_input(*input, lang))).color(C_WARN));
                }
            });
        }

        card(ui, lang, &t("Your controller", "أزرار يد التحكم", lang), &t("What each button does right now", "وظيفة كل زر الآن", lang), |ui| {
            for input in INPUT_ORDER.iter().chain([Input::LeftStick, Input::RightStick].iter()) {
                if let Some(b) = self.working.bindings.iter().find(|b| b.input == *input && b.modifier.is_none()) {
                    let action = if matches!(b.action, Action::Modifier(_)) {
                        t("Hold for the second layer", "اضغط مطولاً للوظائف الإضافية", lang)
                    } else {
                        format_action(&b.action, lang)
                    };
                    row(ui, lang, &format_input(*input, lang), "", |ui| {
                        ui.label(egui::RichText::new(action).color(C_ACCENT));
                    });
                }
            }
            ui.separator();
            row(ui, lang, "Back + Start", "", |ui| {
                ui.label(egui::RichText::new(t("Turn desktop control on/off", "تشغيل/إيقاف التحكم المكتبي", lang)).color(C_ACCENT));
            });
            row(ui, lang, &t("Hold Guide 2.5 s", "اضغط Guide مطولاً 2.5 ث", lang), "", |ui| {
                ui.label(egui::RichText::new(t("Turn the controller off", "إطفاء يد التحكم", lang)).color(C_ACCENT));
            });
        });
    }

    fn page_tutorial(&mut self, ui: &mut egui::Ui, live: &Live) {
        let lang = self.lang;
        if let Some(pad) = live.pad {
            tutorial_progress(&pad, &mut self.tutorial);
        }
        let done = self.tutorial.iter().filter(|d| **d).count();
        card(ui, lang, &t("Welcome to Reverse Controller", "أهلاً بك في Reverse Controller", lang), &t(
            "Your controller now works as a mouse and keyboard. Try each step below; it ticks itself off when you do it.",
            "يد التحكم تعمل الآن كماوس وكيبورد. جرّب كل خطوة بالأسفل، وستُعلَّم تلقائياً عند تنفيذها.",
            lang,
        ), |ui| {
            if live.pad.is_none() {
                ui.label(egui::RichText::new(t("Connect a controller to begin.", "وصّل يد التحكم للبدء.", lang)).color(C_WARN));
            }
            ui.add(
                egui::ProgressBar::new(done as f32 / TUTORIAL_STEPS as f32)
                    .fill(C_ACCENT)
                    .text(format!("{done} / {TUTORIAL_STEPS}")),
            );
        });
        let steps: [(&str, &str, &str, &str); TUTORIAL_STEPS] = [
            ("Move the pointer", "حرّك المؤشر", "Push the left stick in any direction.", "ادفع العصا اليسرى في أي اتجاه."),
            ("Click", "انقر", "Press A to left-click (X right-clicks).", "اضغط A للنقر الأيسر، وX للنقر الأيمن."),
            ("Scroll", "مرّر", "Push the right stick up or down.", "ادفع العصا اليمنى للأعلى أو الأسفل."),
            ("Aim precisely", "صوّب بدقة", "Hold LT while moving the left stick to slow the pointer.", "اضغط LT وأنت تحرك العصا اليسرى لإبطاء المؤشر."),
            ("Type", "اكتب", "Press L3 (click the left stick) to open the keyboard; B closes it.", "اضغط على العصا اليسرى L3 لفتح الكيبورد، وB لإغلاقه."),
            ("Quick menu", "القائمة السريعة", "Hold Back and press R3 for volume, desktop, screenshot and more.", "اضغط Back مطولاً ثم R3 للصوت وسطح المكتب ولقطة الشاشة وغيرها."),
        ];
        for (i, (en, ar_title, en_help, ar_help)) in steps.iter().enumerate() {
            let ok = self.tutorial[i];
            let mark = if ok { "✅" } else { "⬜" };
            let title = format!("{mark}  {}. {}", i + 1, t(en, ar_title, lang));
            row(ui, lang, &title, &t(en_help, ar_help, lang), |ui| {
                if ok {
                    pill(ui, &t("Done", "تم", lang), C_GOOD);
                }
            });
            ui.add_space(4.0);
        }
        if done == TUTORIAL_STEPS {
            card(ui, lang, &t("🎉 You're all set", "🎉 أصبحت جاهزاً", lang), "", |ui| {
                ui.label(t(
                    "Two things to remember: Back + Start turns desktop control on or off (games get the controller back), and holding Guide for 2.5 seconds turns a wireless controller off.",
                    "شيئان للتذكر: Back + Start يشغّل أو يوقف التحكم المكتبي (وتعود اليد للألعاب)، والضغط المطوّل على Guide لمدة 2.5 ثانية يطفئ اليد اللاسلكية.",
                    lang,
                ));
            });
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(t("↺ Start over", "↺ البدء من جديد", lang)).clicked() {
                self.tutorial = [false; TUTORIAL_STEPS];
            }
            if ui.button(t("Every button →", "كل الأزرار ←", lang)).clicked() {
                self.page = SettingsPage::Home;
            }
        });
    }

    fn page_pointer(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        card(ui, lang, &t("Presets", "إعدادات جاهزة", lang), &t("A starting point; fine-tune below", "نقطة بداية، ثم اضبط بالأسفل", lang), |ui| {
            ui.horizontal_wrapped(|ui| {
                for (en, ar_str, speed, accel) in [
                    ("🎯 Precision", "🎯 دقيق", 450.0, 200.0),
                    ("⚖ Balanced", "⚖ متوازن", 1100.0, 800.0),
                    ("🖥 Large display", "🖥 شاشات كبيرة", 1800.0, 1200.0),
                    ("🚀 Fast", "🚀 سريع", 2400.0, 1800.0),
                ] {
                    if ui.button(t(en, ar_str, lang)).clicked() {
                        let m = &mut self.working.mouse;
                        m.speed = speed;
                        m.acceleration = accel;
                        m.speed_x = 0.0;
                        m.speed_y = 0.0;
                        m.altsnap_enabled = false;
                        m.diagonal_range = 0.0;
                    }
                }
            });
        });
        let m = &mut self.working.mouse;
        card(ui, lang, &t("Speed", "السرعة", lang), "", |ui| {
            row(ui, lang, &t("Pointer speed", "سرعة المؤشر", lang), &t("Pixels per second at full tilt", "بكسل في الثانية عند أقصى ميل", lang), |ui| {
                ui.add(egui::Slider::new(&mut m.speed, 50.0..=3000.0).suffix(" px/s"));
            });
            row(ui, lang, &t("Acceleration", "التسارع", lang), &t("Extra speed when the stick is held at the edge", "سرعة إضافية عند تثبيت العصا على الحافة", lang), |ui| {
                ui.add(egui::Slider::new(&mut m.acceleration, 0.0..=8000.0));
            });
            row(ui, lang, &t("Acceleration starts at", "يبدأ التسارع عند", lang), "", |ui| {
                ui.add(egui::Slider::new(&mut m.acceleration_threshold, 0.0..=1.0).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
            });
            row(ui, lang, &t("Horizontal speed", "السرعة الأفقية", lang), &t("0 = same as pointer speed", "0 = مثل سرعة المؤشر", lang), |ui| {
                ui.add(egui::DragValue::new(&mut m.speed_x).speed(10.0).clamp_range(0.0..=4000.0));
            });
            row(ui, lang, &t("Vertical speed", "السرعة العمودية", lang), &t("0 = same as pointer speed", "0 = مثل سرعة المؤشر", lang), |ui| {
                ui.add(egui::DragValue::new(&mut m.speed_y).speed(10.0).clamp_range(0.0..=4000.0));
            });
        });
        card(ui, lang, &t("Precision", "الدقة", lang), "", |ui| {
            row(ui, lang, &t("Sniper mode (LT)", "وضع القناص (LT)", lang), &t("Squeeze LT to slow the pointer for small targets", "اضغط LT لإبطاء المؤشر للأهداف الصغيرة", lang), |ui| {
                toggle(ui, &mut m.sniper_enabled);
            });
            if m.sniper_enabled {
                row(ui, lang, &t("Sniper speed at full squeeze", "سرعة القناص عند الضغط الكامل", lang), "", |ui| {
                    ui.add(egui::Slider::new(&mut m.sniper_scale, 0.05..=0.8).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                });
            }
            row(ui, lang, &t("Smart pointer", "المؤشر الذكي", lang), &t("Slows down over links and buttons; a full push still sweeps past", "يبطئ فوق الروابط والأزرار؛ الدفع الكامل يمر بسرعة", lang), |ui| {
                toggle(ui, &mut m.smart_pointer);
            });
            if m.smart_pointer {
                row(ui, lang, &t("Speed over a link or button", "السرعة فوق رابط أو زر", lang), "", |ui| {
                    ui.add(egui::Slider::new(&mut m.smart_pointer_speed, 0.15..=0.9).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
                });
            }
            row(ui, lang, "AltSnap", &t("Pushing the stick all the way becomes fine movement", "دفع العصا للنهاية يتحول لحركة دقيقة", lang), |ui| {
                toggle(ui, &mut m.altsnap_enabled);
            });
            if m.altsnap_enabled {
                row(ui, lang, &t("AltSnap starts at", "يبدأ AltSnap عند", lang), "", |ui| {
                    ui.add(egui::Slider::new(&mut m.altsnap_zone, 1000.0..=32000.0));
                });
                row(ui, lang, &t("AltSnap speed", "سرعة AltSnap", lang), "", |ui| {
                    ui.add(egui::Slider::new(&mut m.altsnap_speed, 0.05..=1.0));
                });
            }
        });
        card(ui, lang, &t("Stick curve", "منحنى العصا", lang), &t("Advanced shaping of the stick response", "تشكيل متقدم لاستجابة العصا", lang), |ui| {
            row(ui, lang, &t("8-way gate near centre", "قفل 8 اتجاهات قرب المركز", lang), &t("Stops slow diagonal drift; 0 = free analog", "يمنع الانحراف القطري البطيء؛ 0 = حركة حرة", lang), |ui| {
                ui.add(egui::Slider::new(&mut m.diagonal_range, 0.0..=100.0).suffix("%"));
            });
            row(ui, lang, &t("Cruise zone", "منطقة الاندفاع", lang), &t("Holding past this speeds up over time", "التثبيت بعدها يزيد السرعة تدريجياً", lang), |ui| {
                ui.add(egui::Slider::new(&mut m.modifier_zone, 0.0..=32767.0));
            });
            row(ui, lang, &t("Max zone", "منطقة التشبع", lang), &t("Deflection that counts as full speed", "الميل الذي يُعتبر سرعة كاملة", lang), |ui| {
                ui.add(egui::Slider::new(&mut m.max_zone, 8000.0..=32767.0));
            });
        });
        card(ui, lang, &t("Scrolling", "التمرير", lang), "", |ui| {
            row(ui, lang, &t("Scroll speed", "سرعة التمرير", lang), &t("Wheel ticks per second at full tilt", "نبضات العجلة في الثانية عند أقصى ميل", lang), |ui| {
                ui.add(egui::Slider::new(&mut m.wheel_speed, 0.0..=20000.0));
            });
            row(ui, lang, &t("Horizontal scroll speed", "سرعة التمرير الأفقي", lang), &t("0 = automatic", "0 = تلقائي", lang), |ui| {
                ui.add(egui::DragValue::new(&mut m.wheel_speed_x).speed(10.0).clamp_range(0.0..=20000.0));
            });
            row(ui, lang, &t("Vertical scroll speed", "سرعة التمرير العمودي", lang), &t("0 = same as scroll speed", "0 = مثل سرعة التمرير", lang), |ui| {
                ui.add(egui::DragValue::new(&mut m.wheel_speed_y).speed(10.0).clamp_range(0.0..=20000.0));
            });
            row(ui, lang, &t("Lock to 4 directions", "تثبيت على 4 اتجاهات", lang), &t("Scroll straight up/down or left/right", "تمرير مستقيم للأعلى/الأسفل أو اليمين/اليسار", lang), |ui| {
                toggle(ui, &mut m.scroll_four_way);
            });
            row(ui, lang, &t("Invert vertical scrolling", "عكس التمرير العمودي", lang), &t("Turn on if pushing up scrolls down", "فعّلها إذا كان الدفع للأعلى ينزل الصفحة", lang), |ui| {
                toggle(ui, &mut m.scroll_flip_y);
            });
        });
    }

    fn page_controller(&mut self, ui: &mut egui::Ui, live: &Live) {
        let lang = self.lang;
        card(ui, lang, &t("Supported controllers", "أيادي التحكم المدعومة", lang), &t("No extra drivers needed", "بدون تعريفات إضافية", lang), |ui| {
            for (name, how) in [
                ("Xbox Series / One / 360", t("XInput, with vibration and battery", "XInput مع الاهتزاز والبطارية", lang)),
                ("PlayStation 5 (DualSense, Edge)", t("Analog triggers; PS = Guide", "زنادات تناظرية؛ زر PS = Guide", lang)),
                ("PlayStation 4 (DualShock 4)", t("Analog triggers; PS = Guide", "زنادات تناظرية؛ زر PS = Guide", lang)),
                ("Nintendo Switch Pro", t("Buttons by position (bottom = A)", "الأزرار حسب مكانها (السفلي = A)", lang)),
                (&*t("Other USB / Bluetooth gamepads", "أيادٍ أخرى USB / بلوتوث", lang), t("Standard layout", "التخطيط القياسي", lang)),
            ] {
                row(ui, lang, name, "", |ui| {
                    ui.label(egui::RichText::new(how).color(C_DIM));
                });
            }
            ui.label(egui::RichText::new(t(
                "An Xbox-type pad is used first when several are connected. Vibration and battery level are available on Xbox-type pads only.",
                "عند توصيل أكثر من يد تُستخدم يد Xbox أولاً. الاهتزاز ومستوى البطارية متاحان في أيادي Xbox فقط.",
                lang,
            )).small().color(C_DIM));
        });

        card(ui, lang, &t("Live input", "الإدخال المباشر", lang), &if live.name.is_empty() { t("Press buttons and move the sticks to test them", "اضغط الأزرار وحرك العصي لاختبارها", lang) } else { format!("{} · {}", live.name, t("press buttons and move the sticks", "اضغط الأزرار وحرك العصي", lang)) }, |ui| {
            let Some(pad) = live.pad else {
                ui.label(egui::RichText::new(t("Connect a controller to see its input.", "وصّل يد التحكم لرؤية الإدخال.", lang)).color(C_DIM));
                return;
            };
            ui.horizontal_wrapped(|ui| {
                let b = pad.wButtons;
                for (name, bit) in [
                    ("A", 0x1000), ("B", 0x2000), ("X", 0x4000), ("Y", 0x8000), ("LB", 0x0100), ("RB", 0x0200),
                    ("L3", 0x0040), ("R3", 0x0080), ("↑", 0x0001), ("↓", 0x0002), ("←", 0x0004), ("→", 0x0008),
                    ("Back", 0x0020), ("Start", 0x0010), ("Guide", 0x0400),
                ] {
                    let down = b & bit != 0;
                    pill(ui, name, if down { C_ACCENT } else { C_DIM });
                }
            });
            ui.add_space(6.0);
            for (name, value) in [("LT", pad.bLeftTrigger), ("RT", pad.bRightTrigger)] {
                ui.horizontal(|ui| {
                    ui.label(name);
                    ui.add(egui::ProgressBar::new(value as f32 / 255.0).desired_width(260.0).fill(C_ACCENT).text(format!("{:.0}%", value as f32 / 2.55)));
                });
            }
            // Always left stick on the left, like the physical controller.
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 180.0), egui::Layout::left_to_right(egui::Align::Min), |ui| {
                stick_preview(ui, &t("Left stick", "العصا اليسرى", lang), pad.sThumbLX, pad.sThumbLY, self.working.controller.left_stick_dead_zone, lang);
                stick_preview(ui, &t("Right stick", "العصا اليمنى", lang), pad.sThumbRX, pad.sThumbRY, self.working.controller.right_stick_dead_zone, lang);
            });
        });

        self.mapping_card(ui);

        let slot = live.slot;
        card(ui, lang, &t("Drift calibration", "معايرة الانجراف", lang), &t("Leave both sticks untouched for 4 seconds; we suggest dead zones", "اترك العصيين دون لمس 4 ثوانٍ وسنقترح مناطق الخمول", lang), |ui| {
            if let Some((started, _)) = &self.calibration {
                ui.add(egui::ProgressBar::new((started.elapsed().as_secs_f32() / 4.0).min(1.0)).fill(C_ACCENT).text(t("Keep both sticks released", "اترك العصيين ثابتتين", lang)));
                if ui.button(t("Cancel", "إلغاء", lang)).clicked() {
                    self.calibration = None;
                }
            } else if ui.add_enabled(live.pad.is_some(), accent_button(t("Measure stick drift", "قياس الانجراف", lang))).clicked() {
                self.calibration_result = None;
                self.calibration = Some((Instant::now(), crate::calibration::RestCalibration::new(slot)));
            }
            if let Some((_, [left, right])) = self.calibration_result {
                ui.label(format!("{}: {left} · {right}", t("Suggested dead zones (left · right)", "مناطق الخمول المقترحة (يسار · يمين)", lang)));
                if ui.button(t("Use suggested dead zones", "استخدام القيم المقترحة", lang)).clicked() {
                    self.working.controller.left_stick_dead_zone = left;
                    self.working.controller.right_stick_dead_zone = right;
                    self.calibration_result = None;
                }
            }
        });

        let c = &mut self.working.controller;
        card(ui, lang, &t("Sensitivity", "الحساسية", lang), "", |ui| {
            row(ui, lang, &t("Left stick dead zone", "منطقة خمول العصا اليسرى", lang), &t("Higher ignores more drift", "القيمة الأعلى تتجاهل انجرافاً أكثر", lang), |ui| {
                ui.add(egui::Slider::new(&mut c.left_stick_dead_zone, 0..=20000));
            });
            row(ui, lang, &t("Right stick dead zone", "منطقة خمول العصا اليمنى", lang), "", |ui| {
                ui.add(egui::Slider::new(&mut c.right_stick_dead_zone, 0..=20000));
            });
            row(ui, lang, &t("Trigger press point", "نقطة ضغط الزناد", lang), &t("How far LT/RT must travel to count as pressed", "المسافة اللازمة لاعتبار LT/RT مضغوطاً", lang), |ui| {
                ui.add(egui::Slider::new(&mut c.trigger_threshold, 5..=250));
            });
            row(ui, lang, &t("Controller slot", "منفذ يد التحكم", lang), &t("Another connected pad is picked automatically", "يتم اختيار يد أخرى متصلة تلقائياً", lang), |ui| {
                ui.add(egui::Slider::new(&mut c.controller_index, 0..=3));
            });
        });
        card(ui, lang, &t("Feedback", "التنبيهات", lang), "", |ui| {
            row(ui, lang, &t("Vibration feedback", "الاهتزاز عند الضغط", lang), &t("Short taps on clicks and shortcuts", "اهتزازات قصيرة عند النقر والاختصارات", lang), |ui| {
                toggle(ui, &mut self.working.controller.haptic_feedback);
            });
            row(ui, lang, &t("Low battery warning", "تنبيه البطارية المنخفضة", lang), &t("Notify once when a wireless pad runs low", "تنبيه مرة واحدة عند انخفاض بطارية اليد اللاسلكية", lang), |ui| {
                toggle(ui, &mut self.working.low_battery_warning);
            });
        });
    }

    /// Teach the app the buttons of a controller it does not know.
    fn mapping_card(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        if let Some((id, wizard)) = &mut self.wizard {
            match crate::gamepads::read_raw(*id) {
                Some(raw) => wizard.observe(&raw),
                None => {
                    self.wizard = None;
                    self.say("The controller was disconnected; mapping cancelled", "انفصلت يد التحكم؛ أُلغي الربط");
                    return;
                }
            }
        }
        let mut finished = None;
        let mut cancel = false;
        card(ui, lang, &t("Map a controller's buttons", "ربط أزرار يد التحكم", lang), &t(
            "For a PlayStation, Switch or other gamepad whose buttons land in the wrong place. Xbox-type pads never need this.",
            "ليد PlayStation أو Switch أو أي يد أزرارها في غير مكانها. أيادي Xbox لا تحتاج هذا.",
            lang,
        ), |ui| {
            match &mut self.wizard {
                Some((_, wizard)) => {
                    let (done, total) = wizard.progress();
                    ui.label(egui::RichText::new(&wizard.name).strong());
                    ui.add(egui::ProgressBar::new(done as f32 / total as f32).fill(C_ACCENT).text(format!("{done} / {total}")));
                    match wizard.current() {
                        Some(step) => {
                            let (en, ar_str) = step.prompt();
                            let text = if wizard.waiting_for_release() {
                                t("✓ Got it. Let go of everything…", "✓ تم. اترك كل الأزرار…", lang)
                            } else {
                                t(en, ar_str, lang)
                            };
                            ui.label(egui::RichText::new(text).size(18.0).color(C_ACCENT));
                            ui.horizontal(|ui| {
                                if ui.button(t("Skip", "تخطَّ", lang)).clicked() {
                                    wizard.skip();
                                }
                                if ui.button(t("Cancel", "إلغاء", lang)).clicked() {
                                    cancel = true;
                                }
                            });
                        }
                        None => {
                            ui.label(egui::RichText::new(t("All buttons learned.", "تم تعلّم كل الأزرار.", lang)).color(C_GOOD));
                            ui.horizontal(|ui| {
                                if ui.add(accent_button(t("Save this layout", "حفظ هذا التخطيط", lang))).clicked() {
                                    finished = Some(crate::config::PadLayoutOverride {
                                        vendor: wizard.vendor,
                                        product: wizard.product,
                                        name: wizard.name.clone(),
                                        layout: wizard.layout(),
                                    });
                                }
                                if ui.button(t("Cancel", "إلغاء", lang)).clicked() {
                                    cancel = true;
                                }
                            });
                        }
                    }
                }
                None => {
                    if self.other_pads.is_empty() {
                        ui.label(egui::RichText::new(t("No PlayStation, Switch or other non-Xbox controller found.", "لم يتم العثور على يد PlayStation أو Switch أو غيرها.", lang)).color(C_DIM));
                    }
                    let mut start = None;
                    for dev in &self.other_pads {
                        let learned = self.working.custom_pad_layouts.iter().any(|o| o.vendor == dev.vendor && o.product == dev.product);
                        let help = if learned { t("Custom layout saved", "تخطيط مخصص محفوظ", lang) } else { format!("{:04X}:{:04X}", dev.vendor, dev.product) };
                        row(ui, lang, &dev.display_name(), &help, |ui| {
                            if ui.button(t("Map buttons", "ربط الأزرار", lang)).clicked() {
                                start = Some(dev.clone());
                            }
                        });
                    }
                    if let Some(dev) = start {
                        self.wizard = Some((dev.id, crate::gamepads::MapWizard::new(&dev)));
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(t("🔍 Find controllers", "🔍 البحث عن الأيادي", lang)).clicked() {
                            self.other_pads = crate::gamepads::list_models();
                        }
                        if !self.working.custom_pad_layouts.is_empty() && ui.button(t("Forget learned layouts", "حذف التخطيطات المحفوظة", lang)).clicked() {
                            self.working.custom_pad_layouts.clear();
                        }
                    });
                }
            }
        });
        if cancel {
            self.wizard = None;
        }
        if let Some(learned) = finished {
            self.working.custom_pad_layouts.retain(|o| !(o.vendor == learned.vendor && o.product == learned.product));
            self.working.custom_pad_layouts.push(learned);
            self.wizard = None;
            if self.commit() {
                self.say("Layout saved; the controller uses it now", "تم حفظ التخطيط؛ اليد تستخدمه الآن");
            }
        }
    }

    fn page_keyboard(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let k = &mut self.working.keyboard;
        card(ui, lang, &t("Placement", "المكان والحجم", lang), "", |ui| {
            row(ui, lang, &t("Position", "المكان", lang), &t("Edge of the active monitor", "حافة الشاشة النشطة", lang), |ui| {
                use crate::config::KeyboardPosition as KP;
                let options = [
                    (KP::Bottom, t("Bottom", "أسفل", lang)),
                    (KP::FloatBottom, t("Floating", "عائم", lang)),
                    (KP::Top, t("Top", "أعلى", lang)),
                    (KP::Left, t("Left", "يسار", lang)),
                    (KP::Right, t("Right", "يمين", lang)),
                    (KP::Custom, t("Where I left it", "آخر مكان وضعته فيه", lang)),
                ];
                let current = options.iter().find(|(v, _)| *v == k.position).map(|(_, l)| l.clone()).unwrap_or_default();
                egui::ComboBox::from_id_source("kb_pos").selected_text(current).width(200.0).show_ui(ui, |ui| {
                    for (v, label) in options {
                        ui.selectable_value(&mut k.position, v, label);
                    }
                });
            });
            row(ui, lang, &t("Size", "الحجم", lang), "", |ui| {
                ui.add(egui::Slider::new(&mut k.scale, 0.75..=1.5).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
            });
            row(ui, lang, &t("Remember where I move it", "تذكر المكان الذي أنقله إليه", lang), &t("R3 or drag to move; kept after restart", "R3 أو السحب للنقل؛ يبقى بعد إعادة التشغيل", lang), |ui| {
                toggle(ui, &mut k.remember_position);
            });
            if k.last_position.is_some() {
                row(ui, lang, &t("Saved position", "المكان المحفوظ", lang), "", |ui| {
                    if ui.button(t("Forget it", "نسيان المكان", lang)).clicked() {
                        k.last_position = None;
                    }
                });
            }
            row(ui, lang, &t("Avoid the text cursor", "تجنب مؤشر الكتابة", lang), &t("Move to the top when typing near the bottom", "الانتقال للأعلى عند الكتابة قرب الأسفل", lang), |ui| {
                toggle(ui, &mut k.smart_caret_avoidance);
            });
        });
        card(ui, lang, &t("Typing", "الكتابة", lang), "", |ui| {
            row(ui, lang, &t("Open on text fields", "الفتح عند النقر على حقل نصي", lang), &t("Clicking a text box with A shows the keyboard", "النقر على حقل نصي بزر A يظهر الكيبورد", lang), |ui| {
                toggle(ui, &mut self.working.vk_auto_show);
            });
            row(ui, lang, &t("Start in Arabic", "البدء بالعربية", lang), &t("Otherwise follows the Windows input language", "وإلا يتبع لغة الإدخال في ويندوز", lang), |ui| {
                toggle(ui, &mut self.working.keyboard.start_arabic);
            });
            row(ui, lang, &t("Split keyboard by default", "الكيبورد المنقسم افتراضياً", lang), &t("Both sticks move cursors; LT/RT type. L3+R3 switches", "العصيان تحركان المؤشرين وLT/RT للكتابة. L3+R3 للتبديل", lang), |ui| {
                toggle(ui, &mut self.working.keyboard.split_mode);
            });
            row(ui, lang, &t("Show controls when opened", "عرض الاختصارات عند الفتح", lang), "", |ui| {
                toggle(ui, &mut self.working.keyboard.show_hints_on_open);
            });
            row(ui, lang, &t("Sound effects", "المؤثرات الصوتية", lang), &t("Key clicks and menu sounds", "نقرات المفاتيح وأصوات القوائم", lang), |ui| {
                toggle(ui, &mut self.working.keyboard.mechanical_sfx);
            });
        });
        let learned = self.learned_words;
        card(ui, lang, &t("Word suggestions", "اقتراح الكلمات", lang), &t("Learned words never leave this PC", "الكلمات المحفوظة لا تغادر هذا الجهاز", lang), |ui| {
            row(ui, lang, &t("Learn my words", "تعلّم كلماتي", lang), &t("Words you type twice appear as suggestions", "الكلمات التي تكتبها مرتين تظهر كاقتراحات", lang), |ui| {
                toggle(ui, &mut self.working.keyboard.learn_words);
            });
            row(ui, lang, &format!("{}: {learned}", t("Words remembered", "كلمات محفوظة", lang)), "", |ui| {
                if ui.add_enabled(learned > 0, egui::Button::new(t("Forget all", "حذف الكل", lang))).clicked() {
                    self.confirm = Some(Confirm::ForgetWords);
                }
            });
        });
        card(ui, lang, &t("Keyboard controls", "اختصارات الكيبورد", lang), "", |ui| {
            let hints = if lang == GuiLang::Ar { crate::vkeyboard::render::HINTS_AR } else { crate::vkeyboard::render::HINTS };
            for (button, what) in hints {
                let (button, what) = if lang == GuiLang::Ar { (ar(button), ar(what)) } else { (button.to_string(), what.to_string()) };
                row(ui, lang, &button, "", |ui| {
                    ui.label(egui::RichText::new(what).color(C_ACCENT));
                });
            }
        });
    }

    fn page_buttons(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let conflicts = crate::config::binding_conflicts(&self.working.bindings);
        if !conflicts.is_empty() {
            card(ui, lang, &t("⚠ Conflicts", "⚠ تعارضات", lang), &t("These buttons are bound twice; the second binding never runs", "هذه الأزرار معيّنة مرتين؛ التعيين الثاني لا يعمل", lang), |ui| {
                for (input, modifier) in &conflicts {
                    let prefix = if modifier.is_some() { "Back + " } else { "" };
                    ui.label(egui::RichText::new(format!("{prefix}{}", format_input(*input, lang))).color(C_WARN));
                }
            });
        }

        card(ui, lang, &t("Sticks", "العصي", lang), &t("Swap them for left-handed use", "بدّلها للاستخدام باليد اليسرى", lang), |ui| {
            for input in [Input::LeftStick, Input::RightStick] {
                if let Some(b) = self.working.bindings.iter_mut().find(|b| b.input == input && b.modifier.is_none()) {
                    row(ui, lang, &format_input(input, lang), "", |ui| {
                        stick_picker(ui, ("stick", input), &mut b.action, lang);
                    });
                }
            }
            if ui.button(t("⇄ Swap sticks", "⇄ تبديل العصيين", lang)).clicked() {
                let left = self.working.bindings.iter().position(|b| b.input == Input::LeftStick && b.modifier.is_none());
                let right = self.working.bindings.iter().position(|b| b.input == Input::RightStick && b.modifier.is_none());
                if let (Some(l), Some(r)) = (left, right) {
                    let a = self.working.bindings[l].action.clone();
                    self.working.bindings[l].action = self.working.bindings[r].action.clone();
                    self.working.bindings[r].action = a;
                }
            }
        });

        card(ui, lang, &t("Buttons", "الأزرار", lang), &t("Pick from the shortcut library", "اختر من مكتبة الاختصارات", lang), |ui| {
            for input in INPUT_ORDER {
                if let Some(b) = self.working.bindings.iter_mut().find(|b| b.input == input && b.modifier.is_none()) {
                    row(ui, lang, &format_input(input, lang), "", |ui| {
                        if matches!(b.action, Action::Modifier(_)) {
                            ui.label(egui::RichText::new(t("Hold for the second layer", "اضغط مطولاً للوظائف الإضافية", lang)).color(C_DIM));
                        } else {
                            action_picker(ui, ("base", input), &mut b.action, lang);
                        }
                    });
                }
            }
        });

        let mut remove: Option<usize> = None;
        card(ui, lang, &t("Second layer (hold Back)", "الوظائف الإضافية (مع زر Back)", lang), &t("Hold Back, then press a button", "اضغط Back مطولاً ثم زراً آخر", lang), |ui| {
            // Back + Start is handled by the engine itself, so its binding never runs.
            for input in INPUT_ORDER.iter().filter(|i| **i != Input::Start).chain([Input::LeftStick, Input::RightStick].iter()) {
                let Some(index) = self.working.bindings.iter().position(|b| b.input == *input && b.modifier.is_some()) else {
                    continue;
                };
                row(ui, lang, &format!("Back + {}", format_input(*input, lang)), "", |ui| {
                    if ui.small_button("🗑").on_hover_text(t("Remove", "إزالة", lang)).clicked() {
                        remove = Some(index);
                    }
                    let b = &mut self.working.bindings[index];
                    if input.is_stick() {
                        stick_picker(ui, ("mod_stick", *input), &mut b.action, lang);
                    } else {
                        action_picker(ui, ("mod", *input), &mut b.action, lang);
                    }
                });
            }
            row(ui, lang, "Back + Start", &t("Reserved: always turns desktop control on/off", "محجوز: يشغّل/يوقف التحكم المكتبي دائماً", lang), |_| {});
            ui.separator();
            let free: Vec<Input> = INPUT_ORDER
                .iter()
                .copied()
                .filter(|i| !matches!(i, Input::Back | Input::Start))
                .filter(|i| !self.working.bindings.iter().any(|b| b.input == *i && b.modifier.is_some()))
                .collect();
            if !free.is_empty() {
                ui.horizontal(|ui| {
                    let selected = self.add_back_input.filter(|i| free.contains(i)).unwrap_or(free[0]);
                    egui::ComboBox::from_id_source("add_back").selected_text(format!("Back + {}", format_input(selected, lang))).show_ui(ui, |ui| {
                        for i in &free {
                            if ui.selectable_label(*i == selected, format!("Back + {}", format_input(*i, lang))).clicked() {
                                self.add_back_input = Some(*i);
                            }
                        }
                    });
                    if ui.button(t("➕ Add shortcut", "➕ إضافة اختصار", lang)).clicked() {
                        self.working.bindings.push(PadBindingDef {
                            input: selected,
                            modifier: Some(Modifier::Back),
                            action: Action::DoNothing,
                        });
                        self.add_back_input = None;
                    }
                });
            }
        });
        if let Some(index) = remove {
            self.working.bindings.remove(index);
        }
        if ui.button(t("↺ Restore default buttons", "↺ استعادة الأزرار الافتراضية", lang)).clicked() {
            self.working.bindings = crate::bindings::default_profile();
            self.say("Default buttons ready. Save to apply.", "الأزرار الافتراضية جاهزة. اضغط حفظ للتطبيق.");
        }
    }

    fn page_profiles(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        card(ui, lang, &t("Automatic switching", "التبديل التلقائي", lang), "", |ui| {
            row(ui, lang, &t("Switch by focused app", "التبديل حسب البرنامج المفتوح", lang), &t("A profile with matching programs takes over; leaving returns to yours", "البروفايل المطابق يتولى، وعند الخروج يعود بروفايلك", lang), |ui| {
                toggle(ui, &mut self.working.auto_profile_switch);
            });
        });

        let mut activate: Option<String> = None;
        let mut duplicate: Option<(PathBuf, String, String)> = None;
        let active = self.active_profile.clone();
        let list = self.profile_list.clone();
        card(ui, lang, &t("Your profiles", "بروفايلاتك", lang), &t("Click one to make it active", "انقر على بروفايل لتفعيله", lang), |ui| {
            let is_default = active == crate::profiles::DEFAULT_NAME;
            row(ui, lang, &format!("★ {}", t("Default", "الافتراضي", lang)), &t("Built in, always available", "مدمج ومتاح دائماً", lang), |ui| {
                if is_default {
                    pill(ui, &t("Active", "نشط", lang), C_GOOD);
                } else if ui.button(t("Use", "تفعيل", lang)).clicked() {
                    activate = Some(crate::profiles::DEFAULT_NAME.to_string());
                }
            });
            for p in &list {
                let stem = p.path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                let rules = crate::profiles::load(&p.path).map(|prof| prof.processes.join(", ")).unwrap_or_default();
                let help = if rules.is_empty() { t("Manual only", "يدوي فقط", lang) } else { format!("↳ {rules}") };
                row(ui, lang, &format!("🎮 {}", p.name), &help, |ui| {
                    if ui.small_button("🗑").on_hover_text(t("Delete", "حذف", lang)).clicked() {
                        if active == stem {
                            self.status = t("The active profile cannot be deleted", "لا يمكن حذف البروفايل النشط", lang);
                            self.status_error = true;
                        } else {
                            self.confirm = Some(Confirm::DeleteProfile(p.path.clone()));
                        }
                    }
                    if ui.small_button("⧉").on_hover_text(t("Duplicate", "نسخ", lang)).clicked() {
                        duplicate = Some((p.path.clone(), stem.clone(), p.name.clone()));
                    }
                    if p.legacy {
                        pill(ui, "AMGP", C_WARN);
                    }
                    if active == stem {
                        pill(ui, &t("Active", "نشط", lang), C_GOOD);
                    } else if ui.button(t("Use", "تفعيل", lang)).clicked() {
                        activate = Some(stem.clone());
                    }
                });
            }
        });
        if let Some(stem) = activate {
            self.activate_profile(&stem);
        }
        if let Some((path, stem, name)) = duplicate {
            match crate::profiles::load(&path) {
                Ok(mut copy) => {
                    let target = crate::profiles::unique_stem(&self.profiles_dir, &stem);
                    copy.name = Some(format!("{name} copy"));
                    match crate::profiles::save(&self.profiles_dir, &target, &copy) {
                        Ok(_) => {
                            self.refresh_profiles();
                            self.say(&format!("Duplicated to '{target}'"), &format!("تم النسخ إلى '{target}'"));
                        }
                        Err(e) => self.fail("Duplicate failed", "فشل النسخ", e),
                    }
                }
                Err(e) => self.fail("Duplicate failed", "فشل النسخ", e),
            }
        }

        card(ui, lang, &t("New profile from current buttons", "بروفايل جديد من الأزرار الحالية", lang), "", |ui| {
            row(ui, lang, &t("Name", "الاسم", lang), "", |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_profile_name).desired_width(240.0));
            });
            row(ui, lang, &t("Programs", "البرامج", lang), &t("Comma separated, e.g. chrome.exe, vlc.exe", "مفصولة بفاصلة، مثل chrome.exe, vlc.exe", lang), |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_profile_processes).hint_text("chrome.exe").desired_width(240.0));
            });
            if ui.add(accent_button(t("Save profile", "حفظ البروفايل", lang))).clicked() {
                let raw = self.new_profile_name.trim().to_string();
                let stem = crate::profiles::unique_stem(&self.profiles_dir, if raw.is_empty() { "profile" } else { &raw });
                let processes: Vec<String> = self
                    .new_profile_processes
                    .split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
                let prof = crate::profiles::Profile {
                    name: Some(if raw.is_empty() { stem.clone() } else { raw }),
                    processes,
                    bindings: self.working.bindings.clone(),
                };
                match crate::profiles::save(&self.profiles_dir, &stem, &prof) {
                    Ok(_) => {
                        self.refresh_profiles();
                        self.new_profile_name.clear();
                        self.new_profile_processes.clear();
                        self.say(&format!("Saved profile '{stem}'"), &format!("تم حفظ البروفايل '{stem}'"));
                    }
                    Err(e) => self.fail("Save failed", "فشل الحفظ", e),
                }
            }
        });

        card(ui, lang, &t("Import", "استيراد", lang), &t("AntiMicroX profiles (.amgp) from Downloads or the Desktop", "بروفايلات AntiMicroX (.amgp) من التنزيلات أو سطح المكتب", lang), |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button(t("🔍 Find .amgp files", "🔍 البحث عن ملفات .amgp", lang)).clicked() {
                    self.amgp_candidates = find_amgp_files();
                    if self.amgp_candidates.is_empty() {
                        self.say("No .amgp files found", "لم يتم العثور على ملفات .amgp");
                    }
                }
                if ui.button(t("📂 Open profiles folder", "📂 فتح مجلد البروفايلات", lang)).clicked() {
                    let _ = std::process::Command::new("explorer.exe").arg(&self.profiles_dir).spawn();
                }
                if ui.button(t("↻ Refresh", "↻ تحديث", lang)).clicked() {
                    self.refresh_profiles();
                }
            });
            let mut imported = None;
            for path in &self.amgp_candidates {
                let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                row(ui, lang, &name, &path.display().to_string(), |ui| {
                    if ui.button(t("Import as profile", "استيراد كبروفايل", lang)).clicked() {
                        imported = Some(path.clone());
                    }
                });
            }
            if let Some(path) = imported {
                let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
                let target = crate::profiles::unique_stem(&self.profiles_dir, &stem);
                let out = self.profiles_dir.join(format!("{target}.json"));
                match crate::amgp::convert_amgp_file(&path, &out) {
                    Ok(()) => {
                        self.refresh_profiles();
                        self.say(&format!("Imported '{target}'"), &format!("تم استيراد '{target}'"));
                    }
                    Err(e) => self.fail("Import failed", "فشل الاستيراد", e),
                }
            }
        });
    }

    fn page_general(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let mut language = None;
        card(ui, lang, &t("Language", "اللغة", lang), &t("Applies to this window right away", "تُطبق على هذه النافذة فوراً", lang), |ui| {
            ui.horizontal_wrapped(|ui| {
                for (setting, en, ar_str) in [("auto", "Same as Windows", "مثل ويندوز"), ("en", "English", "English"), ("ar", "العربية", "العربية")] {
                    let on = self.working.gui_language.eq_ignore_ascii_case(setting);
                    if ui.radio(on, t(en, ar_str, lang)).clicked() {
                        language = Some(setting);
                    }
                }
            });
        });
        if let Some(setting) = language {
            self.set_language(setting);
        }

        let mut autostart = self.working.autostart;
        card(ui, lang, &t("Startup", "بدء التشغيل", lang), "", |ui| {
            row(ui, lang, &t("Start with Windows", "التشغيل مع ويندوز", lang), &t("Runs in the tray after you sign in", "يعمل في شريط المهام بعد تسجيل الدخول", lang), |ui| {
                toggle(ui, &mut autostart);
            });
        });
        if autostart != self.working.autostart {
            self.set_autostart(autostart);
        }

        card(ui, lang, &t("Games and fullscreen", "الألعاب وملء الشاشة", lang), &t("Give the controller back to games automatically", "إرجاع يد التحكم للألعاب تلقائياً", lang), |ui| {
            row(ui, lang, &t("Pause in fullscreen apps", "الإيقاف في تطبيقات ملء الشاشة", lang), &t("Games get the controller; Back + Start still works", "تحصل الألعاب على اليد؛ Back + Start يعمل دائماً", lang), |ui| {
                toggle(ui, &mut self.working.disable_in_fullscreen);
            });
            ui.label(egui::RichText::new(t("Keep control in these fullscreen programs:", "استمرار التحكم في برامج ملء الشاشة هذه:", lang)).small().color(C_DIM));
            let mut remove = None;
            ui.horizontal_wrapped(|ui| {
                for (i, name) in self.working.fullscreen_exceptions.iter().enumerate() {
                    if ui.button(format!("{name}  ✕")).on_hover_text(t("Remove", "إزالة", lang)).clicked() {
                        remove = Some(i);
                    }
                }
            });
            if let Some(i) = remove {
                self.working.fullscreen_exceptions.remove(i);
            }
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.exception_input).hint_text("program.exe").desired_width(200.0));
                let name = self.exception_input.trim().to_ascii_lowercase();
                let valid = !name.is_empty() && !name.contains(['/', '\\', ':']);
                if ui.add_enabled(valid, egui::Button::new(t("➕ Add", "➕ إضافة", lang))).clicked() {
                    let name = if name.contains('.') { name } else { format!("{name}.exe") };
                    if !self.working.is_fullscreen_exception(&name) {
                        self.working.fullscreen_exceptions.push(name);
                    }
                    self.exception_input.clear();
                }
                if ui.button(t("Restore list", "استعادة القائمة", lang)).clicked() {
                    self.working.fullscreen_exceptions = crate::config::d_fullscreen_exceptions();
                }
            });
        });

        card(ui, lang, &t("Notifications and shortcut", "الإشعارات والاختصار", lang), "", |ui| {
            row(ui, lang, &t("On-screen notifications", "الإشعارات على الشاشة", lang), &t("Volume, battery, profile and snap messages", "رسائل الصوت والبطارية والبروفايل والمحاذاة", lang), |ui| {
                toggle(ui, &mut self.working.osd_enabled);
            });
            row(ui, lang, &t("Keyboard shortcut to toggle control", "اختصار لوحة المفاتيح للتشغيل/الإيقاف", lang), &t("For example Ctrl+K or Ctrl+Alt+F8; empty turns it off", "مثل Ctrl+K أو Ctrl+Alt+F8؛ اتركه فارغاً لإيقافه", lang), |ui| {
                if ui.add(egui::TextEdit::singleline(&mut self.hotkey_text).hint_text("Ctrl+K").desired_width(140.0)).changed() {
                    let trimmed = self.hotkey_text.trim().to_string();
                    self.hotkey_error = !(trimmed.is_empty() || crate::config::parse_hotkey(&trimmed).is_some());
                    if !self.hotkey_error {
                        self.working.toggle_hotkey = trimmed;
                    }
                }
                if self.hotkey_error {
                    ui.label(egui::RichText::new(t("Not a valid shortcut", "اختصار غير صالح", lang)).color(C_BAD));
                }
            });
        });

        let installed = crate::install::running_installed();
        card(ui, lang, &t("Installation", "التثبيت", lang), "", |ui| {
            if installed {
                let dir = crate::install::install_dir().map(|d| d.display().to_string()).unwrap_or_default();
                row(ui, lang, &t("Installed for this user", "مثبّت لهذا المستخدم", lang), &dir, |ui| {
                    if ui.button(t("Uninstall…", "إزالة…", lang)).clicked() {
                        if let Ok(exe) = std::env::current_exe() {
                            let _ = std::process::Command::new(exe).arg("--uninstall").spawn();
                        }
                    }
                });
            } else {
                row(ui, lang, &t("Running without installing", "يعمل بدون تثبيت", lang), &t("Install to get a Start-menu entry and uninstall from Windows Settings", "ثبّته ليظهر في قائمة ابدأ ويمكن إزالته من إعدادات ويندوز", lang), |ui| {
                    if ui.add(accent_button(t("Install", "تثبيت", lang))).clicked() {
                        if let Ok(exe) = std::env::current_exe() {
                            let _ = std::process::Command::new(exe).arg("--install").spawn();
                        }
                    }
                });
            }
        });

        card(ui, lang, &t("Troubleshooting", "حل المشكلات", lang), &t("Something not working? Start here", "هل هناك مشكلة؟ ابدأ من هنا", lang), |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button(t("🎓 Tutorial", "🎓 الدليل التعليمي", lang)).clicked() {
                    self.tutorial = [false; TUTORIAL_STEPS];
                    self.page = SettingsPage::Tutorial;
                }
                if ui.add(accent_button(t("🩺 Diagnostics report", "🩺 تقرير التشخيص", lang))).clicked() {
                    crate::doctor::open_report(&self.config_path);
                }
                if ui.button(t("📂 Open settings folder", "📂 فتح مجلد الإعدادات", lang)).clicked() {
                    if let Some(dir) = self.config_path.parent() {
                        let _ = std::process::Command::new("explorer.exe").arg(dir).spawn();
                    }
                }
                if ui.button(t("↻ Reload saved settings", "↻ إعادة تحميل الإعدادات", lang)).clicked() {
                    match Config::try_load(&self.config_path) {
                        Ok(config) => {
                            self.baseline = config.clone();
                            self.working = config;
                            self.hotkey_text = self.working.toggle_hotkey.clone();
                            self.lang = resolve_lang(&self.working.gui_language);
                            self.say("Reloaded from disk", "تمت إعادة التحميل");
                        }
                        Err(error) => self.fail("Reload failed", "فشلت إعادة التحميل", error),
                    }
                }
                if ui.button(t("⟲ Restore all defaults", "⟲ استعادة كل الافتراضيات", lang)).clicked() {
                    self.confirm = Some(Confirm::RestoreDefaults);
                }
            });
        });
    }

    fn page_about(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        card(ui, lang, "Reverse Controller", &format!("{} {}", t("Version", "الإصدار", lang), env!("CARGO_PKG_VERSION")), |ui| {
            ui.label(t(
                "Use a game controller as a complete mouse and keyboard for Windows: pointer, scrolling, Arabic and English typing, window management and hundreds of shortcuts.",
                "استخدم يد الألعاب كماوس وكيبورد كاملين لويندوز: المؤشر والتمرير والكتابة بالعربية والإنجليزية وإدارة النوافذ ومئات الاختصارات.",
                lang,
            ));
            ui.add_space(6.0);
            ui.label(egui::RichText::new(t(
                "🔒 Private: no internet connection, no telemetry. Settings and learned words stay on this PC.",
                "🔒 خصوصية: بدون إنترنت وبدون جمع بيانات. الإعدادات والكلمات المحفوظة تبقى على هذا الجهاز.",
                lang,
            )).color(C_GOOD));
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button(t("🌐 Project page", "🌐 صفحة المشروع", lang)).clicked() {
                    let _ = std::process::Command::new("explorer.exe").arg("https://github.com/m7kor/Reverse-Controller").spawn();
                }
                ui.label(egui::RichText::new(t("MIT License", "رخصة MIT", lang)).color(C_DIM));
            });
        });
    }
}

/// `.amgp` files in the user's Downloads and Desktop folders.
fn find_amgp_files() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for dir in ["Downloads", "Desktop", "Documents"] {
        if let Ok(entries) = std::fs::read_dir(home.join(dir)) {
            for e in entries.flatten() {
                if e.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("amgp")) {
                    found.push(e.path());
                }
            }
        }
    }
    found.sort();
    found
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        static THEMED: AtomicBool = AtomicBool::new(false);
        if !THEMED.load(Ordering::Relaxed) && crate::platform::enforce_window_modern_theme(SETTINGS_TITLE) {
            THEMED.store(true, Ordering::Relaxed);
        }
        self.frame_count = self.frame_count.saturating_add(1);
        if self.frame_count == 3 || self.frame_count.is_multiple_of(900) {
            crate::platform::trim_working_set();
        }

        // Ask before closing with unsaved edits.
        if ctx.input(|i| i.viewport().close_requested()) && self.is_dirty() && !self.allow_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.confirm = Some(Confirm::CloseUnsaved);
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) && self.confirm.is_none() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::S)) && self.is_dirty() {
            self.commit();
        }

        let live = self.live();
        if self.calibration_result.is_some_and(|(slot, _)| slot != live.slot) || live.pad.is_none() {
            self.calibration_result = None;
        }
        if let Some((started, calibration)) = &mut self.calibration {
            let axes = live.pad.map(|p| [p.sThumbLX, p.sThumbLY, p.sThumbRX, p.sThumbRY]);
            match calibration.observe(started.elapsed(), live.slot, axes) {
                Ok(Some(result)) => {
                    self.calibration_result = Some((live.slot, result));
                    self.calibration = None;
                    self.say("Calibration complete. Review the suggested dead zones.", "اكتملت المعايرة. راجع مناطق الخمول المقترحة.");
                }
                Err(error) => {
                    self.status = error.into();
                    self.status_error = true;
                    self.calibration = None;
                }
                Ok(None) => {}
            }
        }

        let sidebar_frame = egui::Frame::none().fill(C_SIDEBAR).inner_margin(egui::Margin::symmetric(14.0, 6.0));
        let sidebar = if self.lang == GuiLang::Ar {
            egui::SidePanel::right("nav")
        } else {
            egui::SidePanel::left("nav")
        };
        sidebar.resizable(false).exact_width(220.0).frame(sidebar_frame).show(ctx, |ui| self.sidebar(ui, &live));

        let bar = egui::Frame::none().fill(C_BG).inner_margin(egui::Margin::symmetric(22.0, 4.0));
        egui::TopBottomPanel::bottom("footer").frame(bar).show(ctx, |ui| self.footer(ui, ctx));
        egui::TopBottomPanel::top("header").frame(bar).show(ctx, |ui| self.header(ui, &live));
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(C_BG).inner_margin(egui::Margin::symmetric(22.0, 8.0)))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().id_source(self.page).auto_shrink([false, false]).show(ui, |ui| {
                    match self.page {
                        SettingsPage::Home => self.page_home(ui, &live),
                        SettingsPage::Tutorial => self.page_tutorial(ui, &live),
                        SettingsPage::Pointer => self.page_pointer(ui),
                        SettingsPage::Controller => self.page_controller(ui, &live),
                        SettingsPage::Keyboard => self.page_keyboard(ui),
                        SettingsPage::Buttons => self.page_buttons(ui),
                        SettingsPage::Profiles => self.page_profiles(ui),
                        SettingsPage::General => self.page_general(ui),
                        SettingsPage::About => self.page_about(ui),
                    }
                    ui.add_space(12.0);
                });
            });
        self.confirm_dialog(ctx);

        // Redraw continuously only while something live is on screen.
        let live_view = matches!(self.page, SettingsPage::Controller | SettingsPage::Tutorial) && live.pad.is_some();
        if self.calibration.is_some() || live_view || self.wizard.is_some() {
            ctx.request_repaint_after(Duration::from_millis(50));
        } else {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        SETTINGS_OPEN.store(false, Ordering::Relaxed);
    }
}

const SETTINGS_TITLE: &str = "Reverse Controller — Settings";

/// Run the settings GUI window directly (used when invoked with --settings in a dedicated process).
pub fn run_settings_standalone(engine: Arc<Mutex<Engine>>, config_path: PathBuf) {
    crate::platform::attach_to_interactive_desktop();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1000.0, 740.0])
            .with_min_inner_size([780.0, 560.0])
            .with_title(SETTINGS_TITLE),
        ..Default::default()
    };
    let eng = engine.clone();
    let path = config_path.clone();
    let result = eframe::run_native(
        SETTINGS_TITLE,
        options,
        Box::new(move |cc| {
            crate::vkeyboard::install_fonts(&cc.egui_ctx);
            apply_theme(&cc.egui_ctx);
            crate::platform::trim_working_set();
            Box::new(SettingsApp::new(eng.clone(), path.clone()))
        }),
    );
    if let Err(e) = result {
        eprintln!("[gui] eframe error: {e}");
    }
}

/// Open the settings window in a clean separate process to avoid winit's EventLoop collision with the virtual keyboard.
pub fn open_settings(_engine: Arc<Mutex<Engine>>, config_path: PathBuf) {
    open_settings_process(&config_path);
}

/// Start the settings window as its own process (no elevation prompt). If a
/// settings window is already open, bring it to the front instead.
pub fn open_settings_process(config_path: &std::path::Path) {
    open_settings_on(config_path, None);
}

/// Like [`open_settings_process`], opening on `page` (e.g. "tutorial").
pub fn open_settings_on(config_path: &std::path::Path, page: Option<&str>) {
    if crate::platform::focus_window_titled(SETTINGS_TITLE) {
        return;
    }
    if let Ok(exe) = std::env::current_exe() {
        let mut command = std::process::Command::new(exe);
        command.arg("--settings").arg("--no-elevate").arg("--config").arg(config_path);
        if let Some(page) = page {
            command.env("CC_SETTINGS_PAGE", page);
        }
        let _ = command.spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn every_catalog_entry_has_both_languages_and_a_unique_action() {
        let catalog = action_catalog();
        assert!(catalog.len() >= 70, "the shortcut library should stay comprehensive");
        for (i, (_, en, ar_str, act)) in catalog.iter().enumerate() {
            assert!(!en.is_empty() && !ar_str.is_empty());
            assert!(!en.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)), "Arabic in English label {en}");
            assert!(
                !catalog[..i].iter().any(|(_, _, _, other)| same_shortcut(other, act)),
                "duplicate action {act:?}"
            );
            assert_eq!(format_action(act, GuiLang::En), *en);
        }
        for group in ActionGroup::ALL {
            assert!(catalog.iter().any(|(g, ..)| *g == group), "{group:?} is empty");
        }
    }

    #[test]
    fn default_bindings_all_have_friendly_names() {
        for b in crate::bindings::default_profile() {
            let label = format_action(&b.action, GuiLang::En);
            assert!(!label.contains("Combo") && !label.starts_with("Key:"), "unfriendly label {label}");
        }
        assert_eq!(format_action(&Action::Combo(vec![Key::LControl, Key::LShift, Key::D5]), GuiLang::En), "Ctrl+Shift+5");
    }

    #[test]
    fn tutorial_steps_tick_off_from_real_controller_actions() {
        let mut done = [false; TUTORIAL_STEPS];
        let idle = crate::win32::XINPUT_GAMEPAD::default();
        tutorial_progress(&idle, &mut done);
        assert_eq!(done, [false; TUTORIAL_STEPS], "resting does nothing");
        tutorial_progress(&crate::win32::XINPUT_GAMEPAD { sThumbLX: 30000, ..idle }, &mut done);
        assert!(done[0]);
        tutorial_progress(&crate::win32::XINPUT_GAMEPAD { wButtons: 0x1000, ..idle }, &mut done);
        assert!(done[1]);
        tutorial_progress(&crate::win32::XINPUT_GAMEPAD { sThumbRY: -30000, ..idle }, &mut done);
        assert!(done[2]);
        tutorial_progress(&crate::win32::XINPUT_GAMEPAD { bLeftTrigger: 255, sThumbLY: 15000, ..idle }, &mut done);
        assert!(done[3]);
        tutorial_progress(&crate::win32::XINPUT_GAMEPAD { wButtons: 0x0080, ..idle }, &mut done);
        assert!(!done[5], "R3 alone is not the quick menu");
        tutorial_progress(&crate::win32::XINPUT_GAMEPAD { wButtons: 0x00A0, ..idle }, &mut done);
        assert!(done[5]);
        tutorial_progress(&idle, &mut done);
        assert!(done[0] && done[5], "steps stay done");
        assert!(!done[4]);
    }

    #[test]
    fn language_setting_resolves_explicit_choices() {
        assert_eq!(resolve_lang("ar"), GuiLang::Ar);
        assert_eq!(resolve_lang("EN"), GuiLang::En);
    }

    #[test]
    fn save_preserves_external_changes_to_untouched_fields() {
        let original = json!({"enabled": true, "active_profile": "Default", "mouse": {"speed": 600, "acceleration": 800}});
        let edited = json!({"enabled": true, "active_profile": "Default", "mouse": {"speed": 750, "acceleration": 800}});
        let current = json!({"enabled": false, "active_profile": "Browser", "mouse": {"speed": 600, "acceleration": 400}});
        let merged = merge_edits(&original, &edited, &current);
        assert_eq!(merged, json!({"enabled": false, "active_profile": "Browser", "mouse": {"speed": 750, "acceleration": 400}}));
    }

    #[test]
    fn test_gui_language_localization_pure_english_and_arabic() {
        // In English mode, format_input and format_action must NOT contain any Arabic script
        let en_input = format_input(Input::DPadUp, GuiLang::En);
        assert_eq!(en_input, "D-Pad Up");
        assert!(!en_input.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)));

        let en_act = format_action(&Action::MouseClick(crate::mouse::MouseButton::Left), GuiLang::En);
        assert_eq!(en_act, "Left Click");
        assert!(!en_act.chars().any(|c| ('\u{0600}'..='\u{06FF}').contains(&c)));

        // In Arabic mode, format_input and format_action must return valid Arabic text
        let ar_input = format_input(Input::DPadUp, GuiLang::Ar);
        assert!(ar_input.contains(&ar("سهم لأعلى")));

        let ar_act = format_action(&Action::MouseClick(crate::mouse::MouseButton::Left), GuiLang::Ar);
        assert_eq!(ar_act, ar("النقر الأيسر بالماوس"));
    }
}
