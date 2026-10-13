use crate::transcription::Note;
use std::collections::BTreeSet;

const ONSET_CLUSTER_SECONDS: f64 = 0.085;
const MINIMUM_STABILITY_SECONDS: f64 = 0.1;
const PITCH_CLASS_NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];

#[derive(Clone, Debug)]
pub struct Chord {
    pub onset: f64,
    pub offset: f64,
    pub symbol: String,
    pub confidence: f32,
}

struct ChordTemplate {
    suffix: &'static str,
    intervals: &'static [u8],
    allow_missing_fifth: bool,
}

const CHORD_TEMPLATES: &[ChordTemplate] = &[
    ChordTemplate {
        suffix: "",
        intervals: &[0, 4, 7],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "m",
        intervals: &[0, 3, 7],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "dim",
        intervals: &[0, 3, 6],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "aug",
        intervals: &[0, 4, 8],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "sus2",
        intervals: &[0, 2, 7],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "sus4",
        intervals: &[0, 5, 7],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "6",
        intervals: &[0, 4, 7, 9],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "m6",
        intervals: &[0, 3, 7, 9],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "7",
        intervals: &[0, 4, 7, 10],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "maj7",
        intervals: &[0, 4, 7, 11],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "m7",
        intervals: &[0, 3, 7, 10],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "mMaj7",
        intervals: &[0, 3, 7, 11],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "dim7",
        intervals: &[0, 3, 6, 9],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "m7b5",
        intervals: &[0, 3, 6, 10],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "7sus4",
        intervals: &[0, 5, 7, 10],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "9",
        intervals: &[0, 2, 4, 7, 10],
        allow_missing_fifth: true,
    },
    ChordTemplate {
        suffix: "maj9",
        intervals: &[0, 2, 4, 7, 11],
        allow_missing_fifth: true,
    },
    ChordTemplate {
        suffix: "m9",
        intervals: &[0, 2, 3, 7, 10],
        allow_missing_fifth: true,
    },
    ChordTemplate {
        suffix: "add9",
        intervals: &[0, 2, 4, 7],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "11",
        intervals: &[0, 2, 4, 5, 7, 10],
        allow_missing_fifth: true,
    },
    ChordTemplate {
        suffix: "11",
        intervals: &[0, 4, 5, 7, 10],
        allow_missing_fifth: true,
    },
    ChordTemplate {
        suffix: "m11",
        intervals: &[0, 2, 3, 5, 7, 10],
        allow_missing_fifth: true,
    },
    ChordTemplate {
        suffix: "m11",
        intervals: &[0, 3, 5, 7, 10],
        allow_missing_fifth: true,
    },
    ChordTemplate {
        suffix: "13",
        intervals: &[0, 2, 4, 7, 9, 10],
        allow_missing_fifth: true,
    },
    ChordTemplate {
        suffix: "13",
        intervals: &[0, 4, 7, 9, 10],
        allow_missing_fifth: true,
    },
    ChordTemplate {
        suffix: "7b9",
        intervals: &[0, 1, 4, 7, 10],
        allow_missing_fifth: true,
    },
    ChordTemplate {
        suffix: "7#9",
        intervals: &[0, 3, 4, 7, 10],
        allow_missing_fifth: true,
    },
    ChordTemplate {
        suffix: "7b5",
        intervals: &[0, 4, 6, 10],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "7#5",
        intervals: &[0, 4, 8, 10],
        allow_missing_fifth: false,
    },
    ChordTemplate {
        suffix: "7#11",
        intervals: &[0, 4, 6, 7, 10],
        allow_missing_fifth: true,
    },
    ChordTemplate {
        suffix: "7b13",
        intervals: &[0, 4, 7, 8, 10],
        allow_missing_fifth: true,
    },
];

struct NoteEvent {
    time: f64,
    pitch: u8,
    starts: bool,
    note_index: usize,
}

struct Sonority {
    onset: f64,
    chord_onset: f64,
    offset: f64,
    pitch_classes: u16,
    bass: u8,
}

struct ChordCandidate {
    root: u8,
    suffix: &'static str,
    rank: f32,
    missing_fifth: bool,
}

pub fn analyze_chords(notes: &[Note]) -> Vec<Chord> {
    let mut sounding_notes: Vec<_> = notes
        .iter()
        .filter(|note| {
            !note.is_drum
                && note.pitch <= 127
                && note.onset.is_finite()
                && note.offset.is_finite()
                && note.onset >= 0.0
                && note.offset - note.onset >= MINIMUM_STABILITY_SECONDS - f64::EPSILON
        })
        .collect();
    sounding_notes.sort_unstable_by(|left, right| left.onset.total_cmp(&right.onset));

    let mut events = Vec::with_capacity(sounding_notes.len() * 2);
    for (note_index, note) in sounding_notes.iter().enumerate() {
        events.push(NoteEvent {
            time: note.onset,
            pitch: note.pitch,
            starts: true,
            note_index,
        });
        events.push(NoteEvent {
            time: note.offset,
            pitch: note.pitch,
            starts: false,
            note_index,
        });
    }
    events.sort_unstable_by(|left, right| left.time.total_cmp(&right.time));

    let mut active_pitches = [0u32; 128];
    let mut active_note_indices = BTreeSet::<usize>::new();
    let mut sonorities: Vec<Sonority> = Vec::new();
    let mut previous_time = 0.0;
    let mut event_index = 0;
    while event_index < events.len() {
        let time = events[event_index].time;
        if time > previous_time {
            let pitch_classes = active_pitches
                .iter()
                .enumerate()
                .filter(|(_, count)| **count > 0)
                .fold(0u16, |mask, (pitch, _)| mask | (1 << (pitch % 12)));
            if pitch_classes.count_ones() >= 3 {
                let bass = active_pitches.iter().position(|count| *count > 0).unwrap() as u8 % 12;
                if let Some(previous) = sonorities.last_mut()
                    && previous.pitch_classes == pitch_classes
                    && previous.bass == bass
                    && (previous.offset - previous_time).abs() < 1e-9
                {
                    previous.offset = time;
                } else {
                    let earliest_onset =
                        sounding_notes[*active_note_indices.first().unwrap()].onset;
                    let chord_onset = if previous_time - earliest_onset <= ONSET_CLUSTER_SECONDS {
                        earliest_onset
                    } else {
                        previous_time
                    };
                    sonorities.push(Sonority {
                        onset: previous_time,
                        chord_onset,
                        offset: time,
                        pitch_classes,
                        bass,
                    });
                }
            }
        }

        while event_index < events.len() && events[event_index].time == time {
            let event = &events[event_index];
            let pitch_count = &mut active_pitches[usize::from(event.pitch)];
            if event.starts {
                *pitch_count += 1;
                active_note_indices.insert(event.note_index);
            } else {
                *pitch_count = pitch_count.saturating_sub(1);
                active_note_indices.remove(&event.note_index);
            }
            event_index += 1;
        }
        previous_time = time;
    }

    let mut chords: Vec<Chord> = Vec::new();
    for sonority in sonorities {
        if sonority.offset - sonority.onset < MINIMUM_STABILITY_SECONDS - 1e-9 {
            continue;
        }
        let Some(chord) = identify_chord(&sonority) else {
            continue;
        };
        if let Some(previous) = chords.last_mut()
            && previous.symbol == chord.symbol
            && chord.onset - previous.offset <= ONSET_CLUSTER_SECONDS
        {
            previous.offset = chord.offset;
            previous.confidence = previous.confidence.min(chord.confidence);
        } else {
            chords.push(chord);
        }
    }
    chords
}

fn identify_chord(sonority: &Sonority) -> Option<Chord> {
    let mut candidates = Vec::new();
    for root in 0..12u8 {
        let relative_pitch_classes =
            ((sonority.pitch_classes >> root) | (sonority.pitch_classes << (12 - root))) & 0x0fff;
        if relative_pitch_classes & 1 == 0 {
            continue;
        }
        for template in CHORD_TEMPLATES {
            let expected_pitch_classes = template
                .intervals
                .iter()
                .fold(0u16, |mask, interval| mask | (1 << interval));
            if relative_pitch_classes & !expected_pitch_classes != 0 {
                continue;
            }
            let missing_pitch_classes = expected_pitch_classes & !relative_pitch_classes;
            let missing_fifth = missing_pitch_classes == 1 << 7 && template.allow_missing_fifth;
            if missing_pitch_classes != 0 && !missing_fifth {
                continue;
            }
            let rank = if missing_fifth { 0.88 } else { 1.0 }
                + if root == sonority.bass { 0.1 } else { 0.0 };
            candidates.push(ChordCandidate {
                root,
                suffix: template.suffix,
                rank,
                missing_fifth,
            });
        }
    }
    candidates.sort_unstable_by(|left, right| right.rank.total_cmp(&left.rank));
    let best = candidates.first()?;
    let competing = candidates
        .iter()
        .skip(1)
        .find(|candidate| candidate.root != best.root || candidate.suffix != best.suffix);
    if competing.is_some_and(|candidate| best.rank - candidate.rank < 0.065) {
        return None;
    }

    let root_name = PITCH_CLASS_NAMES[usize::from(best.root)];
    let mut symbol = format!("{root_name}{}", best.suffix);
    if best.root != sonority.bass {
        symbol.push('/');
        symbol.push_str(PITCH_CLASS_NAMES[usize::from(sonority.bass)]);
    }
    let duration_bonus = ((sonority.offset - sonority.onset - MINIMUM_STABILITY_SECONDS) / 0.5)
        .clamp(0.0, 1.0) as f32
        * 0.06;
    let confidence = (if best.missing_fifth { 0.8 } else { 0.88 } + duration_bonus
        - if competing.is_some() { 0.08 } else { 0.0 })
    .clamp(0.0, 1.0);
    Some(Chord {
        onset: sonority.chord_onset,
        offset: sonority.offset,
        symbol,
        confidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes_for_pitches(pitches: &[u8]) -> Vec<Note> {
        pitches.iter().map(|pitch| note(*pitch, 0.0, 1.0)).collect()
    }

    fn note(pitch: u8, onset: f64, offset: f64) -> Note {
        Note {
            onset,
            offset,
            pitch,
            program: 0,
            is_drum: false,
        }
    }

    fn assert_symbol(pitches: &[u8], expected: &str) {
        let chords = analyze_chords(&notes_for_pitches(pitches));
        assert_eq!(chords.len(), 1, "{pitches:?}: {chords:?}");
        assert_eq!(chords[0].symbol, expected);
        assert!(chords[0].confidence >= 0.75 && chords[0].confidence <= 1.0);
    }

    #[test]
    fn identifies_triad_qualities_and_inversions() {
        assert_symbol(&[48, 52, 55], "C");
        assert_symbol(&[48, 51, 55], "Cm");
        assert_symbol(&[48, 51, 54], "Cdim");
        assert_symbol(&[48, 52, 56], "Caug");
        assert_symbol(&[48, 50, 55], "Csus2");
        assert_symbol(&[48, 53, 55], "Csus4");
        assert_symbol(&[52, 55, 60], "C/E");
        assert_symbol(&[55, 60, 64], "C/G");
        assert_symbol(&[46, 50, 53], "Bb");
    }

    #[test]
    fn identifies_sevenths_and_sixths_using_the_bass() {
        assert_symbol(&[48, 52, 55, 57], "C6");
        assert_symbol(&[48, 51, 55, 57], "Cm6");
        assert_symbol(&[48, 52, 55, 58], "C7");
        assert_symbol(&[48, 52, 55, 59], "Cmaj7");
        assert_symbol(&[48, 51, 55, 58], "Cm7");
        assert_symbol(&[48, 51, 55, 59], "CmMaj7");
        assert_symbol(&[47, 50, 53, 56], "Bdim7");
        assert_symbol(&[45, 48, 51, 55], "Am7b5");
        assert_symbol(&[48, 53, 55, 58], "C7sus4");
    }

    #[test]
    fn identifies_supported_jazz_extensions() {
        assert_symbol(&[48, 52, 55, 58, 62], "C9");
        assert_symbol(&[48, 52, 55, 59, 62], "Cmaj9");
        assert_symbol(&[48, 51, 55, 58, 62], "Cm9");
        assert_symbol(&[48, 52, 55, 62], "Cadd9");
        assert_symbol(&[48, 52, 53, 55, 58, 62], "C11");
        assert_symbol(&[48, 51, 53, 55, 58, 62], "Cm11");
        assert_symbol(&[48, 52, 55, 58, 62, 69], "C13");
        assert_symbol(&[48, 52, 55, 58, 61], "C7b9");
        assert_symbol(&[48, 52, 55, 58, 63], "C7#9");
        assert_symbol(&[48, 52, 54, 58], "C7b5");
        assert_symbol(&[48, 52, 56, 58], "C7#5");
        assert_symbol(&[48, 52, 55, 58, 66], "C7#11");
        assert_symbol(&[48, 52, 55, 58, 68], "C7b13");
    }

    #[test]
    fn accepts_missing_fifths_only_with_third_seventh_and_extension() {
        assert_symbol(&[48, 52, 58, 62], "C9");
        assert_symbol(&[48, 52, 59, 62], "Cmaj9");
        assert_symbol(&[48, 51, 58, 62], "Cm9");
        assert_symbol(&[48, 52, 58, 61], "C7b9");
        assert!(analyze_chords(&notes_for_pitches(&[48, 52, 58])).is_empty());
        assert!(analyze_chords(&notes_for_pitches(&[48, 52, 62])).is_empty());
    }

    #[test]
    fn aligns_a_short_strum_and_merges_repeated_voicings() {
        let notes = vec![
            note(48, 0.0, 0.5),
            note(52, 0.035, 0.5),
            note(55, 0.07, 0.5),
            note(60, 0.54, 1.0),
            note(64, 0.55, 1.0),
            note(67, 0.57, 1.0),
        ];
        let chords = analyze_chords(&notes);
        assert_eq!(chords.len(), 1);
        assert_eq!(chords[0].symbol, "C");
        assert_eq!(chords[0].onset, 0.0);
        assert_eq!(chords[0].offset, 1.0);
    }

    #[test]
    fn ignores_short_overlaps_dyads_and_melodic_runs() {
        let short_overlap = vec![note(48, 0.0, 0.5), note(52, 0.42, 0.9), note(55, 0.46, 1.0)];
        assert!(analyze_chords(&short_overlap).is_empty());
        assert!(analyze_chords(&notes_for_pitches(&[48, 55])).is_empty());
        let melody = vec![
            note(60, 0.0, 0.2),
            note(64, 0.08, 0.28),
            note(67, 0.16, 0.36),
        ];
        assert!(analyze_chords(&melody).is_empty());
        let clustered_short_overlap = vec![
            note(60, 0.0, 0.1),
            note(64, 0.04, 0.14),
            note(67, 0.08, 0.18),
        ];
        assert!(analyze_chords(&clustered_short_overlap).is_empty());
    }

    #[test]
    fn suppresses_unresolved_equivalent_roots() {
        assert!(analyze_chords(&notes_for_pitches(&[52, 55, 57, 60])).is_empty());
    }

    #[test]
    fn does_not_invent_a_chord_from_an_unmatched_collection() {
        assert!(analyze_chords(&notes_for_pitches(&[48, 49, 50, 54, 55])).is_empty());
    }

    #[test]
    fn excludes_drums_and_invalid_notes() {
        let mut notes = notes_for_pitches(&[48, 52, 55]);
        let mut drum = note(61, 0.0, 1.0);
        drum.is_drum = true;
        notes.extend([
            drum,
            note(62, f64::NAN, 1.0),
            note(63, 0.0, f64::INFINITY),
            note(255, 0.0, 1.0),
            note(65, 0.5, 0.4),
        ]);
        let chords = analyze_chords(&notes);
        assert_eq!(chords.len(), 1);
        assert_eq!(chords[0].symbol, "C");
    }

    #[test]
    fn ignores_a_brief_chromatic_tone_over_a_sustained_chord() {
        let mut notes = notes_for_pitches(&[48, 52, 55]);
        notes.push(note(61, 0.3, 0.36));
        let chords = analyze_chords(&notes);
        assert_eq!(chords.len(), 1);
        assert_eq!(chords[0].symbol, "C");
        assert_eq!(chords[0].offset, 1.0);
    }

    #[test]
    fn keeps_chord_changes_in_time_when_a_sustained_extension_ends() {
        let mut notes = notes_for_pitches(&[48, 52, 55]);
        notes.push(note(58, 0.3, 0.6));
        let chords = analyze_chords(&notes);
        assert_eq!(chords.len(), 3);
        assert_eq!(
            chords
                .iter()
                .map(|chord| chord.symbol.as_str())
                .collect::<Vec<_>>(),
            ["C", "C7", "C"]
        );
        assert_eq!(chords[0].onset, 0.0);
        assert_eq!(chords[0].offset, 0.3);
        assert_eq!(chords[1].onset, 0.3);
        assert_eq!(chords[1].offset, 0.6);
        assert_eq!(chords[2].onset, 0.6);
        assert_eq!(chords[2].offset, 1.0);
    }
}
