//! Modern 8-slice Fluent 2 circular pie menu for rapid desktop actions.

use super::render::{
    C_ACCENT, C_ACCENT_HOVER, C_CHROME, C_CLOSE_HOVER, C_ON_ACCENT, C_PANEL_BORDER, C_TEXT,
    C_TEXT_DIM,
};
use eframe::egui;

pub fn paint_radial_menu(
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
        ("📋 Paste", "لصق"),
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

    let response = ui.allocate_rect(area, egui::Sense::click());
    let ptr = response.hover_pos().or_else(|| ui.input(|i| i.pointer.hover_pos()));
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

    let clicked = response.clicked() || ui.input(|i| i.pointer.primary_clicked());
    if clicked {
        if hovered_center {
            chosen_slice = Some(8); // Cancel
        } else if let Some(s) = hovered_slice.or(selected_slice) {
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
