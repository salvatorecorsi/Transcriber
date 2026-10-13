use crate::score::{
    Clef, NotatedDuration, NoteSegment, ScoreSettings, TICKS_PER_BEAT, choose_clef, drum_spelling,
    first_measure_tick, grid_ticks, instrument_name, measure_ticks, pitch_spelling, quantize_notes,
    split_duration, split_notes, valid_bpm,
};
use crate::transcription::{Note, Transcription};
use midly::{
    Format, Header, MetaMessage, MidiMessage, Smf, Timing, TrackEvent, TrackEventKind,
    num::{u4, u7, u15, u24, u28},
};
use std::{collections::BTreeMap, fmt::Write as _, fs::File, path::Path};

pub fn write_midi(path: &Path, transcription: &Transcription, bpm: f64) -> Result<(), String> {
    let bpm = if bpm.is_finite() && bpm > 0.0 {
        bpm.clamp(20.0, 400.0)
    } else {
        120.0
    };
    let tempo = (60_000_000.0 / bpm).round() as u32;
    let conductor = vec![
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::TrackName(b"Corsivo tempo")),
        },
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::Tempo(u24::new(tempo))),
        },
        TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        },
    ];
    let instruments = group_instruments(&transcription.notes);
    let mut tracks = vec![conductor];
    let mut melodic_index = 0;
    for ((program, is_drum), notes) in instruments {
        let channel = if is_drum {
            9
        } else {
            let index = melodic_index % 15;
            if index < 9 { index } else { index + 1 }
        };
        let port = if is_drum { 0 } else { melodic_index / 15 };
        if !is_drum {
            melodic_index += 1;
        }
        let mut track = vec![TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::TrackName(
                instrument_name(program, is_drum).as_bytes(),
            )),
        }];
        if port > 0 {
            track.push(TrackEvent {
                delta: u28::new(0),
                kind: TrackEventKind::Meta(MetaMessage::MidiPort(u7::new(port as u8))),
            });
        }
        if !is_drum {
            track.push(TrackEvent {
                delta: u28::new(0),
                kind: TrackEventKind::Midi {
                    channel: u4::new(channel as u8),
                    message: MidiMessage::ProgramChange {
                        program: u7::new(program.min(127)),
                    },
                },
            });
        }
        let mut events = Vec::new();
        for note in notes {
            if !note.onset.is_finite() || !note.offset.is_finite() || note.offset <= note.onset {
                continue;
            }
            let start = midi_tick(note.onset, bpm);
            let end = midi_tick(note.offset, bpm).max(start + 1);
            events.push((start, 1, note.pitch.min(127)));
            events.push((end, 0, note.pitch.min(127)));
        }
        events.sort_unstable();
        let mut previous_tick = 0;
        for (tick, starts, pitch) in events {
            let message = if starts == 1 {
                MidiMessage::NoteOn {
                    key: u7::new(pitch),
                    vel: u7::new(90),
                }
            } else {
                MidiMessage::NoteOff {
                    key: u7::new(pitch),
                    vel: u7::new(0),
                }
            };
            let mut delta = tick - previous_tick;
            while delta > 0x0fff_ffff {
                track.push(TrackEvent {
                    delta: u28::new(0x0fff_ffff),
                    kind: TrackEventKind::Meta(MetaMessage::Text(b"")),
                });
                delta -= 0x0fff_ffff;
            }
            track.push(TrackEvent {
                delta: u28::new(delta as u32),
                kind: TrackEventKind::Midi {
                    channel: u4::new(channel as u8),
                    message,
                },
            });
            previous_tick = tick;
        }
        track.push(TrackEvent {
            delta: u28::new(0),
            kind: TrackEventKind::Meta(MetaMessage::EndOfTrack),
        });
        tracks.push(track);
    }
    let score = Smf {
        header: Header::new(Format::Parallel, Timing::Metrical(u15::new(960))),
        tracks,
    };
    let mut file = File::create(path).map_err(|error| error.to_string())?;
    score
        .write_std(&mut file)
        .map_err(|error| error.to_string())
}

fn midi_tick(time: f64, bpm: f64) -> u64 {
    (time.max(0.0) * bpm / 60.0 * 960.0).round() as u64
}

fn group_instruments(notes: &[Note]) -> BTreeMap<(u8, bool), Vec<Note>> {
    let mut instruments: BTreeMap<(u8, bool), Vec<Note>> = BTreeMap::new();
    for note in notes {
        let program = if note.is_drum {
            0
        } else {
            note.program.min(127)
        };
        instruments
            .entry((program, note.is_drum))
            .or_default()
            .push(note.clone());
    }
    instruments
}

pub fn write_musicxml(
    path: &Path,
    notes: &[Note],
    settings: &ScoreSettings,
    title: &str,
) -> Result<(), String> {
    let xml = musicxml_document(notes, settings, title)?;
    std::fs::write(path, xml).map_err(|error| error.to_string())
}

fn musicxml_document(
    notes: &[Note],
    settings: &ScoreSettings,
    title: &str,
) -> Result<String, String> {
    let instruments = group_instruments(notes);
    if instruments.is_empty() {
        return Err("La traccia non contiene note da esportare.".into());
    }
    let mut xml = String::from(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<score-partwise version=\"4.0\">\n",
    );
    writeln!(
        xml,
        "<work><work-title>{}</work-title></work>",
        escape_xml(title)
    )
    .unwrap();
    xml.push_str("<identification><creator type=\"composer\">Trascrizione automatica</creator><encoding><software>Corsivo Transcribe</software></encoding></identification>\n<part-list>\n");
    for (index, ((program, is_drum), part_notes)) in instruments.iter().enumerate() {
        let part_id = index + 1;
        let name = instrument_name(*program, *is_drum);
        writeln!(
            xml,
            "<score-part id=\"P{part_id}\"><part-name>{}</part-name>",
            escape_xml(name)
        )
        .unwrap();
        if *is_drum {
            let mut pitches: Vec<_> = part_notes.iter().map(|note| note.pitch).collect();
            pitches.sort_unstable();
            pitches.dedup();
            for pitch in &pitches {
                writeln!(xml, "<score-instrument id=\"P{part_id}-I{pitch}\"><instrument-name>Percussione {pitch}</instrument-name></score-instrument>").unwrap();
            }
            for pitch in pitches {
                writeln!(xml, "<midi-instrument id=\"P{part_id}-I{pitch}\"><midi-channel>10</midi-channel><midi-unpitched>{}</midi-unpitched></midi-instrument>", u16::from(pitch) + 1).unwrap();
            }
        } else {
            let channel = (index % 15) + 1;
            let channel = if channel >= 10 { channel + 1 } else { channel };
            writeln!(xml, "<score-instrument id=\"P{part_id}-I1\"><instrument-name>{}</instrument-name></score-instrument><midi-instrument id=\"P{part_id}-I1\"><midi-channel>{channel}</midi-channel><midi-program>{}</midi-program></midi-instrument>", escape_xml(name), u16::from(*program) + 1).unwrap();
        }
        xml.push_str("</score-part>\n");
    }
    xml.push_str("</part-list>\n");
    let all_quantized = quantize_notes(notes, settings);
    if all_quantized.is_empty() {
        return Err("La traccia non contiene note con durate valide.".into());
    }
    let first_tick = first_measure_tick(&all_quantized, settings);
    let last_tick = all_quantized.iter().map(|note| note.end).max().unwrap();
    let bar = measure_ticks(settings);
    let bars = ((last_tick - first_tick + bar - 1) / bar).max(1);
    for (index, (_, part_notes)) in instruments.iter().enumerate() {
        let part_id = index + 1;
        let quantized = quantize_notes(part_notes, settings);
        let segments = split_notes(&quantized, settings);
        let voices = quantized.iter().map(|note| note.voice).max().unwrap_or(0) + 1;
        let clef = choose_clef(part_notes);
        writeln!(xml, "<part id=\"P{part_id}\">").unwrap();
        for measure in 0..bars {
            let start = first_tick + measure * bar;
            let end = start + bar;
            writeln!(xml, "<measure number=\"{}\">", measure + 1).unwrap();
            if measure == 0 {
                let (sign, line) = match clef {
                    Clef::Treble | Clef::Guitar => ("G", Some(2)),
                    Clef::Bass => ("F", Some(4)),
                    Clef::Percussion => ("percussion", None),
                };
                write!(xml, "<attributes><divisions>{TICKS_PER_BEAT}</divisions><key><fifths>0</fifths></key><time><beats>{}</beats><beat-type>4</beat-type></time><clef><sign>{sign}</sign>", settings.beats_per_bar.clamp(1, 12)).unwrap();
                if let Some(line) = line {
                    write!(xml, "<line>{line}</line>").unwrap();
                }
                if matches!(clef, Clef::Guitar) {
                    xml.push_str("<clef-octave-change>-1</clef-octave-change>");
                }
                xml.push_str("</clef></attributes>\n");
                writeln!(xml, "<direction placement=\"above\"><direction-type><metronome><beat-unit>quarter</beat-unit><per-minute>{:.2}</per-minute></metronome></direction-type><sound tempo=\"{:.2}\"/></direction>", valid_bpm(settings), valid_bpm(settings)).unwrap();
            }
            for voice in 0..voices {
                if voice > 0 {
                    writeln!(xml, "<backup><duration>{bar}</duration></backup>").unwrap();
                }
                append_measure_voice(&mut xml, &segments, start, end, voice, part_id, settings);
            }
            if measure == bars - 1 {
                xml.push_str(
                    "<barline location=\"right\"><bar-style>light-heavy</bar-style></barline>\n",
                );
            }
            xml.push_str("</measure>\n");
        }
        xml.push_str("</part>\n");
    }
    xml.push_str("</score-partwise>\n");
    Ok(xml)
}

fn append_measure_voice(
    xml: &mut String,
    segments: &[NoteSegment],
    start: i64,
    end: i64,
    voice: usize,
    part_id: usize,
    settings: &ScoreSettings,
) {
    let mut chords: BTreeMap<(i64, i64), Vec<&NoteSegment>> = BTreeMap::new();
    for segment in segments
        .iter()
        .filter(|segment| segment.voice == voice && segment.start >= start && segment.start < end)
    {
        chords
            .entry((segment.start, segment.duration.ticks))
            .or_default()
            .push(segment);
    }
    if chords.is_empty() {
        writeln!(
            xml,
            "<note><rest measure=\"yes\"/><duration>{}</duration><voice>{}</voice></note>",
            end - start,
            voice + 1
        )
        .unwrap();
        return;
    }
    let mut cursor = start;
    for ((onset, duration), chord) in chords {
        append_rests(xml, onset - cursor, voice, settings);
        for (index, segment) in chord.iter().enumerate() {
            append_note(xml, segment, index > 0, part_id);
        }
        cursor = onset + duration;
    }
    append_rests(xml, end - cursor, voice, settings);
}

fn append_rests(xml: &mut String, ticks: i64, voice: usize, settings: &ScoreSettings) {
    for duration in split_duration(ticks, grid_ticks(settings)) {
        xml.push_str("<note><rest/>");
        append_duration(xml, duration, voice);
        xml.push_str("</note>\n");
    }
}

fn append_note(xml: &mut String, note: &NoteSegment, chord: bool, part_id: usize) {
    xml.push_str("<note>");
    if chord {
        xml.push_str("<chord/>");
    }
    if note.is_drum {
        let (step, octave) = drum_spelling(note.pitch);
        write!(xml, "<unpitched><display-step>{step}</display-step><display-octave>{octave}</display-octave></unpitched>").unwrap();
    } else {
        let (step, alteration, octave) = pitch_spelling(note.pitch);
        write!(xml, "<pitch><step>{step}</step>").unwrap();
        if alteration != 0 {
            write!(xml, "<alter>{alteration}</alter>").unwrap();
        }
        write!(xml, "<octave>{octave}</octave></pitch>").unwrap();
    }
    write!(xml, "<duration>{}</duration>", note.duration.ticks).unwrap();
    if note.tied_from {
        xml.push_str("<tie type=\"stop\"/>");
    }
    if note.tied_to {
        xml.push_str("<tie type=\"start\"/>");
    }
    if note.is_drum {
        write!(xml, "<instrument id=\"P{part_id}-I{}\"/>", note.pitch).unwrap();
    }
    write!(xml, "<voice>{}</voice>", note.voice + 1).unwrap();
    append_type(xml, note.duration);
    if note.is_drum && matches!(note.pitch, 42 | 44 | 46 | 49 | 51 | 52 | 53 | 55 | 57 | 59) {
        xml.push_str("<notehead>x</notehead>");
    }
    if note.tied_from || note.tied_to {
        xml.push_str("<notations>");
        if note.tied_from {
            xml.push_str("<tied type=\"stop\"/>");
        }
        if note.tied_to {
            xml.push_str("<tied type=\"start\"/>");
        }
        xml.push_str("</notations>");
    }
    xml.push_str("</note>\n");
}

fn append_duration(xml: &mut String, duration: NotatedDuration, voice: usize) {
    write!(
        xml,
        "<duration>{}</duration><voice>{}</voice>",
        duration.ticks,
        voice + 1
    )
    .unwrap();
    append_type(xml, duration);
}

fn append_type(xml: &mut String, duration: NotatedDuration) {
    write!(xml, "<type>{}</type>", duration.kind).unwrap();
    for _ in 0..duration.dots {
        xml.push_str("<dot/>");
    }
    if duration.triplet {
        xml.push_str("<time-modification><actual-notes>3</actual-notes><normal-notes>2</normal-notes></time-modification>");
    }
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
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
    fn musicxml_exports_cross_bar_ties_and_escaped_title() {
        let xml = musicxml_document(
            &[note(1.5, 2.5, 60)],
            &ScoreSettings::default(),
            "A & B <live>",
        )
        .unwrap();
        assert!(xml.contains("A &amp; B &lt;live&gt;"));
        assert!(xml.contains("<measure number=\"2\">"));
        assert!(xml.contains("<tie type=\"start\"/>"));
        assert!(xml.contains("<tie type=\"stop\"/>"));
        assert!(xml.contains("<tied type=\"start\"/>"));
        assert!(xml.contains("<tied type=\"stop\"/>"));
    }

    #[test]
    fn musicxml_encodes_independent_polyphonic_voices() {
        let notes = [note(0.0, 1.0, 60), note(0.0, 1.0, 64), note(0.5, 1.5, 67)];
        let xml = musicxml_document(&notes, &ScoreSettings::default(), "Chords").unwrap();
        assert_eq!(xml.matches("<chord/>").count(), 1);
        assert!(xml.contains("<voice>2</voice>"));
        assert!(xml.contains("<backup><duration>384</duration></backup>"));
    }

    #[test]
    fn musicxml_encodes_triplet_time_modification() {
        let settings = ScoreSettings {
            subdivisions: 3,
            ..ScoreSettings::default()
        };
        let xml = musicxml_document(&[note(0.0, 1.0 / 6.0, 60)], &settings, "Triplet").unwrap();
        assert!(xml.contains("<type>eighth</type><time-modification><actual-notes>3</actual-notes><normal-notes>2</normal-notes>"));
    }

    #[test]
    fn musicxml_guitar_clef_keeps_concert_pitch_and_does_not_double_transpose() {
        let mut guitar_note = note(0.0, 0.5, 40);
        guitar_note.program = 24;
        let xml = musicxml_document(&[guitar_note], &ScoreSettings::default(), "Guitar").unwrap();
        assert!(xml.contains(
            "<clef><sign>G</sign><line>2</line><clef-octave-change>-1</clef-octave-change></clef>"
        ));
        assert!(xml.contains("<pitch><step>E</step><octave>2</octave></pitch>"));
        assert!(xml.contains("<part-name>Chitarra acustica</part-name>"));
        assert!(!xml.contains("<transpose>"));
    }

    #[test]
    fn midi_preserves_original_note_times_instead_of_score_grid() {
        let transcription = Transcription {
            source: "test.wav".into(),
            duration: 1.0,
            notes: vec![note(0.123, 0.678, 60)],
            engine: "test".into(),
        };
        let path = std::env::temp_dir().join(format!("corsivo-midi-{}.mid", std::process::id()));
        write_midi(&path, &transcription, 120.0).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let smf = Smf::parse(&bytes).unwrap();
        let mut tick = 0;
        let mut boundaries = Vec::new();
        for event in &smf.tracks[1] {
            tick += event.delta.as_int();
            if let TrackEventKind::Midi {
                message: MidiMessage::NoteOn { .. } | MidiMessage::NoteOff { .. },
                ..
            } = event.kind
            {
                boundaries.push(tick);
            }
        }
        assert_eq!(boundaries, vec![236, 1302]);
        std::fs::remove_file(path).unwrap();
    }
}
