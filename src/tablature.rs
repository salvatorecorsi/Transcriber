use crate::{
    score::{
        NotationAction, NoteHit, QuantizedNote, ScoreSettings, TICKS_PER_BEAT, draw_chord_symbols,
        first_measure_tick, grid_ticks, measure_ticks, notation_action, pitch_spelling,
        quantize_notes, tick_to_time, time_to_tick, valid_bpm,
    },
    transcription::Note,
};
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    sync::Arc,
};

const GUITAR_TUNING: &[u8] = &[40, 45, 50, 55, 59, 64];
const BASS_TUNING: &[u8] = &[28, 33, 38, 43];
const MAX_FRET: u8 = 24;
const STRING_SPACING: f32 = 15.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FretPosition {
    string: usize,
    fret: u8,
}

#[derive(Clone, Copy)]
struct HeldString {
    end: i64,
    fret: u8,
}

#[derive(Clone)]
struct ChordAssignment {
    cost: f32,
    lowest_fret: Option<u8>,
    highest_fret: Option<u8>,
    positions: Vec<(usize, FretPosition)>,
}

struct TabSegment {
    start: i64,
    end: i64,
    pitch: u8,
    position: Option<FretPosition>,
    tied_from: bool,
    tied_to: bool,
}

struct PreparedTablature {
    key: u64,
    segments: Vec<TabSegment>,
    unknown_notes: usize,
    first_tick: i64,
    bar: i64,
    bars: usize,
    row_height: f32,
}

pub fn supports_tablature(notes: &[Note]) -> bool {
    tuning_for_notes(notes).is_some()
}

fn minimum_bar_width(settings: &ScoreSettings) -> f32 {
    let slots = measure_ticks(settings) as f32 / grid_ticks(settings) as f32;
    (slots * 30.0 + 28.0).max(280.0)
}

fn tuning_for_notes(notes: &[Note]) -> Option<&'static [u8]> {
    if notes.is_empty() || notes.iter().any(|note| note.is_drum) {
        return None;
    }
    if notes.iter().all(|note| (24..=31).contains(&note.program)) {
        Some(GUITAR_TUNING)
    } else if notes.iter().all(|note| (32..=39).contains(&note.program)) {
        Some(BASS_TUNING)
    } else {
        None
    }
}

fn fret_positions(pitch: u8, tuning: &[u8]) -> Vec<FretPosition> {
    tuning
        .iter()
        .enumerate()
        .filter_map(|(string, open_pitch)| {
            pitch
                .checked_sub(*open_pitch)
                .filter(|fret| *fret <= MAX_FRET)
                .map(|fret| FretPosition { string, fret })
        })
        .collect()
}

fn fret_span_cost(lowest: Option<u8>, highest: Option<u8>) -> f32 {
    match (lowest, highest) {
        (Some(lowest), Some(highest)) => {
            let span = f32::from(highest - lowest);
            span * 0.18 + (span - 4.0).max(0.0).powi(2) * 1.8
        }
        _ => 0.0,
    }
}

fn assign_positions(notes: &[QuantizedNote], tuning: &[u8]) -> Vec<Option<FretPosition>> {
    let mut positions = vec![None; notes.len()];
    let playable_positions = notes
        .iter()
        .map(|note| fret_positions(note.pitch, tuning))
        .collect::<Vec<_>>();
    let mut held = vec![None::<HeldString>; tuning.len()];
    let mut previous_frets = vec![None::<u8>; tuning.len()];
    let mut hand_position = 5.0_f32;
    let mut group_start = 0;
    while group_start < notes.len() {
        let start = notes[group_start].start;
        let group_end =
            group_start + notes[group_start..].partition_point(|note| note.start == start);
        for string in &mut held {
            if string.is_some_and(|held_note| held_note.end <= start) {
                *string = None;
            }
        }
        let occupied = held
            .iter()
            .enumerate()
            .fold(0_usize, |mask, (index, note)| {
                mask | if note.is_some() { 1 << index } else { 0 }
            });
        let held_frets: Vec<_> = held
            .iter()
            .flatten()
            .map(|note| note.fret)
            .filter(|fret| *fret > 0)
            .collect();
        let mut states = vec![None::<ChordAssignment>; 1 << tuning.len()];
        states[0] = Some(ChordAssignment {
            cost: 0.0,
            lowest_fret: held_frets.iter().copied().min(),
            highest_fret: held_frets.iter().copied().max(),
            positions: Vec::with_capacity(tuning.len()),
        });
        for note_index in group_start..group_end {
            let note = &notes[note_index];
            let candidates = &playable_positions[note_index];
            let future_costs = candidates
                .iter()
                .map(|candidate| {
                    notes[group_end..]
                        .iter()
                        .enumerate()
                        .take(24)
                        .take_while(|(_, future)| future.start < note.end)
                        .map(|(index, future)| {
                            let available = playable_positions[group_end + index]
                                .iter()
                                .filter(|position| {
                                    held[position.string]
                                        .is_none_or(|held_note| held_note.end <= future.start)
                                })
                                .fold(0_usize, |mask, position| mask | 1 << position.string);
                            if available == 1 << candidate.string {
                                16.0 / (1.0 + (future.start - start) as f32 / TICKS_PER_BEAT as f32)
                            } else {
                                0.0
                            }
                        })
                        .sum::<f32>()
                })
                .collect::<Vec<_>>();
            let mut next = states.clone();
            for (mask, state) in states.iter().enumerate() {
                let Some(state) = state else {
                    continue;
                };
                for (candidate_index, candidate) in candidates.iter().enumerate() {
                    let string_mask = 1 << candidate.string;
                    if (mask | occupied) & string_mask != 0 {
                        continue;
                    }
                    let mut assigned = state.clone();
                    assigned.cost += future_costs[candidate_index];
                    let previous_span = fret_span_cost(assigned.lowest_fret, assigned.highest_fret);
                    if candidate.fret > 0 {
                        assigned.lowest_fret = Some(
                            assigned
                                .lowest_fret
                                .map_or(candidate.fret, |fret| fret.min(candidate.fret)),
                        );
                        assigned.highest_fret = Some(
                            assigned
                                .highest_fret
                                .map_or(candidate.fret, |fret| fret.max(candidate.fret)),
                        );
                        assigned.cost += (f32::from(candidate.fret) - hand_position).abs() * 0.7;
                    } else {
                        assigned.cost += 0.65;
                    }
                    assigned.cost += f32::from(candidate.fret) * 0.045;
                    if let Some(previous) = previous_frets[candidate.string] {
                        assigned.cost += f32::from(candidate.fret.abs_diff(previous)) * 0.35;
                    }
                    assigned.cost +=
                        fret_span_cost(assigned.lowest_fret, assigned.highest_fret) - previous_span;
                    for (previous_index, previous) in &assigned.positions {
                        let previous_pitch = notes[*previous_index].pitch;
                        if (previous_pitch < note.pitch && previous.string > candidate.string)
                            || (previous_pitch > note.pitch && previous.string < candidate.string)
                        {
                            assigned.cost += 4.0;
                        }
                    }
                    assigned.positions.push((note_index, *candidate));
                    let next_mask = mask | string_mask;
                    if next[next_mask]
                        .as_ref()
                        .is_none_or(|previous| assigned.cost < previous.cost)
                    {
                        next[next_mask] = Some(assigned);
                    }
                }
            }
            states = next;
        }
        let best = states
            .into_iter()
            .enumerate()
            .filter_map(|(mask, state)| state.map(|state| (mask.count_ones(), state)))
            .max_by(|(left_count, left), (right_count, right)| {
                left_count
                    .cmp(right_count)
                    .then_with(|| right.cost.total_cmp(&left.cost))
            });
        if let Some((_, assignment)) = best {
            let mut frets = held_frets;
            for (index, position) in assignment.positions {
                positions[index] = Some(position);
                held[position.string] = Some(HeldString {
                    end: notes[index].end,
                    fret: position.fret,
                });
                previous_frets[position.string] = Some(position.fret);
                if position.fret > 0 {
                    frets.push(position.fret);
                }
            }
            frets.sort_unstable();
            if let Some(fret) = frets.get(frets.len() / 2) {
                hand_position = f32::from(*fret);
            }
        }
        group_start = group_end;
    }
    positions
}

fn prepare_tablature(
    ui: &egui::Ui,
    notes: &[Note],
    settings: &ScoreSettings,
    tuning: &'static [u8],
) -> Arc<PreparedTablature> {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    settings.bpm.to_bits().hash(&mut hash);
    settings.offset.to_bits().hash(&mut hash);
    settings.beats_per_bar.hash(&mut hash);
    settings.subdivisions.hash(&mut hash);
    for note in notes {
        note.onset.to_bits().hash(&mut hash);
        note.offset.to_bits().hash(&mut hash);
        note.pitch.hash(&mut hash);
        note.program.hash(&mut hash);
        note.is_drum.hash(&mut hash);
    }
    let key = hash.finish();
    let id = ui.id().with("prepared_tablature");
    if let Some(cached) = ui
        .ctx()
        .data(|storage| storage.get_temp::<Arc<PreparedTablature>>(id))
        && cached.key == key
    {
        return cached;
    }
    let quantized = quantize_notes(notes, settings);
    let positions = assign_positions(&quantized, tuning);
    let unknown_notes = positions
        .iter()
        .filter(|position| position.is_none())
        .count();
    let first_tick = first_measure_tick(&quantized, settings);
    let bar = measure_ticks(settings);
    let last_tick = quantized
        .iter()
        .map(|note| note.end)
        .max()
        .unwrap_or(first_tick + bar);
    let bars = ((last_tick - first_tick + bar - 1) / bar).max(1) as usize;
    let mut segments = Vec::new();
    for (note, position) in quantized.iter().zip(positions) {
        let mut start = note.start;
        while start < note.end {
            let end = note.end.min((start.div_euclid(bar) + 1) * bar);
            segments.push(TabSegment {
                start,
                end,
                pitch: note.pitch,
                position,
                tied_from: start > note.start,
                tied_to: end < note.end,
            });
            start = end;
        }
    }
    segments.sort_by_key(|segment| (segment.start, segment.pitch));
    let prepared = Arc::new(PreparedTablature {
        key,
        segments,
        unknown_notes,
        first_tick,
        bar,
        bars,
        row_height: 110.0 + (tuning.len() - 1) as f32 * STRING_SPACING,
    });
    ui.ctx()
        .data_mut(|storage| storage.insert_temp(id, Arc::clone(&prepared)));
    prepared
}

pub fn render_tablature(
    ui: &mut egui::Ui,
    notes: &[Note],
    settings: &ScoreSettings,
    position: f64,
    chords: &[crate::harmony::Chord],
) -> Option<NotationAction> {
    let paper = Color32::WHITE;
    let ink = Color32::BLACK;
    let Some(tuning) = tuning_for_notes(notes) else {
        ui.label("La tablatura è disponibile per le tracce di chitarra e basso.");
        return None;
    };
    let prepared = prepare_tablature(ui, notes, settings, tuning);
    let mut action = None;
    let frame = egui::Frame::new()
        .fill(paper)
        .corner_radius(0.0)
        .inner_margin(16.0)
        .show(ui, |ui| {
            if prepared.segments.is_empty() {
                ui.allocate_ui(Vec2::new(ui.available_width(), 240.0), |ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            egui::RichText::new("Nessuna nota disponibile per questa traccia.")
                                .color(ink),
                        );
                    });
                });
                return;
            }
            let available_width = ui.available_width();
            let minimum_bar_width = minimum_bar_width(settings);
            let columns = ((available_width - 60.0) / minimum_bar_width)
                .floor()
                .clamp(1.0, 4.0) as usize;
            let row_width = available_width.max(minimum_bar_width * columns as f32 + 60.0);
            let rows = prepared.bars.div_ceil(columns);
            let playback_tick = time_to_tick(position, settings);
            let playback_bar = ((playback_tick - prepared.first_tick as f64) / prepared.bar as f64)
                .floor()
                .max(0.0) as usize;
            let active_row = (playback_bar / columns).min(rows - 1);
            let follow_id = ui.id().with("tablature_follow_row");
            let follow_state = (prepared.key, active_row, settings.follow, columns);
            let follow_changed = ui.ctx().data(|storage| {
                storage.get_temp::<(u64, usize, bool, usize)>(follow_id) != Some(follow_state)
            });
            ui.ctx()
                .data_mut(|storage| storage.insert_temp(follow_id, follow_state));
            let max_height = (ui.available_height() - 30.0).max(80.0);
            egui::ScrollArea::both()
                .id_salt("tablature_pages")
                .max_width(available_width)
                .max_height(max_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for row in 0..rows {
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(row_width, prepared.row_height),
                            Sense::click(),
                        );
                        let row_bars = columns.min(prepared.bars - row * columns);
                        let start =
                            prepared.first_tick + row as i64 * columns as i64 * prepared.bar;
                        let end = start + row_bars as i64 * prepared.bar;
                        let left = rect.left() + 52.0;
                        let width = (rect.right() - 8.0 - left) / columns as f32;
                        let geometry = RowGeometry {
                            start,
                            end,
                            bar_ticks: prepared.bar,
                            left,
                            width,
                            top: rect.top() + 64.0,
                            string_count: tuning.len(),
                        };
                        if settings.follow
                            && row == active_row
                            && (follow_changed || row_width > available_width)
                        {
                            let target = Rect::from_center_size(
                                Pos2::new(geometry.x(playback_tick), rect.center().y),
                                Vec2::new(2.0, prepared.row_height),
                            );
                            ui.scroll_to_rect(target, Some(egui::Align::Center));
                        }
                        if !ui.is_rect_visible(rect) {
                            continue;
                        }
                        let first_segment = prepared
                            .segments
                            .partition_point(|segment| segment.start < start);
                        let last_segment = prepared
                            .segments
                            .partition_point(|segment| segment.start < end);
                        let hits = draw_row(
                            ui,
                            &geometry,
                            &prepared.segments[first_segment..last_segment],
                            tuning,
                            row * columns,
                            row_bars,
                            settings,
                            playback_tick,
                            paper,
                            ink,
                        );
                        draw_chord_symbols(
                            ui.painter(),
                            chords,
                            settings,
                            start,
                            end,
                            rect.top() + 13.0,
                            left + row_bars as f32 * width,
                            |tick| geometry.x(tick),
                        );
                        if ui
                            .input(|input| input.pointer.hover_pos())
                            .is_some_and(|pointer| {
                                hits.iter().any(|hit| hit.rect.contains(pointer))
                            })
                        {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if response.clicked()
                            && let Some(pointer) = response.interact_pointer_pos()
                        {
                            let measure = ((pointer.x - left) / width)
                                .floor()
                                .clamp(0.0, row_bars.saturating_sub(1) as f32)
                                as i64;
                            let fraction = ((pointer.x - left - measure as f32 * width - 14.0)
                                / (width - 28.0))
                                .clamp(0.0, 1.0);
                            let time = tick_to_time(
                                (start + measure * prepared.bar) as f64
                                    + f64::from(fraction) * prepared.bar as f64,
                                settings,
                            );
                            action = Some(notation_action(pointer, &hits, time));
                        }
                    }
                });
        });
    if prepared.unknown_notes > 0 {
        frame.response.on_hover_text(format!(
            "? {} note senza posizione: fuori dai 24 tasti o corde già occupate",
            prepared.unknown_notes
        ));
    }
    action
}

struct RowGeometry {
    start: i64,
    end: i64,
    bar_ticks: i64,
    left: f32,
    width: f32,
    top: f32,
    string_count: usize,
}

impl RowGeometry {
    fn x(&self, tick: f64) -> f32 {
        let relative = (tick - self.start as f64) / self.bar_ticks as f64;
        let measure = relative.floor();
        let fraction = relative - measure;
        self.left + measure as f32 * self.width + 14.0 + fraction as f32 * (self.width - 28.0)
    }

    fn end_x(&self, tick: i64) -> f32 {
        if tick.rem_euclid(self.bar_ticks) == 0 {
            self.left + ((tick - self.start) / self.bar_ticks) as f32 * self.width - 14.0
        } else {
            self.x(tick as f64)
        }
    }

    fn y(&self, string: usize) -> f32 {
        self.top + (self.string_count - string - 1) as f32 * STRING_SPACING
    }

    fn bottom(&self) -> f32 {
        self.top + (self.string_count - 1) as f32 * STRING_SPACING
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_row(
    ui: &mut egui::Ui,
    geometry: &RowGeometry,
    segments: &[TabSegment],
    tuning: &[u8],
    first_bar: usize,
    bars: usize,
    settings: &ScoreSettings,
    playback_tick: f64,
    paper: Color32,
    ink: Color32,
) -> Vec<NoteHit> {
    let mut hits = Vec::new();
    let painter = ui.painter();
    let end_x = geometry.left + bars as f32 * geometry.width;
    let staff_stroke = Stroke::new(0.85, ink);
    for (string, pitch) in tuning.iter().enumerate() {
        let y = geometry.y(string);
        painter.line_segment(
            [Pos2::new(geometry.left - 22.0, y), Pos2::new(end_x, y)],
            staff_stroke,
        );
        let (name, _, _) = pitch_spelling(*pitch);
        painter.text(
            Pos2::new(geometry.left - 32.0, y),
            egui::Align2::CENTER_CENTER,
            name,
            FontId::proportional(15.0),
            ink,
        );
    }
    for measure in 0..=bars {
        let x = geometry.left + measure as f32 * geometry.width;
        painter.line_segment(
            [Pos2::new(x, geometry.top), Pos2::new(x, geometry.bottom())],
            staff_stroke,
        );
        if measure == bars {
            continue;
        }
        painter.text(
            Pos2::new(x + 2.0, geometry.top - 36.0),
            egui::Align2::LEFT_CENTER,
            (first_bar + measure + 1).to_string(),
            FontId::proportional(11.0),
            ink,
        );
        let measure_start = geometry.start + measure as i64 * geometry.bar_ticks;
        for beat in 0..settings.beats_per_bar.clamp(1, 12) {
            let tick = measure_start + i64::from(beat) * TICKS_PER_BEAT;
            let x = geometry.x(tick as f64);
            let active =
                playback_tick >= tick as f64 && playback_tick < (tick + TICKS_PER_BEAT) as f64;
            painter.circle_filled(
                Pos2::new(x, geometry.bottom() + 20.0),
                if active {
                    3.1
                } else if beat == 0 {
                    2.4
                } else {
                    1.6
                },
                ink,
            );
        }
        let grid = grid_ticks(settings);
        for tick in (measure_start..measure_start + geometry.bar_ticks).step_by(grid as usize) {
            if tick.rem_euclid(TICKS_PER_BEAT) != 0 {
                let x = geometry.x(tick as f64);
                painter.line_segment(
                    [
                        Pos2::new(x, geometry.bottom() + 19.0),
                        Pos2::new(x, geometry.bottom() + 21.0),
                    ],
                    Stroke::new(0.7, ink),
                );
            }
        }
    }
    if playback_tick >= geometry.start as f64 && playback_tick < geometry.end as f64 {
        let x = geometry.x(playback_tick);
        painter.line_segment(
            [
                Pos2::new(x, geometry.top - 15.0),
                Pos2::new(x, geometry.bottom() + 10.0),
            ],
            Stroke::new(1.2, ink),
        );
        painter.circle_filled(Pos2::new(x, geometry.top - 18.0), 3.0, ink);
    }
    let mut unknown: BTreeMap<i64, Vec<&TabSegment>> = BTreeMap::new();
    for segment in segments {
        let Some(position) = segment.position else {
            unknown.entry(segment.start).or_default().push(segment);
            continue;
        };
        let active = playback_tick >= segment.start as f64 && playback_tick < segment.end as f64;
        let x = geometry.x(segment.start as f64);
        let y = geometry.y(position.string);
        let label = if segment.tied_from {
            format!("({})", position.fret)
        } else {
            position.fret.to_string()
        };
        let label_color = if active { paper } else { ink };
        let galley = painter.layout_no_wrap(label, FontId::proportional(16.0), label_color);
        let label_rect = tab_label_rect(Pos2::new(x, y), galley.size());
        let label_width = label_rect.width();
        let line_start = x + label_width / 2.0 + 2.0;
        let line_end = geometry.end_x(segment.end) - 3.0;
        if line_end > line_start {
            painter.line_segment(
                [Pos2::new(line_start, y + 5.0), Pos2::new(line_end, y + 5.0)],
                Stroke::new(1.15, ink),
            );
            painter.line_segment(
                [Pos2::new(line_end, y + 2.0), Pos2::new(line_end, y + 7.0)],
                Stroke::new(1.0, ink),
            );
        }
        painter.rect_filled(label_rect, 0.0, if active { ink } else { paper });
        painter.galley(Pos2::new(x, y) - galley.size() / 2.0, galley, label_color);
        hits.push(NoteHit {
            rect: label_rect.expand(1.0),
            pitch: segment.pitch,
            duration: (segment.end - segment.start) as f64 * 60.0
                / (valid_bpm(settings) * TICKS_PER_BEAT as f64),
        });
        if segment.tied_to {
            let boundary_x = geometry.left
                + ((segment.end - geometry.start) / geometry.bar_ticks) as f32 * geometry.width;
            painter.line_segment(
                [
                    Pos2::new(line_end.max(x + 4.0), y + 5.0),
                    Pos2::new(boundary_x - 3.0, y + 5.0),
                ],
                Stroke::new(1.15, ink),
            );
            painter.line_segment(
                [
                    Pos2::new(boundary_x - 6.0, y + 2.0),
                    Pos2::new(boundary_x - 3.0, y + 5.0),
                ],
                Stroke::new(1.0, ink),
            );
        }
    }
    for (start, missing) in unknown {
        for (index, segment) in missing.iter().enumerate() {
            let center = Pos2::new(
                geometry.x(start as f64) + index as f32 * 14.0,
                geometry.top - 17.0,
            );
            let marker = Rect::from_center_size(center, Vec2::new(13.0, 18.0));
            ui.painter().rect_filled(marker, 0.0, paper);
            ui.painter().text(
                center,
                egui::Align2::CENTER_CENTER,
                "?",
                FontId::proportional(16.0),
                ink,
            );
            hits.push(NoteHit {
                rect: marker,
                pitch: segment.pitch,
                duration: (segment.end - segment.start) as f64 * 60.0
                    / (valid_bpm(settings) * TICKS_PER_BEAT as f64),
            });
            let (name, alteration, octave) = pitch_spelling(segment.pitch);
            ui.interact(
                marker,
                ui.id().with(("tab_missing", start, index)),
                Sense::hover(),
            )
            .on_hover_text(format!(
                "{}{}{} · {:.2} s\nNessuna posizione disponibile. Clic per sentire la nota.",
                name,
                if alteration == 1 { "♯" } else { "" },
                octave,
                tick_to_time(start as f64, settings)
            ));
        }
    }
    hits
}

fn tab_label_rect(center: Pos2, size: Vec2) -> Rect {
    Rect::from_center_size(center, Vec2::new(size.x + 6.0, size.y.max(15.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(onset: f64, offset: f64, pitch: u8, program: u8) -> Note {
        Note {
            onset,
            offset,
            pitch,
            program,
            is_drum: false,
        }
    }

    #[test]
    fn clicking_a_two_digit_fret_or_its_tied_repeat_previews_the_concert_pitch() {
        let context = egui::Context::default();
        let settings = ScoreSettings::default();
        let geometry = RowGeometry {
            start: 0,
            end: 384,
            bar_ticks: 384,
            left: 40.0,
            width: 400.0,
            top: 100.0,
            string_count: 6,
        };
        for tied_from in [false, true] {
            let segment = TabSegment {
                start: 96,
                end: 192,
                pitch: 76,
                position: Some(FretPosition {
                    string: 5,
                    fret: 12,
                }),
                tied_from,
                tied_to: false,
            };
            let mut hits = Vec::new();
            let _ = context.run(egui::RawInput::default(), |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    hits = draw_row(
                        ui,
                        &geometry,
                        std::slice::from_ref(&segment),
                        GUITAR_TUNING,
                        0,
                        1,
                        &settings,
                        -1.0,
                        Color32::WHITE,
                        Color32::BLACK,
                    );
                });
            });
            let hit = &hits[0];
            assert!(hit.rect.width() > 14.0);
            let pointer = Pos2::new(hit.rect.right() - 1.0, hit.rect.center().y);
            assert_eq!(
                notation_action(pointer, &hits, 10.0),
                NotationAction::Preview {
                    pitch: 76,
                    duration: 0.5
                },
            );
        }
    }

    #[test]
    fn a_note_without_a_playable_fret_remains_auditionable() {
        let context = egui::Context::default();
        let settings = ScoreSettings::default();
        let geometry = RowGeometry {
            start: 0,
            end: 384,
            bar_ticks: 384,
            left: 40.0,
            width: 400.0,
            top: 100.0,
            string_count: 6,
        };
        let segment = TabSegment {
            start: 96,
            end: 192,
            pitch: 39,
            position: None,
            tied_from: false,
            tied_to: false,
        };
        let mut hits = Vec::new();
        let _ = context.run(egui::RawInput::default(), |context| {
            egui::CentralPanel::default().show(context, |ui| {
                hits = draw_row(
                    ui,
                    &geometry,
                    std::slice::from_ref(&segment),
                    GUITAR_TUNING,
                    0,
                    1,
                    &settings,
                    -1.0,
                    Color32::WHITE,
                    Color32::BLACK,
                );
            });
        });
        assert_eq!(
            notation_action(hits[0].rect.center(), &hits, 10.0),
            NotationAction::Preview {
                pitch: 39,
                duration: 0.5
            },
        );
    }

    #[test]
    fn dense_grids_leave_room_for_two_digit_frets() {
        for beats_per_bar in [4, 7] {
            for subdivisions in [4, 6] {
                let settings = ScoreSettings {
                    beats_per_bar,
                    subdivisions,
                    ..Default::default()
                };
                let bar_ticks = measure_ticks(&settings);
                let geometry = RowGeometry {
                    start: 0,
                    end: bar_ticks,
                    bar_ticks,
                    left: 0.0,
                    width: minimum_bar_width(&settings),
                    top: 0.0,
                    string_count: 6,
                };
                let spacing = geometry.x(grid_ticks(&settings) as f64) - geometry.x(0.0);
                assert!(spacing >= 29.9);
            }
        }
    }

    #[test]
    fn every_playable_pitch_roundtrips_without_octave_transposition() {
        for tuning in [GUITAR_TUNING, BASS_TUNING] {
            for pitch in tuning[0]..=tuning[tuning.len() - 1] + MAX_FRET {
                let candidates = fret_positions(pitch, tuning);
                assert!(!candidates.is_empty());
                for position in candidates {
                    assert_eq!(tuning[position.string] + position.fret, pitch);
                    assert!(position.fret <= MAX_FRET);
                }
            }
        }
    }

    #[test]
    fn six_note_chord_uses_six_distinct_strings() {
        let notes = GUITAR_TUNING
            .iter()
            .map(|pitch| note(0.0, 1.0, *pitch, 26))
            .collect::<Vec<_>>();
        let quantized = quantize_notes(&notes, &ScoreSettings::default());
        let positions = assign_positions(&quantized, GUITAR_TUNING);
        let mut strings = positions
            .iter()
            .map(|position| position.unwrap().string)
            .collect::<Vec<_>>();
        strings.sort_unstable();
        strings.dedup();
        assert_eq!(strings.len(), 6);
    }

    #[test]
    fn sustained_note_keeps_its_string_reserved_until_its_end() {
        let notes = [
            note(0.0, 1.0, 40, 26),
            note(0.5, 0.75, 41, 26),
            note(1.0, 1.5, 41, 26),
        ];
        let quantized = quantize_notes(&notes, &ScoreSettings::default());
        let positions = assign_positions(&quantized, GUITAR_TUNING);
        assert_eq!(positions[0], Some(FretPosition { string: 0, fret: 0 }));
        assert_eq!(positions[1], None);
        assert_eq!(positions[2], Some(FretPosition { string: 0, fret: 1 }));
    }

    #[test]
    fn playable_overlap_uses_another_string() {
        let notes = [note(0.0, 1.0, 55, 26), note(0.5, 1.5, 57, 26)];
        let quantized = quantize_notes(&notes, &ScoreSettings::default());
        let positions = assign_positions(&quantized, GUITAR_TUNING);
        assert!(positions.iter().all(Option::is_some));
        assert_ne!(positions[0].unwrap().string, positions[1].unwrap().string);
    }

    #[test]
    fn upcoming_low_note_prevents_an_unnecessary_string_collision() {
        let notes = [note(0.0, 1.0, 45, 26), note(0.5, 1.5, 40, 26)];
        let quantized = quantize_notes(&notes, &ScoreSettings::default());
        let positions = assign_positions(&quantized, GUITAR_TUNING);
        assert_eq!(positions[0], Some(FretPosition { string: 1, fret: 0 }));
        assert_eq!(positions[1], Some(FretPosition { string: 0, fret: 0 }));
    }

    #[test]
    fn unsupported_tracks_do_not_receive_a_guessed_tuning() {
        assert!(!supports_tablature(&[]));
        assert!(!supports_tablature(&[note(0.0, 1.0, 60, 0)]));
        let mut drum = note(0.0, 1.0, 40, 26);
        drum.is_drum = true;
        assert!(!supports_tablature(&[drum]));
        assert!(!supports_tablature(&[
            note(0.0, 1.0, 40, 26),
            note(0.0, 1.0, 40, 33),
        ]));
        for program in 24..=39 {
            assert!(supports_tablature(&[note(0.0, 1.0, 40, program)]));
        }
    }

    #[test]
    fn out_of_range_and_impossible_chords_preserve_unassigned_notes() {
        let notes = [
            note(0.0, 1.0, 39, 26),
            note(0.0, 1.0, 40, 26),
            note(0.0, 1.0, 41, 26),
            note(0.0, 1.0, 89, 26),
        ];
        let quantized = quantize_notes(&notes, &ScoreSettings::default());
        let positions = assign_positions(&quantized, GUITAR_TUNING);
        assert_eq!(positions.len(), notes.len());
        assert_eq!(
            positions
                .iter()
                .filter(|position| position.is_none())
                .count(),
            3
        );
        assert_eq!(
            quantized.iter().map(|note| note.pitch).collect::<Vec<_>>(),
            [39, 40, 41, 89]
        );
        assert_eq!(notes[0].pitch, 39);
        assert_eq!(notes[3].pitch, 89);
    }
}
