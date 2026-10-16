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

fn stick_preview(ui: &mut egui::Ui, title: &str, x: i16, y: i16, dead_zone: u16) {
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
        ui.label(if in_dead_zone { "Inside dead zone" } else { "Active input" });
    });
}

fn format_input(input: Input) -> String {
    match input {
        Input::A => "A".into(),
        Input::B => "B".into(),
        Input::X => "X".into(),
        Input::Y => "Y".into(),
        Input::DPadUp => format!("D-Pad Up ({})", ar("سهم لأعلى")),
        Input::DPadDown => format!("D-Pad Down ({})", ar("سهم لأسفل")),
        Input::DPadLeft => format!("D-Pad Left ({})", ar("سهم يسار")),
        Input::DPadRight => format!("D-Pad Right ({})", ar("سهم يمين")),
        Input::LeftShoulder => "LB (Left Bumper)".into(),
        Input::RightShoulder => "RB (Right Bumper)".into(),
        Input::LeftTrigger => "LT (Left Trigger)".into(),
        Input::RightTrigger => "RT (Right Trigger)".into(),
        Input::LeftThumb => "L3 (Left Stick Click)".into(),
        Input::RightThumb => "R3 (Right Stick Click)".into(),
        Input::LeftStick => format!("Left Stick ({})", ar("عصا التحكم اليسرى")),
        Input::RightStick => format!("Right Stick ({})", ar("عصا التحكم اليمنى")),
        Input::Start => "Start".into(),
        Input::Back => format!("Back ({})", ar("زر الوظائف")),
        Input::Guide => format!("Guide ({})", ar("زر Xbox")),
    }
}

fn format_action(action: &Action) -> String {
    match action {
        Action::MouseClick(MouseButton::Left) => format!("Left Click ({})", ar("النقر الأيسر بالماوس")),
        Action::MouseClick(MouseButton::Right) => format!("Right Click ({})", ar("النقر الأيمن بالماوس")),
        Action::MouseClick(MouseButton::Middle) => format!("Middle Click ({})", ar("النقر الأوسط بالماوس")),
        Action::MouseClick(MouseButton::X1) => format!("Mouse X1 ({})", ar("زر الماوس الجانبي 1")),
        Action::MouseClick(MouseButton::X2) => format!("Mouse X2 ({})", ar("زر الماوس الجانبي 2")),
        Action::ToggleVirtualKeyboard => format!("Toggle On-screen Keyboard ({})", ar("فتح/إغلاق الكيبورد")),
        Action::MouseMove { .. } => format!("Mouse Pointer ({})", ar("تحريك مؤشر الفأرة")),
        Action::MouseScroll { flip_y } => {
            if *flip_y {
                format!("Scroll Wheel — Y flipped ({})", ar("التمرير معكوس عمودياً"))
            } else {
                format!("Scroll Wheel ({})", ar("التمرير بالبكرة لأعلى/أسفل"))
            }
        }
        Action::Key { key: Key::MediaPlayPause, .. } => format!("Media Play/Pause ({})", ar("تشغيل/إيقاف الوسائط")),
        Action::Key { key: Key::MediaPreviousTrack, .. } => format!("Previous Track ({})", ar("المقطع السابق")),
        Action::Key { key: Key::MediaNextTrack, .. } => format!("Next Track ({})", ar("المقطع التالي")),
        Action::Key { key: Key::Backspace, .. } => format!("Backspace ({})", ar("مسح للخلف")),
        Action::Key { key: Key::Enter, .. } => format!("Enter ({})", ar("إدخال")),
        Action::Key { key: Key::LControl, .. } => format!("Ctrl ({})", ar("الضغط مع الاستمرار")),
        Action::Key { key: Key::LShift, .. } => format!("Shift ({})", ar("الضغط مع الاستمرار")),
        Action::Key { key: Key::Escape, .. } => format!("Esc ({})", ar("إلغاء / خروج")),
        Action::Key { key: Key::VolumeUp, .. } => format!("Volume Up ({})", ar("رفع الصوت")),
        Action::Key { key: Key::VolumeDown, .. } => format!("Volume Down ({})", ar("خفض الصوت")),
        Action::Key { key: Key::Left, .. } => format!("Arrow Left ({})", ar("سهم يسار")),
        Action::Key { key: Key::Right, .. } => format!("Arrow Right ({})", ar("سهم يمين")),
        Action::Key { key: Key::Up, .. } => format!("Arrow Up ({})", ar("سهم لأعلى")),
        Action::Key { key: Key::Down, .. } => format!("Arrow Down ({})", ar("سهم لأسفل")),
        Action::Key { key: Key::LWin, .. } => format!("Windows Start Menu ({})", ar("قائمة ابدأ")),
        Action::Key { key, .. } => format!("Key: {key:?}"),
        Action::DisplaySwitch(DisplayMode::External) => format!("Display Switch ({})", ar("الشاشة الخارجية")),
        Action::DisplaySwitch(DisplayMode::Internal) => format!("Display Switch ({})", ar("شاشة الكمبيوتر")),
        Action::DisplaySwitch(mode) => format!("Display Switch ({mode:?})"),
        Action::TurnOffController => format!("Turn Off Controller ({})", ar("إيقاف تشغيل اليد")),
        Action::Combo(keys) => {
            if keys.contains(&Key::LWin) && keys.contains(&Key::M) {
                format!("Show Desktop / Win+M ({})", ar("عرض سطح المكتب"))
            } else if keys.contains(&Key::Tab) {
                format!("Task View / Window Switch ({})", ar("عرض المهام / تبديل النوافذ"))
            } else {
                format!("Key Combo: {keys:?}")
            }
        }
        Action::Modifier(Modifier::Back) => format!("Hold Modifier ({})", ar("زر الوظائف الإضافية")),
        Action::ShowDesktop => format!("Show Desktop / Win+D ({})", ar("عرض سطح المكتب")),
        Action::ToggleEnabled => format!("Toggle Enable ({})", ar("تشغيل/إيقاف البرنامج")),
        Action::ToggleDragLock => format!("Toggle Drag-Lock ({})", ar("قفل السحب والنقر")),
        Action::VoiceDictation => format!("Voice Dictation / Win+H ({})", ar("الإملاء والكتابة الصوتية")),
        Action::TaskView => format!("Task View / Win+Tab ({})", ar("عرض كافة المهام والنوافذ")),
        Action::SnapWindowLeft => format!("Snap Window Left ({})", ar("محاذاة النافذة لليسار")),
        Action::SnapWindowRight => format!("Snap Window Right ({})", ar("محاذاة النافذة لليمين")),
        Action::MaximizeWindow => format!("Maximize Window ({})", ar("تكبير النافذة")),
        Action::MinimizeWindow => format!("Minimize Window ({})", ar("تصغير النافذة")),
        Action::CloseActiveWindow => format!("Close Window / Alt+F4 ({})", ar("إغلاق النافذة الحالية")),
        Action::NextVirtualDesktop => format!("Next Virtual Desktop / Win+Ctrl+Right ({})", ar("سطح المكتب الافتراضي التالي")),
        Action::PrevVirtualDesktop => format!("Prev Virtual Desktop / Win+Ctrl+Left ({})", ar("سطح المكتب الافتراضي السابق")),
        Action::RadialMenu => format!("Radial Menu / Quick Wheel ({})", ar("القائمة الدائرية السريعة")),
        Action::ActivateProfile(n) => format!("Activate Profile: {n} ({})", ar("تفعيل بروفايل")),
        Action::Excluded(s) => format!("Excluded ({s})"),
        Action::DoNothing => format!("Disabled ({})", ar("معطل")),
    }
}

fn available_actions() -> Vec<(&'static str, Action)> {
    vec![
        ("Left Click / النقر الأيسر", Action::MouseClick(MouseButton::Left)),
        ("Right Click / النقر الأيمن", Action::MouseClick(MouseButton::Right)),
        ("Middle Click / النقر الأوسط", Action::MouseClick(MouseButton::Middle)),
        ("Mouse X1 / زر جانبي 1", Action::MouseClick(MouseButton::X1)),
        ("Mouse X2 / زر جانبي 2", Action::MouseClick(MouseButton::X2)),
        ("Toggle Drag-Lock / قفل السحب", Action::ToggleDragLock),
        ("Toggle On-screen Keyboard / الكيبورد", Action::ToggleVirtualKeyboard),
        ("Voice Dictation (Win+H) / الإملاء الصوتي", Action::VoiceDictation),
        ("Radial Menu / القائمة الدائرية السريعة", Action::RadialMenu),
        ("Task View (Win+Tab) / عرض المهام", Action::TaskView),
        ("Show Desktop (Win+D) / سطح المكتب", Action::ShowDesktop),
        ("Close Window (Alt+F4) / إغلاق النافذة", Action::CloseActiveWindow),
        ("Snap Window Left (Win+Left) / محاذاة لليسار", Action::SnapWindowLeft),
        ("Snap Window Right (Win+Right) / محاذاة لليمين", Action::SnapWindowRight),
        ("Maximize Window (Win+Up) / تكبير النافذة", Action::MaximizeWindow),
        ("Minimize Window (Win+Down) / تصغير النافذة", Action::MinimizeWindow),
        ("Next Desktop (Win+Ctrl+Right) / سطح المكتب التالي", Action::NextVirtualDesktop),
        ("Prev Desktop (Win+Ctrl+Left) / سطح المكتب السابق", Action::PrevVirtualDesktop),
        ("Volume Up / رفع الصوت", Action::Key { key: Key::VolumeUp, repeat: false }),
        ("Volume Down / خفض الصوت", Action::Key { key: Key::VolumeDown, repeat: false }),
        ("Media Play/Pause / تشغيل الوسائط", Action::Key { key: Key::MediaPlayPause, repeat: false }),
        ("Media Next Track / المقطع التالي", Action::Key { key: Key::MediaNextTrack, repeat: false }),
        ("Media Prev Track / المقطع السابق", Action::Key { key: Key::MediaPreviousTrack, repeat: false }),
        ("Backspace / مسح للخلف", Action::Key { key: Key::Backspace, repeat: true }),
        ("Enter / إدخال", Action::Key { key: Key::Enter, repeat: false }),
        ("Escape / إلغاء", Action::Key { key: Key::Escape, repeat: false }),
        ("Ctrl Modifier / زر Ctrl", Action::Key { key: Key::LControl, repeat: false }),
        ("Shift Modifier / زر Shift", Action::Key { key: Key::LShift, repeat: false }),
        ("Windows Start Menu / قائمة ابدأ", Action::Key { key: Key::LWin, repeat: false }),
        ("Turn Off Controller / إيقاف تشغيل اليد", Action::TurnOffController),
        ("Display Switch (External) / شاشة خارجية", Action::DisplaySwitch(DisplayMode::External)),
        ("Display Switch (Internal) / شاشة الكمبيوتر", Action::DisplaySwitch(DisplayMode::Internal)),
        ("Disabled / معطل", Action::DoNothing),
    ]
}

fn action_picker(ui: &mut egui::Ui, id: impl std::hash::Hash, current: &mut Action) {
    let current_label = format_action(current);
    egui::ComboBox::from_id_source(id)
        .selected_text(current_label)
        .width(280.0)
        .show_ui(ui, |ui| {
            for (label, act) in available_actions() {
                let is_sel = current == &act;
                let reshaped = if label.contains('/') {
                    let parts: Vec<&str> = label.split(" / ").collect();
                    if parts.len() == 2 {
                        format!("{} / {}", parts[0], ar(parts[1]))
                    } else {
                        label.to_string()
                    }
                } else {
                    label.to_string()
                };
                if ui.selectable_label(is_sel, reshaped).clicked() {
                    *current = act.clone();
                }
            }
        });
}

static SETTINGS_OPEN: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum SettingsPage {
    Overview,
    Pointer,
    Keyboard,
    Profiles,
    Bindings,
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
    runtime_status: Option<crate::runtime_status::RuntimeStatus>,
    last_status_check: Instant,
    last_status_seen: Instant,
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
            runtime_status: None,
            last_status_check: Instant::now() - Duration::from_secs(1),
            last_status_seen: Instant::now() - Duration::from_secs(10),
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
                    "Desktop control turned on".into()
                } else {
                    "Desktop control turned off".into()
                };
            }
            Err(error) => self.status = format!("Could not change desktop control: {error}"),
        }
    }

    /// Persist the working config and push it into the running engine.
    fn commit(&mut self) -> bool {
        let latest = if self.config_path.exists() {
            match Config::try_load(&self.config_path) {
                Ok(config) => config,
                Err(error) => {
                    self.status = format!("Save stopped: {error}");
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
            self.status = "Could not prepare settings for saving".into();
            return false;
        };
        let mut merged: Config = match serde_json::from_value(merge_edits(&before, &edited, &current)) {
            Ok(config) => config,
            Err(error) => {
                self.status = format!("Could not merge settings: {error}");
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
                self.status = format!("Saved to {}", self.config_path.display());
                true
            }
            Err(e) => {
                self.status = format!("Save failed: {e}");
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
                    self.status = format!("Could not load {stem}: {e}");
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
                "Default profile activated".into()
            } else {
                format!("Profile '{stem}' activated")
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

        let (local_connected, local_enabled, local_battery, gp_state, local_profile) = self
            .engine
            .lock()
            .map(|e| {
                let name = &e.config().active_profile;
                let controller_index = self.runtime_status.as_ref()
                    .map(|snapshot| snapshot.controller_index)
                    .unwrap_or(e.config().controller.controller_index);
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

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        egui::TopBottomPanel::top("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Controller Companion");
                ui.separator();
                let conn = if connected { "Controller connected" } else { "No controller connected" };
                let col = if connected { egui::Color32::GREEN } else { egui::Color32::GRAY };
                ui.colored_label(col, conn);
                if let Some((_t, l)) = battery {
                    let level = match l {
                        0 => "Empty",
                        1 => "Low",
                        2 => "Medium",
                        3 => "Full",
                        _ => "Unknown",
                    };
                    ui.label(format!("Battery: {level}"));
                }
                ui.separator();
                let is_on = enabled;
                let toggle_label = if is_on { ar("التحكم شغال (انقر للإيقاف)") } else { ar("التحكم موقوف (انقر للتشغيل)") };
                let toggle_col = if is_on { egui::Color32::from_rgb(0, 160, 60) } else { egui::Color32::from_rgb(190, 40, 40) };
                let btn = egui::Button::new(egui::RichText::new(toggle_label).color(egui::Color32::WHITE).strong())
                    .fill(toggle_col);
                if ui.add(btn).clicked() {
                    self.toggle_desktop_control(!enabled);
                }
            });

            if self.page == SettingsPage::Pointer {
            if let Some(gp) = gp_state {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new(ar("مراقبة الأزرار:")).strong());
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
                if ui.button(egui::RichText::new("Save & close").strong()).clicked() && self.commit() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui.button("Save changes").clicked() {
                    self.commit();
                }
                if ui.button("Reload saved settings").clicked() {
                    match Config::try_load(&self.config_path) {
                        Ok(config) => {
                            self.baseline = config.clone();
                            self.working = config;
                            self.active_profile = if self.working.active_profile.is_empty() {
                                crate::profiles::DEFAULT_NAME.to_string()
                            } else {
                                self.working.active_profile.clone()
                            };
                            if let Ok(mut engine) = self.engine.lock() {
                                engine.apply_config(self.working.clone());
                            }
                            self.status = "Reloaded from disk".into();
                        }
                        Err(error) => self.status = format!("Reload failed: {error}"),
                    }
                }
                if ui.button("Reset to defaults").clicked() {
                    self.working = Config::default();
                    self.active_profile = crate::profiles::DEFAULT_NAME.to_string();
                    self.status = "Defaults are ready. Save to apply them.".into();
                }
                if ui.button("Close").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.label(
                egui::RichText::new("Controller: left stick moves · A selects · right stick scrolls · L3 opens keyboard")
                    .weak()
                    .small(),
            );
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (page, title) in [
                    (SettingsPage::Overview, "Overview"),
                    (SettingsPage::Pointer, "Pointer & controller"),
                    (SettingsPage::Keyboard, "Keyboard"),
                    (SettingsPage::Profiles, "Profiles"),
                    (SettingsPage::Bindings, "Bindings"),
                ] {
                    ui.selectable_value(&mut self.page, page, title);
                }
            });
            ui.separator();
            egui::ScrollArea::vertical().id_source(self.page).show(ui, |ui| {
                ui.add_space(6.0);

                if self.page == SettingsPage::Overview {
                    ui.heading("Ready for the desktop");
                    ui.label("Use your controller to move, click, type, and switch back to a game.");
                    ui.add_space(8.0);
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Connection and settings").strong());
                        let state = match runtime_mode {
                            Some(mode) => mode.label(),
                            None => "Background app status unavailable. These are saved settings.",
                        };
                        ui.label(state);
                        ui.label(format!("Active profile: {active_profile}"));
                    });
                    ui.add_space(8.0);
                    ui.group(|ui| {
                        ui.label(egui::RichText::new("Quick controls").strong());
                        ui.label("Left stick · Move pointer     A · Click     Right stick · Scroll");
                        ui.label("L3 · Open keyboard     Back + R3 · Quick menu");
                        ui.label("Back + Start · Turn desktop control on or off");
                    });
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Tune pointer").clicked() {
                            self.page = SettingsPage::Pointer;
                        }
                        if ui.button("Set up keyboard").clicked() {
                            self.page = SettingsPage::Keyboard;
                        }
                        if ui.button("Manage profiles").clicked() {
                            self.page = SettingsPage::Profiles;
                        }
                    });
                    ui.separator();
                    ui.label(egui::RichText::new("Everyday behavior").strong());
                }

                // --- Profile Management ---
                if self.page == SettingsPage::Profiles {
                ui.group(|ui| {
                    ui.label(egui::RichText::new(ar("البروفايلات")).strong());
                    ui.checkbox(
                        &mut self.working.auto_profile_switch,
                        ar("تبديل البروفايل تلقائياً حسب البرنامج المفتوح"),
                    );
                    ui.horizontal(|ui| {
                        ui.label(ar("اختصار عام"));
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
                            ui.colored_label(egui::Color32::LIGHT_RED, ar("اختصار غير صالح"));
                        }
                    });
                    ui.label(
                        egui::RichText::new(ar("مثال: Ctrl+K أو Alt+Shift+F4 — يفعّل أو يوقف التحويل فوراً"))
                            .weak()
                            .small(),
                    );
                    ui.add_space(4.0);

                    // --- Activate ---
                    let is_default = self.active_profile == crate::profiles::DEFAULT_NAME;
                    if ui
                        .selectable_label(is_default, format!("★ {} (default)", crate::profiles::DEFAULT_NAME))
                        .clicked()
                    {
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
                            if ui.small_button("⧉").on_hover_text("Duplicate").clicked() {
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
                            if ui.small_button("🗑").on_hover_text("Delete").clicked() {
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
                            self.status = "Cannot delete the active profile".into();
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
                        ui.label(ar("اسم جديد"));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_profile_name)
                                .desired_width(160.0),
                        );
                        ui.label(ar("برامج (بفاصلة)"));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.new_profile_processes)
                                .hint_text("notepad.exe, chrome.exe")
                                .desired_width(200.0),
                        );
                    });
                    ui.horizontal(|ui| {
                        if ui.button(ar("حفظ البروفايل الحالي")).clicked() {
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
                        if ui.button("Refresh").clicked() {
                            self.refresh_profiles();
                        }
                        if ui.button(ar("فتح مجلد البروفايلات")).clicked() {
                            let _ = std::process::Command::new("explorer.exe")
                                .arg(&self.profiles_dir)
                                .spawn();
                        }
                    });

                    ui.horizontal(|ui| {
                        if ui.button(ar("تحويل كل ملفات .amgp")).clicked() {
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

                // Enable toggle
                if self.page == SettingsPage::Overview {
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut self.working.enabled, "Enable controller -> keyboard/mouse").changed() {
                        let v = self.working.enabled;
                        if let Ok(mut e) = self.engine.lock() {
                            e.set_enabled(v);
                        }
                    }
                });
                ui.checkbox(
                    &mut self.working.disable_in_fullscreen,
                    "Auto-disable while a fullscreen app is focused",
                );
                ui.checkbox(
                    &mut self.working.vk_auto_show,
                    ar("إظهار الكيبورد تلقائياً عند النقر على حقل نصي (أو اضغط L3 للفتح يدوياً)"),
                );
                if ui
                    .checkbox(&mut self.working.autostart, "Start automatically with Windows")
                    .changed()
                {
                    let enable = self.working.autostart;
                    let exe = std::env::current_exe()
                        .map(|p| format!("\"{}\"", p.display()))
                        .unwrap_or_default();
                    self.status = match crate::platform::set_autostart(enable, &exe) {
                        Ok(()) => if enable { "Autostart enabled".into() } else { "Autostart removed".into() },
                        Err(e) => format!("Autostart error: {e}"),
                    };
                }
                ui.separator();
                }

                // Mouse
                if self.page == SettingsPage::Pointer {
                ui.group(|ui| {
                    ui.label(egui::RichText::new("Live stick calibration").strong());
                    ui.label("Move each stick. The amber ring is its dead zone; green means movement is registered.");
                    if let Some(pad) = gp_state {
                        ui.horizontal(|ui| {
                            stick_preview(
                                ui,
                                "Pointer · left stick",
                                pad.sThumbLX,
                                pad.sThumbLY,
                                self.working.controller.left_stick_dead_zone,
                            );
                            stick_preview(
                                ui,
                                "Scroll · right stick",
                                pad.sThumbRX,
                                pad.sThumbRY,
                                self.working.controller.right_stick_dead_zone,
                            );
                        });
                    } else {
                        ui.label("Connect a controller to preview its input.");
                    }
                });
                ui.add_space(8.0);
                ui.label(egui::RichText::new("Mouse").strong());
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.speed, 50.0..=3000.0).text("Speed (px/s)"),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.acceleration, 0.0..=8000.0)
                        .text("Acceleration"),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.acceleration_threshold, 0.0..=1.0)
                        .text("Acceleration threshold"),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.wheel_speed, 0.0..=20000.0)
                        .text("Scroll speed (ticks/sec at full deflection)"),
                );
                ui.label(egui::RichText::new(ar("منحنى العصا (مثل بروفايل Smooth Native Scroll)")).weak());
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.diagonal_range, 0.0..=100.0)
                        .text("8-way gate near centre (%) — stops diagonal drift"),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.modifier_zone, 0.0..=32767.0)
                        .text("Cruise zone — speed-up starts past this"),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.max_zone, 8000.0..=32767.0)
                        .text("Max zone — saturation point"),
                );

                // --- AltSnap: fine positioning ---
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut self.working.mouse.altsnap_enabled,
                        egui::RichText::new("AltSnap").strong(),
                    );
                    ui.label(
                        egui::RichText::new(ar("دفع العصا بقوة = حركة أدق، مناسبة لضبط الماوس بدقة"))
                            .weak()
                            .small(),
                    );
                });
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.altsnap_zone, 1000.0..=32000.0)
                        .text("AltSnap zone — precision starts past this"),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.mouse.altsnap_speed, 0.05..=1.0)
                        .text("AltSnap speed at full deflection (lower = finer)"),
                );
                // --- Sniper Mode: analog precision scaling on LT ---
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut self.working.mouse.sniper_enabled,
                        egui::RichText::new("Sniper Mode (وضع القناص)").strong(),
                    );
                    ui.label(
                        egui::RichText::new(ar("الضغط على LT يبطئ حركة الماوس تدريجياً لدقة فائقة"))
                            .weak()
                            .small(),
                    );
                });
                if self.working.mouse.sniper_enabled {
                    ui.add(
                        egui::Slider::new(&mut self.working.mouse.sniper_scale, 0.05..=0.8)
                            .text("Sniper speed multiplier at full squeeze (LT)"),
                    );
                }
                ui.checkbox(
                    &mut self.working.mouse.scroll_four_way,
                    ar("تثبيت عصة التمرير على 4 اتجاهات"),
                );
                ui.checkbox(
                    &mut self.working.mouse.scroll_flip_y,
                    ar("عكس التمرير عمودياً (إذا كان الرفع ينزل)"),
                );
                ui.horizontal(|ui| {
                    ui.label("Horizontal pointer speed (0 = same)");
                    let mut sx = self.working.mouse.speed_x;
                    if ui.add(egui::DragValue::new(&mut sx).speed(10.0)).changed() {
                        self.working.mouse.speed_x = sx.clamp(0.0, 4000.0);
                    }
                    ui.label("Vertical (0 = same)");
                    let mut sy = self.working.mouse.speed_y;
                    if ui.add(egui::DragValue::new(&mut sy).speed(10.0)).changed() {
                        self.working.mouse.speed_y = sy.clamp(0.0, 4000.0);
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Scroll X");
                    let mut wx = self.working.mouse.wheel_speed_x;
                    if ui.add(egui::DragValue::new(&mut wx).speed(10.0)).changed() {
                        self.working.mouse.wheel_speed_x = wx.clamp(0.0, 20000.0);
                    }
                    ui.label("Scroll Y (0 = same)");
                    let mut wy = self.working.mouse.wheel_speed_y;
                    if ui.add(egui::DragValue::new(&mut wy).speed(10.0)).changed() {
                        self.working.mouse.wheel_speed_y = wy.clamp(0.0, 20000.0);
                    }
                });
                ui.separator();
                }

                // On-screen keyboard preferences.
                if self.page == SettingsPage::Keyboard {
                ui.label(egui::RichText::new(ar("الكيبورد الافتراضي")).strong());
                ui.checkbox(
                    &mut self.working.keyboard.remember_position,
                    ar("تذكر مكان الكيبورد بعد التشغيل"),
                );
                ui.checkbox(&mut self.working.keyboard.start_arabic, ar("ابدأ بلغة عربية"));
                ui.checkbox(
                    &mut self.working.keyboard.show_hints_on_open,
                    ar("اظهر شرح الأزرار عند الفتح"),
                );
                ui.checkbox(
                    &mut self.working.keyboard.split_mode,
                    ar("الكيبورد المنقسم السريع افتراضياً (Dual-Stick Split Mode)"),
                );
                ui.checkbox(
                    &mut self.working.keyboard.smart_caret_avoidance,
                    ar("تفادي حقل النصوص تلقائياً (Smart Caret Collision Avoidance)"),
                );
                ui.checkbox(
                    &mut self.working.keyboard.mechanical_sfx,
                    ar("أصوات نقر ميكانيكية ناعمة (Mechanical Keyboard SFX)"),
                );
                ui.horizontal(|ui| {
                    egui::ComboBox::new("kb_pos", ar("المكان"))
                        .selected_text(match self.working.keyboard.position {
                            crate::config::KeyboardPosition::FloatBottom => "Float (Above Input)",
                            crate::config::KeyboardPosition::Bottom => "Bottom",
                            crate::config::KeyboardPosition::Top => "Top",
                            crate::config::KeyboardPosition::Left => "Left",
                            crate::config::KeyboardPosition::Right => "Right",
                            crate::config::KeyboardPosition::Custom => "Custom",
                        })
                        .show_ui(ui, |ui| {
                            use crate::config::KeyboardPosition as KP;
                            for (v, label) in [
                                (KP::FloatBottom, "Float (Above Input)"),
                                (KP::Bottom, "Bottom"),
                                (KP::Top, "Top"),
                                (KP::Left, "Left"),
                                (KP::Right, "Right"),
                                (KP::Custom, "Custom (remembered)"),
                            ] {
                                ui.selectable_value(&mut self.working.keyboard.position, v, label);
                            }
                        });
                    ui.label(ar("الحجم"));
                    ui.add(
                        egui::Slider::new(&mut self.working.keyboard.scale, 0.75..=1.5)
                            .text("Scale"),
                    );
                });
                ui.separator();
                }

                // Controller
                if self.page == SettingsPage::Pointer {
                ui.label(egui::RichText::new("Controller").strong());
                ui.add(
                    egui::Slider::new(&mut self.working.controller.left_stick_dead_zone, 0..=32767)
                        .text("Left stick dead zone"),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.controller.right_stick_dead_zone, 0..=32767)
                        .text("Right stick dead zone"),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.controller.trigger_threshold, 0..=255)
                        .text("Trigger press threshold"),
                );
                ui.add(
                    egui::Slider::new(&mut self.working.controller.controller_index, 0..=3)
                        .text("Controller slot"),
                );
                ui.checkbox(
                    &mut self.working.controller.haptic_feedback,
                    ar("تفعيل الاهتزاز اللمسي التفاعلي (Haptic Feedback)"),
                );
                ui.checkbox(
                    &mut self.working.osd_enabled,
                    ar("تفعيل شاشة الإشعارات العائمة (Minimalist OSD / HUD Overlay)"),
                );
                ui.separator();
                }

                // Bindings overview partitioned into Base layer and Modifier layer.
                if self.page == SettingsPage::Bindings {
                ui.label(egui::RichText::new(ar("Controller Bindings / تعيينات الأزرار:")).strong());
                
                // 1. Base Layer (Direct button presses)
                ui.group(|ui| {
                    ui.label(egui::RichText::new(ar("1. الأزرار الأساسية (الوضع المباشر - بدون زر Back)")).color(egui::Color32::from_rgb(100, 200, 255)).strong());
                    egui::Grid::new("base_bindings_grid").striped(true).min_col_width(120.0).show(ui, |ui| {
                        ui.label(egui::RichText::new(ar("الزر (Button)")).strong());
                        ui.label(egui::RichText::new(ar("الوظيفة (Action)")).strong());
                        ui.end_row();

                        // Fixed primary axes
                        ui.label(ar("Left Stick (عصا التحكم اليسرى)"));
                        ui.label(ar("Mouse Pointer (تحريك مؤشر الفأرة)"));
                        ui.end_row();

                        ui.label(ar("Right Stick (عصا التحكم اليمنى)"));
                        ui.label(ar("Scroll Wheel (التمرير بالبكرة)"));
                        ui.end_row();

                        for b in self.working.bindings.iter_mut().filter(|b| b.modifier.is_none()) {
                            if matches!(b.input, Input::LeftStick | Input::RightStick) {
                                continue;
                            }
                            let input_name = format_input(b.input);
                            ui.label(egui::RichText::new(input_name).strong());
                            if matches!(b.action, Action::Modifier(_)) {
                                ui.label(format_action(&b.action));
                            } else {
                                action_picker(ui, ("base_act", b.input), &mut b.action);
                            }
                            ui.end_row();
                        }
                    });
                });

                ui.add_space(8.0);

                // 2. Secondary Modifier Layer (Hold Back button)
                ui.group(|ui| {
                    ui.label(egui::RichText::new(ar("2. الوظائف الإضافية (عند الضغط على زر Back)")).color(egui::Color32::from_rgb(255, 180, 80)).strong());
                    egui::Grid::new("modifier_bindings_grid").striped(true).min_col_width(140.0).show(ui, |ui| {
                        ui.label(egui::RichText::new(ar("الاختصار (Combination)")).strong());
                        ui.label(egui::RichText::new(ar("الوظيفة (Action)")).strong());
                        ui.end_row();

                        for b in self.working.bindings.iter_mut().filter(|b| b.modifier.is_some()) {
                            let combo = format!("Back + {}", format_input(b.input));
                            ui.label(egui::RichText::new(combo).strong());
                            action_picker(ui, ("mod_act", b.input), &mut b.action);
                            ui.end_row();
                        }
                    });
                });

                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if self.page == SettingsPage::Profiles && ui.button("Import .amgp profile...").clicked() {
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
                            self.status = "No .amgp profiles found in Downloads folder".into();
                        }
                    }
                });
            });
        });

        // Keep live controller visualizer updating smoothly while settings window is open
        ctx.request_repaint_after(std::time::Duration::from_millis(33));
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
    use super::merge_edits;
    use serde_json::json;

    #[test]
    fn save_preserves_external_changes_to_untouched_fields() {
        let original = json!({"enabled": true, "active_profile": "Default", "mouse": {"speed": 600, "acceleration": 800}});
        let edited = json!({"enabled": true, "active_profile": "Default", "mouse": {"speed": 750, "acceleration": 800}});
        let current = json!({"enabled": false, "active_profile": "Browser", "mouse": {"speed": 600, "acceleration": 400}});
        let merged = merge_edits(&original, &edited, &current);
        assert_eq!(merged, json!({"enabled": false, "active_profile": "Browser", "mouse": {"speed": 750, "acceleration": 400}}));
    }
}
