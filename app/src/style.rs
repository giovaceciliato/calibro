//! Shared dark-blue workspace palette for desktop, browser and viewport overlays.

use egui::{Color32, CornerRadius, FontId, Stroke, TextStyle};

pub const ACCENT: Color32 = Color32::from_rgb(91, 173, 255);
pub const PANEL: Color32 = Color32::from_rgb(17, 28, 45);
pub const SURFACE: Color32 = Color32::from_rgb(25, 40, 61);
pub const BORDER: Color32 = Color32::from_rgb(43, 62, 85);
pub const TEXT: Color32 = Color32::from_rgb(225, 234, 246);
pub const MUTED: Color32 = Color32::from_rgb(149, 169, 194);

pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(13.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(13.0));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::proportional(18.0));
        style.spacing.item_spacing = egui::vec2(8.0, 7.0);
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.interact_size.y = 30.0;
        style.spacing.window_margin = egui::Margin::same(14);
        let v = &mut style.visuals;
        v.override_text_color = Some(TEXT);
        v.panel_fill = PANEL;
        v.window_fill = SURFACE;
        v.extreme_bg_color = Color32::from_rgb(12, 22, 37);
        v.faint_bg_color = SURFACE;
        // the slider rail uses the inactive widget fill (same as the window): show the
        // filled part in the selection colour so the rail stays visible
        v.slider_trailing_fill = true;
        v.hyperlink_color = ACCENT;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.selection.bg_fill = Color32::from_rgb(33, 77, 121);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
        v.widgets.inactive.bg_fill = SURFACE;
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(22, 36, 55);
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.hovered.bg_fill = Color32::from_rgb(37, 58, 83);
        v.widgets.hovered.weak_bg_fill = v.widgets.hovered.bg_fill;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(75, 116, 156));
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        v.widgets.active.bg_fill = Color32::from_rgb(35, 83, 130);
        v.widgets.active.weak_bg_fill = v.widgets.active.bg_fill;
        v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
        v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        for widget in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
        ] {
            widget.corner_radius = CornerRadius::same(6);
        }
        v.window_corner_radius = CornerRadius::same(10);
    });
}

pub fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(title).size(10.0).strong().color(MUTED));
    ui.add_space(2.0);
}

/// Draw small native vector icons, so labels never depend on icon-font glyphs.
fn tool_icon(p: &egui::Painter, center: egui::Pos2, label: &str, color: Color32) {
    let s = Stroke::new(1.4, color);
    let pt = |x, y| center + egui::vec2(x, y);
    let line = |a, b| {
        p.line_segment([a, b], s);
    };
    match label {
        "Select" => {
            p.add(egui::Shape::closed_line(
                vec![pt(-5.0, -7.0), pt(6.0, 1.0), pt(0.0, 2.0), pt(-3.0, 7.0)],
                s,
            ));
        }
        "Circle" | "Sphere" | "Circular pattern" => {
            p.circle_stroke(center, 6.0, s);
        }
        "Arc" | "3-point arc" | "Revolve" | "Fillet" => {
            p.add(egui::Shape::line(
                vec![
                    pt(-6.0, 6.0),
                    pt(-6.0, 0.0),
                    pt(-4.0, -4.0),
                    pt(0.0, -6.0),
                    pt(6.0, -6.0),
                ],
                s,
            ));
        }
        "Line" | "Measure" | "New sketch" => {
            line(pt(-6.0, 6.0), pt(6.0, -6.0));
            line(pt(-6.0, 2.0), pt(-2.0, 6.0));
            line(pt(2.0, -6.0), pt(6.0, -2.0));
        }
        "Move / Rotate" | "Extrude" => {
            line(pt(0.0, 7.0), pt(0.0, -7.0));
            line(pt(-4.0, -3.0), pt(0.0, -7.0));
            line(pt(4.0, -3.0), pt(0.0, -7.0));
            line(pt(-6.0, 7.0), pt(6.0, 7.0));
        }
        "Trim" => {
            // scissors: two blades and two rings
            line(pt(-6.0, -6.0), pt(5.0, 3.0));
            line(pt(-6.0, 6.0), pt(5.0, -3.0));
            p.circle_stroke(pt(5.5, 5.0), 2.2, s);
            p.circle_stroke(pt(5.5, -5.0), 2.2, s);
        }
        "Union" | "Subtract" | "Intersect" => {
            p.rect_stroke(
                egui::Rect::from_min_max(pt(-7.0, -6.0), pt(2.0, 3.0)),
                1,
                s,
                egui::StrokeKind::Inside,
            );
            p.rect_stroke(
                egui::Rect::from_min_max(pt(-2.0, -2.0), pt(7.0, 7.0)),
                1,
                s,
                egui::StrokeKind::Inside,
            );
        }
        _ => {
            p.rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(12.0, 12.0)),
                2,
                s,
                egui::StrokeKind::Inside,
            );
            line(pt(-6.0, -2.0), pt(6.0, -2.0));
        }
    }
}

pub fn tool_button(ui: &mut egui::Ui, selected: bool, label: &str, tip: &str) -> bool {
    let response = ui.add_sized(
        [ui.available_width(), 34.0],
        egui::Button::selectable(selected, ""),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    let color = if !ui.is_enabled() {
        MUTED.gamma_multiply(0.45)
    } else if selected {
        ACCENT
    } else {
        TEXT
    };
    let r = response.rect;
    tool_icon(
        ui.painter(),
        egui::pos2(r.left() + 18.0, r.center().y),
        label,
        color,
    );
    ui.painter().text(
        egui::pos2(r.left() + 36.0, r.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        color,
    );
    let shortcut = match label {
        "Select" => "Esc",
        "Measure" => "M",
        "New sketch" => "S",
        "Extrude" => "E",
        "Line" => "L",
        "Rectangle" => "R",
        "Circle" => "C",
        "Arc" => "A",
        "Polygon" => "P",
        "Trim" => "T",
        _ => "",
    };
    ui.painter().text(
        egui::pos2(r.right() - 10.0, r.center().y),
        egui::Align2::RIGHT_CENTER,
        shortcut,
        FontId::proportional(10.0),
        if ui.is_enabled() {
            MUTED
        } else {
            MUTED.gamma_multiply(0.45)
        },
    );
    response.on_hover_text(tip).clicked()
}
