//! Small shared widgets for the settings screen.

use crate::ui::theme::Theme;

/// An iOS-style switch followed by an explicit "ON"/"OFF" badge and a label.
/// Clicking anywhere on the row flips the value. Returns the row's response
/// (`.changed()` is set when the value flipped).
pub fn toggle(ui: &mut egui::Ui, theme: &Theme, on: &mut bool, label: &str) -> egui::Response {
    toggle_enabled(ui, theme, true, on, label)
}

/// Like [`toggle`], but greyed out and not clickable when `enabled` is false.
pub fn toggle_enabled(
    ui: &mut egui::Ui,
    theme: &Theme,
    enabled: bool,
    on: &mut bool,
    label: &str,
) -> egui::Response {
    let inner = ui.horizontal(|ui| {
        let size = egui::vec2(34.0, 18.0);
        let sense = if enabled {
            egui::Sense::click()
        } else {
            egui::Sense::hover()
        };
        let (rect, mut switch) = ui.allocate_exact_size(size, sense);

        let badge_color = if *on {
            theme.color(theme.status_ok)
        } else {
            theme.color(theme.text_muted)
        };
        let badge = ui.add(
            egui::Label::new(
                egui::RichText::new(if *on { "ON " } else { "OFF" })
                    .monospace()
                    .strong()
                    .color(badge_color),
            )
            .sense(sense),
        );
        let text = ui.add_enabled(enabled, egui::Label::new(label).sense(sense));

        let clicked = switch.clicked() || badge.clicked() || text.clicked();
        if enabled && clicked {
            *on = !*on;
            switch.mark_changed();
        }

        if ui.is_rect_visible(rect) {
            let t = ui.ctx().animate_bool_responsive(switch.id, *on);
            let off_fill = theme.color(theme.text_muted);
            let on_fill = theme.color(theme.accent);
            let mut fill = egui::Color32::from_rgb(
                lerp_u8(off_fill.r(), on_fill.r(), t),
                lerp_u8(off_fill.g(), on_fill.g(), t),
                lerp_u8(off_fill.b(), on_fill.b(), t),
            );
            if !enabled {
                fill = fill.gamma_multiply(0.4);
            }
            let radius = rect.height() / 2.0;
            ui.painter().rect_filled(rect, radius, fill);
            if switch.hovered() {
                ui.painter().rect_stroke(
                    rect,
                    radius,
                    egui::Stroke::new(1.0, theme.color(theme.text_secondary)),
                    egui::StrokeKind::Outside,
                );
            }
            let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
            let knob = egui::pos2(knob_x, rect.center().y);
            let knob_color = if enabled {
                egui::Color32::WHITE
            } else {
                egui::Color32::from_gray(160)
            };
            ui.painter().circle_filled(knob, radius - 3.0, knob_color);
        }
        switch
    });
    inner.inner | inner.response
}

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}
