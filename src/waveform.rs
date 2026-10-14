use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};

use crate::audio::AudioClip;
use crate::design::{BLUE, CORAL, INK, MUTED};

const MINIMUM_LOOP: f64 = 0.05;
const WAVEFORM_HEIGHT: f32 = 96.0;
const MAXIMUM_ZOOM: f64 = 64.0;

#[derive(Clone, Copy, Default)]
struct ViewportInteraction {
    manual_view: bool,
    last_position: f64,
}

#[derive(Default)]
pub struct WaveformAction {
    pub seek: Option<f64>,
    pub loop_changed: bool,
    pub loop_dragged: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum DragTarget {
    Start,
    End,
    Seek,
}

#[derive(Clone, Copy)]
struct WaveformDrag {
    target: DragTarget,
    pointer_x: f32,
    start: f64,
    end: f64,
    view_start: f64,
    visible_duration: f64,
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    clip: Option<&AudioClip>,
    position: f64,
    zoom: &mut f64,
    view_start: &mut f64,
    loop_start: &mut f64,
    loop_end: &mut f64,
    loop_enabled: bool,
    reset_view: bool,
) -> WaveformAction {
    let mut action = WaveformAction::default();
    let height = if ui.ctx().viewport_rect().height() < 800.0 {
        72.0
    } else {
        WAVEFORM_HEIGHT
    };
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), height),
        Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect);
    let Some(clip) = clip.filter(|clip| clip.duration > 0.0 && clip.duration.is_finite()) else {
        painter.line_segment(
            [rect.left_center(), rect.right_center()],
            Stroke::new(1.0, MUTED.gamma_multiply(0.3)),
        );
        return action;
    };
    let duration = clip.duration;
    *zoom = finite_or(*zoom, 1.0).clamp(1.0, MAXIMUM_ZOOM);
    let mut visible_duration = duration / *zoom;
    let drag_id = response.id.with("loop_drag");
    let viewport_id = response.id.with("viewport_interaction");
    if reset_view {
        ui.data_mut(|storage| storage.remove::<ViewportInteraction>(viewport_id));
    }
    let mut viewport = ui.data(|storage| {
        storage
            .get_temp::<ViewportInteraction>(viewport_id)
            .unwrap_or_default()
    });
    let mut drag = ui.data(|storage| storage.get_temp::<WaveformDrag>(drag_id));
    let dragging_marker = drag.is_some_and(|drag| drag.target != DragTarget::Seek);
    let position = finite_or(position, 0.0).clamp(0.0, duration);
    *view_start = finite_or(*view_start, 0.0).clamp(0.0, duration - visible_duration);
    if viewport.manual_view
        && position != viewport.last_position
        && (*view_start..=*view_start + visible_duration).contains(&position)
    {
        viewport.manual_view = false;
    }
    if !dragging_marker
        && !viewport.manual_view
        && (position < *view_start || position > *view_start + visible_duration)
    {
        *view_start = (position - visible_duration * 0.2).clamp(0.0, duration - visible_duration);
    }
    if response.hovered()
        && drag.is_none()
        && let Some(pointer) = ui.input(|input| input.pointer.hover_pos())
    {
        let wheel_steps = ui.input_mut(consume_zoom_wheel);
        if wheel_steps != 0.0 {
            let anchor = ((pointer.x - rect.left()) / rect.width().max(1.0)) as f64;
            (*zoom, *view_start) = zoomed_view(duration, *zoom, *view_start, anchor, wheel_steps);
            visible_duration = duration / *zoom;
            viewport.manual_view = true;
        }
    }
    let normalized = normalize_loop(*loop_start, *loop_end, duration);
    action.loop_changed = normalized != (*loop_start, *loop_end);
    (*loop_start, *loop_end) = normalized;
    let start_x = time_x(rect, *loop_start, *view_start, visible_duration);
    let end_x = time_x(rect, *loop_end, *view_start, visible_duration);
    let (pointer, pressed, released, down) = ui.input(|input| {
        (
            input.pointer.interact_pos(),
            input.pointer.primary_pressed(),
            input.pointer.primary_released(),
            input.pointer.primary_down(),
        )
    });
    if pressed
        && response.hovered()
        && let Some(pointer) = pointer
    {
        let pressed_drag = WaveformDrag {
            target: hit_target(rect, pointer, start_x, end_x),
            pointer_x: pointer.x,
            start: *loop_start,
            end: *loop_end,
            view_start: *view_start,
            visible_duration,
        };
        ui.data_mut(|storage| storage.insert_temp(drag_id, pressed_drag));
        drag = Some(pressed_drag);
    }
    if let Some(drag) = drag
        && let Some(pointer) = pointer
        && (down || released)
    {
        match drag.target {
            DragTarget::Start | DragTarget::End => {
                let delta = (pointer.x - drag.pointer_x) as f64 / rect.width().max(1.0) as f64
                    * drag.visible_duration;
                let (start, end) = dragged_loop(drag, delta, duration);
                action.loop_dragged = (start, end) != (*loop_start, *loop_end);
                action.loop_changed |= (start, end) != (*loop_start, *loop_end);
                (*loop_start, *loop_end) = (start, end);
            }
            DragTarget::Seek if response.clicked() || response.dragged() => {
                action.seek = Some(
                    (drag.view_start
                        + (pointer.x - rect.left()) as f64 / rect.width().max(1.0) as f64
                            * drag.visible_duration)
                        .clamp(0.0, duration),
                );
            }
            DragTarget::Seek => {}
        }
    }
    if released || !down && !pressed {
        ui.data_mut(|storage| storage.remove::<WaveformDrag>(drag_id));
    }
    let hovered_target = pointer
        .filter(|_| response.hovered())
        .map(|pointer| hit_target(rect, pointer, start_x, end_x));
    if dragging_marker || hovered_target.is_some_and(|target| target != DragTarget::Seek) {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    } else if response.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let wave_rect = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top() + 14.0),
        Pos2::new(rect.right(), rect.bottom() - 22.0),
    );
    if loop_enabled {
        let selection = Rect::from_min_max(
            Pos2::new(
                time_x(rect, *loop_start, *view_start, visible_duration)
                    .clamp(rect.left(), rect.right()),
                wave_rect.top(),
            ),
            Pos2::new(
                time_x(rect, *loop_end, *view_start, visible_duration)
                    .clamp(rect.left(), rect.right()),
                wave_rect.bottom(),
            ),
        );
        if selection.is_positive() {
            painter.rect_filled(selection, 0.0, BLUE.gamma_multiply(0.07));
        }
    }
    draw_waveform(&painter, wave_rect, clip, *view_start, visible_duration);
    for step in 0..=6 {
        let seconds = *view_start + visible_duration * step as f64 / 6.0;
        let alignment = match step {
            0 => Align2::LEFT_BOTTOM,
            6 => Align2::RIGHT_BOTTOM,
            _ => Align2::CENTER_BOTTOM,
        };
        painter.text(
            Pos2::new(
                time_x(rect, seconds, *view_start, visible_duration),
                rect.bottom(),
            ),
            alignment,
            time_label(seconds),
            FontId::monospace(10.0),
            MUTED,
        );
    }
    let cursor_x = time_x(rect, position, *view_start, visible_duration);
    if rect.x_range().contains(cursor_x) {
        painter.line_segment(
            [
                Pos2::new(cursor_x, rect.top() + 6.0),
                Pos2::new(cursor_x, wave_rect.bottom() + 4.0),
            ],
            Stroke::new(1.0, INK),
        );
        painter.circle_filled(Pos2::new(cursor_x, rect.top() + 6.0), 3.0, INK);
    }
    draw_marker(
        &painter,
        rect,
        time_x(rect, *loop_start, *view_start, visible_duration),
        DragTarget::Start,
        BLUE,
    );
    draw_marker(
        &painter,
        rect,
        time_x(rect, *loop_end, *view_start, visible_duration),
        DragTarget::End,
        CORAL,
    );
    if action.seek.is_some() {
        viewport.manual_view = false;
    }
    viewport.last_position = position;
    ui.data_mut(|storage| storage.insert_temp(viewport_id, viewport));
    action
}

fn consume_zoom_wheel(input: &mut egui::InputState) -> f64 {
    let mut steps = 0.0;
    input.events.retain(|event| {
        let egui::Event::MouseWheel {
            unit,
            delta,
            modifiers,
        } = event
        else {
            return true;
        };
        if !modifiers.ctrl || delta.y == 0.0 {
            return true;
        }
        steps += delta.y as f64
            * match unit {
                egui::MouseWheelUnit::Point => 1.0 / 40.0,
                egui::MouseWheelUnit::Line => 1.0,
                egui::MouseWheelUnit::Page => 4.0,
            };
        false
    });
    steps
}

fn zoomed_view(
    duration: f64,
    zoom: f64,
    view_start: f64,
    anchor: f64,
    wheel_steps: f64,
) -> (f64, f64) {
    let anchor = anchor.clamp(0.0, 1.0);
    let anchor_time = view_start + duration / zoom * anchor;
    let zoom = (zoom * 1.25_f64.powf(wheel_steps.clamp(-64.0, 64.0))).clamp(1.0, MAXIMUM_ZOOM);
    let visible_duration = duration / zoom;
    let view_start =
        (anchor_time - visible_duration * anchor).clamp(0.0, duration - visible_duration);
    (zoom, view_start)
}

fn draw_waveform(
    painter: &egui::Painter,
    rect: Rect,
    clip: &AudioClip,
    view_start: f64,
    visible_duration: f64,
) {
    painter.line_segment(
        [rect.left_center(), rect.right_center()],
        Stroke::new(0.7, INK.gamma_multiply(0.15)),
    );
    if clip.waveform.is_empty() {
        return;
    }
    let columns = (rect.width() / 3.0).ceil().max(1.0) as usize;
    for column in 0..columns {
        let onset = view_start + visible_duration * column as f64 / columns as f64;
        let offset = onset + visible_duration / columns as f64;
        let first = ((onset / clip.duration * clip.waveform.len() as f64) as usize)
            .min(clip.waveform.len() - 1);
        let last = ((offset / clip.duration * clip.waveform.len() as f64).ceil() as usize)
            .clamp(first + 1, clip.waveform.len());
        let (minimum, maximum) = clip.waveform[first..last]
            .iter()
            .fold((1.0_f32, -1.0_f32), |(minimum, maximum), (lower, upper)| {
                (minimum.min(*lower), maximum.max(*upper))
            });
        let x = rect.left() + (column as f32 + 0.5) * rect.width() / columns as f32;
        painter.line_segment(
            [
                Pos2::new(x, rect.center().y - maximum * rect.height() * 0.5),
                Pos2::new(x, rect.center().y - minimum * rect.height() * 0.5),
            ],
            Stroke::new(1.6, INK.gamma_multiply(0.87)),
        );
    }
}

fn draw_marker(painter: &egui::Painter, rect: Rect, x: f32, target: DragTarget, color: Color32) {
    let visible_x = x.clamp(rect.left(), rect.right());
    painter.line_segment(
        [
            Pos2::new(visible_x, rect.top() + 2.0),
            Pos2::new(visible_x, rect.bottom() - 22.0),
        ],
        Stroke::new(2.0, color),
    );
    let cap = marker_cap(rect, visible_x, target);
    let vertices = if target == DragTarget::Start {
        vec![
            cap.left_top(),
            cap.right_top(),
            cap.right_center(),
            cap.left_bottom(),
        ]
    } else {
        vec![
            cap.left_top(),
            cap.right_top(),
            cap.right_bottom(),
            cap.left_center(),
        ]
    };
    painter.add(egui::Shape::convex_polygon(vertices, color, Stroke::NONE));
    if x < rect.left() || x > rect.right() {
        let direction = if x < rect.left() { 1.0 } else { -1.0 };
        let y = marker_cap(rect, visible_x, target).center().y;
        painter.line_segment(
            [
                Pos2::new(visible_x, y),
                Pos2::new(visible_x + direction * 7.0, y - 3.0),
            ],
            Stroke::new(1.5, Color32::WHITE),
        );
        painter.line_segment(
            [
                Pos2::new(visible_x, y),
                Pos2::new(visible_x + direction * 7.0, y + 3.0),
            ],
            Stroke::new(1.5, Color32::WHITE),
        );
    }
}

fn marker_cap(rect: Rect, x: f32, target: DragTarget) -> Rect {
    let top = rect.top()
        + if target == DragTarget::Start {
            1.0
        } else {
            13.0
        };
    Rect::from_min_size(
        Pos2::new((x - 6.0).clamp(rect.left(), rect.right() - 12.0), top),
        Vec2::new(12.0, 10.0),
    )
}

fn hit_target(rect: Rect, pointer: Pos2, start_x: f32, end_x: f32) -> DragTarget {
    let start_x = start_x.clamp(rect.left(), rect.right());
    let end_x = end_x.clamp(rect.left(), rect.right());
    if marker_cap(rect, start_x, DragTarget::Start)
        .expand(1.0)
        .contains(pointer)
    {
        return DragTarget::Start;
    }
    if marker_cap(rect, end_x, DragTarget::End)
        .expand(1.0)
        .contains(pointer)
    {
        return DragTarget::End;
    }
    if pointer.y > rect.bottom() - 22.0 {
        return DragTarget::Seek;
    }
    let start_distance = (pointer.x - start_x).abs();
    let end_distance = (pointer.x - end_x).abs();
    if start_distance.min(end_distance) > 7.0 {
        DragTarget::Seek
    } else if start_distance <= end_distance {
        DragTarget::Start
    } else {
        DragTarget::End
    }
}

fn normalize_loop(start: f64, end: f64, duration: f64) -> (f64, f64) {
    let minimum_span = MINIMUM_LOOP.min(duration);
    let end = finite_or(end, duration).clamp(minimum_span, duration);
    let start = finite_or(start, 0.0).clamp(0.0, end - minimum_span);
    (start, end)
}

fn dragged_loop(drag: WaveformDrag, delta: f64, duration: f64) -> (f64, f64) {
    let minimum_span = MINIMUM_LOOP.min(duration);
    match drag.target {
        DragTarget::Start => (
            (drag.start + delta).clamp(0.0, drag.end - minimum_span),
            drag.end,
        ),
        DragTarget::End => (
            drag.start,
            (drag.end + delta).clamp(drag.start + minimum_span, duration),
        ),
        DragTarget::Seek => (drag.start, drag.end),
    }
}

fn time_x(rect: Rect, seconds: f64, view_start: f64, visible_duration: f64) -> f32 {
    rect.left() + ((seconds - view_start) / visible_duration) as f32 * rect.width()
}

fn finite_or(value: f64, fallback: f64) -> f64 {
    if value.is_finite() { value } else { fallback }
}

fn time_label(seconds: f64) -> String {
    let minutes = (seconds / 60.0).floor() as u32;
    format!("{minutes:02}:{:04.1}", seconds % 60.0)
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Arc};

    use super::*;

    #[test]
    fn coincident_markers_have_distinct_grab_caps() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 110.0));
        assert!(hit_target(rect, Pos2::new(100.0, 6.0), 100.0, 100.5) == DragTarget::Start);
        assert!(hit_target(rect, Pos2::new(100.0, 18.0), 100.0, 100.5) == DragTarget::End);
        assert!(hit_target(rect, Pos2::new(200.0, 50.0), 100.0, 100.5) == DragTarget::Seek);
    }

    #[test]
    fn dragging_outside_the_view_preserves_the_original_endpoint() {
        let drag = WaveformDrag {
            target: DragTarget::Start,
            pointer_x: 0.0,
            start: 10.0,
            end: 40.0,
            view_start: 20.0,
            visible_duration: 10.0,
        };
        assert_eq!(dragged_loop(drag, 0.0, 100.0), (10.0, 40.0));
        assert_eq!(dragged_loop(drag, 2.0, 100.0), (12.0, 40.0));
        assert_eq!(dragged_loop(drag, 99.0, 100.0), (39.95, 40.0));
    }

    #[test]
    fn boundaries_stay_ordered_inside_short_audio() {
        assert_eq!(normalize_loop(5.0, 15.0, 0.02), (0.0, 0.02));
        assert_eq!(normalize_loop(f64::NAN, f64::NAN, 1.0), (0.0, 1.0));
        let drag = WaveformDrag {
            target: DragTarget::End,
            pointer_x: 0.0,
            start: 0.2,
            end: 0.5,
            view_start: 0.0,
            visible_duration: 1.0,
        };
        assert_eq!(dragged_loop(drag, -2.0, 1.0), (0.2, 0.25));
        assert_eq!(dragged_loop(drag, 2.0, 1.0), (0.2, 1.0));
    }

    #[test]
    fn normalizing_saved_boundaries_is_not_a_user_drag() {
        let context = egui::Context::default();
        let clip = AudioClip {
            path: PathBuf::new(),
            title: String::new(),
            samples: Arc::new(Vec::new()),
            sample_rate: 44100,
            duration: 100.0,
            waveform: vec![(-0.5, 0.5); 100],
        };
        let mut action = WaveformAction::default();
        let mut zoom = 1.0;
        let mut view_start = 0.0;
        let mut start = 60.0;
        let mut end = 40.0;
        let _ = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                action = show(
                    ui,
                    Some(&clip),
                    0.0,
                    &mut zoom,
                    &mut view_start,
                    &mut start,
                    &mut end,
                    false,
                    false,
                );
            });
        });
        assert!(action.loop_changed);
        assert!(!action.loop_dragged);
        assert!(action.seek.is_none());
        assert!(start < end);
        assert_eq!(end, 40.0);
    }

    #[test]
    fn dragging_a_marker_changes_the_loop_without_seeking() {
        let context = egui::Context::default();
        let clip = AudioClip {
            path: PathBuf::new(),
            title: String::new(),
            samples: Arc::new(Vec::new()),
            sample_rate: 44100,
            duration: 100.0,
            waveform: vec![(-0.5, 0.5); 100],
        };
        let mut view_start = 0.0;
        let mut zoom = 1.0;
        let mut loop_start = 10.0;
        let mut loop_end = 40.0;
        {
            let mut render = |events: Vec<egui::Event>| {
                let mut action = WaveformAction::default();
                let mut waveform_rect = Rect::NOTHING;
                let _ = context.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            Pos2::ZERO,
                            Vec2::new(1000.0, 200.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |context| {
                        egui::CentralPanel::default().show(context, |ui| {
                            waveform_rect = Rect::from_min_size(
                                ui.cursor().min,
                                Vec2::new(ui.available_width(), WAVEFORM_HEIGHT),
                            );
                            action = show(
                                ui,
                                Some(&clip),
                                25.0,
                                &mut zoom,
                                &mut view_start,
                                &mut loop_start,
                                &mut loop_end,
                                true,
                                false,
                            );
                        });
                    },
                );
                (action, waveform_rect)
            };
            let (_, waveform_rect) = render(Vec::new());
            let start_pointer = Pos2::new(
                waveform_rect.left() + waveform_rect.width() * 0.1,
                waveform_rect.top() + 6.0,
            );
            let pressed = egui::Event::PointerButton {
                pos: start_pointer,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            };
            assert!(
                render(vec![egui::Event::PointerMoved(start_pointer), pressed])
                    .0
                    .seek
                    .is_none()
            );
            let moved = Pos2::new(start_pointer.x + 98.4, start_pointer.y);
            let action = render(vec![egui::Event::PointerMoved(moved)]).0;
            assert!(action.loop_changed);
            assert!(action.loop_dragged);
            assert!(action.seek.is_none());
            let released = egui::Event::PointerButton {
                pos: moved,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            };
            assert!(render(vec![released]).0.seek.is_none());
            let end_pointer = Pos2::new(
                waveform_rect.left() + waveform_rect.width() * 0.4,
                waveform_rect.top() + 18.0,
            );
            let pressed = egui::Event::PointerButton {
                pos: end_pointer,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            };
            assert!(
                render(vec![egui::Event::PointerMoved(end_pointer), pressed])
                    .0
                    .seek
                    .is_none()
            );
            let moved = Pos2::new(end_pointer.x + 49.2, end_pointer.y);
            let action = render(vec![egui::Event::PointerMoved(moved)]).0;
            assert!(action.loop_changed);
            assert!(action.loop_dragged);
            assert!(action.seek.is_none());
            let released = egui::Event::PointerButton {
                pos: moved,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            };
            assert!(render(vec![released]).0.seek.is_none());
        }
        assert!((loop_start - 20.0).abs() < 0.01);
        assert!((loop_end - 45.0).abs() < 0.01);
    }

    #[test]
    fn zoom_keeps_the_time_under_the_pointer_and_accumulates() {
        let (zoom, start) = zoomed_view(100.0, 2.0, 20.0, 0.75, 1.0);
        assert_eq!(zoom, 2.5);
        assert!((start + 100.0 / zoom * 0.75 - 57.5).abs() < 1e-9);
        let (zoom, start) = zoomed_view(100.0, zoom, start, 0.75, 1.0);
        assert_eq!(zoom, 3.125);
        assert!((start + 100.0 / zoom * 0.75 - 57.5).abs() < 1e-9);
        let (zoom, start) = zoomed_view(100.0, zoom, start, 0.75, -2.0);
        assert!((zoom - 2.0).abs() < 1e-9);
        assert!((start - 20.0).abs() < 1e-9);
    }

    #[test]
    fn zoom_limits_keep_the_view_inside_the_audio() {
        assert_eq!(zoomed_view(100.0, 1.0, 0.0, 0.5, -10.0), (1.0, 0.0));
        let (zoom, start) = zoomed_view(100.0, 32.0, 0.0, 0.0, 64.0);
        assert_eq!(zoom, MAXIMUM_ZOOM);
        assert_eq!(start, 0.0);
        let (zoom, start) = zoomed_view(100.0, 32.0, 96.875, 1.0, 64.0);
        assert_eq!(zoom, MAXIMUM_ZOOM);
        assert_eq!(start, 100.0 - 100.0 / MAXIMUM_ZOOM);
        assert_eq!(zoomed_view(100.0, zoom, start, 1.0, -64.0), (1.0, 0.0));
    }

    #[test]
    fn control_wheel_zooms_only_the_hovered_waveform_without_moving_audio_or_loop() {
        let context = egui::Context::default();
        let clip = AudioClip {
            path: PathBuf::new(),
            title: String::new(),
            samples: Arc::new(Vec::new()),
            sample_rate: 44100,
            duration: 100.0,
            waveform: vec![(-0.5, 0.5); 100],
        };
        let mut zoom = 1.0;
        let mut view_start = 0.0;
        let mut loop_start = 10.0;
        let mut loop_end = 40.0;
        let mut render = |pointer, wheel_modifiers, wheel_delta| {
            let mut action = WaveformAction::default();
            let mut waveform_rect = Rect::NOTHING;
            let mut remaining_wheel = false;
            let mut events = Vec::new();
            if let Some(pointer) = pointer {
                events.push(egui::Event::PointerMoved(pointer));
            }
            if wheel_delta != 0.0 {
                events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: Vec2::new(0.0, wheel_delta),
                    modifiers: wheel_modifiers,
                });
            }
            let _ = context.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 300.0))),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        waveform_rect = Rect::from_min_size(
                            ui.cursor().min,
                            Vec2::new(ui.available_width(), WAVEFORM_HEIGHT),
                        );
                        action = show(
                            ui,
                            Some(&clip),
                            0.0,
                            &mut zoom,
                            &mut view_start,
                            &mut loop_start,
                            &mut loop_end,
                            true,
                            false,
                        );
                        remaining_wheel = ui.input(|input| {
                            input
                                .events
                                .iter()
                                .any(|event| matches!(event, egui::Event::MouseWheel { .. }))
                        });
                    });
                },
            );
            (action, waveform_rect, zoom, view_start, remaining_wheel)
        };
        let (_, rect, _, _, _) = render(None, egui::Modifiers::NONE, 0.0);
        let hovered = rect.center();
        let outside = Pos2::new(hovered.x, rect.bottom() + 50.0);
        let (_, _, zoom, _, remaining) = render(Some(outside), egui::Modifiers::CTRL, 1.0);
        assert_eq!(zoom, 1.0);
        assert!(remaining);
        let (_, _, zoom, _, remaining) = render(Some(hovered), egui::Modifiers::NONE, 1.0);
        assert_eq!(zoom, 1.0);
        assert!(remaining);
        let (action, _, zoom, start, remaining) = render(Some(hovered), egui::Modifiers::CTRL, 1.0);
        assert_eq!(zoom, 1.25);
        assert_eq!(start, 10.0);
        assert!(!remaining);
        assert!(action.seek.is_none());
        assert!(!action.loop_changed);
        assert!(!action.loop_dragged);
        let (_, _, zoom, start, _) = render(None, egui::Modifiers::NONE, 0.0);
        assert_eq!(zoom, 1.25);
        assert_eq!(start, 10.0);
        let (_, _, zoom, start, _) = render(Some(hovered), egui::Modifiers::CTRL, -1.0);
        assert_eq!(zoom, 1.0);
        assert_eq!(start, 0.0);
        assert_eq!(context.zoom_factor(), 1.0);
        assert_eq!((loop_start, loop_end), (10.0, 40.0));
    }

    #[test]
    fn explicit_seek_restores_follow_after_zooming_away_from_the_playhead() {
        let context = egui::Context::default();
        let clip = AudioClip {
            path: PathBuf::new(),
            title: String::new(),
            samples: Arc::new(Vec::new()),
            sample_rate: 44100,
            duration: 100.0,
            waveform: vec![(-0.5, 0.5); 100],
        };
        let mut zoom = 1.0;
        let mut view_start = 0.0;
        let mut loop_start = 10.0;
        let mut loop_end = 40.0;
        let mut render = |position, reset_view, events| {
            let mut waveform_rect = Rect::NOTHING;
            let _ = context.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 300.0))),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        waveform_rect = Rect::from_min_size(
                            ui.cursor().min,
                            Vec2::new(ui.available_width(), WAVEFORM_HEIGHT),
                        );
                        show(
                            ui,
                            Some(&clip),
                            position,
                            &mut zoom,
                            &mut view_start,
                            &mut loop_start,
                            &mut loop_end,
                            false,
                            reset_view,
                        );
                    });
                },
            );
            (waveform_rect, view_start, view_start + clip.duration / zoom)
        };
        let (rect, _, _) = render(0.0, false, Vec::new());
        let pointer = Pos2::new(rect.left() + rect.width() * 0.75, rect.center().y);
        let (_, start, end) = render(
            0.0,
            false,
            vec![
                egui::Event::PointerMoved(pointer),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Line,
                    delta: Vec2::new(0.0, 8.0),
                    modifiers: egui::Modifiers::CTRL,
                },
            ],
        );
        assert!(start > 5.0);
        assert!(end < clip.duration);
        let (_, unchanged_start, _) = render(0.1, false, Vec::new());
        assert_eq!(unchanged_start, start);
        let (_, start, end) = render(5.0, true, Vec::new());
        assert!((start..=end).contains(&5.0));
        let (_, start, end) = render(95.0, true, Vec::new());
        assert!((start..=end).contains(&95.0));
        assert!(end <= clip.duration);
        let (_, start, _) = render(0.0, true, Vec::new());
        assert_eq!(start, 0.0);
    }
}
