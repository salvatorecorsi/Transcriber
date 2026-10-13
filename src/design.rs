use eframe::egui::{self, Color32, FontId, Pos2, Response, RichText, Sense, Shape, Stroke, Vec2};

pub fn configure(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Petaluma Script".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/PetalumaScript.otf")).into(),
    );
    fonts.font_data.insert(
        "Petaluma".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Petaluma.otf")).into(),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .insert(0, "Petaluma Script".into());
    }
    fonts.families.insert(
        egui::FontFamily::Name("JazzMusic".into()),
        vec!["Petaluma".into()],
    );
    context.set_fonts(fonts);
    context.set_theme(egui::Theme::Light);
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = Color32::WHITE;
    visuals.window_fill = Color32::WHITE;
    visuals.extreme_bg_color = Color32::WHITE;
    visuals.faint_bg_color = Color32::WHITE;
    visuals.selection.bg_fill = Color32::BLACK;
    visuals.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::BLACK);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::BLACK);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.bg_fill = Color32::WHITE;
        widget.weak_bg_fill = Color32::WHITE;
        widget.bg_stroke = Stroke::new(1.0, Color32::BLACK);
        widget.fg_stroke = Stroke::new(1.0, Color32::BLACK);
        widget.corner_radius = egui::CornerRadius::ZERO;
    }
    visuals.window_shadow = egui::epaint::Shadow::NONE;
    visuals.popup_shadow = egui::epaint::Shadow::NONE;
    context.set_visuals(visuals);
    context.style_mut(|style| {
        style.spacing.item_spacing = Vec2::new(8.0, 7.0);
        style.spacing.button_padding = Vec2::new(11.0, 7.0);
        style.spacing.slider_width = 105.0;
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(17.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(17.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(13.0));
    });
}

pub fn button(label: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label.to_owned()).color(Color32::WHITE))
        .fill(Color32::BLACK)
        .corner_radius(0.0)
}

pub fn small_button(ui: &mut egui::Ui, label: &str) -> Response {
    ui.add(button(label).small())
}

pub fn slider(ui: &mut egui::Ui, slider: egui::Slider<'_>) -> Response {
    ui.scope(|ui| {
        ui.visuals_mut().widgets.inactive.bg_fill = Color32::BLACK;
        ui.add(slider)
    })
    .inner
}

pub fn ink_button(ui: &mut egui::Ui, label: &str, minimum: Vec2, selected: bool) -> Response {
    let font = FontId::proportional(17.0);
    let text = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, Color32::WHITE);
    let size = (text.size() + Vec2::new(24.0, 15.0)).max(minimum);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let seed = label.bytes().fold(0usize, |total, byte| {
        total.wrapping_mul(31).wrapping_add(byte as usize)
    });
    let skew = 0.6 + (seed % 5) as f32 * 0.22;
    let polygon = vec![
        Pos2::new(rect.left() + 0.5, rect.top() + skew),
        Pos2::new(rect.right() - 0.5, rect.top()),
        Pos2::new(rect.right(), rect.bottom() - skew),
        Pos2::new(rect.left(), rect.bottom()),
    ];
    let ink = if ui.is_enabled() {
        Color32::BLACK
    } else {
        Color32::BLACK.gamma_multiply(0.4)
    };
    ui.painter()
        .add(Shape::convex_polygon(polygon, ink, Stroke::NONE));
    ui.painter().galley(
        rect.center() - text.size() * 0.5 + Vec2::new(0.0, 0.25),
        text,
        Color32::WHITE,
    );
    if selected || response.hovered() {
        ui.painter().line_segment(
            [
                Pos2::new(rect.left() + 9.0, rect.bottom() - 4.5),
                Pos2::new(rect.right() - 8.0, rect.bottom() - 5.4),
            ],
            Stroke::new(1.0, Color32::WHITE),
        );
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
