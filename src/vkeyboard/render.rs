//! Fluent 2 visual styling, keycap painting, vector glyphs, and chrome headers.

use super::layout::{
    Act, Glyph, Lang, Layer, ShiftState, CHROME_H, KEY_H, PAD, ROUNDING,
};
use eframe::egui;

// ---- Modern Windows 11 Fluent 2 / Mica Dark Theme ----
pub const C_PANEL: egui::Color32 = egui::Color32::from_rgba_premultiplied(18, 19, 24, 235);
pub const C_PANEL_BORDER: egui::Color32 = egui::Color32::from_rgba_premultiplied(35, 35, 45, 35);
pub const C_CHROME: egui::Color32 = egui::Color32::from_rgb(24, 25, 30);
pub const C_KEY: egui::Color32 = egui::Color32::from_rgb(44, 46, 52);
pub const C_KEY_SPECIAL: egui::Color32 = egui::Color32::from_rgb(32, 33, 38);
pub const C_KEY_HOVER: egui::Color32 = egui::Color32::from_rgb(58, 61, 70);
pub const C_KEY_DOWN: egui::Color32 = egui::Color32::from_rgb(24, 25, 29);
pub const C_KEY_TOP_BEVEL: egui::Color32 = egui::Color32::from_rgba_premultiplied(12, 12, 12, 12);
pub const C_KEY_BOT_SHADOW: egui::Color32 = egui::Color32::from_rgba_premultiplied(0, 0, 0, 75);
pub const C_ACCENT: egui::Color32 = egui::Color32::from_rgb(255, 170, 0);
pub const C_ACCENT_HOVER: egui::Color32 = egui::Color32::from_rgb(255, 188, 50);
pub const C_ACCENT_DOWN: egui::Color32 = egui::Color32::from_rgb(224, 145, 0);
pub const C_ACCENT_GLOW: egui::Color32 = egui::Color32::from_rgba_premultiplied(45, 30, 0, 45);
pub const C_ACCENT_SUBTLE: egui::Color32 = egui::Color32::from_rgba_premultiplied(25, 16, 0, 25);
pub const C_ON_ACCENT: egui::Color32 = egui::Color32::from_rgb(16, 16, 18);
pub const C_TEXT: egui::Color32 = egui::Color32::from_rgb(245, 246, 250);
pub const C_TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(148, 153, 166);
pub const C_PILL: egui::Color32 = egui::Color32::from_rgba_premultiplied(55, 55, 55, 55);
pub const C_SCRIM: egui::Color32 = egui::Color32::from_rgba_premultiplied(11, 12, 15, 245);
pub const C_CLOSE_HOVER: egui::Color32 = egui::Color32::from_rgb(215, 38, 38);

/// Controller cheat-sheet shown by the 🎮 key / `?` button / R3.
pub const HINTS: &[(&str, &str)] = &[
    ("D-Pad / Left stick", "Move between keys"),
    ("Up from row 0", "Quick actions & word predictions"),
    ("L1 / R1 (LB / RB)", "Move text cursor ◄ / ►"),
    ("R2 (RT)", "Capitalize letters (Shift)"),
    ("L2 (LT)", "Numbers & symbols (#+=)"),
    ("R3 (Right stick click)", "Move keyboard anywhere"),
    ("A", "Press the highlighted key"),
    ("X / Y", "Backspace / Space"),
    ("RT + X / RT + Y", "Delete word / Auto-complete"),
    ("Start", "Enter"),
    ("RT + Start", "Shift + Enter (New line)"),
    ("Back (Select)", "Switch language (EN / ع)"),
    ("B / L3", "Close keyboard"),
];

pub const HINTS_AR: &[(&str, &str)] = &[
    ("الأسهم / عصا التحكم", "التنقل بين الأزرار"),
    ("أعلى من الصف 0", "الإجراءات السريعة واقتراحات الكلمات"),
    ("L1 / R1 (LB / RB)", "تحريك مؤشر الكتابة ◄ / ►"),
    ("R2 (RT)", "التبديل إلى الحروف الكبيرة / الرموز (Shift)"),
    ("L2 (LT)", "الأرقام والرموز (#+=)"),
    ("R3 (ضغط العصا)", "تحريك موضع الكيبورد بحرية"),
    ("A", "الضغط على الزر المحدد"),
    ("X / Y", "مسح (Backspace) / مسافة (Space)"),
    ("RT + X / RT + Y", "حذف كلمة / اختيار الاقتراح الأول"),
    ("Start", "إدخال (Enter)"),
    ("RT + Start", "سطر جديد (Shift + Enter)"),
    ("Back (Select)", "تبديل اللغة (ع / EN)"),
    ("B / L3", "إغلاق الكيبورد"),
];

pub struct ChromeStatus {
    pub lang: Lang,
    pub layer: Layer,
    pub shift: ShiftState,
    pub caps: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub altgr: bool,
    pub hints_on: bool,
}

pub struct Chrome {
    pub close: bool,
    pub help: bool,
    pub radial: bool,
    pub drag: egui::Response,
}

/// Top Fluent chrome strip
pub fn paint_chrome(ui: &mut egui::Ui, area: egui::Rect, st: &ChromeStatus) -> Chrome {
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

    let radial = button(ui, area.right() - 44.0, "vk_radial_btn", "◎", false, C_KEY_HOVER);
    let close = button(ui, area.right() - 14.0, "vk_close", "\u{2715}", false, C_CLOSE_HOVER);

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

pub struct KeyView<'a> {
    pub label: &'a str,
    pub alt: Option<&'a str>,
    pub act: Act,
    pub glyph: Glyph,
    pub special: bool,
    pub focused: bool,
    pub split_left: bool,
    pub split_right: bool,
    pub active: bool,
    pub locked: bool,
    pub flash: bool,
    pub enabled: bool,
}

pub struct KeyEvent {
    pub down_now: bool,
    #[allow(dead_code)]
    pub clicked: bool,
    pub held: bool,
}

/// Paint one Fluent 2 tactile keycap at an explicit rect.
pub fn paint_key(ui: &mut egui::Ui, id: egui::Id, rect: egui::Rect, v: &KeyView) -> KeyEvent {
    let sense = if v.enabled { egui::Sense::click() } else { egui::Sense::hover() };
    let resp = ui.interact(rect, id, sense);
    let painter = ui.painter_at(rect.expand(4.0));

    let hovered = v.enabled && resp.hovered();
    let down_now = hovered && ui.input(|i| i.pointer.primary_pressed());
    let clicked = v.enabled && resp.clicked();
    let held = v.enabled && resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_down());
    let is_down = held || v.flash;
    let is_focused = v.focused || v.split_left || v.split_right;

    let draw_rect = if is_down {
        rect.translate(egui::vec2(0.0, 1.0))
    } else {
        rect
    };

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

    if !is_down && !is_focused {
        let shadow_rect = egui::Rect::from_min_max(
            egui::pos2(draw_rect.left() + 1.0, draw_rect.bottom() - 2.0),
            egui::pos2(draw_rect.right() - 1.0, draw_rect.bottom() + 1.0),
        );
        painter.rect_filled(shadow_rect, ROUNDING, C_KEY_BOT_SHADOW);
    }

    painter.rect_filled(draw_rect, ROUNDING, fill);

    if !is_down && !is_focused && !v.active {
        painter.line_segment(
            [
                egui::pos2(draw_rect.left() + ROUNDING, draw_rect.top() + 0.8),
                egui::pos2(draw_rect.right() - ROUNDING, draw_rect.top() + 0.8),
            ],
            egui::Stroke::new(1.0, C_KEY_TOP_BEVEL),
        );
    }

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

    if v.act == Act::Caps {
        let led_col = if v.active { C_ON_ACCENT } else { C_PILL };
        painter.circle_filled(
            egui::pos2(draw_rect.left() + 7.5, draw_rect.top() + 7.5),
            2.5,
            led_col,
        );
    }

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

pub fn draw_spacebar(p: &egui::Painter, rect: egui::Rect, label: &str, col: egui::Color32) {
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

pub fn draw_win(p: &egui::Painter, rect: egui::Rect, col: egui::Color32) {
    let s = rect.height() * 0.17;
    let gap = 1.8;
    let c = rect.center();
    for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let center_sq = egui::pos2(c.x + dx * (s * 0.5 + gap * 0.5), c.y + dy * (s * 0.5 + gap * 0.5));
        p.rect_filled(egui::Rect::from_center_size(center_sq, egui::vec2(s, s)), 1.5, col);
    }
}

pub fn draw_pad(p: &egui::Painter, rect: egui::Rect, col: egui::Color32) {
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

pub fn draw_enter(p: &egui::Painter, rect: egui::Rect, col: egui::Color32) {
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

pub fn draw_shift_enter(p: &egui::Painter, rect: egui::Rect, col: egui::Color32) {
    let c = rect.center();
    let h = rect.height() * 0.36;
    let stroke = egui::Stroke::new(2.0, col);

    let sx = c.x - 12.0;
    let sy = c.y;
    p.line_segment([egui::pos2(sx, sy + h * 0.45), egui::pos2(sx, sy - h * 0.45)], stroke);
    p.line_segment([egui::pos2(sx, sy - h * 0.45), egui::pos2(sx - 4.5, sy - h * 0.05)], stroke);
    p.line_segment([egui::pos2(sx, sy - h * 0.45), egui::pos2(sx + 4.5, sy - h * 0.05)], stroke);

    let ex = c.x + 9.0;
    let w = rect.height() * 0.28;
    let pt = |x: f32, y: f32| egui::pos2(ex + x, c.y + y);
    p.line_segment([pt(w * 0.45, -h * 0.40), pt(w * 0.45, h * 0.30)], stroke);
    p.line_segment([pt(w * 0.45, h * 0.30), pt(-w * 0.45, h * 0.30)], stroke);
    p.line_segment([pt(-w * 0.45, h * 0.30), pt(-w * 0.12, 0.0)], stroke);
    p.line_segment([pt(-w * 0.45, h * 0.30), pt(-w * 0.12, h * 0.60)], stroke);
}

pub fn draw_backspace(p: &egui::Painter, rect: egui::Rect, col: egui::Color32) {
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

pub fn paint_hints(ui: &mut egui::Ui, area: egui::Rect, lang: Lang) -> bool {
    let panel = egui::Rect::from_min_max(
        egui::pos2(area.left() + 4.0, area.top() + CHROME_H + 2.0),
        egui::pos2(area.right() - 4.0, area.bottom() - 2.0),
    );
    let resp = ui.interact(panel, egui::Id::new("vk_hints"), egui::Sense::click());
    let p = ui.painter();
    p.rect_filled(panel, 10.0, C_SCRIM);
    p.rect_stroke(panel, 10.0, egui::Stroke::new(1.5, C_ACCENT));

    let is_ar = lang == Lang::Ar;
    let title = if is_ar {
        crate::arabic::reshape_ar("🎮 اختصارات يد التحكم")
    } else {
        "🎮 Controller Shortcuts".to_string()
    };
    p.text(
        egui::pos2(panel.left() + 18.0, panel.top() + 12.0),
        egui::Align2::LEFT_TOP,
        title,
        egui::FontId::proportional(15.0),
        C_ACCENT,
    );

    let hints_list = if is_ar { HINTS_AR } else { HINTS };
    let per_col = hints_list.len().div_ceil(2);
    let col_w = panel.width() * 0.5;
    let line_h = 24.0;
    for (i, (button, what)) in hints_list.iter().enumerate() {
        let (col, row) = (i / per_col, i % per_col);
        let x = panel.left() + 18.0 + col as f32 * col_w;
        let y = panel.top() + 42.0 + row as f32 * line_h;

        let badge_rect = egui::Rect::from_min_size(egui::pos2(x, y - 1.0), egui::vec2(126.0, 19.0));
        p.rect_filled(badge_rect, 4.0, C_KEY_SPECIAL);
        p.rect_stroke(badge_rect, 4.0, egui::Stroke::new(1.0, C_PANEL_BORDER));
        let button_text = if is_ar { crate::arabic::reshape_ar(button) } else { button.to_string() };
        p.text(
            badge_rect.center(),
            egui::Align2::CENTER_CENTER,
            button_text,
            egui::FontId::proportional(12.0),
            C_TEXT,
        );
        let what_text = if is_ar { crate::arabic::reshape_ar(what) } else { what.to_string() };
        p.text(
            egui::pos2(x + 136.0, y),
            egui::Align2::LEFT_TOP,
            what_text,
            egui::FontId::proportional(13.0),
            C_TEXT_DIM,
        );
    }
    let footer = if is_ar {
        crate::arabic::reshape_ar("انقر في أي مكان أو اضغط R3 للإغلاق")
    } else {
        "Click anywhere or press R3 to dismiss".to_string()
    };
    p.text(
        egui::pos2(panel.center().x, panel.bottom() - 10.0),
        egui::Align2::CENTER_BOTTOM,
        footer,
        egui::FontId::proportional(11.5),
        C_TEXT_DIM,
    );
    resp.clicked()
}
