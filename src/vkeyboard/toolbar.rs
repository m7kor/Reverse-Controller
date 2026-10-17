//! Interactive text preview capsule, quick action buttons, and predictive word suggestions.

use super::layout::{intern, Act, Lang};
use super::render::{
    C_ACCENT, C_ACCENT_GLOW, C_ACCENT_HOVER, C_ACCENT_SUBTLE, C_CLOSE_HOVER, C_KEY_HOVER,
    C_ON_ACCENT, C_TEXT, C_TEXT_DIM,
};
use eframe::egui;
use std::time::Duration;

pub struct ToolbarEvent {
    pub action: Option<Act>,
}

#[allow(clippy::too_many_arguments)]
pub fn paint_toolbar(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    typed_buffer: &str,
    typed_word: &str,
    lang: Lang,
    _split_mode: bool,
    focused: bool,
    focused_col: usize,
) -> (ToolbarEvent, usize) {
    let mut event = ToolbarEvent { action: None };
    let painter = ui.painter_at(rect.expand(2.0));
    painter.rect_filled(rect, 8.0, egui::Color32::from_rgba_premultiplied(0, 0, 0, 45));

    let is_ar = lang == Lang::Ar;
    let has_clear = !typed_buffer.is_empty();

    struct RightBtn {
        label: String,
        act: Act,
        is_primary: bool,
        col_idx: usize,
    }

    let mut right_btns = Vec::new();
    let start_col = if has_clear { 1 } else { 0 };

    let copy_lbl = if is_ar {
        crate::arabic::reshape_ar("📄 نسخ")
    } else {
        "📄 Copy".to_string()
    };
    let paste_lbl = if is_ar {
        crate::arabic::reshape_ar("📋 لصق")
    } else {
        "📋 Paste".to_string()
    };
    let newline_lbl = if is_ar {
        crate::arabic::reshape_ar("↵ سطر جديد")
    } else {
        "↵ New Line".to_string()
    };

    let add_shortcut = |btns: &mut Vec<RightBtn>, label: String, act: Act| {
        let col = start_col + btns.len();
        btns.push(RightBtn {
            label,
            act,
            is_primary: false,
            col_idx: col,
        });
    };
    let add_common = |btns: &mut Vec<RightBtn>| {
        add_shortcut(btns, copy_lbl.clone(), Act::Copy);
        add_shortcut(btns, paste_lbl.clone(), Act::Paste);
        add_shortcut(btns, newline_lbl.clone(), Act::ShiftEnter);
    };

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
        add_common(&mut right_btns);
    } else {
        add_common(&mut right_btns);
    }

    // Left Section: Live Search & Typing Bar Capsule
    let search_w = (rect.width() * 0.26).round();
    let search_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + 2.0, rect.top() + 2.0),
        egui::vec2(search_w - 4.0, rect.height() - 4.0),
    );

    let search_fill = egui::Color32::from_rgb(18, 19, 23);
    let search_stroke =
        egui::Stroke::new(1.0, egui::Color32::from_rgba_premultiplied(35, 35, 42, 35));
    painter.rect_filled(search_rect, 7.0, search_fill);
    painter.rect_stroke(search_rect, 7.0, search_stroke);

    let icon_col = if !typed_buffer.is_empty() {
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

        if resp.clicked() || (hovered && ui.input(|i| i.pointer.primary_pressed())) {
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

    // Right Section: Quick Actions / Word Predictions
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

        let (fill, stroke, text_col) = if is_focus {
            (C_ACCENT, egui::Stroke::new(1.5, C_ACCENT_HOVER), C_ON_ACCENT)
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

        if resp.clicked() || (hovered && ui.input(|i| i.pointer.primary_pressed())) {
            event.action = Some(btn.act);
        }
    }

    let total_count = (if has_clear { 1 } else { 0 }) + right_btns.len();
    (event, total_count)
}
