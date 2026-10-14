use crate::transcription::Note;
use eframe::egui::{self, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Vec2};
use std::{
    collections::BTreeMap,
    hash::{Hash, Hasher},
    sync::Arc,
};

pub(crate) const TICKS_PER_BEAT: i64 = 96;

#[derive(Clone, Debug, PartialEq)]
pub enum NotationAction {
    Seek(f64),
    Preview { pitch: u8, duration: f64 },
}

pub(crate) struct NoteHit {
    pub rect: Rect,
    pub pitch: u8,
    pub duration: f64,
}

pub(crate) fn notation_action(pointer: Pos2, hits: &[NoteHit], time: f64) -> NotationAction {
    hits.iter()
        .filter(|hit| hit.rect.contains(pointer))
        .min_by(|first, second| {
            first
                .rect
                .center()
                .distance_sq(pointer)
                .total_cmp(&second.rect.center().distance_sq(pointer))
        })
        .map(|hit| NotationAction::Preview {
            pitch: hit.pitch,
            duration: hit.duration,
        })
        .unwrap_or(NotationAction::Seek(time))
}

pub struct ScoreSettings {
    pub bpm: f64,
    pub beats_per_bar: u8,
    pub subdivisions: u8,
    pub offset: f64,
    pub follow: bool,
}

impl Default for ScoreSettings {
    fn default() -> Self {
        Self {
            bpm: 120.0,
            beats_per_bar: 4,
            subdivisions: 4,
            offset: 0.0,
            follow: true,
        }
    }
}

pub(crate) struct QuantizedNote {
    pub start: i64,
    pub end: i64,
    pub pitch: u8,
    pub program: u8,
    pub is_drum: bool,
    pub voice: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NotatedDuration {
    pub ticks: i64,
    pub kind: &'static str,
    pub dots: u8,
    pub triplet: bool,
}

pub(crate) struct NoteSegment {
    pub start: i64,
    pub duration: NotatedDuration,
    pub pitch: u8,
    pub is_drum: bool,
    pub voice: usize,
    pub tied_from: bool,
    pub tied_to: bool,
}

pub(crate) fn valid_bpm(settings: &ScoreSettings) -> f64 {
    if settings.bpm.is_finite() && settings.bpm > 0.0 {
        settings.bpm.clamp(20.0, 400.0)
    } else {
        120.0
    }
}

pub(crate) fn measure_ticks(settings: &ScoreSettings) -> i64 {
    i64::from(settings.beats_per_bar.clamp(1, 12)) * TICKS_PER_BEAT
}

pub(crate) fn grid_ticks(settings: &ScoreSettings) -> i64 {
    let subdivisions = i64::from(settings.subdivisions.clamp(1, 24));
    if TICKS_PER_BEAT % subdivisions == 0 {
        TICKS_PER_BEAT / subdivisions
    } else {
        TICKS_PER_BEAT / 4
    }
}

pub(crate) fn time_to_tick(time: f64, settings: &ScoreSettings) -> f64 {
    (time - settings.offset) * valid_bpm(settings) * TICKS_PER_BEAT as f64 / 60.0
}

pub(crate) fn tick_to_time(tick: f64, settings: &ScoreSettings) -> f64 {
    (settings.offset + tick * 60.0 / (valid_bpm(settings) * TICKS_PER_BEAT as f64)).max(0.0)
}

pub(crate) fn quantize_notes(notes: &[Note], settings: &ScoreSettings) -> Vec<QuantizedNote> {
    let grid = grid_ticks(settings);
    let mut quantized: Vec<_> = notes
        .iter()
        .filter(|note| {
            note.onset.is_finite() && note.offset.is_finite() && note.offset > note.onset
        })
        .map(|note| {
            let start = (time_to_tick(note.onset, settings) / grid as f64).round() as i64 * grid;
            let end = (time_to_tick(note.offset, settings) / grid as f64).round() as i64 * grid;
            QuantizedNote {
                start,
                end: end.max(start + grid),
                pitch: note.pitch,
                program: note.program,
                is_drum: note.is_drum,
                voice: 0,
            }
        })
        .collect();
    quantized.sort_by_key(|note| (note.start, note.end, note.pitch));
    let mut voices: Vec<i64> = Vec::new();
    let mut last_chord = None;
    for note in &mut quantized {
        let chord = (note.start, note.end, note.program, note.is_drum);
        let voice = if let Some((previous, voice)) = last_chord {
            if previous == chord {
                voice
            } else {
                allocate_voice(&mut voices, note.start, note.end)
            }
        } else {
            allocate_voice(&mut voices, note.start, note.end)
        };
        note.voice = voice;
        last_chord = Some((chord, voice));
    }
    quantized
}

fn allocate_voice(voices: &mut Vec<i64>, start: i64, end: i64) -> usize {
    if let Some(voice) = voices
        .iter()
        .position(|previous_end| *previous_end <= start)
    {
        voices[voice] = end;
        voice
    } else {
        voices.push(end);
        voices.len() - 1
    }
}

pub(crate) fn first_measure_tick(notes: &[QuantizedNote], settings: &ScoreSettings) -> i64 {
    notes
        .iter()
        .map(|note| note.start)
        .min()
        .unwrap_or(0)
        .min(time_to_tick(0.0, settings).floor() as i64)
        .div_euclid(measure_ticks(settings))
        * measure_ticks(settings)
}

pub(crate) fn split_duration(mut ticks: i64, grid: i64) -> Vec<NotatedDuration> {
    let kinds = [
        ("whole", 384),
        ("half", 192),
        ("quarter", 96),
        ("eighth", 48),
        ("16th", 24),
        ("32nd", 12),
        ("64th", 6),
    ];
    let mut candidates = Vec::new();
    for (kind, length) in kinds {
        for dots in 0..=1 {
            let duration = length + if dots == 1 { length / 2 } else { 0 };
            if duration % grid == 0 {
                candidates.push(NotatedDuration {
                    ticks: duration,
                    kind,
                    dots,
                    triplet: false,
                });
            }
        }
        let triplet = length * 2 / 3;
        if triplet > 0 && triplet % grid == 0 {
            candidates.push(NotatedDuration {
                ticks: triplet,
                kind,
                dots: 0,
                triplet: true,
            });
        }
    }
    candidates.sort_by_key(|duration| std::cmp::Reverse(duration.ticks));
    let mut durations = Vec::new();
    while ticks > 0 {
        let duration = candidates
            .iter()
            .find(|duration| duration.ticks <= ticks)
            .copied()
            .unwrap_or(NotatedDuration {
                ticks,
                kind: "64th",
                dots: 0,
                triplet: false,
            });
        durations.push(duration);
        ticks -= duration.ticks;
    }
    durations
}

pub(crate) fn split_notes(notes: &[QuantizedNote], settings: &ScoreSettings) -> Vec<NoteSegment> {
    let bar = measure_ticks(settings);
    let grid = grid_ticks(settings);
    let mut segments = Vec::new();
    for note in notes {
        let mut start = note.start;
        while start < note.end {
            let bar_end = (start.div_euclid(bar) + 1) * bar;
            let length = (note.end.min(bar_end) - start).max(1);
            for duration in split_duration(length, grid) {
                segments.push(NoteSegment {
                    start,
                    duration,
                    pitch: note.pitch,
                    is_drum: note.is_drum,
                    voice: note.voice,
                    tied_from: start > note.start,
                    tied_to: start + duration.ticks < note.end,
                });
                start += duration.ticks;
            }
        }
    }
    segments.sort_by_key(|segment| (segment.start, segment.voice, segment.pitch));
    segments
}

pub fn instrument_name(program: u8, is_drum: bool) -> &'static str {
    if is_drum {
        return "Batteria";
    }
    const INSTRUMENTS: [&str; 128] = [
        "Pianoforte",
        "Pianoforte brillante",
        "Piano elettrico a coda",
        "Honky-tonk",
        "Piano elettrico 1",
        "Piano elettrico 2",
        "Clavicembalo",
        "Clavinet",
        "Celesta",
        "Glockenspiel",
        "Music box",
        "Vibrafono",
        "Marimba",
        "Xilofono",
        "Campane tubolari",
        "Dulcimer",
        "Organo drawbar",
        "Organo percussivo",
        "Organo rock",
        "Organo a canne",
        "Harmonium",
        "Fisarmonica",
        "Armonica",
        "Bandoneon",
        "Chitarra acustica",
        "Chitarra acustica",
        "Chitarra jazz",
        "Chitarra clean",
        "Chitarra muted",
        "Chitarra overdrive",
        "Chitarra distorta",
        "Armonici di chitarra",
        "Contrabbasso",
        "Basso elettrico finger",
        "Basso elettrico pick",
        "Basso fretless",
        "Basso slap 1",
        "Basso slap 2",
        "Basso synth 1",
        "Basso synth 2",
        "Violino",
        "Viola",
        "Violoncello",
        "Contrabbasso ad arco",
        "Archi tremolo",
        "Archi pizzicato",
        "Arpa",
        "Timpani",
        "Archi 1",
        "Archi 2",
        "Archi synth 1",
        "Archi synth 2",
        "Coro ah",
        "Voci ooh",
        "Voce synth",
        "Orchestra hit",
        "Tromba",
        "Trombone",
        "Tuba",
        "Tromba con sordina",
        "Corno",
        "Ottoni",
        "Ottoni synth 1",
        "Ottoni synth 2",
        "Sax soprano",
        "Sax alto",
        "Sax tenore",
        "Sax baritono",
        "Oboe",
        "Corno inglese",
        "Fagotto",
        "Clarinetto",
        "Ottavino",
        "Flauto",
        "Flauto dolce",
        "Flauto di Pan",
        "Soffio",
        "Shakuhachi",
        "Fischio",
        "Ocarina",
        "Lead square",
        "Lead sawtooth",
        "Lead calliope",
        "Lead chiff",
        "Lead charang",
        "Lead voice",
        "Lead fifths",
        "Lead bass + lead",
        "Pad new age",
        "Pad warm",
        "Pad polysynth",
        "Pad choir",
        "Pad bowed",
        "Pad metallic",
        "Pad halo",
        "Pad sweep",
        "FX rain",
        "FX soundtrack",
        "FX crystal",
        "FX atmosphere",
        "FX brightness",
        "FX goblins",
        "FX echoes",
        "FX sci-fi",
        "Sitar",
        "Banjo",
        "Shamisen",
        "Koto",
        "Kalimba",
        "Cornamusa",
        "Fiddle",
        "Shanai",
        "Campanello",
        "Agogo",
        "Steel drums",
        "Woodblock",
        "Taiko",
        "Tom melodico",
        "Tamburo synth",
        "Piatto reverse",
        "Rumore delle corde",
        "Respiro",
        "Spiaggia",
        "Canto degli uccelli",
        "Telefono",
        "Elicottero",
        "Applausi",
        "Sparo",
    ];
    INSTRUMENTS[usize::from(program.min(127))]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Clef {
    Treble,
    Guitar,
    Bass,
    Percussion,
}

pub(crate) fn choose_clef(notes: &[Note]) -> Clef {
    if notes.iter().any(|note| note.is_drum) {
        return Clef::Percussion;
    }
    if !notes.is_empty() && notes.iter().all(|note| (24..=31).contains(&note.program)) {
        return Clef::Guitar;
    }
    let mut pitches: Vec<_> = notes.iter().map(|note| note.pitch).collect();
    pitches.sort_unstable();
    if pitches.get(pitches.len() / 2).copied().unwrap_or(60) < 60 {
        Clef::Bass
    } else {
        Clef::Treble
    }
}

pub(crate) fn pitch_spelling(pitch: u8) -> (&'static str, i8, i8) {
    let names = ["C", "C", "D", "D", "E", "F", "F", "G", "G", "A", "A", "B"];
    let alterations = [0, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1, 0];
    let index = usize::from(pitch % 12);
    (names[index], alterations[index], (pitch / 12) as i8 - 1)
}

pub(crate) fn drum_spelling(pitch: u8) -> (&'static str, i8) {
    match pitch {
        35 | 36 => ("F", 4),
        38 | 40 => ("C", 5),
        41 | 43 => ("G", 4),
        45 | 47 => ("A", 4),
        48 | 50 => ("D", 5),
        42 | 44 | 46 => ("G", 5),
        49 | 52 | 55 | 57 => ("A", 5),
        51 | 53 | 59 => ("F", 5),
        _ => ("B", 4),
    }
}

fn diatonic_step(step: &str, octave: i8) -> i32 {
    let letter = match step {
        "C" => 0,
        "D" => 1,
        "E" => 2,
        "F" => 3,
        "G" => 4,
        "A" => 5,
        _ => 6,
    };
    i32::from(octave) * 7 + letter
}

fn staff_step(pitch: u8, clef: Clef, is_drum: bool) -> i32 {
    let absolute = if is_drum {
        let (step, octave) = drum_spelling(pitch);
        diatonic_step(step, octave)
    } else {
        let (step, _, octave) = pitch_spelling(pitch);
        diatonic_step(step, octave)
    };
    absolute
        - match clef {
            Clef::Bass => 18,
            Clef::Treble => 30,
            Clef::Guitar => 23,
            Clef::Percussion => 31,
        }
}

struct PreparedScore {
    key: u64,
    quantized: Vec<QuantizedNote>,
    segments: Vec<NoteSegment>,
    clef: Clef,
    first_tick: i64,
    bar: i64,
    bars: usize,
    staff_offset: f32,
    row_height: f32,
}

fn prepare_score(ui: &egui::Ui, notes: &[Note], settings: &ScoreSettings) -> Arc<PreparedScore> {
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
    let id = ui.id().with("prepared_score");
    if let Some(cached) = ui
        .ctx()
        .data(|storage| storage.get_temp::<Arc<PreparedScore>>(id))
        && cached.key == key
    {
        return cached;
    }
    let quantized = quantize_notes(notes, settings);
    let clef = choose_clef(notes);
    let segments = split_notes(&quantized, settings);
    let first_tick = first_measure_tick(&quantized, settings);
    let bar = measure_ticks(settings);
    let last_tick = quantized
        .iter()
        .map(|note| note.end)
        .max()
        .unwrap_or(first_tick + bar);
    let bars = ((last_tick - first_tick + bar - 1) / bar).max(1) as usize;
    let lowest = notes
        .iter()
        .map(|note| staff_step(note.pitch, clef, note.is_drum))
        .min()
        .unwrap_or(0);
    let highest = notes
        .iter()
        .map(|note| staff_step(note.pitch, clef, note.is_drum))
        .max()
        .unwrap_or(8);
    let staff_offset = (42.0 + (highest - 8).max(0) as f32 * 5.0).max(54.0) + 28.0;
    let row_height = (staff_offset + 40.0 + (-lowest).max(0) as f32 * 5.0 + 48.0).max(166.0);
    let prepared = Arc::new(PreparedScore {
        key,
        quantized,
        segments,
        clef,
        first_tick,
        bar,
        bars,
        staff_offset,
        row_height,
    });
    ui.ctx()
        .data_mut(|storage| storage.insert_temp(id, Arc::clone(&prepared)));
    prepared
}

pub fn render_score(
    ui: &mut egui::Ui,
    notes: &[Note],
    settings: &ScoreSettings,
    position: f64,
    chords: &[crate::harmony::Chord],
) -> Option<NotationAction> {
    let paper = Color32::WHITE;
    let ink = Color32::BLACK;
    let prepared = prepare_score(ui, notes, settings);
    let mut action = None;
    egui::Frame::new()
        .fill(paper)
        .corner_radius(0.0)
        .inner_margin(16.0)
        .show(ui, |ui| {
            if prepared.quantized.is_empty() {
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
            let clef = prepared.clef;
            let segments = &prepared.segments;
            let bar = prepared.bar;
            let first_tick = prepared.first_tick;
            let bars = prepared.bars;
            let columns = ((ui.available_width() - 56.0) / 280.0)
                .floor()
                .clamp(1.0, 4.0) as usize;
            let rows = bars.div_ceil(columns);
            let playback_tick = time_to_tick(position, settings);
            let playback_bar = ((playback_tick - first_tick as f64) / bar as f64)
                .floor()
                .max(0.0) as usize;
            let active_row = (playback_bar / columns).min(rows - 1);
            let follow_id = ui.id().with("score_follow_row");
            let follow_state = (prepared.key, active_row, settings.follow, columns);
            let follow_changed = ui.ctx().data(|storage| {
                storage.get_temp::<(u64, usize, bool, usize)>(follow_id) != Some(follow_state)
            });
            ui.ctx()
                .data_mut(|storage| storage.insert_temp(follow_id, follow_state));
            let max_height = ui.available_height().max(80.0);
            egui::ScrollArea::vertical()
                .id_salt("score_pages")
                .max_height(max_height)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for row in 0..rows {
                        let (rect, response) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), prepared.row_height),
                            Sense::click(),
                        );
                        if settings.follow && follow_changed && row == active_row {
                            ui.scroll_to_rect(rect, Some(egui::Align::Center));
                        }
                        if !ui.is_rect_visible(rect) {
                            continue;
                        }
                        let row_bars = columns.min(bars - row * columns);
                        let row_start = first_tick + row as i64 * columns as i64 * bar;
                        let row_end = row_start + row_bars as i64 * bar;
                        let staff_left = rect.left() + 52.0;
                        let staff_right = rect.right() - 8.0;
                        let bar_width = (staff_right - staff_left) / columns as f32;
                        let staff_top = rect.top() + prepared.staff_offset;
                        let geometry = RowGeometry {
                            start: row_start,
                            end: row_end,
                            bar_ticks: bar,
                            left: staff_left,
                            width: bar_width,
                            staff_top,
                        };
                        let first_segment =
                            segments.partition_point(|segment| segment.start < row_start);
                        let last_segment =
                            segments.partition_point(|segment| segment.start < row_end);
                        let hits = draw_row(
                            ui.painter(),
                            &geometry,
                            &segments[first_segment..last_segment],
                            clef,
                            row * columns,
                            row_bars,
                            settings,
                            playback_tick,
                            ink,
                        );
                        draw_chord_symbols(
                            ui.painter(),
                            chords,
                            settings,
                            row_start,
                            row_end,
                            rect.top() + 13.0,
                            staff_left + row_bars as f32 * bar_width,
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
                            let measure = ((pointer.x - staff_left) / bar_width)
                                .floor()
                                .clamp(0.0, row_bars.saturating_sub(1) as f32)
                                as i64;
                            let fraction =
                                ((pointer.x - staff_left - measure as f32 * bar_width - 14.0)
                                    / (bar_width - 28.0))
                                    .clamp(0.0, 1.0);
                            let time = tick_to_time(
                                (row_start + measure * bar) as f64
                                    + f64::from(fraction) * bar as f64,
                                settings,
                            );
                            action = Some(notation_action(pointer, &hits, time));
                        }
                    }
                });
        });
    action
}

struct RowGeometry {
    start: i64,
    end: i64,
    bar_ticks: i64,
    left: f32,
    width: f32,
    staff_top: f32,
}

impl RowGeometry {
    fn x(&self, tick: f64) -> f32 {
        let relative = tick - self.start as f64;
        let measure = (relative / self.bar_ticks as f64).floor();
        let fraction = relative / self.bar_ticks as f64 - measure;
        self.left + measure as f32 * self.width + 14.0 + fraction as f32 * (self.width - 28.0)
    }

    fn y(&self, pitch: u8, clef: Clef, is_drum: bool) -> f32 {
        self.staff_top + 40.0 - staff_step(pitch, clef, is_drum) as f32 * 5.0
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_row(
    painter: &egui::Painter,
    geometry: &RowGeometry,
    segments: &[NoteSegment],
    clef: Clef,
    first_bar: usize,
    bars: usize,
    settings: &ScoreSettings,
    playback_tick: f64,
    ink: Color32,
) -> Vec<NoteHit> {
    let mut hits = Vec::new();
    let end_x = geometry.left + bars as f32 * geometry.width;
    if playback_tick >= geometry.start as f64 && playback_tick < geometry.end as f64 {
        let measure = ((playback_tick - geometry.start as f64) / geometry.bar_ticks as f64).floor();
        let left = geometry.left + measure as f32 * geometry.width;
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(left, geometry.staff_top - 45.0),
                Pos2::new(left + geometry.width, geometry.staff_top + 66.0),
            ),
            0.0,
            crate::design::BLUE.gamma_multiply(0.035),
        );
    }
    let stroke = Stroke::new(0.75, crate::design::STAFF_COLOR);
    for line in 0..5 {
        let y = geometry.staff_top + line as f32 * 10.0;
        painter.line_segment(
            [Pos2::new(geometry.left - 45.0, y), Pos2::new(end_x, y)],
            stroke,
        );
    }
    draw_clef(
        painter,
        Pos2::new(geometry.left - 38.0, geometry.staff_top),
        clef,
        ink,
    );
    painter.text(
        Pos2::new(geometry.left - 8.0, geometry.staff_top + 10.0),
        egui::Align2::CENTER_CENTER,
        settings.beats_per_bar.to_string(),
        crate::design::jazz_font(18.0),
        ink,
    );
    painter.text(
        Pos2::new(geometry.left - 8.0, geometry.staff_top + 30.0),
        egui::Align2::CENTER_CENTER,
        "4",
        crate::design::jazz_font(18.0),
        ink,
    );
    for index in 0..=bars {
        let x = geometry.left + index as f32 * geometry.width;
        painter.line_segment(
            [
                Pos2::new(x, geometry.staff_top),
                Pos2::new(x, geometry.staff_top + 40.0),
            ],
            stroke,
        );
        if index < bars {
            painter.text(
                Pos2::new(x + 2.0, geometry.staff_top - 32.0),
                egui::Align2::LEFT_CENTER,
                (first_bar + index + 1).to_string(),
                crate::design::number_font(10.0),
                crate::design::MUTED,
            );
            draw_measure_rests(painter, geometry, segments, index, settings, ink);
        }
    }
    if playback_tick >= geometry.start as f64 && playback_tick < geometry.end as f64 {
        let x = geometry.x(playback_tick);
        painter.line_segment(
            [
                Pos2::new(x, geometry.staff_top - 17.0),
                Pos2::new(x, geometry.staff_top + 59.0),
            ],
            Stroke::new(1.2, crate::design::BLUE),
        );
        painter.circle_filled(
            Pos2::new(x, geometry.staff_top - 20.0),
            3.0,
            crate::design::BLUE,
        );
    }
    let mut chords: BTreeMap<(i64, usize, i64), Vec<&NoteSegment>> = BTreeMap::new();
    for segment in segments
        .iter()
        .filter(|segment| segment.start >= geometry.start && segment.start < geometry.end)
    {
        chords
            .entry((segment.start, segment.voice, segment.duration.ticks))
            .or_default()
            .push(segment);
    }
    let mut accidentals: BTreeMap<(i64, i32), i8> = BTreeMap::new();
    for ((start, _, _), chord) in chords {
        let x = geometry.x(start as f64);
        let active = playback_tick >= start as f64
            && playback_tick < (start + chord[0].duration.ticks) as f64;
        let chord_hits = draw_chord(painter, geometry, &chord, clef, x, ink, settings);
        if active {
            for hit in &chord_hits {
                painter.circle_stroke(hit.rect.center(), 8.2, Stroke::new(0.8, ink));
            }
        }
        hits.extend(chord_hits);
        for segment in chord {
            let y = geometry.y(segment.pitch, clef, segment.is_drum);
            if !segment.is_drum && !segment.tied_from {
                let (step, alteration, octave) = pitch_spelling(segment.pitch);
                let key = (
                    start.div_euclid(geometry.bar_ticks),
                    diatonic_step(step, octave),
                );
                let previous = accidentals.get(&key).copied().unwrap_or(0);
                if previous != alteration {
                    draw_accidental(painter, Pos2::new(x - 15.0, y), alteration, ink);
                    accidentals.insert(key, alteration);
                }
            }
            if segment.tied_to {
                let next_tick = start + segment.duration.ticks;
                let next_x = if next_tick < geometry.end {
                    geometry.x(next_tick as f64) - 5.0
                } else {
                    end_x - 2.0
                };
                draw_tie(
                    painter,
                    Pos2::new(x + 5.0, y + 8.0),
                    Pos2::new(next_x, y + 8.0),
                    ink,
                );
            }
            if segment.tied_from && start == geometry.start {
                draw_tie(
                    painter,
                    Pos2::new(geometry.left + 2.0, y + 8.0),
                    Pos2::new(x - 5.0, y + 8.0),
                    ink,
                );
            }
        }
    }
    hits
}

fn draw_measure_rests(
    painter: &egui::Painter,
    geometry: &RowGeometry,
    segments: &[NoteSegment],
    index: usize,
    settings: &ScoreSettings,
    ink: Color32,
) {
    let start = geometry.start + index as i64 * geometry.bar_ticks;
    let end = start + geometry.bar_ticks;
    let mut intervals: Vec<_> = segments
        .iter()
        .filter(|segment| segment.start < end && segment.start + segment.duration.ticks > start)
        .map(|segment| {
            (
                segment.start.max(start),
                (segment.start + segment.duration.ticks).min(end),
            )
        })
        .collect();
    intervals.sort_unstable();
    if intervals.is_empty() {
        let center = geometry.left + (index as f32 + 0.5) * geometry.width;
        draw_rest(
            painter,
            Pos2::new(center, geometry.staff_top + 20.0),
            NotatedDuration {
                ticks: geometry.bar_ticks,
                kind: "whole",
                dots: 0,
                triplet: false,
            },
            ink,
        );
        return;
    }
    let mut cursor = start;
    for (occupied_start, occupied_end) in intervals {
        if occupied_start > cursor {
            draw_rest_gap(painter, geometry, cursor, occupied_start, settings, ink);
        }
        cursor = cursor.max(occupied_end);
    }
    if cursor < end {
        draw_rest_gap(painter, geometry, cursor, end, settings, ink);
    }
}

fn draw_rest_gap(
    painter: &egui::Painter,
    geometry: &RowGeometry,
    mut start: i64,
    end: i64,
    settings: &ScoreSettings,
    ink: Color32,
) {
    for duration in split_duration(end - start, grid_ticks(settings)) {
        draw_rest(
            painter,
            Pos2::new(geometry.x(start as f64), geometry.staff_top + 20.0),
            duration,
            ink,
        );
        start += duration.ticks;
    }
}

fn draw_chord(
    painter: &egui::Painter,
    geometry: &RowGeometry,
    chord: &[&NoteSegment],
    clef: Clef,
    x: f32,
    color: Color32,
    settings: &ScoreSettings,
) -> Vec<NoteHit> {
    let mut hits = Vec::new();
    let duration = chord[0].duration;
    let heads = chord_heads(geometry, chord, clef, x);
    let top = heads[0].0.y;
    let bottom = heads[heads.len() - 1].0.y;
    let down = (top + bottom) / 2.0 < geometry.staff_top + 20.0;
    for (center, note) in heads {
        let head_x = center.x;
        let y = center.y;
        let step = staff_step(note.pitch, clef, note.is_drum);
        if step < 0 {
            for ledger in (step..=0).filter(|ledger| ledger % 2 == 0 && *ledger < 0) {
                let ledger_y = geometry.staff_top + 40.0 - ledger as f32 * 5.0;
                painter.line_segment(
                    [
                        Pos2::new(head_x - 9.0, ledger_y),
                        Pos2::new(head_x + 9.0, ledger_y),
                    ],
                    Stroke::new(0.8, crate::design::STAFF_COLOR),
                );
            }
        } else if step > 8 {
            for ledger in (9..=step).filter(|ledger| ledger % 2 == 0) {
                let ledger_y = geometry.staff_top + 40.0 - ledger as f32 * 5.0;
                painter.line_segment(
                    [
                        Pos2::new(head_x - 9.0, ledger_y),
                        Pos2::new(head_x + 9.0, ledger_y),
                    ],
                    Stroke::new(0.8, crate::design::STAFF_COLOR),
                );
            }
        }
        let glyph = if note.is_drum
            && matches!(note.pitch, 42 | 44 | 46 | 49 | 51 | 52 | 53 | 55 | 57 | 59)
        {
            '\u{e0a9}'
        } else {
            match duration.kind {
                "whole" => '\u{e0a2}',
                "half" => '\u{e0a3}',
                _ => '\u{e0a4}',
            }
        };
        let head_rect = draw_music_glyph(painter, center, glyph, 11.0, color);
        hits.push(NoteHit {
            rect: head_rect.expand2(Vec2::new(2.0, 1.0)),
            pitch: note.pitch,
            duration: duration.ticks as f64 * 60.0 / (valid_bpm(settings) * TICKS_PER_BEAT as f64),
        });
        for dot in 0..duration.dots {
            let dot_y = if step % 2 == 0 { y - 5.0 } else { y };
            painter.circle_filled(
                Pos2::new(head_x + 10.0 + f32::from(dot) * 4.0, dot_y),
                1.6,
                color,
            );
        }
    }
    if duration.kind != "whole" {
        let stem_x = x + if down { -5.0 } else { 5.0 };
        let stem_start = if down { top } else { bottom };
        let stem_end = if down { bottom + 31.0 } else { top - 31.0 };
        painter.line_segment(
            [Pos2::new(stem_x, stem_start), Pos2::new(stem_x, stem_end)],
            Stroke::new(1.35, color),
        );
        let flags = match duration.kind {
            "eighth" => 1,
            "16th" => 2,
            "32nd" => 3,
            "64th" => 4,
            _ => 0,
        };
        for flag in 0..flags {
            let direction = if down { -1.0 } else { 1.0 };
            let y = stem_end + direction * flag as f32 * 6.0;
            draw_curve(
                painter,
                [
                    Pos2::new(stem_x, y),
                    Pos2::new(stem_x + 11.0, y + direction * 8.0),
                    Pos2::new(stem_x + 10.0, y + direction * 13.0),
                    Pos2::new(stem_x + 5.0, y + direction * 18.0),
                ],
                Stroke::new(2.0, color),
            );
        }
    }
    if duration.triplet {
        painter.text(
            Pos2::new(x, top - 44.0),
            egui::Align2::CENTER_CENTER,
            "3",
            crate::design::jazz_font(11.0),
            color,
        );
    }
    hits
}

fn chord_heads<'note>(
    geometry: &RowGeometry,
    chord: &[&'note NoteSegment],
    clef: Clef,
    x: f32,
) -> Vec<(Pos2, &'note NoteSegment)> {
    let mut positions: Vec<_> = chord
        .iter()
        .map(|note| (geometry.y(note.pitch, clef, note.is_drum), *note))
        .collect();
    positions.sort_by(|first, second| first.0.total_cmp(&second.0));
    let down =
        (positions[0].0 + positions[positions.len() - 1].0) / 2.0 < geometry.staff_top + 20.0;
    let mut previous_y = f32::NEG_INFINITY;
    let mut shifted = false;
    positions
        .into_iter()
        .map(|(y, note)| {
            shifted = (y - previous_y).abs() <= 5.1 && !shifted;
            previous_y = y;
            let offset = if shifted {
                if down { -9.0 } else { 9.0 }
            } else {
                0.0
            };
            (Pos2::new(x + offset, y), note)
        })
        .collect()
}

fn draw_music_glyph(
    painter: &egui::Painter,
    center: Pos2,
    glyph: char,
    width: f32,
    color: Color32,
) -> Rect {
    let family = egui::FontFamily::Name("JazzMusic".into());
    let prototype =
        painter.layout_no_wrap(glyph.to_string(), FontId::new(40.0, family.clone()), color);
    let size = 40.0 * width / prototype.mesh_bounds.width().max(1.0);
    let galley = painter.layout_no_wrap(glyph.to_string(), FontId::new(size, family), color);
    let origin = center - galley.mesh_bounds.center().to_vec2();
    let rect = galley.mesh_bounds.translate(origin.to_vec2());
    painter.galley(origin, galley, color);
    rect
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_chord_symbols(
    painter: &egui::Painter,
    chords: &[crate::harmony::Chord],
    settings: &ScoreSettings,
    start: i64,
    end: i64,
    top: f32,
    right: f32,
    x_at: impl Fn(f64) -> f32,
) {
    let mut previous_right = f32::NEG_INFINITY;
    let mut previous_symbol = "";
    let mut previous_bar = i64::MIN;
    let first = chords
        .partition_point(|chord| time_to_tick(chord.onset, settings) < start as f64)
        .saturating_sub(1);
    for chord in &chords[first..] {
        let onset = time_to_tick(chord.onset, settings);
        if onset >= end as f64 {
            break;
        }
        if time_to_tick(chord.offset, settings) <= start as f64 {
            continue;
        }
        let tick = onset.max(start as f64);
        let bar = (tick / measure_ticks(settings) as f64).floor() as i64;
        if previous_symbol == chord.symbol && previous_bar == bar {
            continue;
        }
        let x = x_at(tick);
        if x < previous_right + 7.0 {
            continue;
        }
        let galley = painter.layout_no_wrap(
            chord.symbol.clone(),
            crate::design::jazz_font(18.0),
            Color32::BLACK,
        );
        previous_right = x + galley.size().x;
        previous_symbol = &chord.symbol;
        previous_bar = bar;
        painter
            .with_clip_rect(painter.clip_rect().intersect(Rect::from_min_max(
                Pos2::new(x, top - 3.0),
                Pos2::new(right, top + 30.0),
            )))
            .galley(Pos2::new(x, top), galley, Color32::BLACK);
    }
}

fn draw_rest(painter: &egui::Painter, center: Pos2, duration: NotatedDuration, color: Color32) {
    let (glyph, width, offset) = match duration.kind {
        "whole" => ('\u{e4e3}', 14.0, -8.0),
        "half" => ('\u{e4e4}', 14.0, -2.0),
        "quarter" => ('\u{e4e5}', 10.0, 0.0),
        "eighth" => ('\u{e4e6}', 10.0, 0.0),
        "16th" => ('\u{e4e7}', 11.0, 0.0),
        "32nd" => ('\u{e4e8}', 12.0, 0.0),
        _ => ('\u{e4e9}', 13.0, 0.0),
    };
    draw_music_glyph(
        painter,
        center + Vec2::new(0.0, offset),
        glyph,
        width,
        color,
    );
    for dot in 0..duration.dots {
        painter.circle_filled(
            center + Vec2::new(10.0 + f32::from(dot) * 4.0, -5.0),
            1.6,
            color,
        );
    }
    if duration.triplet {
        painter.text(
            center + Vec2::new(0.0, -24.0),
            egui::Align2::CENTER_CENTER,
            "3",
            crate::design::jazz_font(11.0),
            color,
        );
    }
}

fn draw_accidental(painter: &egui::Painter, center: Pos2, alteration: i8, color: Color32) {
    let (glyph, width) = if alteration == 1 {
        ('\u{e262}', 8.0)
    } else {
        ('\u{e261}', 5.5)
    };
    draw_music_glyph(painter, center, glyph, width, color);
}
fn draw_tie(painter: &egui::Painter, start: Pos2, end: Pos2, color: Color32) {
    if end.x <= start.x {
        return;
    }
    let lift = ((end.x - start.x) * 0.13).clamp(3.0, 10.0);
    draw_curve(
        painter,
        [
            start,
            start + Vec2::new((end.x - start.x) / 3.0, lift),
            end + Vec2::new(-(end.x - start.x) / 3.0, lift),
            end,
        ],
        Stroke::new(1.05, color),
    );
}

fn draw_curve(painter: &egui::Painter, control: [Pos2; 4], stroke: Stroke) {
    let points = (0..=20)
        .map(|index| {
            let t = index as f32 / 20.0;
            let s = 1.0 - t;
            Pos2::new(
                s.powi(3) * control[0].x
                    + 3.0 * s.powi(2) * t * control[1].x
                    + 3.0 * s * t.powi(2) * control[2].x
                    + t.powi(3) * control[3].x,
                s.powi(3) * control[0].y
                    + 3.0 * s.powi(2) * t * control[1].y
                    + 3.0 * s * t.powi(2) * control[2].y
                    + t.powi(3) * control[3].y,
            )
        })
        .collect();
    painter.add(Shape::line(points, stroke));
}

fn draw_clef(painter: &egui::Painter, origin: Pos2, clef: Clef, color: Color32) {
    let (glyph, width, center) = match clef {
        Clef::Treble => ('\u{e050}', 26.0, Vec2::new(13.0, 21.0)),
        Clef::Guitar => ('\u{e052}', 26.0, Vec2::new(13.0, 27.0)),
        Clef::Bass => ('\u{e062}', 28.0, Vec2::new(14.0, 15.0)),
        Clef::Percussion => ('\u{e069}', 15.0, Vec2::new(13.0, 20.0)),
    };
    draw_music_glyph(painter, origin + center, glyph, width, color);
}
#[cfg(test)]
mod tests {
    use super::*;

    fn note(onset: f64, offset: f64, pitch: u8) -> Note {
        Note {
            onset,
            offset,
            pitch,
            program: 26,
            is_drum: false,
        }
    }

    #[test]
    fn shifted_chord_heads_preview_the_original_concert_pitch() {
        let context = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "Petaluma".into(),
            egui::FontData::from_static(include_bytes!("../assets/fonts/Petaluma.otf")).into(),
        );
        fonts.families.insert(
            egui::FontFamily::Name("JazzMusic".into()),
            vec!["Petaluma".into()],
        );
        context.set_fonts(fonts);
        let settings = ScoreSettings::default();
        let geometry = RowGeometry {
            start: 0,
            end: 384,
            bar_ticks: 384,
            left: 0.0,
            width: 280.0,
            staff_top: 100.0,
        };
        for (pitches, shift) in [([40, 41], 9.0), ([60, 62], -9.0)] {
            let quantized = quantize_notes(&pitches.map(|pitch| note(0.0, 0.25, pitch)), &settings);
            let segments = split_notes(&quantized, &settings);
            let chord = segments.iter().collect::<Vec<_>>();
            let mut hits = Vec::new();
            let _ = context.run(egui::RawInput::default(), |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    hits = draw_chord(
                        ui.painter(),
                        &geometry,
                        &chord,
                        Clef::Guitar,
                        100.0,
                        Color32::BLACK,
                        &settings,
                    );
                });
            });
            let shifted = hits
                .iter()
                .find(|hit| (hit.rect.center().x - 100.0).abs() > 1.0)
                .unwrap();
            assert!((shifted.rect.center().x - 100.0 - shift).abs() < 0.1);
            assert!(pitches.contains(&shifted.pitch));
            assert_eq!(
                notation_action(shifted.rect.center(), &hits, 99.0),
                NotationAction::Preview {
                    pitch: shifted.pitch,
                    duration: 0.25
                },
            );
        }
    }

    #[test]
    fn clicking_empty_staff_seeks_without_a_preview() {
        let hit = NoteHit {
            rect: Rect::from_center_size(Pos2::new(100.0, 100.0), Vec2::new(13.0, 11.0)),
            pitch: 60,
            duration: 0.5,
        };
        assert_eq!(
            notation_action(Pos2::new(100.0, 160.0), &[hit], 3.75),
            NotationAction::Seek(3.75),
        );
    }

    #[test]
    fn overlapping_note_hitboxes_choose_the_nearest_head() {
        let hits = [
            NoteHit {
                rect: Rect::from_center_size(Pos2::new(100.0, 100.0), Vec2::new(15.0, 13.0)),
                pitch: 60,
                duration: 0.5,
            },
            NoteHit {
                rect: Rect::from_center_size(Pos2::new(109.0, 105.0), Vec2::new(15.0, 13.0)),
                pitch: 62,
                duration: 0.5,
            },
        ];
        assert_eq!(
            notation_action(Pos2::new(108.0, 104.0), &hits, 1.0),
            NotationAction::Preview {
                pitch: 62,
                duration: 0.5
            },
        );
    }

    #[test]
    fn sustained_note_splits_at_bar_with_tie() {
        let settings = ScoreSettings::default();
        let quantized = quantize_notes(&[note(1.5, 2.5, 60)], &settings);
        let segments = split_notes(&quantized, &settings);
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].start, 288);
        assert_eq!(segments[0].duration.ticks, 96);
        assert!(segments[0].tied_to);
        assert!(!segments[0].tied_from);
        assert_eq!(segments[1].start, 384);
        assert!(segments[1].tied_from);
        assert!(!segments[1].tied_to);
    }

    #[test]
    fn offset_preserves_notes_before_the_selected_downbeat() {
        let settings = ScoreSettings {
            offset: 1.0,
            ..ScoreSettings::default()
        };
        let quantized = quantize_notes(&[note(0.0, 0.5, 60)], &settings);
        assert_eq!(quantized[0].start, -192);
        assert_eq!(first_measure_tick(&quantized, &settings), -384);
        assert_eq!(tick_to_time(-192.0, &settings), 0.0);
    }

    #[test]
    fn triplet_grid_keeps_tuplet_duration() {
        let settings = ScoreSettings {
            subdivisions: 3,
            ..ScoreSettings::default()
        };
        let quantized = quantize_notes(&[note(0.0, 1.0 / 6.0, 60)], &settings);
        let segments = split_notes(&quantized, &settings);
        assert_eq!(segments[0].duration.ticks, 32);
        assert_eq!(segments[0].duration.kind, "eighth");
        assert!(segments[0].duration.triplet);
    }

    #[test]
    fn chord_uses_one_voice_and_overlapping_notes_use_another() {
        let notes = [note(0.0, 1.0, 60), note(0.0, 1.0, 64), note(0.5, 1.5, 67)];
        let quantized = quantize_notes(&notes, &ScoreSettings::default());
        assert_eq!(quantized[0].voice, quantized[1].voice);
        assert_ne!(quantized[0].voice, quantized[2].voice);
    }

    #[test]
    fn guitar_clef_moves_display_one_octave_without_changing_notes() {
        for program in 24..=31 {
            let mut guitar_note = note(0.0, 0.5, 40);
            guitar_note.program = program;
            assert_eq!(choose_clef(&[guitar_note.clone()]), Clef::Guitar);
            assert_eq!(
                staff_step(40, Clef::Guitar, false),
                staff_step(40, Clef::Treble, false) + 7
            );
            let quantized = quantize_notes(&[guitar_note], &ScoreSettings::default());
            assert_eq!(quantized[0].pitch, 40);
        }
    }
}
