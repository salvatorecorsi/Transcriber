use eframe::egui::{self, Color32, FontId, Pos2, Response, RichText, Sense, Shape, Stroke, Vec2};

pub const PAPER: Color32 = Color32::from_rgb(245, 245, 245);
pub const WHITE: Color32 = Color32::WHITE;
pub const INK: Color32 = Color32::BLACK;
pub const BLUE: Color32 = Color32::from_rgb(51, 63, 255);
pub const CORAL: Color32 = Color32::from_rgb(233, 91, 56);
pub const MUTED: Color32 = Color32::from_gray(118);
pub const STAFF_COLOR: Color32 = Color32::from_gray(186);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    Backward,
    Forward,
    Repeat,
    Audio,
    Midi,
    Volume,
    Score,
    Tablature,
    Settings,
    Menu,
    Folder,
    Transcribe,
    Instruments,
    ExportMidi,
    ExportScore,
    Info,
    Tap,
    Downbeat,
    Cancel,
    Chords,
    Follow,
}

pub fn configure(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, font) in [
        (
            "Apfel",
            include_bytes!("../assets/fonts/ApfelGrotezk-Regular.ttf").as_slice(),
        ),
        (
            "Apfel Strong",
            include_bytes!("../assets/fonts/ApfelGrotezk-Mittel.ttf").as_slice(),
        ),
        (
            "Monaspace",
            include_bytes!("../assets/fonts/MonaspaceNeon-Regular.ttf").as_slice(),
        ),
        (
            "Fraunces",
            include_bytes!("../assets/fonts/Fraunces-Black.ttf").as_slice(),
        ),
        (
            "Petaluma Script",
            include_bytes!("../assets/fonts/PetalumaScript.otf").as_slice(),
        ),
        (
            "Petaluma",
            include_bytes!("../assets/fonts/Petaluma.otf").as_slice(),
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), egui::FontData::from_static(font).into());
    }
    let fallback = fonts.families[&egui::FontFamily::Proportional].clone();
    for (family, name) in [
        (egui::FontFamily::Proportional, "Apfel"),
        (egui::FontFamily::Monospace, "Monaspace"),
        (egui::FontFamily::Name("Editorial".into()), "Fraunces"),
        (
            egui::FontFamily::Name("InterfaceStrong".into()),
            "Apfel Strong",
        ),
        (
            egui::FontFamily::Name("JazzScript".into()),
            "Petaluma Script",
        ),
    ] {
        let mut names = vec![name.into()];
        names.extend(fallback.iter().cloned());
        fonts.families.insert(family, names);
    }
    fonts.families.insert(
        egui::FontFamily::Name("JazzMusic".into()),
        vec!["Petaluma".into()],
    );
    context.set_fonts(fonts);
    context.set_theme(egui::Theme::Light);
    let mut visuals = egui::Visuals::light();
    visuals.panel_fill = PAPER;
    visuals.window_fill = WHITE;
    visuals.extreme_bg_color = WHITE;
    visuals.faint_bg_color = PAPER;
    visuals.hyperlink_color = BLUE;
    visuals.warn_fg_color = CORAL;
    visuals.error_fg_color = CORAL;
    visuals.selection.bg_fill = BLUE;
    visuals.selection.stroke = Stroke::new(1.0, WHITE);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, INK);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_gray(225));
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.bg_fill = WHITE;
        widget.weak_bg_fill = PAPER;
        widget.bg_stroke = Stroke::NONE;
        widget.fg_stroke = Stroke::new(1.0, INK);
        widget.corner_radius = egui::CornerRadius::same(6);
    }
    visuals.widgets.hovered.weak_bg_fill = Color32::from_gray(235);
    visuals.widgets.active.weak_bg_fill = BLUE.gamma_multiply(0.08);
    visuals.window_stroke = Stroke::new(1.0, Color32::from_gray(225));
    visuals.window_corner_radius = egui::CornerRadius::same(10);
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 3],
        blur: 16,
        spread: 0,
        color: Color32::from_black_alpha(20),
    };
    visuals.popup_shadow = visuals.window_shadow;
    visuals.handle_shape = egui::style::HandleShape::Circle;
    context.set_visuals(visuals);
    context.style_mut(|style| {
        style.spacing.item_spacing = Vec2::new(9.0, 7.0);
        style.spacing.button_padding = Vec2::new(11.0, 7.0);
        style.spacing.slider_width = 105.0;
        style.spacing.slider_rail_height = 2.0;
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(11.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, display_font(27.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, number_font(13.0));
    });
    context.options_mut(|options| options.zoom_with_keyboard = false);
}

pub fn button(label: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label.to_owned()).color(INK))
        .fill(PAPER)
        .corner_radius(6.0)
}

pub fn display_font(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("Editorial".into()))
}

pub fn strong_font(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("InterfaceStrong".into()))
}

pub fn number_font(size: f32) -> FontId {
    FontId::monospace(size)
}

pub fn jazz_font(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("JazzScript".into()))
}

pub fn slider(ui: &mut egui::Ui, slider: egui::Slider<'_>) -> Response {
    ui.scope(|ui| {
        ui.visuals_mut().widgets.inactive.bg_fill = INK;
        ui.add(slider)
    })
    .inner
}

pub fn icon_button(ui: &mut egui::Ui, icon: Icon, tooltip: &str, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(36.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, tooltip)
    });
    if ui.is_rect_visible(rect) {
        let background = if selected || response.hovered() {
            PAPER
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 6.0, background);
        let foreground = control_ink(ui, if selected { BLUE } else { INK });
        paint_icon(
            ui.painter(),
            icon,
            rect.center() - Vec2::splat(12.0),
            foreground,
            if background == Color32::TRANSPARENT {
                ui.visuals().panel_fill
            } else {
                background
            },
        );
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                6.0,
                Stroke::new(1.0, BLUE),
                egui::StrokeKind::Inside,
            );
        }
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tooltip)
}

pub fn round_icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    tooltip: &str,
    selected: bool,
    size: f32,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), selected, tooltip)
    });
    if ui.is_rect_visible(rect) {
        let primary = matches!(icon, Icon::Play | Icon::Pause | Icon::Menu);
        let background = if primary {
            control_ink(ui, INK)
        } else if selected || response.hovered() {
            PAPER
        } else {
            Color32::TRANSPARENT
        };
        ui.painter()
            .circle_filled(rect.center(), size * 0.5, background);
        let foreground = if primary {
            WHITE
        } else {
            control_ink(ui, if selected { BLUE } else { INK })
        };
        paint_icon(
            ui.painter(),
            icon,
            rect.center() - Vec2::splat(12.0),
            foreground,
            background,
        );
        if response.has_focus() {
            ui.painter()
                .circle_stroke(rect.center(), size * 0.5 - 1.0, Stroke::new(1.0, BLUE));
        }
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tooltip)
}

pub fn tab_button(ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool) -> Response {
    let text = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(13.0),
        if selected { INK } else { MUTED },
    );
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(text.size().x + 48.0, 36.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    if ui.is_rect_visible(rect) {
        let background = if selected || response.hovered() {
            PAPER
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 6.0, background);
        let foreground = control_ink(ui, if selected { INK } else { MUTED });
        paint_icon(
            ui.painter(),
            icon,
            Pos2::new(rect.left() + 8.0, rect.center().y - 12.0),
            foreground,
            if background == Color32::TRANSPARENT {
                ui.visuals().panel_fill
            } else {
                background
            },
        );
        ui.painter().galley(
            Pos2::new(rect.left() + 35.0, rect.center().y - text.size().y * 0.5),
            text,
            foreground,
        );
        if response.has_focus() {
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                6.0,
                Stroke::new(1.0, BLUE),
                egui::StrokeKind::Inside,
            );
        }
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(label)
}

fn control_ink(ui: &egui::Ui, color: Color32) -> Color32 {
    if ui.is_enabled() {
        color
    } else {
        color.gamma_multiply(0.35)
    }
}

pub fn paint_paper(painter: &egui::Painter, rect: egui::Rect) {
    let outline = [
        rect.left_top(),
        rect.right_top(),
        rect.right_bottom() - Vec2::new(1.0, 4.0),
        Pos2::new(rect.left() + rect.width() * 0.82, rect.bottom() - 1.0),
        Pos2::new(rect.left() + rect.width() * 0.65, rect.bottom() - 3.0),
        Pos2::new(rect.left() + rect.width() * 0.47, rect.bottom()),
        Pos2::new(rect.left() + rect.width() * 0.24, rect.bottom() - 2.0),
        rect.left_bottom() - Vec2::new(0.0, 1.0),
    ];
    for (offset, color) in [
        (Vec2::new(1.0, 2.0), Color32::from_gray(229)),
        (Vec2::ZERO, WHITE),
    ] {
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(rect.center() + offset, color);
        for point in outline {
            mesh.colored_vertex(point + offset, color);
        }
        for index in 1..=outline.len() as u32 {
            mesh.add_triangle(0, index, index % outline.len() as u32 + 1);
        }
        painter.add(Shape::mesh(mesh));
    }
}

pub fn icon_label(ui: &mut egui::Ui, icon: Icon, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), tooltip)
    });
    if ui.is_rect_visible(rect) {
        paint_icon(
            ui.painter(),
            icon,
            rect.center() - Vec2::splat(12.0),
            MUTED,
            PAPER,
        );
    }
    response.on_hover_text(tooltip)
}

fn paint_icon(
    painter: &egui::Painter,
    icon: Icon,
    origin: Pos2,
    foreground: Color32,
    background: Color32,
) {
    let position = |x: f32, y: f32| origin + Vec2::new(x, y);
    let stroke = Stroke::new(1.8, foreground);
    let line = |points: &[(f32, f32)]| {
        painter.add(Shape::line(
            points.iter().map(|&(x, y)| position(x, y)).collect(),
            stroke,
        ));
    };
    let polygon = |points: &[(f32, f32)]| {
        painter.add(Shape::convex_polygon(
            points.iter().map(|&(x, y)| position(x, y)).collect(),
            foreground,
            Stroke::NONE,
        ));
    };
    let rectangle = |left: f32, top: f32, right: f32, bottom: f32| {
        painter.rect_filled(
            egui::Rect::from_min_max(position(left, top), position(right, bottom)),
            0.0,
            foreground,
        );
    };
    let ring = |x: f32, y: f32, radius: f32| {
        painter.circle_stroke(position(x, y), radius, stroke);
    };
    match icon {
        Icon::Play => polygon(&[(8.0, 4.0), (20.0, 12.0), (8.0, 20.0)]),
        Icon::Pause => {
            rectangle(6.0, 4.0, 10.0, 20.0);
            rectangle(14.0, 4.0, 18.0, 20.0);
        }
        Icon::Backward | Icon::Forward => {
            let forward = icon == Icon::Forward;
            let direction = if forward { -1.0 } else { 1.0 };
            let mirror = |x: f32| if forward { 24.0 - x } else { x };
            let arc = (0..=20)
                .map(|step| {
                    let angle = -std::f32::consts::PI * 0.83
                        + step as f32 * std::f32::consts::PI * 1.65 / 20.0;
                    position(
                        12.0 + direction * angle.cos() * 8.0,
                        12.0 + angle.sin() * 8.0,
                    )
                })
                .collect();
            painter.add(Shape::line(arc, stroke));
            line(&[(mirror(4.0), 4.0), (mirror(4.0), 9.0), (mirror(9.0), 9.0)]);
            painter.text(
                position(12.0, 12.0),
                egui::Align2::CENTER_CENTER,
                "5",
                number_font(10.0),
                foreground,
            );
        }
        Icon::Repeat => {
            line(&[(4.0, 10.0), (4.0, 6.0), (19.0, 6.0)]);
            line(&[(20.0, 14.0), (20.0, 18.0), (5.0, 18.0)]);
            polygon(&[(17.0, 2.0), (22.0, 6.0), (17.0, 10.0)]);
            polygon(&[(7.0, 14.0), (2.0, 18.0), (7.0, 22.0)]);
        }
        Icon::Audio => {
            for (x, height) in [
                (3.0, 3.0),
                (7.5, 7.0),
                (12.0, 10.0),
                (16.5, 5.0),
                (21.0, 2.0),
            ] {
                line(&[(x, 12.0 - height), (x, 12.0 + height)]);
            }
        }
        Icon::Midi => paint_keyboard(
            painter,
            egui::Rect::from_min_size(position(3.0, 5.0), Vec2::new(18.0, 14.0)),
            foreground,
            background,
        ),
        Icon::Volume => {
            polygon(&[
                (3.0, 9.0),
                (7.0, 9.0),
                (12.0, 5.0),
                (12.0, 19.0),
                (7.0, 15.0),
                (3.0, 15.0),
            ]);
            line(&[(16.0, 8.0), (18.0, 10.0), (18.0, 14.0), (16.0, 16.0)]);
            line(&[(19.0, 5.0), (22.0, 8.0), (22.0, 16.0), (19.0, 19.0)]);
        }
        Icon::Score => {
            for y in [5.0, 8.0, 11.0, 14.0, 17.0] {
                painter.line_segment(
                    [position(2.0, y), position(22.0, y)],
                    Stroke::new(0.7, foreground),
                );
            }
            line(&[(16.0, 16.0), (16.0, 3.0)]);
            painter.add(Shape::ellipse_filled(
                position(13.0, 16.0),
                Vec2::new(3.3, 2.1),
                foreground,
            ));
        }
        Icon::Tablature => {
            for y in [4.0, 7.0, 10.0, 13.0, 16.0, 19.0] {
                painter.line_segment(
                    [position(2.0, y), position(22.0, y)],
                    Stroke::new(0.7, foreground),
                );
            }
            painter.rect_filled(
                egui::Rect::from_min_max(position(7.0, 6.0), position(17.0, 19.5)),
                0.0,
                background,
            );
            line(&[
                (16.0, 7.0),
                (9.0, 7.0),
                (9.0, 12.0),
                (15.0, 12.0),
                (16.0, 14.0),
                (16.0, 16.0),
                (14.0, 18.0),
                (9.0, 18.0),
            ]);
        }
        Icon::Settings => {
            ring(12.0, 12.0, 6.7);
            ring(12.0, 12.0, 2.5);
            for index in 0..8 {
                let angle = index as f32 * std::f32::consts::FRAC_PI_4;
                line(&[
                    (12.0 + angle.cos() * 7.0, 12.0 + angle.sin() * 7.0),
                    (12.0 + angle.cos() * 10.0, 12.0 + angle.sin() * 10.0),
                ]);
            }
        }
        Icon::Menu => {
            for y in [6.0, 12.0, 18.0] {
                line(&[(3.0, y), (21.0, y)]);
            }
        }
        Icon::Folder => {
            line(&[
                (3.0, 19.0),
                (3.0, 5.0),
                (9.0, 5.0),
                (12.0, 8.0),
                (21.0, 8.0),
                (21.0, 19.0),
                (3.0, 19.0),
            ]);
            line(&[(3.0, 10.0), (21.0, 10.0)]);
        }
        Icon::Transcribe => {
            line(&[(3.0, 9.0), (3.0, 15.0)]);
            line(&[(6.0, 5.0), (6.0, 19.0)]);
            line(&[(9.0, 8.0), (9.0, 16.0)]);
            line(&[(12.0, 12.0), (17.0, 12.0)]);
            line(&[(14.0, 9.0), (17.0, 12.0), (14.0, 15.0)]);
            line(&[(21.0, 17.0), (21.0, 4.0), (17.0, 5.0)]);
            painter.add(Shape::ellipse_filled(
                position(19.0, 18.0),
                Vec2::new(2.5, 1.9),
                foreground,
            ));
        }
        Icon::Instruments => {
            for (x, y) in [(5.0, 8.0), (12.0, 16.0), (19.0, 11.0)] {
                line(&[(x, 3.0), (x, y - 3.0)]);
                line(&[(x, y + 3.0), (x, 21.0)]);
                ring(x, y, 2.8);
            }
        }
        Icon::ExportMidi => {
            paint_keyboard(
                painter,
                egui::Rect::from_min_size(position(3.0, 14.0), Vec2::new(18.0, 7.0)),
                foreground,
                background,
            );
            line(&[(12.0, 2.0), (12.0, 11.0)]);
            line(&[(8.0, 7.0), (12.0, 11.0), (16.0, 7.0)]);
        }
        Icon::ExportScore => {
            line(&[
                (3.0, 3.0),
                (11.0, 3.0),
                (15.0, 7.0),
                (15.0, 21.0),
                (3.0, 21.0),
                (3.0, 3.0),
            ]);
            line(&[(11.0, 3.0), (11.0, 7.0), (15.0, 7.0)]);
            line(&[(6.0, 12.0), (12.0, 12.0)]);
            line(&[(6.0, 16.0), (12.0, 16.0)]);
            line(&[(20.0, 4.0), (20.0, 16.0)]);
            line(&[(17.0, 13.0), (20.0, 16.0), (23.0, 13.0)]);
        }
        Icon::Info => {
            ring(12.0, 12.0, 9.0);
            painter.circle_filled(position(12.0, 7.0), 1.2, foreground);
            line(&[(12.0, 11.0), (12.0, 17.0)]);
        }
        Icon::Tap => {
            line(&[
                (7.0, 21.0),
                (4.0, 15.0),
                (4.0, 12.0),
                (6.0, 11.0),
                (9.0, 15.0),
                (9.0, 6.0),
                (11.0, 4.0),
                (13.0, 6.0),
                (13.0, 11.0),
                (18.0, 12.0),
                (20.0, 14.0),
                (19.0, 21.0),
            ]);
            line(&[(5.0, 3.0), (3.0, 1.0)]);
            line(&[(17.0, 3.0), (19.0, 1.0)]);
        }
        Icon::Downbeat => {
            line(&[(4.0, 20.0), (20.0, 20.0)]);
            line(&[(12.0, 3.0), (12.0, 16.0)]);
            polygon(&[(7.0, 11.0), (12.0, 17.0), (17.0, 11.0)]);
        }
        Icon::Cancel => {
            line(&[(5.0, 5.0), (19.0, 19.0)]);
            line(&[(19.0, 5.0), (5.0, 19.0)]);
        }
        Icon::Chords => {
            for (x, y) in [(6.0, 18.0), (12.0, 14.0), (18.0, 18.0)] {
                line(&[(x, 4.0), (x, y)]);
                painter.add(Shape::ellipse_filled(
                    position(x - 2.0, y),
                    Vec2::new(2.5, 1.6),
                    foreground,
                ));
            }
        }
        Icon::Follow => {
            line(&[(4.0, 4.0), (4.0, 20.0)]);
            line(&[(9.0, 5.0), (21.0, 5.0)]);
            line(&[(9.0, 10.0), (21.0, 10.0)]);
            line(&[(9.0, 15.0), (16.0, 15.0)]);
            line(&[(18.0, 14.0), (22.0, 18.0), (18.0, 22.0)]);
        }
    }
}

fn paint_keyboard(
    painter: &egui::Painter,
    rect: egui::Rect,
    foreground: Color32,
    background: Color32,
) {
    let position = |x: f32, y: f32| rect.min + Vec2::new(x, y);
    painter.rect_filled(rect, 0.0, foreground);
    for index in 1..4 {
        let x = rect.width() * index as f32 / 4.0;
        painter.line_segment(
            [
                position(x, rect.height() * 0.55),
                position(x, rect.height()),
            ],
            Stroke::new(1.0, background),
        );
        painter.rect_filled(
            egui::Rect::from_min_max(
                position(x - 1.3, 0.0),
                position(x + 1.3, rect.height() * 0.55),
            ),
            0.0,
            background,
        );
    }
}
