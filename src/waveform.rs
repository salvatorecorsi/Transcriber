use eframe::egui::{
    self, Align2, Color32, CursorIcon, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};

use crate::audio::AudioClip;

const START_COLOR: Color32 = Color32::from_rgb(174, 63, 54);
const END_COLOR: Color32 = Color32::from_rgb(46, 97, 145);
const MINIMUM_LOOP: f64 = 0.05;

#[derive(Default)]
pub struct WaveformAction {
    pub seek: Option<f64>,
    pub loop_changed: bool,
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
    zoom: f64,
    view_start: &mut f64,
    loop_start: &mut f64,
    loop_end: &mut f64,
    loop_enabled: bool,
) -> WaveformAction {
    let mut action = WaveformAction::default();
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 110.0),
        Sense::click_and_drag(),
    );
    let painter = ui.painter_at(rect);
    let Some(clip) = clip.filter(|clip| clip.duration > 0.0 && clip.duration.is_finite()) else {
        painter.line_segment(
            [rect.left_center(), rect.right_center()],
            Stroke::new(1.0, Color32::BLACK),
        );
        return action;
    };
    let duration = clip.duration;
    let visible_duration = duration / finite_or(zoom, 1.0).max(1.0);
    let drag_id = response.id.with("loop_drag");
    let mut drag = ui.data(|storage| storage.get_temp::<WaveformDrag>(drag_id));
    let dragging_marker = drag.is_some_and(|drag| drag.target != DragTarget::Seek);
    let position = finite_or(position, 0.0).clamp(0.0, duration);
    *view_start = finite_or(*view_start, 0.0).clamp(0.0, duration - visible_duration);
    if !dragging_marker && (position < *view_start || position > *view_start + visible_duration) {
        *view_start = (position - visible_duration * 0.2).clamp(0.0, duration - visible_duration);
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
        Pos2::new(rect.left(), rect.top() + 24.0),
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
            painter.rect_filled(selection, 0.0, Color32::from_black_alpha(16));
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
            FontId::monospace(12.0),
            Color32::BLACK,
        );
    }
    let cursor_x = time_x(rect, position, *view_start, visible_duration);
    if rect.x_range().contains(cursor_x) {
        painter.line_segment(
            [
                Pos2::new(cursor_x, wave_rect.top()),
                Pos2::new(cursor_x, wave_rect.bottom()),
            ],
            Stroke::new(1.5, Color32::BLACK),
        );
    }
    draw_marker(
        &painter,
        rect,
        time_x(rect, *loop_start, *view_start, visible_duration),
        DragTarget::Start,
        START_COLOR,
    );
    draw_marker(
        &painter,
        rect,
        time_x(rect, *loop_end, *view_start, visible_duration),
        DragTarget::End,
        END_COLOR,
    );
    action
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
        Stroke::new(1.0, Color32::BLACK),
    );
    if clip.waveform.is_empty() {
        return;
    }
    let columns = rect.width().ceil().max(1.0) as usize;
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
        let x = rect.left() + column as f32;
        painter.line_segment(
            [
                Pos2::new(x, rect.center().y - maximum * rect.height() * 0.5),
                Pos2::new(x, rect.center().y - minimum * rect.height() * 0.5),
            ],
            Stroke::new(1.0, Color32::BLACK),
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
        Stroke::new(3.0, color),
    );
    painter.rect_filled(marker_cap(rect, visible_x, target), 0.0, color);
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
                                Vec2::new(ui.available_width(), 110.0),
                            );
                            action = show(
                                ui,
                                Some(&clip),
                                25.0,
                                1.0,
                                &mut view_start,
                                &mut loop_start,
                                &mut loop_end,
                                true,
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
}
