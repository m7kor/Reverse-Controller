//! Native settings window built with egui/eframe.
//!
//! Lets you edit every tunable (mouse speed/acceleration/wheel, stick dead
//! zones, trigger threshold, controller slot), toggle enable, view controller
//! status/battery, and view the full binding table. Save applies live to the
//! running engine and persists to disk.

#![cfg(feature = "gui")]

use crate::actions::{Action, DisplayMode, Modifier};
use crate::bindings::Input;
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

fn format_action(action: &Action, lang: GuiLang) -> String {
    match lang {
        GuiLang::En => match action {
            Action::MouseClick(MouseButton::Left) => "Left Click".into(),
            Action::MouseClick(MouseButton::Right) => "Right Click".into(),
            Action::MouseClick(MouseButton::Middle) => "Middle Click".into(),
            Action::MouseClick(MouseButton::X1) => "Mouse X1 (Side 1)".into(),
            Action::MouseClick(MouseButton::X2) => "Mouse X2 (Side 2)".into(),
            Action::ToggleVirtualKeyboard => "Toggle On-screen Keyboard".into(),
            Action::MouseMove { .. } => "Mouse Pointer".into(),
            Action::MouseScroll { flip_y } => {
                if *flip_y {
                    "Scroll Wheel (Inverted Y)".into()
                } else {
                    "Scroll Wheel".into()
                }
            }
            Action::Key { key: Key::MediaPlayPause, .. } => "Media Play/Pause".into(),
            Action::Key { key: Key::MediaPreviousTrack, .. } => "Previous Track".into(),
            Action::Key { key: Key::MediaNextTrack, .. } => "Next Track".into(),
            Action::Key { key: Key::Backspace, .. } => "Backspace".into(),
            Action::Key { key: Key::Enter, .. } => "Enter".into(),
            Action::Key { key: Key::LControl, .. } => "Ctrl Modifier".into(),
            Action::Key { key: Key::LShift, .. } => "Shift Modifier".into(),
            Action::Key { key: Key::Escape, .. } => "Escape".into(),
            Action::Key { key: Key::VolumeUp, .. } => "Volume Up".into(),
            Action::Key { key: Key::VolumeDown, .. } => "Volume Down".into(),
            Action::Key { key: Key::Left, .. } => "Arrow Left".into(),
            Action::Key { key: Key::Right, .. } => "Arrow Right".into(),
            Action::Key { key: Key::Up, .. } => "Arrow Up".into(),
            Action::Key { key: Key::Down, .. } => "Arrow Down".into(),
            Action::Key { key: Key::LWin, .. } => "Windows Start Menu".into(),
            Action::Key { key, .. } => format!("Key: {key:?}"),
            Action::DisplaySwitch(DisplayMode::External) => "Display Switch (External)".into(),
            Action::DisplaySwitch(DisplayMode::Internal) => "Display Switch (Internal)".into(),
            Action::DisplaySwitch(mode) => format!("Display Switch ({mode:?})"),
            Action::TurnOffController => "Turn Off Controller".into(),
            Action::Combo(keys) => {
                if keys.contains(&Key::LWin) && keys.contains(&Key::M) {
                    "Show Desktop (Win+M)".into()
                } else if keys.contains(&Key::Tab) {
                    "Task View / Window Switch".into()
                } else {
                    format!("Key Combo: {keys:?}")
                }
            }
            Action::Modifier(Modifier::Back) => "Hold Modifier (Back)".into(),
            Action::ShowDesktop => "Show Desktop (Win+D)".into(),
            Action::ToggleEnabled => "Toggle Desktop Control".into(),
            Action::ToggleDragLock => "Toggle Drag-Lock".into(),
            Action::VoiceDictation => "Voice Dictation (Win+H)".into(),
            Action::TaskView => "Task View (Win+Tab)".into(),
            Action::SnapWindowLeft => "Snap Window Left".into(),
            Action::SnapWindowRight => "Snap Window Right".into(),
            Action::MaximizeWindow => "Maximize Window".into(),
            Action::MinimizeWindow => "Minimize Window".into(),
            Action::CloseActiveWindow => "Close Window (Alt+F4)".into(),
            Action::NextVirtualDesktop => "Next Virtual Desktop".into(),
            Action::PrevVirtualDesktop => "Prev Virtual Desktop".into(),
            Action::RadialMenu => "Radial Menu".into(),
            Action::WindowDrag => "Window Drag & Move (AltSnap)".into(),
            Action::ActivateProfile(n) => format!("Activate Profile: {n}"),
            Action::Excluded(s) => format!("Excluded ({s})"),
            Action::DoNothing => "Disabled".into(),
        },
        GuiLang::Ar => match action {
            Action::MouseClick(MouseButton::Left) => ar("النقر الأيسر بالماوس"),
            Action::MouseClick(MouseButton::Right) => ar("النقر الأيمن بالماوس"),
            Action::MouseClick(MouseButton::Middle) => ar("النقر الأوسط بالماوس"),
            Action::MouseClick(MouseButton::X1) => ar("زر الماوس الجانبي 1"),
            Action::MouseClick(MouseButton::X2) => ar("زر الماوس الجانبي 2"),
            Action::ToggleVirtualKeyboard => ar("فتح/إغلاق الكيبورد الافتراضي"),
            Action::MouseMove { .. } => ar("تحريك مؤشر الفأرة"),
            Action::MouseScroll { flip_y } => {
                if *flip_y {
                    ar("التمرير معكوس عمودياً")
                } else {
                    ar("التمرير بالبكرة لأعلى/أسفل")
                }
            }
            Action::Key { key: Key::MediaPlayPause, .. } => ar("تشغيل/إيقاف الوسائط"),
            Action::Key { key: Key::MediaPreviousTrack, .. } => ar("المقطع السابق"),
            Action::Key { key: Key::MediaNextTrack, .. } => ar("المقطع التالي"),
            Action::Key { key: Key::Backspace, .. } => ar("مسح للخلف (Backspace)"),
            Action::Key { key: Key::Enter, .. } => ar("إدخال (Enter)"),
            Action::Key { key: Key::LControl, .. } => ar("زر التحكم (Ctrl)"),
            Action::Key { key: Key::LShift, .. } => ar("زر التبديل (Shift)"),
            Action::Key { key: Key::Escape, .. } => ar("إلغاء / خروج (Esc)"),
            Action::Key { key: Key::VolumeUp, .. } => ar("رفع الصوت"),
            Action::Key { key: Key::VolumeDown, .. } => ar("خفض الصوت"),
            Action::Key { key: Key::Left, .. } => ar("سهم يسار"),
            Action::Key { key: Key::Right, .. } => ar("سهم يمين"),
            Action::Key { key: Key::Up, .. } => ar("سهم لأعلى"),
            Action::Key { key: Key::Down, .. } => ar("سهم لأسفل"),
            Action::Key { key: Key::LWin, .. } => ar("قائمة ابدأ (Windows)"),
            Action::Key { key, .. } => format!("Key: {key:?}"),
            Action::DisplaySwitch(DisplayMode::External) => ar("تبديل إلى الشاشة الخارجية"),
            Action::DisplaySwitch(DisplayMode::Internal) => ar("تبديل إلى شاشة الكمبيوتر"),
            Action::DisplaySwitch(mode) => format!("Display Switch ({mode:?})"),
            Action::TurnOffController => ar("إيقاف تشغيل يد التحكم"),
            Action::Combo(keys) => {
                if keys.contains(&Key::LWin) && keys.contains(&Key::M) {
                    ar("عرض سطح المكتب (Win+M)")
                } else if keys.contains(&Key::Tab) {
                    ar("عرض المهام / تبديل النوافذ")
                } else {
                    format!("Key Combo: {keys:?}")
                }
            }
            Action::Modifier(Modifier::Back) => ar("زر الوظائف الإضافية (Back)"),
            Action::ShowDesktop => ar("عرض سطح المكتب (Win+D)"),
            Action::ToggleEnabled => ar("تشغيل/إيقاف التحكم"),
            Action::ToggleDragLock => ar("قفل السحب والنقر"),
            Action::VoiceDictation => ar("الكتابة الصوتية (Win+H)"),
            Action::TaskView => ar("عرض كافة المهام والنوافذ"),
            Action::SnapWindowLeft => ar("محاذاة النافذة لليسار"),
            Action::SnapWindowRight => ar("محاذاة النافذة لليمين"),
            Action::MaximizeWindow => ar("تكبير النافذة"),
            Action::MinimizeWindow => ar("تصغير النافذة"),
            Action::CloseActiveWindow => ar("إغلاق النافذة الحالية (Alt+F4)"),
            Action::NextVirtualDesktop => ar("سطح المكتب الافتراضي التالي"),
            Action::PrevVirtualDesktop => ar("سطح المكتب الافتراضي السابق"),
            Action::RadialMenu => ar("القائمة الدائرية السريعة"),
            Action::WindowDrag => ar("تحريك وسحب النوافذ (AltSnap)"),
            Action::ActivateProfile(n) => format!("{}: {n}", ar("تفعيل بروفايل")),
            Action::Excluded(s) => format!("Excluded ({s})"),
            Action::DoNothing => ar("معطل"),
        },
    }
}

fn available_actions(lang: GuiLang) -> Vec<(String, Action)> {
    match lang {
        GuiLang::En => vec![
            ("Left Click".into(), Action::MouseClick(MouseButton::Left)),
            ("Right Click".into(), Action::MouseClick(MouseButton::Right)),
            ("Middle Click".into(), Action::MouseClick(MouseButton::Middle)),
            ("Mouse X1 (Side 1)".into(), Action::MouseClick(MouseButton::X1)),
            ("Mouse X2 (Side 2)".into(), Action::MouseClick(MouseButton::X2)),
            ("Window Drag & Move (AltSnap)".into(), Action::WindowDrag),
            ("Toggle Drag-Lock".into(), Action::ToggleDragLock),
            ("Toggle On-screen Keyboard".into(), Action::ToggleVirtualKeyboard),
            ("Voice Dictation (Win+H)".into(), Action::VoiceDictation),
            ("Radial Menu".into(), Action::RadialMenu),
            ("Task View (Win+Tab)".into(), Action::TaskView),
            ("Show Desktop (Win+D)".into(), Action::ShowDesktop),
            ("Close Window (Alt+F4)".into(), Action::CloseActiveWindow),
            ("Snap Window Left (Win+Left)".into(), Action::SnapWindowLeft),
            ("Snap Window Right (Win+Right)".into(), Action::SnapWindowRight),
            ("Maximize Window (Win+Up)".into(), Action::MaximizeWindow),
            ("Minimize Window (Win+Down)".into(), Action::MinimizeWindow),
            ("Next Desktop (Win+Ctrl+Right)".into(), Action::NextVirtualDesktop),
            ("Prev Desktop (Win+Ctrl+Left)".into(), Action::PrevVirtualDesktop),
            ("Volume Up".into(), Action::Key { key: Key::VolumeUp, repeat: false }),
            ("Volume Down".into(), Action::Key { key: Key::VolumeDown, repeat: false }),
            ("Media Play/Pause".into(), Action::Key { key: Key::MediaPlayPause, repeat: false }),
            ("Media Next Track".into(), Action::Key { key: Key::MediaNextTrack, repeat: false }),
            ("Media Prev Track".into(), Action::Key { key: Key::MediaPreviousTrack, repeat: false }),
            ("Backspace".into(), Action::Key { key: Key::Backspace, repeat: true }),
            ("Enter".into(), Action::Key { key: Key::Enter, repeat: false }),
            ("Escape".into(), Action::Key { key: Key::Escape, repeat: false }),
            ("Ctrl Modifier".into(), Action::Key { key: Key::LControl, repeat: false }),
            ("Shift Modifier".into(), Action::Key { key: Key::LShift, repeat: false }),
            ("Windows Start Menu".into(), Action::Key { key: Key::LWin, repeat: false }),
            ("Turn Off Controller".into(), Action::TurnOffController),
            ("Display Switch (External)".into(), Action::DisplaySwitch(DisplayMode::External)),
            ("Display Switch (Internal)".into(), Action::DisplaySwitch(DisplayMode::Internal)),
            ("Disabled".into(), Action::DoNothing),
        ],
        GuiLang::Ar => vec![
            (ar("النقر الأيسر بالماوس"), Action::MouseClick(MouseButton::Left)),
            (ar("النقر الأيمن بالماوس"), Action::MouseClick(MouseButton::Right)),
            (ar("النقر الأوسط بالماوس"), Action::MouseClick(MouseButton::Middle)),
            (ar("زر الماوس الجانبي 1"), Action::MouseClick(MouseButton::X1)),
            (ar("زر الماوس الجانبي 2"), Action::MouseClick(MouseButton::X2)),
            (ar("تحريك وسحب النوافذ (AltSnap)"), Action::WindowDrag),
            (ar("قفل السحب والنقر"), Action::ToggleDragLock),
            (ar("فتح/إغلاق الكيبورد الافتراضي"), Action::ToggleVirtualKeyboard),
            (ar("الكتابة والإملاء الصوتي (Win+H)"), Action::VoiceDictation),
            (ar("القائمة الدائرية السريعة"), Action::RadialMenu),
            (ar("عرض المهام (Win+Tab)"), Action::TaskView),
            (ar("عرض سطح المكتب (Win+D)"), Action::ShowDesktop),
            (ar("إغلاق النافذة الحالية (Alt+F4)"), Action::CloseActiveWindow),
            (ar("محاذاة النافذة لليسار (Win+Left)"), Action::SnapWindowLeft),
            (ar("محاذاة النافذة لليمين (Win+Right)"), Action::SnapWindowRight),
            (ar("تكبير النافذة (Win+Up)"), Action::MaximizeWindow),
            (ar("تصغير النافذة (Win+Down)"), Action::MinimizeWindow),
            (ar("سطح المكتب التالي (Win+Ctrl+Right)"), Action::NextVirtualDesktop),
            (ar("سطح المكتب السابق (Win+Ctrl+Left)"), Action::PrevVirtualDesktop),
            (ar("رفع الصوت"), Action::Key { key: Key::VolumeUp, repeat: false }),
            (ar("خفض الصوت"), Action::Key { key: Key::VolumeDown, repeat: false }),
            (ar("تشغيل/إيقاف الوسائط"), Action::Key { key: Key::MediaPlayPause, repeat: false }),
            (ar("المقطع التالي"), Action::Key { key: Key::MediaNextTrack, repeat: false }),
            (ar("المقطع السابق"), Action::Key { key: Key::MediaPreviousTrack, repeat: false }),
            (ar("مسح للخلف (Backspace)"), Action::Key { key: Key::Backspace, repeat: true }),
            (ar("إدخال (Enter)"), Action::Key { key: Key::Enter, repeat: false }),
            (ar("إلغاء / خروج (Esc)"), Action::Key { key: Key::Escape, repeat: false }),
            (ar("زر التحكم (Ctrl)"), Action::Key { key: Key::LControl, repeat: false }),
            (ar("زر التبديل (Shift)"), Action::Key { key: Key::LShift, repeat: false }),
            (ar("قائمة ابدأ (Windows)"), Action::Key { key: Key::LWin, repeat: false }),
            (ar("إيقاف تشغيل يد التحكم"), Action::TurnOffController),
            (ar("الشاشة الخارجية"), Action::DisplaySwitch(DisplayMode::External)),
            (ar("شاشة الكمبيوتر"), Action::DisplaySwitch(DisplayMode::Internal)),
            (ar("معطل"), Action::DoNothing),
        ],
    }
}

fn action_picker(ui: &mut egui::Ui, id: impl std::hash::Hash, current: &mut Action, lang: GuiLang) {
    let current_label = format_action(current, lang);
    egui::ComboBox::from_id_source(id)
        .selected_text(current_label)
        .width(280.0)
        .show_ui(ui, |ui| {
            for (label, act) in available_actions(lang) {
                let is_sel = current == &act;
                if ui.selectable_label(is_sel, label).clicked() {
                    *current = act.clone();
                }
            }
        });
}

static SETTINGS_OPEN: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum SettingsPage {
    Overview,
    Pointer,
    Keyboard,
    Profiles,
    Bindings,
    Voice,
}

struct SettingsApp {
    engine: Arc<Mutex<Engine>>,
    config_path: PathBuf,
    working: Config,
    baseline: Config,
    status: String,
    active_profile: String,
    /// Profile directory, re-read whenever the user asks for a refresh.
    profiles_dir: PathBuf,
    /// Cached profile list plus the text fields used to create a new one.
    profile_list: Vec<crate::profiles::ProfileEntry>,
    new_profile_name: String,
    new_profile_processes: String,
    /// Set while the hotkey text box holds something that cannot be parsed.
    hotkey_error: bool,
    page: SettingsPage,
    calibration: Option<(Instant, crate::calibration::RestCalibration)>,
    calibration_result: Option<(u32, [u16; 2])>,
    runtime_status: Option<crate::runtime_status::RuntimeStatus>,
    last_status_check: Instant,
    last_status_seen: Instant,
    custom_vocab_input: String,
    lang: GuiLang,
    frame_count: u32,
}

impl SettingsApp {
    fn new(engine: Arc<Mutex<Engine>>, config_path: PathBuf) -> Self {
        let working = engine
            .lock()
            .map(|e| e.config().clone())
            .unwrap_or_default();
        let dir = crate::config::profiles_dir();
        let list = crate::profiles::list(&dir);
        let active_profile = if working.active_profile.is_empty() {
            crate::profiles::DEFAULT_NAME.to_string()
        } else {
            working.active_profile.clone()
        };
        let custom_vocab_input = working.voice.custom_vocabulary.join(", ");
        let lang = if working.gui_language.eq_ignore_ascii_case("ar") {
            GuiLang::Ar
        } else {
            GuiLang::En
        };
        SettingsApp {
            engine,
            config_path,
            baseline: working.clone(),
            working,
            status: String::new(),
            active_profile,
            profiles_dir: dir,
            profile_list: list,
            new_profile_name: String::new(),
            new_profile_processes: String::new(),
            hotkey_error: false,
            page: SettingsPage::Overview,
            calibration: None,
            calibration_result: None,
            runtime_status: None,
            last_status_check: Instant::now() - Duration::from_secs(1),
            last_status_seen: Instant::now() - Duration::from_secs(10),
            custom_vocab_input,
            lang,
            frame_count: 0,
        }
    }

    fn refresh_profiles(&mut self) {
        self.profile_list = crate::profiles::list(&self.profiles_dir);
    }

    fn toggle_desktop_control(&mut self, enabled: bool) {
        // Only change this setting. The background process may have switched
        // profiles while this separate settings window was open.
        let mut latest = if self.config_path.exists() {
            match Config::try_load(&self.config_path) {
                Ok(config) => config,
                Err(error) => {
                    self.status = format!("Could not read settings: {error}");
                    return;
                }
            }
        } else {
            Config::default()
        };
        latest.enabled = enabled;
        match latest.save(&self.config_path) {
            Ok(()) => {
                self.working.enabled = enabled;
                self.baseline.enabled = enabled;
                if let Ok(mut engine) = self.engine.lock() {
                    engine.apply_config(latest);
                }
                if let Some(snapshot) = self.runtime_status.as_mut() {
                    snapshot.enabled = enabled;
                    snapshot.mode = crate::runtime_status::RuntimeMode::from_state(
                        snapshot.connected,
                        enabled,
                        snapshot.mode == crate::runtime_status::RuntimeMode::PausedFullscreen,
                    );
                }
                self.status = if enabled {
                    t("Desktop control turned on", "تم تشغيل التحكم المكتبي", self.lang)
                } else {
                    t("Desktop control turned off", "تم إيقاف التحكم المكتبي", self.lang)
                };
            }
            Err(error) => self.status = format!("{}: {error}", t("Could not change desktop control", "فشل تغيير حالة التحكم المكتبي", self.lang)),
        }
    }

    /// Persist the working config and push it into the running engine.
    fn commit(&mut self) -> bool {
        let latest = if self.config_path.exists() {
            match Config::try_load(&self.config_path) {
                Ok(config) => config,
                Err(error) => {
                    self.status = format!("{}: {error}", t("Save stopped", "توقف الحفظ", self.lang));
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
            self.status = t("Could not prepare settings for saving", "تعذر تحضير الإعدادات للحفظ", self.lang);
            return false;
        };
        let mut merged: Config = match serde_json::from_value(merge_edits(&before, &edited, &current)) {
            Ok(config) => config,
            Err(error) => {
                self.status = format!("{}: {error}", t("Could not merge settings", "تعذر دمج الإعدادات", self.lang));
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
                self.active_profile = if self.working.active_profile.is_empty() {
                    crate::profiles::DEFAULT_NAME.to_string()
                } else {
                    self.working.active_profile.clone()
                };
                self.status = t(
                    &format!("Saved to {}", self.config_path.display()),
                    &format!("{}: {}", ar("تم الحفظ بنجاح في"), self.config_path.display()),
                    self.lang,
                );
                #[cfg(feature = "voice")]
                crate::voice::set_voice_settings(self.working.voice.clone());
                true
            }
            Err(e) => {
                self.status = format!("{}: {e}", t("Save failed", "فشل الحفظ", self.lang));
                false
            }
        }
    }

    /// Switch to `stem`, loading its bindings into the working config.
    ///
    /// Goes through `Engine::set_profile` rather than `apply_config` so the
    /// switch survives: `apply_config` replaces the whole config, which would
    /// reset the mouse-shape state the engine carries between frames.
    fn activate_profile(&mut self, stem: &str) {
        let (bindings, clear_name) = if stem == crate::profiles::DEFAULT_NAME {
            (crate::bindings::default_profile(), true)
        } else {
            let path = self.profiles_dir.join(format!("{stem}.json"));
            match crate::profiles::load(&path) {
                Ok(p) => (p.bindings, false),
                Err(e) => {
                    self.status = format!("{}: {e}", t(&format!("Could not load {stem}"), &format!("{} {stem}", ar("تعذر تحميل البروفايل")), self.lang));
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
            self.status = if clear_name {
                t("Default profile activated", "تم تفعيل البروفايل الافتراضي", self.lang)
            } else {
                t(&format!("Profile '{stem}' activated"), &format!("{} '{stem}'", ar("تم تفعيل البروفايل")), self.lang)
            };
        }
    }
}

impl eframe::App for SettingsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        static THEMED: AtomicBool = AtomicBool::new(false);
        if !THEMED.load(Ordering::Relaxed)
            && crate::platform::enforce_window_modern_theme("Controller Companion — Settings")
        {
            THEMED.store(true, Ordering::Relaxed);
        }

        self.frame_count = self.frame_count.saturating_add(1);
        if self.frame_count == 3 {
            crate::platform::trim_working_set();
        }

        if self.last_status_check.elapsed() >= Duration::from_millis(250) {
            let path = crate::runtime_status::path_for_config(&self.config_path);
            if let Some(snapshot) = crate::runtime_status::read_recent(&path) {
                self.runtime_status = Some(snapshot);
                self.last_status_seen = Instant::now();
            } else if self.last_status_seen.elapsed() >= Duration::from_secs(3) {
                self.runtime_status = None;
            }
            if self.last_status_check.elapsed() >= Duration::from_secs(15) {
                crate::platform::trim_working_set();
            }
            self.last_status_check = Instant::now();
        }

        let (local_connected, local_enabled, local_battery, gp_state, local_profile) = self
            .engine
            .lock()
            .map(|e| {
                let name = &e.config().active_profile;
                let controller_index = self.runtime_status.as_ref()
                    .map(|snapshot| snapshot.controller_index)
                    .unwrap_or(self.working.controller.controller_index);
                (
                    e.is_connected(),
                    e.is_enabled(),
                    e.battery_info(),
                    e.gamepad_state_for(controller_index),
                    if name.is_empty() { crate::profiles::DEFAULT_NAME.to_string() } else { name.clone() },
                )
            })
            .unwrap_or((false, false, None, None, crate::profiles::DEFAULT_NAME.to_string()));
        let runtime_snapshot = self.runtime_status.clone();
        let live = runtime_snapshot.as_ref();
        let connected = live.map_or(local_connected, |s| s.connected);
        let enabled = live.map_or(local_enabled, |s| s.enabled);
        let battery = live
            .and_then(|s| s.battery_level.map(|level| (0, level)))
            .or(local_battery);
        let active_profile = live
            .map(|s| s.active_profile.as_str())
            .unwrap_or(&local_profile);
        let runtime_mode = live.map(|s| s.mode);
        let calibration_slot = live.map_or(self.working.controller.controller_index, |s| s.controller_index);
        if self.calibration_result.is_some_and(|(slot, _)| slot != calibration_slot) || gp_state.is_none() {
            self.calibration_result = None;
        }
        if let Some((started, calibration)) = &mut self.calibration {
            let axes = gp_state.map(|p| [p.sThumbLX, p.sThumbLY, p.sThumbRX, p.sThumbRY]);
            match calibration.observe(started.elapsed(), calibration_slot, axes) {
                Ok(Some(result)) => {
                    self.calibration_result = Some((calibration_slot, result));
                    self.calibration = None;
                    self.status = t("Calibration complete. Review the suggested dead zones on the Pointer page.", "اكتملت المعايرة. راجع مناطق الخمول المقترحة في صفحة الماوس.", self.lang);
                }
                Err(error) => { self.status = error.into(); self.calibration = None; }
                Ok(None) => {}
            }
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        egui::TopBottomPanel::top("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(t("Controller Companion", "مساعد يد التحكم", self.lang));
                ui.separator();
                let conn = if connected {
                    t("Controller connected", "يد التحكم متصلة", self.lang)
                } else {
                    t("No controller connected", "لا توجد يد متصلة", self.lang)
                };
                let col = if connected { egui::Color32::GREEN } else { egui::Color32::GRAY };
                ui.colored_label(col, conn);
                if let Some((_t, l)) = battery {
                    let level = match l {
                        0 => t("Empty", "فارغة", self.lang),
                        1 => t("Low", "منخفضة", self.lang),
                        2 => t("Medium", "متوسطة", self.lang),
                        3 => t("Full", "ممتلئة", self.lang),
                        _ => t("Unknown", "غير معروفة", self.lang),
                    };
                    ui.label(format!("{}: {level}", t("Battery", "البطارية", self.lang)));
                }
                ui.separator();
                let is_on = enabled;
                let toggle_label = if is_on {
                    t("Desktop Control ON (Click to Pause)", "التحكم شغال (انقر للإيقاف)", self.lang)
                } else {
                    t("Desktop Control OFF (Click to Resume)", "التحكم موقوف (انقر للتشغيل)", self.lang)
                };
                let toggle_col = if is_on { egui::Color32::from_rgb(0, 160, 60) } else { egui::Color32::from_rgb(190, 40, 40) };
                let btn = egui::Button::new(egui::RichText::new(toggle_label).color(egui::Color32::WHITE).strong())
                    .fill(toggle_col);
                if ui.add(btn).clicked() {
                    self.toggle_desktop_control(!enabled);
                }

                // Prominent language switcher button on the side (User request: "وسو علامه عالجنب تحولها عربي بالكامل")
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (flag, label) = match self.lang {
                        GuiLang::En => ("🇸🇦", "العربية"),
                        GuiLang::Ar => ("🌐", "English"),
                    };
                    let lang_btn = egui::Button::new(
                        egui::RichText::new(format!("{flag} {label}"))
                            .strong()
                            .color(egui::Color32::from_rgb(240, 240, 240))
                    ).fill(egui::Color32::from_rgb(45, 60, 80));
                    let hover_hint = t("Switch settings to Arabic", "Switch settings to English", self.lang);
                    if ui.add(lang_btn).on_hover_text(hover_hint).clicked() {
                        self.lang = match self.lang {
                            GuiLang::En => GuiLang::Ar,
                            GuiLang::Ar => GuiLang::En,
                        };
                        self.working.gui_language = match self.lang {
                            GuiLang::En => "en".to_string(),
                            GuiLang::Ar => "ar".to_string(),
                        };
                    }
                });
            });

            if self.page == SettingsPage::Pointer {
            if let Some(gp) = gp_state {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(t("Button Monitor:", "مراقبة الأزرار:", self.lang)).strong());
                    let b = gp.wButtons;
                    let mut btn = |name: &str, down: bool| {
                        let fill = if down { egui::Color32::from_rgb(0, 200, 80) } else { egui::Color32::from_rgb(45, 45, 45) };
                        let text_col = if down { egui::Color32::BLACK } else { egui::Color32::from_rgb(200, 200, 200) };
                        egui::Frame::none()
                            .fill(fill)
                            .rounding(egui::Rounding::same(4.0))
                            .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new(name).color(text_col).strong());
                            });
                    };
                    btn("A", b & 0x1000 != 0);
                    btn("B", b & 0x2000 != 0);
                    btn("X", b & 0x4000 != 0);
                    btn("Y", b & 0x8000 != 0);
                    btn("LB", b & 0x0100 != 0);
                    btn("RB", b & 0x0200 != 0);
                    let lt_pct = ((gp.bLeftTrigger as f32) / 255.0 * 100.0).round();
                    let rt_pct = ((gp.bRightTrigger as f32) / 255.0 * 100.0).round();
                    btn(&format!("LT: {lt_pct:.0}%"), gp.bLeftTrigger > 30);
                    btn(&format!("RT: {rt_pct:.0}%"), gp.bRightTrigger > 30);
                    btn("DPad ↑", b & 0x0001 != 0);
                    btn("DPad ↓", b & 0x0002 != 0);
                    btn("DPad ←", b & 0x0004 != 0);
                    btn("DPad →", b & 0x0008 != 0);
                    btn("L3", b & 0x0040 != 0);
                    btn("R3", b & 0x0080 != 0);
                    btn("Back", b & 0x0020 != 0);
                    btn("Start", b & 0x0010 != 0);
                    btn("Guide", b & 0x0400 != 0);
                });
            }
            }
        });

        if !self.status.is_empty() {
            egui::TopBottomPanel::bottom("save_status").show(ctx, |ui| {
                ui.label(&self.status);
            });
        }

        egui::TopBottomPanel::bottom("actions").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui.button(egui::RichText::new(t("Save & close", "حفظ وإغلاق", self.lang)).strong()).clicked() && self.commit() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui.button(t("Save changes", "حفظ التغييرات", self.lang)).clicked() {
                    self.commit();
                }
                if ui.button(t("Reload saved settings", "إعادة تحميل الإعدادات", self.lang)).clicked() {
                    match Config::try_load(&self.config_path) {
                        Ok(config) => {
                            self.baseline = config.clone();
                            self.working = config;
                            self.lang = if self.working.gui_language.eq_ignore_ascii_case("ar") {
                                GuiLang::Ar
                            } else {
                                GuiLang::En
                            };
                            self.active_profile = if self.working.active_profile.is_empty() {
                                crate::profiles::DEFAULT_NAME.to_string()
                            } else {
                                self.working.active_profile.clone()
                            };
                            self.custom_vocab_input = self.working.voice.custom_vocabulary.join(", ");
                            if let Ok(mut engine) = self.engine.lock() {
                                engine.apply_config(self.working.clone());
                            }
                            self.status = t("Reloaded from disk", "تمت إعادة التحميل من القرص", self.lang);
                        }
                        Err(error) => self.status = format!("{}: {error}", t("Reload failed", "فشل التحميل", self.lang)),
                    }
                }
                if ui.button(t("Reset to defaults", "استعادة الافتراضيات", self.lang)).clicked() {
                    self.working = Config::default();
                    self.lang = GuiLang::En;
                    self.active_profile = crate::profiles::DEFAULT_NAME.to_string();
                    self.custom_vocab_input = self.working.voice.custom_vocabulary.join(", ");
                    self.status = t("Defaults are ready. Save to apply them.", "تم استرجاع الإعدادات الافتراضية. اضغط حفظ للتطبيق.", self.lang);
                }
                if ui.button(t("Close", "إغلاق", self.lang)).clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.label(
                egui::RichText::new(t(
                    "Controller: left stick moves · A selects · right stick scrolls · L3 opens keyboard",
                    "يد التحكم: العصا اليسرى تحرك · A يختار · العصا اليمنى تمرر · L3 يفتح الكيبورد",
                    self.lang,
                ))
                .weak()
                .small(),
            );
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (page, title) in [
                    (SettingsPage::Overview, t("Overview", "نظرة عامة", self.lang)),
                    (SettingsPage::Pointer, t("Pointer & Controller", "الماوس ويد التحكم", self.lang)),
                    (SettingsPage::Keyboard, t("Keyboard", "لوحة المفاتيح", self.lang)),
                    (SettingsPage::Profiles, t("Profiles", "البروفايلات", self.lang)),
                    (SettingsPage::Bindings, t("Bindings", "تعيينات الأزرار", self.lang)),
                    (SettingsPage::Voice, t("Voice & AI", "الصوت والذكاء الاصطناعي", self.lang)),
                ] {
                    ui.selectable_value(&mut self.page, page, title);
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().id_source(self.page).show(ui, |ui| {
                ui.add_space(6.0);

                if self.page == SettingsPage::Overview {
                    ui.heading(t("Ready for the desktop", "جاهز للاستخدام المكتبي", self.lang));
                    ui.label(t(
                        "Use your controller to move, click, type, and switch back to a game.",
                        "استخدم يد التحكم للتحريك، النقر، الكتابة، والتبديل إلى الألعاب بسلاسة.",
                        self.lang,
                    ));
                    ui.add_space(8.0);
                    ui.group(|ui| {
                        ui.label(egui::RichText::new(t("Connection and settings", "الاتصال والإعدادات", self.lang)).strong());
                        let state = match runtime_mode {
                            Some(mode) => match mode {
                                crate::runtime_status::RuntimeMode::Active => t("Desktop mode active", "وضع سطح المكتب نشط", self.lang),
                                crate::runtime_status::RuntimeMode::PausedFullscreen => t("Paused for fullscreen app", "موقوف مؤقتاً لتطبيق ملء الشاشة", self.lang),
                                crate::runtime_status::RuntimeMode::Disabled => t("Disabled by user", "معطل بواسطة المستخدم", self.lang),
                                crate::runtime_status::RuntimeMode::Disconnected => t("Controller disconnected", "يد التحكم غير متصلة", self.lang),
                            },
                            None => t("Background app status unavailable. These are saved settings.", "حالة التطبيق في الخلفية غير متاحة. هذه هي الإعدادات المحفوظة.", self.lang),
                        };
                        ui.label(state);
                        ui.label(format!("{}: {active_profile}", t("Active profile", "البروفايل النشط", self.lang)));
                    });
                    ui.add_space(8.0);
                    ui.group(|ui| {
                        ui.label(egui::RichText::new(t("Quick controls", "تحكم سريع", self.lang)).strong());
                        ui.label(t("Left stick · Move pointer     A · Click     Right stick · Scroll", "العصا اليسرى · تحريك الفأرة     A · نقر     العصا اليمنى · تمرير", self.lang));
                        ui.label(t("L3 · Open keyboard     Back + R3 · Quick menu", "L3 · فتح الكيبورد     Back + R3 · القائمة الدائرية السريعة", self.lang));
                        ui.label(t("Back + Start · Turn desktop control on or off", "Back + Start · تشغيل أو إيقاف التحكم المكتبي", self.lang));
                    });
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(t("Tune pointer", "ضبط حركة الماوس", self.lang)).clicked() {
                            self.page = SettingsPage::Pointer;
                        }
                        if ui.button(t("Set up keyboard", "إعدادات الكيبورد", self.lang)).clicked() {
                            self.page = SettingsPage::Keyboard;
                        }
                        if ui.button(t("Manage profiles", "إدارة البروفايلات", self.lang)).clicked() {
                            self.page = SettingsPage::Profiles;
                        }
                    });
                    ui.separator();
                    ui.label(egui::RichText::new(t("Everyday behavior", "السلوك العام والتفضيلات", self.lang)).strong());

                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut self.working.enabled, t("Enable controller -> keyboard/mouse", "تفعيل تحويل يد التحكم إلى ماوس وكيبورد", self.lang)).changed() {
                            let v = self.working.enabled;
                            if let Ok(mut e) = self.engine.lock() {
                                e.set_enabled(v);
                            }
                        }
                    });
                    ui.checkbox(
                        &mut self.working.disable_in_fullscreen,
                        t("Auto-disable while a fullscreen app is focused", "إيقاف تلقائي عند تشغيل لعبة أو تطبيق ملء الشاشة", self.lang),
                    );
                    ui.checkbox(
                        &mut self.working.vk_auto_show,
                        t("Auto-show virtual keyboard when clicking text fields (or press L3 to open)", "إظهار الكيبورد تلقائياً عند النقر على حقل نصي (أو اضغط L3 للفتح يدوياً)", self.lang),
                    );
                    if ui
                        .checkbox(&mut self.working.autostart, t("Start automatically with Windows", "التشغيل التلقائي مع بدء تشغيل ويندوز", self.lang))
                        .changed()
                    {
                        let enable = self.working.autostart;
                        let exe = std::env::current_exe()
                            .map(|p| format!("\"{}\"", p.display()))
                            .unwrap_or_default();
                        self.status = match crate::platform::set_autostart(enable, &exe) {
                            Ok(()) => if enable {
                                t("Autostart enabled", "تم تفعيل التشغيل التلقائي", self.lang)
                            } else {
                                t("Autostart removed", "تم إلغاء التشغيل التلقائي", self.lang)
                            },
                            Err(e) => format!("{}: {e}", t("Autostart error", "خطأ في التشغيل التلقائي", self.lang)),
                        };
                    }
                    ui.separator();
                }

                // --- Profile Management ---
                if self.page == SettingsPage::Profiles {
                ui.group(|ui| {
                    ui.label(egui::RichText::new(t("Profiles", "البروفايلات", self.lang)).strong());
                    ui.checkbox(
                        &mut self.working.auto_profile_switch,
                        t("Auto-switch profile based on focused application", "تبديل البروفايل تلقائياً حسب البرنامج المفتوح", self.lang),
                    );
                    ui.horizontal(|ui| {
                        ui.label(t("Global shortcut", "اختصار عام للتشغيل/الإيقاف", self.lang));
                        let mut text = self.working.toggle_hotkey.clone();
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut text)
                                .desired_width(120.0)
                                .hint_text("Ctrl+K"),
                        );
                        if resp.changed() {
                            let trimmed = text.trim().to_string();
                            // Reject a typo here rather than saving a chord that
                            // can never fire.
                            if trimmed.is_empty() || crate::config::parse_hotkey(&trimmed).is_some() {
                                self.working.toggle_hotkey = trimmed;
                                self.hotkey_error = false;
                            } else {
                                self.hotkey_error = true;
                            }
                        }
                        if self.hotkey_error {
                            ui.colored_label(egui::Color32::LIGHT_RED, t("Invalid shortcut", "اختصار غير صالح", self.lang));
                        }
                    });
                    ui.label(
                        egui::RichText::new(t(
                            "Example: Ctrl+K or Alt+Shift+F4 — toggles desktop control instantly",
                            "مثال: Ctrl+K أو Alt+Shift+F4 — يفعّل أو يوقف التحويل فوراً",
                            self.lang,
                        ))
                        .weak()
                        .small(),
                    );
                    ui.add_space(4.0);

                    // --- Activate ---
                    let is_default = self.active_profile == crate::profiles::DEFAULT_NAME;
                    let def_label = format!("★ {} ({})", crate::profiles::DEFAULT_NAME, t("default", "افتراضي", self.lang));
                    if ui.selectable_label(is_default, def_label).clicked() {
                        self.activate_profile(crate::profiles::DEFAULT_NAME);
                    }
                    let mut to_delete: Option<String> = None;
                    let list = self.profile_list.clone();
                    for p in &list {
                        let stem = p
                            .path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        let is_curr = self.active_profile == stem;
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(is_curr, format!("🎮 {}", p.name))
                                .on_hover_text(p.path.display().to_string())
                                .clicked()
                            {
                                self.activate_profile(&stem);
                            }
                            if p.legacy {
                                ui.colored_label(egui::Color32::from_rgb(255, 180, 80), "AMGP");
                            }
                            if ui.small_button("⧉").on_hover_text(t("Duplicate", "نسخ البروفايل", self.lang)).clicked() {
                                if let Ok(src) = crate::profiles::load(&p.path) {
                                    let target = crate::profiles::unique_stem(&self.profiles_dir, &stem);
                                    let mut copy = src.clone();
                                    copy.name = Some(format!("{} copy", p.name));
                                    match crate::profiles::save(&self.profiles_dir, &target, &copy) {
                                        Ok(_) => {
                                            self.refresh_profiles();
                                            self.status = format!("Duplicated to '{target}'");
                                        }
                                        Err(e) => self.status = format!("Duplicate failed: {e}"),
                                    }
                                }
                            }
                            if ui.small_button("🗑").on_hover_text(t("Delete", "حذف البروفايل", self.lang)).clicked() {
                                to_delete = Some(p.path.display().to_string());
                            }
                        });
                        // Process rules for this profile.
                        if let Ok(prof) = crate::profiles::load(&p.path) {
                            if !prof.processes.is_empty() {
                                ui.label(
                                    egui::RichText::new(format!("     ↳ {}", prof.processes.join(", ")))
                                        .weak()
                                        .small(),
                                );
                            }
                        }
                    }

                    if let Some(path) = to_delete {
                        if self.active_profile
                            == std::path::Path::new(&path)
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy()
                        {
                            self.status = t("Cannot delete the active profile", "لا يمكن حذف البروفايل النشط حالياً", self.lang);
                        } else {
                            match std::fs::remove_file(&path) {
                                Ok(()) => {
                                    self.refresh_profiles();
                                    self.status = format!("Deleted {}", path);
                                }
                                Err(e) => self.status = format!("Delete failed: {e}"),
                            }
                        }
                    }

                    ui.separator();

                    // --- Create from the current bindings ---
                    ui.horizontal_wrapped(|ui| {
                        ui.label(t("New profile name", "اسم جديد", self.lang));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_profile_name)
                                .desired_width(160.0),
                        );
                        ui.label(t("Processes (comma separated)", "برامج (بفاصلة)", self.lang));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_profile_processes)
                                .hint_text("notepad.exe, chrome.exe")
                                .desired_width(200.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        if ui.button(t("Save current profile", "حفظ البروفايل الحالي", self.lang)).clicked() {
                            let raw = self.new_profile_name.trim().to_string();
                            let stem = if raw.is_empty() { "profile".into() } else { raw };
                            let stem = crate::profiles::unique_stem(&self.profiles_dir, &stem);
                            let processes: Vec<String> = self
                                .new_profile_processes
                                .split(',')
                                .map(|s| s.trim().to_ascii_lowercase())
                                .filter(|s| !s.is_empty())
                                .collect();
                            let prof = crate::profiles::Profile {
                                name: Some(if self.new_profile_name.trim().is_empty() {
                                    stem.clone()
                                } else {
                                    self.new_profile_name.trim().to_string()
                                }),
                                processes,
                                bindings: self.working.bindings.clone(),
                            };
                            match crate::profiles::save(&self.profiles_dir, &stem, &prof) {
                                Ok(_) => {
                                    self.refresh_profiles();
                                    self.new_profile_name.clear();
                                    self.new_profile_processes.clear();
                                    self.status = format!("Saved profile '{stem}'");
                                }
                                Err(e) => self.status = format!("Save failed: {e}"),
                            }
                        }
                        if ui.button(t("Refresh", "تحديث", self.lang)).clicked() {
                            self.refresh_profiles();
                        }
                        if ui.button(t("Open profiles folder", "فتح مجلد البروفايلات", self.lang)).clicked() {
                            let _ = std::process::Command::new("explorer.exe")
                                .arg(&self.profiles_dir)
                                .spawn();
                        }
                    });

                    ui.horizontal(|ui| {
                        if ui.button(t("Convert all .amgp files", "تحويل كل ملفات .amgp", self.lang)).clicked() {
                            let mut count = 0;
                            for p in self.profile_list.clone() {
                                if p.legacy {
                                    let stem = p
                                        .path
                                        .file_stem()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .to_string();
                                    let out = self.profiles_dir.join(format!("{stem}.json"));
                                    if crate::amgp::convert_amgp_file(&p.path, &out).is_ok() {
                                        count += 1;
                                    }
                                }
                            }
                            self.refresh_profiles();
                            self.status = format!("Converted {count} .amgp profile(s)");
                        }
                    });
                });
                ui.separator();
                }

                // Mouse
                if self.page == SettingsPage::Pointer {
                ui.horizontal_wrapped(|ui| {
                    ui.label(t("Movement presets:", "إعدادات حركة جاهزة:", self.lang));
                    for (label_en, label_ar, speed, acceleration) in [
                        ("Precision", "دقيق", 450.0, 200.0),
                        ("Balanced", "متوازن", 1100.0, 800.0),
                        ("Large display", "شاشات كبيرة", 1800.0, 1200.0),
                    ] {
                        let label = t(label_en, label_ar, self.lang);
                        if ui.button(&label).clicked() {
                            let m = &mut self.working.mouse;
                            m.speed = speed;
                            m.acceleration = acceleration;
                            m.speed_x = 0.0;
                            m.speed_y = 0.0;
                            m.altsnap_enabled = false;
                            m.diagonal_range = 0.0;
                            self.status = format!("{label_en} preset ready. Save to apply.");
                        }
                    }
                });
                ui.group(|ui| {
                    ui.label(egui::RichText::new(t("Live stick calibration", "معايرة الانجراف والعصي مباشرة", self.lang)).strong());
                    ui.label(t(
                        "Rest calibration: release both sticks for 4 seconds. Settings stay unchanged until you apply and save.",
                        "المعايرة أثناء السكون: اترك العصيان لمدة 4 ثوانٍ. لن تتغير الإعدادات حتى تطبقها وتحفظها.",
                        self.lang,
                    ));
                    if let Some((started, _)) = &self.calibration {
                        ui.add(egui::ProgressBar::new((started.elapsed().as_secs_f32() / 4.0).min(1.0)).text(t("Keep both sticks released", "اترك العصيان ثابتة دون لمس", self.lang)));
                        if ui.button(t("Cancel calibration", "إلغاء المعايرة", self.lang)).clicked() { self.calibration = None; }
                    } else if ui.add_enabled(gp_state.is_some(), egui::Button::new(t("Measure stick drift", "قياس انجراف العصي (Drift)", self.lang))).clicked() {
                        self.calibration_result = None;
                        self.calibration = Some((Instant::now(), crate::calibration::RestCalibration::new(calibration_slot)));
                    }
                    if let Some((_, [left, right])) = self.calibration_result {
                        ui.label(format!("{}: {} {left}, {} {right}", t("Suggested dead zones", "مناطق الخمول المقترحة", self.lang), t("left", "اليسرى", self.lang), t("right", "اليمنى", self.lang)));
                        if ui.button(t("Use suggested dead zones", "تطبيق مناطق الخمول المقترحة", self.lang)).clicked() {
                            self.working.controller.left_stick_dead_zone = left;
                            self.working.controller.right_stick_dead_zone = right;
                            self.calibration_result = None;
                            self.status = t("Calibrated dead zones ready. Save to apply.", "تم تجهيز مناطق الخمول المحسوبة. اضغط حفظ للتطبيق.", self.lang);
                        }
                    }
                    ui.label(t(
                        "Move each stick. The amber ring is its dead zone; green means movement is registered.",
                        "حرك كل عصا. الدائرة الصفراء هي منطقة الخمول، والأخضر يعني حركة مسجلة.",
                        self.lang,
                    ));
                    if let Some(pad) = gp_state {
                        ui.horizontal(|ui| {
                            stick_preview(
                                ui,
                                &t("Pointer · left stick", "المؤشر · العصا اليسرى", self.lang),
                                pad.sThumbLX,
                                pad.sThumbLY,
                                self.working.controller.left_stick_dead_zone,
                                self.lang,
                            );
                            stick_preview(
                                ui,
                                &t("Scroll · right stick", "التمرير · العصا اليمنى", self.lang),
                                pad.sThumbRX,
                                pad.sThumbRY,
                                self.working.controller.right_stick_dead_zone,
                                self.lang,
                            );
                        });
                    } else {
                        ui.label(t("Connect a controller to preview its input.", "قم بتوصيل يد التحكم لمعاينة حركتها.", self.lang));
                    }
                });
                ui.add_space(8.0);
                ui.label(egui::RichText::new(t("Mouse", "الفأرة (الماوس)", self.lang)).strong());
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.speed, 50.0..=3000.0).text(t("Speed (px/s)", "السرعة (بكسل/ث)", self.lang)),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.acceleration, 0.0..=8000.0)
                        .text(t("Acceleration", "التسارع", self.lang)),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.acceleration_threshold, 0.0..=1.0)
                        .text(t("Acceleration threshold", "عتبة التسارع", self.lang)),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.wheel_speed, 0.0..=20000.0)
                        .text(t("Scroll speed (ticks/sec at full deflection)", "سرعة التمرير (نبضة/ث عند أقصى ميل)", self.lang)),
                );
                ui.label(egui::RichText::new(t("Stick curve (Smooth Native Scroll)", "منحنى العصا (Smooth Native Scroll)", self.lang)).weak());
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.diagonal_range, 0.0..=100.0)
                        .text(t("8-way gate near centre (%) — stops diagonal drift", "بوابة 8 اتجاهات في المركز (%) — لمنع الانجراف القطري", self.lang)),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.modifier_zone, 0.0..=32767.0)
                        .text(t("Cruise zone — speed-up starts past this", "منطقة التسارع (Cruise) — يبدأ التسارع بعدها", self.lang)),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.max_zone, 8000.0..=32767.0)
                        .text(t("Max zone — saturation point", "المنطقة القصوى — نقطة التشبع", self.lang)),
                );

                // --- AltSnap: fine positioning ---
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut self.working.mouse.altsnap_enabled,
                        egui::RichText::new("AltSnap").strong(),
                    );
                    ui.label(
                        egui::RichText::new(t(
                            "Pushing stick to full deflection enters precision mode, ideal for pixel-level positioning",
                            "دفع العصا لأقصى حد يفعّل الحركة الدقيقة لضبط الماوس بالبكسل",
                            self.lang,
                        ))
                        .weak()
                        .small(),
                    );
                });
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.altsnap_zone, 1000.0..=32000.0)
                        .text(t("AltSnap zone — precision starts past this", "منطقة AltSnap — تبدأ الدقة بعدها", self.lang)),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.altsnap_speed, 0.05..=1.0)
                        .text(t("AltSnap speed at full deflection (lower = finer)", "سرعة AltSnap عند أقصى ميل (الأقل = أدق)", self.lang)),
                );
                // --- Sniper Mode: analog precision scaling on LT ---
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut self.working.mouse.sniper_enabled,
                        egui::RichText::new(t("Sniper Mode", "وضع القناص (Sniper Mode)", self.lang)).strong(),
                    );
                    ui.label(
                        egui::RichText::new(t(
                            "Squeezing LT gradually slows mouse pointer for ultra-fine precision",
                            "الضغط على زناد LT يبطئ حركة الماوس تدريجياً لدقة تصويب فائقة",
                            self.lang,
                        ))
                        .weak()
                        .small(),
                    );
                });
                if self.working.mouse.sniper_enabled {
                    ui.add(
                        egui::Slider::new(&mut self.working.mouse.sniper_scale, 0.05..=0.8)
                            .text(t("Sniper speed multiplier at full squeeze (LT)", "معامل سرعة القناص عند الضغط الكامل (LT)", self.lang)),
                    );
                }
                ui.checkbox(
                    &mut self.working.mouse.scroll_four_way,
                    t("Lock scroll stick to 4 directions", "تثبيت عصا التمرير على 4 اتجاهات فقط", self.lang),
                );
                ui.checkbox(
                    &mut self.working.mouse.scroll_flip_y,
                    t("Invert vertical scroll direction", "عكس التمرير عمودياً (إذا كان الرفع ينزل)", self.lang),
                );
                ui.horizontal(|ui| {
                    ui.label(t("Horizontal pointer speed (0 = same)", "السرعة الأفقية للمؤشر (0 = نفس الأساسية)", self.lang));
                    let mut sx = self.working.mouse.speed_x;
                    if ui.add(egui::DragValue::new(&mut sx).speed(10.0)).changed() {
                        self.working.mouse.speed_x = sx.clamp(0.0, 4000.0);
                    }
                    ui.label(t("Vertical (0 = same)", "السرعة العمودية للمؤشر (0 = نفس الأساسية)", self.lang));
                    let mut sy = self.working.mouse.speed_y;
                    if ui.add(egui::DragValue::new(&mut sy).speed(10.0)).changed() {
                        self.working.mouse.speed_y = sy.clamp(0.0, 4000.0);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(t("Scroll X", "تمرير أفقي", self.lang));
                    let mut wx = self.working.mouse.wheel_speed_x;
                    if ui.add(egui::DragValue::new(&mut wx).speed(10.0)).changed() {
                        self.working.mouse.wheel_speed_x = wx.clamp(0.0, 20000.0);
                    }
                    ui.label(t("Scroll Y (0 = same)", "تمرير عمودي (0 = نفس الأساسي)", self.lang));
                    let mut wy = self.working.mouse.wheel_speed_y;
                    if ui.add(egui::DragValue::new(&mut wy).speed(10.0)).changed() {
                        self.working.mouse.wheel_speed_y = wy.clamp(0.0, 20000.0);
                    }
                });
                ui.separator();

                // Controller section
                ui.label(egui::RichText::new(t("Controller", "يد التحكم", self.lang)).strong());
                ui.add(
                    egui::Slider::new(&mut self.working.controller.left_stick_dead_zone, 0..=32767)
                        .text(t("Left stick dead zone", "منطقة خمول العصا اليسرى", self.lang)),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.controller.right_stick_dead_zone, 0..=32767)
                        .text(t("Right stick dead zone", "منطقة خمول العصا اليمنى", self.lang)),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.controller.trigger_threshold, 0..=255)
                        .text(t("Trigger press threshold", "عتبة حساسية ضغط الزناد", self.lang)),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.controller.controller_index, 0..=3)
                        .text(t("Controller slot", "منفذ يد التحكم (Slot)", self.lang)),
                );
                ui.checkbox(
                    &mut self.working.controller.haptic_feedback,
                    t("Enable interactive tactile haptic feedback (vibration)", "تفعيل الاهتزاز اللمسي التفاعلي (Haptic Feedback)", self.lang),
                );
                ui.checkbox(
                    &mut self.working.osd_enabled,
                    t("Enable floating OSD / HUD overlay", "تفعيل شاشة الإشعارات العائمة (OSD / HUD Overlay)", self.lang),
                );
                ui.separator();
                }

                // On-screen keyboard preferences.
                if self.page == SettingsPage::Keyboard {
                ui.label(egui::RichText::new(t("On-Screen Virtual Keyboard", "لوحة المفاتيح الافتراضية", self.lang)).strong());
                ui.checkbox(
                    &mut self.working.keyboard.remember_position,
                    t("Remember keyboard position after restart", "تذكر مكان الكيبورد بعد إعادة التشغيل", self.lang),
                );
                ui.checkbox(&mut self.working.keyboard.start_arabic, t("Start in Arabic layout by default", "ابدأ بلغة عربية افتراضياً", self.lang));
                ui.checkbox(
                    &mut self.working.keyboard.show_hints_on_open,
                    t("Show controller cheat-sheet upon opening", "إظهار شرح أزرار التحكم عند فتح الكيبورد", self.lang),
                );
                ui.checkbox(
                    &mut self.working.keyboard.split_mode,
                    t("Steam Deck-style Dual-Stick Split Mode by default", "الكيبورد المنقسم السريع افتراضياً (Dual-Stick Split Mode)", self.lang),
                );
                ui.checkbox(
                    &mut self.working.keyboard.smart_caret_avoidance,
                    t("Smart Caret Collision Avoidance (auto-reposition away from input field)", "تفادي حقل النصوص تلقائياً (Smart Caret Collision Avoidance)", self.lang),
                );
                ui.checkbox(
                    &mut self.working.keyboard.mechanical_sfx,
                    t("Subtle mechanical keyboard click SFX", "أصوات نقر ميكانيكية ناعمة (Mechanical Keyboard SFX)", self.lang),
                );
                ui.horizontal(|ui| {
                    let pos_label = match self.working.keyboard.position {
                        crate::config::KeyboardPosition::FloatBottom => t("Float (Above Input)", "عائم (فوق حقل الإدخال)", self.lang),
                        crate::config::KeyboardPosition::Bottom => t("Bottom", "أسفل الشاشة", self.lang),
                        crate::config::KeyboardPosition::Top => t("Top", "أعلى الشاشة", self.lang),
                        crate::config::KeyboardPosition::Left => t("Left", "يسار الشاشة", self.lang),
                        crate::config::KeyboardPosition::Right => t("Right", "يمين الشاشة", self.lang),
                        crate::config::KeyboardPosition::Custom => t("Custom (remembered)", "مخصص (المكان المحفوظ)", self.lang),
                    };
                    egui::ComboBox::new("kb_pos", t("Position", "المكان", self.lang))
                        .selected_text(pos_label)
                        .show_ui(ui, |ui| {
                            use crate::config::KeyboardPosition as KP;
                            for (v, en_label, ar_label) in [
                                (KP::FloatBottom, "Float (Above Input)", "عائم (فوق حقل الإدخال)"),
                                (KP::Bottom, "Bottom", "أسفل الشاشة"),
                                (KP::Top, "Top", "أعلى الشاشة"),
                                (KP::Left, "Left", "يسار الشاشة"),
                                (KP::Right, "Right", "يمين الشاشة"),
                                (KP::Custom, "Custom (remembered)", "مخصص (المكان المحفوظ)"),
                            ] {
                                let label = t(en_label, ar_label, self.lang);
                                ui.selectable_value(&mut self.working.keyboard.position, v, label);
                            }
                        });
                    ui.label(t("Scale", "الحجم", self.lang));
                    ui.add(
                        egui::Slider::new(&mut self.working.keyboard.scale, 0.75..=1.5)
                            .text(t("Scale", "المقياس", self.lang)),
                    );
                });
                ui.separator();
                }

                // Bindings overview partitioned into Base layer and Modifier layer.
                if self.page == SettingsPage::Bindings {
                ui.label(egui::RichText::new(t("Controller Bindings:", "تعيينات أزرار يد التحكم:", self.lang)).strong());
                
                // 1. Base Layer (Direct button presses)
                ui.group(|ui| {
                    ui.label(egui::RichText::new(t("1. Primary Buttons (Direct press - without Back button)", "1. الأزرار الأساسية (الوضع المباشر - بدون زر Back)", self.lang)).color(egui::Color32::from_rgb(100, 200, 255)).strong());
                    egui::Grid::new("base_bindings_grid").striped(true).min_col_width(120.0).show(ui, |ui| {
                        ui.label(egui::RichText::new(t("Button", "الزر", self.lang)).strong());
                        ui.label(egui::RichText::new(t("Action", "الوظيفة", self.lang)).strong());
                        ui.end_row();

                        // Fixed primary axes
                        ui.label(t("Left Stick", "عصا التحكم اليسرى", self.lang));
                        ui.label(t("Mouse Pointer", "تحريك مؤشر الفأرة", self.lang));
                        ui.end_row();

                        ui.label(t("Right Stick", "عصا التحكم اليمنى", self.lang));
                        ui.label(t("Scroll Wheel", "التمرير بالبكرة", self.lang));
                        ui.end_row();

                        for b in self.working.bindings.iter_mut().filter(|b| b.modifier.is_none()) {
                            if matches!(b.input, Input::LeftStick | Input::RightStick) {
                                continue;
                            }
                            let input_name = format_input(b.input, self.lang);
                            ui.label(egui::RichText::new(input_name).strong());
                            if matches!(b.action, Action::Modifier(_)) {
                                ui.label(format_action(&b.action, self.lang));
                            } else {
                                action_picker(ui, ("base_act", b.input), &mut b.action, self.lang);
                            }
                            ui.end_row();
                        }
                    });
                });

                ui.add_space(8.0);

                // 2. Secondary Modifier Layer (Hold Back button)
                ui.group(|ui| {
                    ui.label(egui::RichText::new(t("2. Secondary Modifier Layer (Hold Back button)", "2. الوظائف الإضافية (عند الضغط مع زر Back)", self.lang)).color(egui::Color32::from_rgb(255, 180, 80)).strong());
                    egui::Grid::new("modifier_bindings_grid").striped(true).min_col_width(140.0).show(ui, |ui| {
                        ui.label(egui::RichText::new(t("Combination", "الاختصار", self.lang)).strong());
                        ui.label(egui::RichText::new(t("Action", "الوظيفة", self.lang)).strong());
                        ui.end_row();

                        for b in self.working.bindings.iter_mut().filter(|b| b.modifier.is_some()) {
                            let combo = format!("Back + {}", format_input(b.input, self.lang));
                            ui.label(egui::RichText::new(combo).strong());
                            action_picker(ui, ("mod_act", b.input), &mut b.action, self.lang);
                            ui.end_row();
                        }
                    });
                });

                }

                if self.page == SettingsPage::Voice {
                    ui.label(egui::RichText::new(t("Offline Voice AI & Speech-to-Text", "الكتابة الصوتية والذكاء الاصطناعي المحلي", self.lang)).strong());
                    ui.label(t(
                        "Speech-to-Text powered locally by OpenAI Whisper. 100% private, runs entirely on CPU without internet.",
                        "تحويل الصوت إلى نص محلياً عبر OpenAI Whisper. خصوصية 100%، يعمل على المعالج بدون إنترنت.",
                        self.lang,
                    ));
                    ui.add_space(8.0);

                    // 1. Microphone status & Live VU meter
                    ui.group(|ui| {
                        ui.label(egui::RichText::new(t("1. Microphone Status & Live Audio Level", "1. حالة الميكروفون ومستوى الصوت الحية", self.lang)).strong());
                        #[cfg(feature = "voice")]
                        {
                            let level = crate::voice::audio_level();
                            let is_rec = crate::voice::is_recording();
                            let is_trans = crate::voice::is_transcribing();
                            let status_str = if is_trans {
                                t("⚡ Transcribing with Whisper CPU...", "⚡ جاري تحويل الصوت عبر الذكاء الاصطناعي...", self.lang)
                            } else if is_rec {
                                t("🔴 Recording in progress...", "🔴 جاري تسجيل الصوت الآن...", self.lang)
                            } else {
                                t("Ready / Microphone Standby", "جاهز / الميكروفون في وضع الاستعداد", self.lang)
                            };
                            ui.label(format!("{}: {status_str}", t("State", "الحالة", self.lang)));
                            ui.horizontal(|ui| {
                                ui.label(t("Input Level:", "مستوى الصوت:", self.lang));
                                let bar = egui::ProgressBar::new(level)
                                    .text(format!("{:.0}%", level * 100.0))
                                    .animate(is_rec);
                                ui.add(bar);
                            });
                            if is_rec || is_trans {
                                ui.ctx().request_repaint_after(Duration::from_millis(30));
                            }
                        }
                        #[cfg(not(feature = "voice"))]
                        {
                            ui.label(t("Voice feature is not enabled in this build.", "ميزة الصوت غير مفعلة في هذا الإصدار.", self.lang));
                        }
                    });

                    ui.add_space(8.0);

                    // 2. Whisper Model Status
                    ui.group(|ui| {
                        ui.label(egui::RichText::new(t("2. Whisper AI Model Status", "2. نموذج الذكاء الاصطناعي (Whisper Model)", self.lang)).strong());
                        #[cfg(feature = "voice")]
                        {
                            let model_found = crate::voice::model_path();
                            match model_found {
                                Some(p) => {
                                    ui.colored_label(egui::Color32::from_rgb(91, 217, 167), t(&format!("✔ Model loaded from: {}", p.display()), &format!("{}: {}", ar("✔ تم تحميل النموذج بنجاح من"), p.display()), self.lang));
                                    if let Ok(meta) = std::fs::metadata(&p) {
                                        let mb = meta.len() as f64 / (1024.0 * 1024.0);
                                        ui.label(t(&format!("File size: {:.1} MB", mb), &format!("{}: {:.1} {}", ar("حجم الملف"), mb, ar("ميجابايت")), self.lang));
                                    }
                                }
                                None => {
                                    ui.colored_label(egui::Color32::from_rgb(255, 100, 100), t("⚠ No Whisper model found!", "⚠ لم يتم العثور على نموذج Whisper!", self.lang));
                                    ui.label(t("Place ggml-tiny.bin (75MB) or ggml-base.bin (140MB) in:", "ضع ملف ggml-tiny.bin أو ggml-base.bin في المجلد:", self.lang));
                                    ui.monospace("%APPDATA%\\ControllerCompanionRust\\models");
                                }
                            }
                        }
                        ui.horizontal(|ui| {
                            ui.label(t("Model file name:", "اسم ملف النموذج:", self.lang));
                            ui.text_edit_singleline(&mut self.working.voice.model);
                        });
                    });

                    ui.add_space(8.0);

                    // 3. Language & VAD settings
                    ui.group(|ui| {
                        ui.label(egui::RichText::new(t("3. Recognition Language & Silence Detection (VAD)", "3. لغة التعرف واكتشاف السكون (Language & VAD)", self.lang)).strong());
                        ui.horizontal(|ui| {
                            ui.label(t("Recognition Language:", "لغة التعرف الصوتي:", self.lang));
                            let selected_text = match self.working.voice.language.as_str() {
                                "ar" => t("العربية (Arabic)", "العربية", self.lang),
                                "en" => t("English", "الإنجليزية (English)", self.lang),
                                _ => t("Auto (Match Keyboard)", "تلقائي (حسب لغة الكيبورد)", self.lang),
                            };
                            egui::ComboBox::from_id_source("voice_lang_picker")
                                .selected_text(selected_text)
                                .show_ui(ui, |ui| {
                                    let ar_opt = t("العربية (Arabic)", "العربية", self.lang);
                                    let en_opt = t("English", "الإنجليزية (English)", self.lang);
                                    let auto_opt = t("Auto (Match Keyboard)", "تلقائي (حسب لغة الكيبورد)", self.lang);
                                    ui.selectable_value(&mut self.working.voice.language, "ar".into(), ar_opt);
                                    ui.selectable_value(&mut self.working.voice.language, "en".into(), en_opt);
                                    ui.selectable_value(&mut self.working.voice.language, "auto".into(), auto_opt);
                                });
                        });

                        ui.horizontal(|ui| {
                            ui.label(t("VAD Silence Pause (ms):", "مهلة السكون قبل الإنهاء التلقائي (مللي ثانية):", self.lang));
                            ui.add(egui::Slider::new(&mut self.working.voice.vad_silence_ms, 500..=3000).text(t("Pause duration before auto-stop", "مدة التوقف قبل إيقاف التسجيل", self.lang)));
                        });

                        ui.checkbox(
                            &mut self.working.voice.enable_macros,
                            t("Enable natural voice macros (saying 'send' presses Enter, 'new line', punctuation)", "تفعيل ماكرو الأوامر الصوتية (نطق 'أرسل' يضغط Enter، 'سطر جديد'، علامات الترقيم)", self.lang),
                        );
                    });

                    ui.add_space(8.0);

                    // 4. Custom Vocabulary Biasing
                    ui.group(|ui| {
                        ui.label(egui::RichText::new(t("4. Custom Vocabulary Biasing", "4. قاموس الكلمات والمصطلحات المخصصة (Vocabulary Bias)", self.lang)).strong());
                        ui.label(t(
                            "Type custom terms, gamer tags, or technical names separated by commas. Whisper will prioritize recognizing them:",
                            "اكتب مصطلحات مخصصة، أسماء ألعاب، أو كلمات دارجة مفصولة بفاصلة. سيعطيها الذكاء الاصطناعي أولوية في التعرف:",
                            self.lang,
                        ));
                        let resp = ui.text_edit_singleline(&mut self.custom_vocab_input);
                        if resp.changed() {
                            self.working.voice.custom_vocabulary = self.custom_vocab_input
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();
                        }
                        if !self.working.voice.custom_vocabulary.is_empty() {
                            ui.label(t(
                                &format!("Active biased terms ({}): {}", self.working.voice.custom_vocabulary.len(), self.working.voice.custom_vocabulary.join(" · ")),
                                &format!("{}: ({}) {}", ar("الكلمات ذات الأولوية النشطة"), self.working.voice.custom_vocabulary.len(), self.working.voice.custom_vocabulary.join(" · ")),
                                self.lang,
                            ));
                        }
                    });

                    ui.add_space(8.0);

                    // 5. Test dictation button
                    ui.horizontal(|ui| {
                        #[cfg(feature = "voice")]
                        {
                            let is_rec = crate::voice::is_recording();
                            let btn_label = if is_rec {
                                t("⏹ Stop Recording & Transcribe", "⏹ إيقاف التسجيل والتحويل إلى نص", self.lang)
                            } else {
                                t("🎙 Start Voice Test (Speak now)", "🎙 بدء اختبار الصوت (تحدث الآن)", self.lang)
                            };
                            if ui.button(btn_label).clicked() {
                                crate::voice::toggle_voice_typing(self.working.voice.language == "ar");
                            }
                            if let Some(last) = crate::voice::last_transcribed() {
                                ui.label(t(&format!("Last result: \"{last}\""), &format!("{}: \"{last}\"", ar("آخر نتيجة")), self.lang));
                            }
                        }
                    });
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if self.page == SettingsPage::Profiles && ui.button(t("Import .amgp profile...", "استيراد بروفايل .amgp...", self.lang)).clicked() {
                        // Scan common locations for .amgp files (Downloads, Desktop, AppData)
                        let mut candidates = Vec::new();
                        if let Some(user_prof) = std::env::var_os("USERPROFILE") {
                            let downloads = std::path::Path::new(&user_prof).join("Downloads");
                            if let Ok(entries) = std::fs::read_dir(downloads) {
                                for e in entries.flatten() {
                                    if e.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("amgp")) {
                                        candidates.push(e.path());
                                    }
                                }
                            }
                        }
                        if let Some(amgp_file) = candidates.first() {
                            match crate::amgp::load_amgp_file(amgp_file) {
                                Ok(cfg) => {
                                    self.working = cfg;
                                    self.active_profile = if self.working.active_profile.is_empty() {
                                        crate::profiles::DEFAULT_NAME.to_string()
                                    } else {
                                        self.working.active_profile.clone()
                                    };
                                    self.status = format!("Imported {} (Click Save to apply)", amgp_file.file_name().unwrap_or_default().to_string_lossy());
                                }
                                Err(err) => {
                                    self.status = format!("Failed to import {}: {err}", amgp_file.display());
                                }
                            }
                        } else {
                            self.status = t("No .amgp profiles found in Downloads folder", "لم يتم العثور على بروفايلات .amgp في مجلد التنزيلات", self.lang);
                        }
                    }
                });
            });
        });

        // Only request continuous redraw when live real-time views are active.
        // This drops idle CPU from 1.0% down to 0.0% and GPU from 0.6% down to 0.0%!
        if self.calibration.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        } else if self.page == SettingsPage::Pointer && gp_state.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        SETTINGS_OPEN.store(false, Ordering::Relaxed);
    }
}

/// Run the settings GUI window directly (used when invoked with --settings in a dedicated process).
pub fn run_settings_standalone(engine: Arc<Mutex<Engine>>, config_path: PathBuf) {
    crate::platform::attach_to_interactive_desktop();
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([720.0, 700.0])
            .with_min_inner_size([540.0, 480.0])
            .with_title("Controller Companion — Settings"),
        ..Default::default()
    };
    let eng = engine.clone();
    let path = config_path.clone();
    let result = eframe::run_native(
        "Controller Companion — Settings",
        options,
        Box::new(move |cc| {
            crate::vkeyboard::install_fonts(&cc.egui_ctx);
            let mut style = (*cc.egui_ctx.style()).clone();
            style.spacing.item_spacing = egui::vec2(10.0, 9.0);
            style.spacing.button_padding = egui::vec2(12.0, 6.0);
            style.spacing.interact_size.y = 34.0;
            cc.egui_ctx.set_style(style);
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
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe)
            .arg("--settings")
            .arg("--config")
            .arg(config_path)
            .spawn();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn save_preserves_external_changes_to_untouched_fields() {
        let original = json!({"enabled": true, "active_profile": "Default", "mouse": {"speed": 600, "acceleration": 800}});
        let edited = json!({"enabled": true, "active_profile": "Default", "mouse": {"speed": 750, "acceleration": 800}});
        let current = json!({"enabled": false, "active_profile": "Browser", "mouse": {"speed": 600, "acceleration": 400}});
        let merged = merge_edits(&original, &edited, &current);
        assert_eq!(merged, json!({"enabled": false, "active_profile": "Browser", "mouse": {"speed": 750, "acceleration": 400}}));
    }

    #[test]
    fn test_gui_voice_tab_settings_state() {
        let xinput = crate::win32::XInputApi::load().expect("xinput available");
        let engine = std::sync::Arc::new(std::sync::Mutex::new(crate::engine::Engine::new(
            xinput,
            crate::config::Config::default(),
        )));
        let mut app = super::SettingsApp::new(engine, std::path::PathBuf::from("test.json"));
        assert_eq!(app.page, super::SettingsPage::Overview);
        app.page = super::SettingsPage::Voice;
        assert_eq!(app.page, super::SettingsPage::Voice);
        app.custom_vocab_input = "فيفا, فورتنايت, ديسكورد".into();
        app.working.voice.custom_vocabulary = app.custom_vocab_input
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        assert_eq!(app.working.voice.custom_vocabulary.len(), 3);
        assert_eq!(app.working.voice.custom_vocabulary[1], "فورتنايت");
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
