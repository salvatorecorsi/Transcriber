use crate::transcription::Note;
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use std::io::Cursor;
use std::sync::{Arc, OnceLock};

const RENDER_FRAMES: usize = 64;
const CHANNEL_COUNT: usize = 16;
const PITCH_COUNT: usize = 128;

#[derive(Clone, Copy, Debug)]
struct NoteEvent {
    seconds: f64,
    pitch: u8,
    program: u8,
    channel: u8,
    starts: bool,
}

pub(crate) struct MidiScore {
    notes: Vec<Note>,
    events: Vec<NoteEvent>,
    channels: [u8; PITCH_COUNT],
}

impl MidiScore {
    pub(crate) fn new(notes: &[Note]) -> Self {
        let mut notes: Vec<_> = notes
            .iter()
            .filter(|note| {
                note.onset.is_finite()
                    && note.offset.is_finite()
                    && note.onset >= 0.0
                    && note.offset > note.onset
                    && note.pitch < 128
            })
            .cloned()
            .collect();
        notes.sort_by(|first, second| first.onset.total_cmp(&second.onset));
        let melodic_channels = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15];
        let mut channels = [0; PITCH_COUNT];
        let mut present = [false; PITCH_COUNT];
        for note in &notes {
            if !note.is_drum {
                present[note.program.min(127) as usize] = true;
            }
        }
        let mut assigned = 0;
        for (program, exists) in present.into_iter().enumerate() {
            if exists {
                channels[program] = melodic_channels[assigned % melodic_channels.len()];
                assigned += 1;
            }
        }
        let mut events = Vec::with_capacity(notes.len() * 2);
        for note in &notes {
            let program = note.program.min(127);
            let channel = if note.is_drum {
                9
            } else {
                channels[program as usize]
            };
            events.push(NoteEvent {
                seconds: note.onset,
                pitch: note.pitch,
                program,
                channel,
                starts: true,
            });
            events.push(NoteEvent {
                seconds: note.offset,
                pitch: note.pitch,
                program,
                channel,
                starts: false,
            });
        }
        events.sort_by(|first, second| {
            first
                .seconds
                .total_cmp(&second.seconds)
                .then_with(|| first.starts.cmp(&second.starts))
        });
        Self {
            notes,
            events,
            channels,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum MidiCommand {
    Reset,
    Start { channel: u8, pitch: u8, program: u8 },
    Stop { channel: u8, pitch: u8 },
}

struct MidiSchedule {
    score: Arc<MidiScore>,
    next_event: usize,
    held: [u16; CHANNEL_COUNT * PITCH_COUNT],
    position: f64,
}

impl MidiSchedule {
    fn new(score: Arc<MidiScore>) -> Self {
        Self {
            score,
            next_event: 0,
            held: [0; CHANNEL_COUNT * PITCH_COUNT],
            position: 0.0,
        }
    }

    fn reset(&mut self, position: f64, mut emit: impl FnMut(MidiCommand)) {
        emit(MidiCommand::Reset);
        self.held.fill(0);
        self.position = position;
        self.next_event = self
            .score
            .events
            .partition_point(|event| event.seconds < position);
        let preceding = self
            .score
            .notes
            .partition_point(|note| note.onset < position);
        for note in &self.score.notes[..preceding] {
            if note.offset > position {
                let program = note.program.min(127);
                let channel = if note.is_drum {
                    9
                } else {
                    self.score.channels[program as usize]
                };
                let held = &mut self.held[channel as usize * PITCH_COUNT + note.pitch as usize];
                *held = held.saturating_add(1);
                emit(MidiCommand::Start {
                    channel,
                    pitch: note.pitch,
                    program,
                });
            }
        }
    }

    fn advance_to(&mut self, position: f64, mut emit: impl FnMut(MidiCommand)) {
        if position + 1e-9 < self.position {
            self.reset(position, &mut emit);
        }
        self.position = position;
        while let Some(event) = self.score.events.get(self.next_event).copied() {
            if event.seconds > position + 1e-10 {
                break;
            }
            self.next_event += 1;
            let held = &mut self.held[event.channel as usize * PITCH_COUNT + event.pitch as usize];
            if event.starts {
                *held = held.saturating_add(1);
                emit(MidiCommand::Start {
                    channel: event.channel,
                    pitch: event.pitch,
                    program: event.program,
                });
            } else if *held > 0 {
                *held -= 1;
                if *held == 0 {
                    emit(MidiCommand::Stop {
                        channel: event.channel,
                        pitch: event.pitch,
                    });
                }
            }
        }
    }

    fn next_time(&self) -> Option<f64> {
        self.score
            .events
            .get(self.next_event)
            .map(|event| event.seconds)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct MidiTiming {
    pub(crate) sample_rate: u32,
    pub(crate) speed: f32,
    pub(crate) duration: f64,
    pub(crate) loop_range: Option<(f64, f64)>,
}

impl MidiTiming {
    fn step(self) -> f64 {
        self.speed as f64 / self.sample_rate as f64
    }

    pub(crate) fn normalize_position(self, position: f64) -> f64 {
        if let Some((start, end)) = self.loop_range
            && (position < start || position >= end)
        {
            return start;
        }
        position
    }

    fn advance(self, position: f64) -> (f64, bool) {
        let next = position + self.step();
        if let Some((start, end)) = self.loop_range
            && next >= end - 1e-10
        {
            return (start, false);
        }
        (next.min(self.duration), next >= self.duration - 1e-10)
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PreviewNote {
    pub(crate) pitch: u8,
    pub(crate) program: u8,
    pub(crate) is_drum: bool,
    pub(crate) duration: f64,
}

pub(crate) struct MidiRenderer {
    transport: Synthesizer,
    preview: Synthesizer,
    schedule: MidiSchedule,
    revision: u64,
    enabled: bool,
    left: [f32; RENDER_FRAMES],
    right: [f32; RENDER_FRAMES],
    positions: [f64; RENDER_FRAMES],
    finished: [bool; RENDER_FRAMES],
    consumed: usize,
    preview_left: [f32; RENDER_FRAMES],
    preview_right: [f32; RENDER_FRAMES],
    preview_consumed: usize,
    preview_remaining: usize,
    preview_releasing: usize,
    preview_pitch: u8,
    preview_channel: u8,
    sample_rate: u32,
}

fn embedded_sound_font() -> Result<Arc<SoundFont>, String> {
    static SOUND_FONT: OnceLock<Result<Arc<SoundFont>, String>> = OnceLock::new();
    SOUND_FONT
        .get_or_init(|| {
            let mut reader =
                Cursor::new(include_bytes!("../assets/sounds/GeneralUser-GS.sf2").as_slice());
            SoundFont::new(&mut reader)
                .map(Arc::new)
                .map_err(|error| format!("Impossibile caricare i suoni MIDI incorporati: {error}"))
        })
        .clone()
}

fn apply_command(synthesizer: &mut Synthesizer, command: MidiCommand) {
    match command {
        MidiCommand::Reset => synthesizer.reset(),
        MidiCommand::Start {
            channel,
            pitch,
            program,
        } => {
            if channel != 9 {
                synthesizer.process_midi_message(channel as i32, 0xC0, program as i32, 0);
            }
            synthesizer.note_on(channel as i32, pitch as i32, 96);
        }
        MidiCommand::Stop { channel, pitch } => synthesizer.note_off(channel as i32, pitch as i32),
    }
}

impl MidiRenderer {
    pub(crate) fn new(sample_rate: u32) -> Result<Self, String> {
        let sound_font = embedded_sound_font()?;
        let mut settings = SynthesizerSettings::new(sample_rate as i32);
        settings.maximum_polyphony = 128;
        settings.block_size = RENDER_FRAMES;
        settings.enable_reverb_and_chorus = false;
        let mut transport = Synthesizer::new(&sound_font, &settings)
            .map_err(|error| format!("Impossibile avviare il sintetizzatore MIDI: {error}"))?;
        let mut preview = Synthesizer::new(&sound_font, &settings)
            .map_err(|error| format!("Impossibile avviare l'ascolto delle note: {error}"))?;
        transport.set_master_volume(0.7);
        preview.set_master_volume(0.8);
        Ok(Self {
            transport,
            preview,
            schedule: MidiSchedule::new(Arc::new(MidiScore::new(&[]))),
            revision: u64::MAX,
            enabled: false,
            left: [0.0; RENDER_FRAMES],
            right: [0.0; RENDER_FRAMES],
            positions: [0.0; RENDER_FRAMES],
            finished: [false; RENDER_FRAMES],
            consumed: RENDER_FRAMES,
            preview_left: [0.0; RENDER_FRAMES],
            preview_right: [0.0; RENDER_FRAMES],
            preview_consumed: RENDER_FRAMES,
            preview_remaining: 0,
            preview_releasing: 0,
            preview_pitch: 0,
            preview_channel: 0,
            sample_rate,
        })
    }

    pub(crate) fn synchronize(
        &mut self,
        score: &Arc<MidiScore>,
        revision: u64,
        enabled: bool,
        position: f64,
    ) {
        if self.revision == revision && self.enabled == enabled {
            return;
        }
        self.revision = revision;
        self.enabled = enabled;
        self.consumed = RENDER_FRAMES;
        self.schedule.score = score.clone();
        if enabled {
            let synthesizer = &mut self.transport;
            self.schedule
                .reset(position, |command| apply_command(synthesizer, command));
        } else {
            self.transport.reset();
            self.schedule.held.fill(0);
            self.schedule.position = position;
        }
    }

    pub(crate) fn preview(&mut self, note: PreviewNote) {
        self.preview.reset();
        self.preview_channel = if note.is_drum { 9 } else { 0 };
        self.preview_pitch = note.pitch;
        if !note.is_drum {
            self.preview
                .process_midi_message(0, 0xC0, note.program.min(127) as i32, 0);
        }
        self.preview
            .note_on(self.preview_channel as i32, note.pitch as i32, 108);
        self.preview_remaining =
            (note.duration.clamp(0.08, 4.0) * self.sample_rate as f64).round() as usize;
        self.preview_releasing = (self.sample_rate as f64 * 0.7) as usize;
        self.preview_consumed = RENDER_FRAMES;
    }

    pub(crate) fn transport_frame(
        &mut self,
        position: f64,
        timing: MidiTiming,
    ) -> ([f32; 2], f64, bool) {
        if self.consumed == RENDER_FRAMES {
            self.fill_transport(position, timing);
            self.consumed = 0;
        }
        let index = self.consumed;
        self.consumed += 1;
        (
            [self.left[index], self.right[index]],
            self.positions[index],
            self.finished[index],
        )
    }

    fn fill_transport(&mut self, position: f64, timing: MidiTiming) {
        let mut position = timing.normalize_position(position);
        let mut rendered = 0;
        while rendered < RENDER_FRAMES {
            if position >= timing.duration - 1e-10 && timing.loop_range.is_none() {
                self.transport.reset();
                self.left[rendered..].fill(0.0);
                self.right[rendered..].fill(0.0);
                self.positions[rendered..].fill(timing.duration);
                self.finished[rendered..].fill(true);
                break;
            }
            let synthesizer = &mut self.transport;
            self.schedule
                .advance_to(position, |command| apply_command(synthesizer, command));
            let limit = timing.loop_range.map_or(timing.duration, |(_, end)| end);
            let next_time = self.schedule.next_time().unwrap_or(limit).min(limit);
            let until_event = ((next_time - position) / timing.step()).ceil().max(1.0) as usize;
            let count = (RENDER_FRAMES - rendered).min(until_event);
            self.transport.render(
                &mut self.left[rendered..rendered + count],
                &mut self.right[rendered..rendered + count],
            );
            for index in rendered..rendered + count {
                let (next, finished) = timing.advance(position);
                self.positions[index] = next;
                self.finished[index] = finished;
                position = next;
            }
            rendered += count;
        }
    }

    pub(crate) fn preview_frame(&mut self) -> [f32; 2] {
        if self.preview_consumed == RENDER_FRAMES {
            self.fill_preview();
            self.preview_consumed = 0;
        }
        let index = self.preview_consumed;
        self.preview_consumed += 1;
        [self.preview_left[index], self.preview_right[index]]
    }

    fn fill_preview(&mut self) {
        let mut rendered = 0;
        while rendered < RENDER_FRAMES {
            if self.preview_remaining == 0 && self.preview_releasing == 0 {
                self.preview_left[rendered..].fill(0.0);
                self.preview_right[rendered..].fill(0.0);
                break;
            }
            let remaining = if self.preview_remaining > 0 {
                self.preview_remaining
            } else {
                self.preview_releasing
            };
            let count = (RENDER_FRAMES - rendered).min(remaining);
            self.preview.render(
                &mut self.preview_left[rendered..rendered + count],
                &mut self.preview_right[rendered..rendered + count],
            );
            if self.preview_remaining > 0 {
                self.preview_remaining -= count;
                if self.preview_remaining == 0 {
                    self.preview
                        .note_off(self.preview_channel as i32, self.preview_pitch as i32);
                }
            } else {
                self.preview_releasing -= count;
            }
            rendered += count;
        }
    }
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
    fn polyphonic_schedule_starts_and_stops_at_original_timestamps() {
        let score = Arc::new(MidiScore::new(&[
            note(0.1, 0.7, 60),
            note(0.1, 0.9, 64),
            note(0.4, 0.7, 67),
        ]));
        let mut schedule = MidiSchedule::new(score);
        let mut commands = Vec::new();
        schedule.advance_to(0.099, |command| commands.push(command));
        assert!(commands.is_empty());
        schedule.advance_to(0.1, |command| commands.push(command));
        assert_eq!(commands.len(), 2);
        schedule.advance_to(0.7, |command| commands.push(command));
        assert_eq!(
            commands
                .iter()
                .filter(|command| matches!(command, MidiCommand::Start { .. }))
                .count(),
            3
        );
        assert_eq!(
            commands
                .iter()
                .filter(|command| matches!(command, MidiCommand::Stop { .. }))
                .count(),
            2
        );
        schedule.advance_to(0.9, |command| commands.push(command));
        assert!(schedule.held.iter().all(|held| *held == 0));
    }

    #[test]
    fn seeking_restores_only_notes_still_sounding() {
        let score = Arc::new(MidiScore::new(&[
            note(0.0, 0.5, 60),
            note(0.2, 1.4, 64),
            note(0.9, 1.5, 67),
        ]));
        let mut schedule = MidiSchedule::new(score);
        let mut commands = Vec::new();
        schedule.reset(0.8, |command| commands.push(command));
        assert_eq!(
            commands,
            [
                MidiCommand::Reset,
                MidiCommand::Start {
                    channel: 0,
                    pitch: 64,
                    program: 26
                }
            ]
        );
        commands.clear();
        schedule.advance_to(0.9, |command| commands.push(command));
        assert_eq!(
            commands,
            [MidiCommand::Start {
                channel: 0,
                pitch: 67,
                program: 26
            }]
        );
    }

    #[test]
    fn overlapping_notes_do_not_stop_each_other_early() {
        let mut schedule = MidiSchedule::new(Arc::new(MidiScore::new(&[
            note(0.0, 0.5, 60),
            note(0.2, 0.8, 60),
        ])));
        let mut stops = Vec::new();
        schedule.advance_to(0.5, |command| {
            if matches!(command, MidiCommand::Stop { .. }) {
                stops.push(command);
            }
        });
        assert!(stops.is_empty());
        schedule.advance_to(0.8, |command| {
            if matches!(command, MidiCommand::Stop { .. }) {
                stops.push(command);
            }
        });
        assert_eq!(stops.len(), 1);
    }

    #[test]
    fn wrapping_loop_resets_and_restores_held_notes() {
        let mut schedule = MidiSchedule::new(Arc::new(MidiScore::new(&[
            note(0.1, 0.8, 60),
            note(0.5, 0.7, 67),
        ])));
        schedule.reset(0.4, |_| {});
        schedule.advance_to(0.6, |_| {});
        let mut commands = Vec::new();
        schedule.advance_to(0.4, |command| commands.push(command));
        assert_eq!(
            commands,
            [
                MidiCommand::Reset,
                MidiCommand::Start {
                    channel: 0,
                    pitch: 60,
                    program: 26
                }
            ]
        );
    }

    #[test]
    fn tempo_changes_event_duration_without_changing_pitch() {
        for speed in [0.25, 0.5, 1.0, 1.5] {
            let timing = MidiTiming {
                sample_rate: 24_000,
                speed,
                duration: 2.0,
                loop_range: None,
            };
            let mut schedule = MidiSchedule::new(Arc::new(MidiScore::new(&[note(0.0, 0.5, 69)])));
            let mut position = 0.0;
            let mut stopped_at = None;
            for frame in 0..96_000_usize {
                schedule.advance_to(position, |command| match command {
                    MidiCommand::Start { pitch, .. } => assert_eq!(pitch, 69),
                    MidiCommand::Stop { pitch, .. } => {
                        assert_eq!(pitch, 69);
                        stopped_at = Some(frame);
                    }
                    _ => {}
                });
                if stopped_at.is_some() {
                    break;
                }
                position = timing.advance(position).0;
            }
            let expected = (12_000.0 / speed as f64).ceil() as usize;
            assert!(stopped_at.unwrap().abs_diff(expected) <= 1);
        }
    }

    #[test]
    fn preview_is_audible_while_transport_stays_paused() {
        let mut renderer = MidiRenderer::new(24_000).unwrap();
        renderer.synchronize(&Arc::new(MidiScore::new(&[])), 1, false, 4.25);
        renderer.preview(PreviewNote {
            pitch: 69,
            program: 26,
            is_drum: false,
            duration: 0.15,
        });
        let energy: f32 = (0..6_000)
            .map(|_| renderer.preview_frame()[0].powi(2))
            .sum();
        assert!(energy > 0.1, "preview energy {energy}");
        assert_eq!(renderer.schedule.position, 4.25);
        for _ in 0..24_000 {
            renderer.preview_frame();
        }
        assert_eq!(renderer.preview_frame(), [0.0, 0.0]);
    }

    #[test]
    fn preview_does_not_change_samples_or_position_of_the_playing_part() {
        let mut reference = MidiRenderer::new(24_000).unwrap();
        let mut auditioned = MidiRenderer::new(24_000).unwrap();
        let score = Arc::new(MidiScore::new(&[note(0.0, 1.0, 60)]));
        let timing = MidiTiming {
            sample_rate: 24_000,
            speed: 0.5,
            duration: 1.0,
            loop_range: None,
        };
        reference.synchronize(&score, 1, true, 0.0);
        auditioned.synchronize(&score, 1, true, 0.0);
        let mut position = 0.0;
        for frame in 0..6_000 {
            if frame == 2_000 {
                auditioned.preview(PreviewNote {
                    pitch: 76,
                    program: 0,
                    is_drum: false,
                    duration: 0.15,
                });
            }
            let expected = reference.transport_frame(position, timing);
            let actual = auditioned.transport_frame(position, timing);
            assert_eq!(expected, actual);
            auditioned.preview_frame();
            position = expected.1;
        }
        assert!((position - 0.125).abs() < 1e-9);
    }

    #[test]
    fn general_midi_instruments_and_percussion_are_audible() {
        let mut renderer = MidiRenderer::new(24_000).unwrap();
        let mut beginnings = Vec::new();
        for (pitch, program, is_drum) in [(60, 0, false), (60, 26, false), (36, 128, true)] {
            renderer.preview(PreviewNote {
                pitch,
                program,
                is_drum,
                duration: 0.2,
            });
            let samples: Vec<_> = (0..4_000).map(|_| renderer.preview_frame()[0]).collect();
            assert!(samples.iter().map(|sample| sample * sample).sum::<f32>() > 0.1);
            beginnings.push(samples);
        }
        assert_ne!(beginnings[0], beginnings[1]);
        assert_ne!(beginnings[1], beginnings[2]);
    }

    #[test]
    fn pause_and_part_changes_clear_transport_voices_without_stopping_preview() {
        let mut renderer = MidiRenderer::new(24_000).unwrap();
        let score = Arc::new(MidiScore::new(&[note(0.0, 2.0, 60)]));
        let timing = MidiTiming {
            sample_rate: 24_000,
            speed: 1.0,
            duration: 2.0,
            loop_range: None,
        };
        renderer.synchronize(&score, 1, true, 0.0);
        for frame in 0..3_000 {
            renderer.transport_frame(frame as f64 / 24_000.0, timing);
        }
        renderer.preview(PreviewNote {
            pitch: 72,
            program: 0,
            is_drum: false,
            duration: 0.3,
        });
        renderer.synchronize(&score, 2, false, 0.125);
        assert!(!renderer.enabled);
        assert!(renderer.schedule.held.iter().all(|held| *held == 0));
        let energy: f32 = (0..2_000)
            .map(|_| renderer.preview_frame()[0].powi(2))
            .sum();
        assert!(energy > 0.1);
        let alternate = Arc::new(MidiScore::new(&[note(0.0, 1.0, 67)]));
        renderer.synchronize(&alternate, 3, true, 0.125);
        assert_eq!(renderer.schedule.held[60], 0);
        assert_eq!(renderer.schedule.held[67], 1);
    }

    #[test]
    fn midi_transport_loop_keeps_sample_clock_within_boundaries() {
        let mut renderer = MidiRenderer::new(24_000).unwrap();
        let score = Arc::new(MidiScore::new(&[note(0.0, 1.0, 60)]));
        let timing = MidiTiming {
            sample_rate: 24_000,
            speed: 1.5,
            duration: 1.0,
            loop_range: Some((0.2, 0.3)),
        };
        renderer.synchronize(&score, 1, true, 0.2);
        let mut position = 0.2;
        let mut wrapped = false;
        for _ in 0..4_000 {
            let (_, next, finished) = renderer.transport_frame(position, timing);
            assert!(!finished);
            assert!((0.2..0.3).contains(&next));
            wrapped |= next < position;
            position = next;
        }
        assert!(wrapped);
    }
}
