use std::collections::VecDeque;
use std::f64::consts::PI;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, CodecParameters, DecoderOptions};
use symphonia::core::errors::Error as DecodeError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::midi::{MidiRenderer, MidiScore, MidiTiming, PreviewNote};
use crate::transcription::Note;

const FILTER_TAPS: usize = 64;
const FILTER_PHASES: usize = 512;
const BUFFER_SECONDS: f64 = 0.3;

#[derive(Clone)]
pub struct AudioClip {
    pub path: PathBuf,
    pub title: String,
    pub samples: Arc<Vec<f32>>,
    pub sample_rate: u32,
    pub duration: f64,
    pub waveform: Vec<(f32, f32)>,
}

pub fn load_audio(path: &Path) -> Result<AudioClip, String> {
    let file = File::open(path).map_err(|error| format!("Impossibile aprire l'audio: {error}"))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
        hint.with_extension(extension);
    }
    let options = FormatOptions {
        enable_gapless: true,
        ..Default::default()
    };
    let probed = symphonia::default::get_probe()
        .format(&hint, stream, &options, &MetadataOptions::default())
        .map_err(|error| format!("Formato audio non leggibile: {error}"))?;
    let mut format = probed.format;
    let (track_id, parameters) = format
        .default_track()
        .or_else(|| {
            format
                .tracks()
                .iter()
                .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        })
        .map(|track| (track.id, track.codec_params.clone()))
        .ok_or_else(|| "Il file non contiene una traccia audio.".to_string())?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&parameters, &DecoderOptions::default())
        .map_err(|error| format!("Codec audio non disponibile: {error}"))?;
    let mut samples = Vec::with_capacity(estimated_sample_count(&parameters));
    let mut sample_rate = parameters.sample_rate.unwrap_or(0);
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(DecodeError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(error) => return Err(format!("Lettura audio interrotta: {error}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(DecodeError::DecodeError(_)) => continue,
            Err(error) => return Err(format!("Decodifica audio fallita: {error}")),
        };
        if sample_rate == 0 {
            sample_rate = decoded.spec().rate;
        }
        if sample_rate != decoded.spec().rate {
            return Err(
                "Il file cambia frequenza di campionamento durante la riproduzione.".into(),
            );
        }
        let channels = decoded.spec().channels.count();
        if channels == 0 {
            continue;
        }
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        buffer.copy_interleaved_ref(decoded);
        for frame in buffer.samples().chunks_exact(channels) {
            let left = finite_sample(frame[0]);
            let right = finite_sample(*frame.get(1).unwrap_or(&frame[0]));
            if channels <= 2 {
                samples.extend_from_slice(&[left, right]);
            } else {
                let center = frame[2..]
                    .iter()
                    .map(|sample| finite_sample(*sample))
                    .sum::<f32>()
                    / channels as f32;
                samples.extend_from_slice(&[left * 0.75 + center, right * 0.75 + center]);
            }
        }
    }
    if samples.is_empty() || sample_rate == 0 {
        return Err("Il file audio è vuoto o non può essere decodificato.".into());
    }
    let duration = samples.len() as f64 / (sample_rate as f64 * 2.0);
    let frames = samples.len() / 2;
    let waveform_count = frames.min(128_000);
    let mut waveform = Vec::with_capacity(waveform_count);
    for index in 0..waveform_count {
        let start = index * frames / waveform_count * 2;
        let end = (index + 1) * frames / waveform_count * 2;
        let (minimum, maximum) = samples[start..end]
            .iter()
            .fold((1.0_f32, -1.0_f32), |(minimum, maximum), &sample| {
                (minimum.min(sample), maximum.max(sample))
            });
        waveform.push((minimum, maximum));
    }
    Ok(AudioClip {
        path: path.to_path_buf(),
        title: path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("Audio")
            .to_string(),
        samples: Arc::new(samples),
        sample_rate,
        duration,
        waveform,
    })
}

fn estimated_sample_count(parameters: &CodecParameters) -> usize {
    parameters
        .n_frames
        .and_then(|frames| usize::try_from(frames).ok())
        .and_then(|frames| frames.checked_mul(2))
        .unwrap_or(0)
}

fn finite_sample(sample: f32) -> f32 {
    if sample.is_finite() { sample } else { 0.0 }
}

impl AudioClip {
    pub fn mono_16khz(&self) -> Vec<f32> {
        let source_frames = self.samples.len() / 2;
        if self.sample_rate == 16_000 {
            return self
                .samples
                .chunks_exact(2)
                .map(|frame| (frame[0] + frame[1]) * 0.5)
                .collect();
        }
        if source_frames == 0 || self.sample_rate == 0 {
            return Vec::new();
        }
        let coefficients = filter_coefficients(self.sample_rate, 16_000);
        let ratio = self.sample_rate as f64 / 16_000.0;
        let frame_count = (source_frames as f64 / ratio).round() as usize;
        let mut mono = Vec::with_capacity(frame_count);
        for output_frame in 0..frame_count {
            let position = output_frame as f64 * ratio;
            let center = position.floor() as i64;
            let phase = ((position - center as f64) * FILTER_PHASES as f64) as usize;
            let weights = &coefficients[phase.min(FILTER_PHASES - 1)];
            let mut sample = 0.0;
            for (tap, &weight) in weights.iter().enumerate() {
                let index = (center + tap as i64 - FILTER_TAPS as i64 / 2 + 1)
                    .clamp(0, source_frames as i64 - 1) as usize
                    * 2;
                sample += (self.samples[index] + self.samples[index + 1]) * 0.5 * weight;
            }
            mono.push(sample);
        }
        mono
    }
}

fn filter_coefficients(source_rate: u32, output_rate: u32) -> Vec<[f32; FILTER_TAPS]> {
    let cutoff = (output_rate as f64 / source_rate as f64).min(1.0) * 0.94;
    (0..FILTER_PHASES)
        .map(|phase| {
            let fraction = phase as f64 / FILTER_PHASES as f64;
            let mut weights = [0.0; FILTER_TAPS];
            let mut sum = 0.0;
            for (index, weight) in weights.iter_mut().enumerate() {
                let offset = index as f64 - FILTER_TAPS as f64 / 2.0 + 1.0 - fraction;
                let normalized = offset / (FILTER_TAPS as f64 / 2.0);
                let window = if normalized.abs() >= 1.0 {
                    0.0
                } else {
                    0.42 + 0.5 * (PI * normalized).cos() + 0.08 * (2.0 * PI * normalized).cos()
                };
                let argument = PI * offset * cutoff;
                let sinc = if argument.abs() < 1e-9 {
                    1.0
                } else {
                    argument.sin() / argument
                };
                *weight = (sinc * cutoff * window) as f32;
                sum += *weight;
            }
            for weight in &mut weights {
                *weight /= sum;
            }
            weights
        })
        .collect()
}

#[derive(Clone)]
struct RenderConfiguration {
    samples: Arc<Vec<f32>>,
    sample_rate: u32,
    output_rate: u32,
    duration: f64,
    speed: f32,
    start: f64,
    loop_range: Option<(f64, f64)>,
}

struct RenderedBlock {
    samples: Vec<f32>,
    consumed: usize,
    start: f64,
    end: f64,
}

struct PlaybackState {
    configuration: Option<RenderConfiguration>,
    revision: u64,
    blocks: VecDeque<RenderedBlock>,
    buffered_frames: usize,
    position: f64,
    volume: f32,
    playing: bool,
    exhausted: bool,
    midi_score: Arc<MidiScore>,
    midi_mode: bool,
    midi_revision: u64,
    preview: Option<PreviewNote>,
}

struct SharedPlayback {
    state: Mutex<PlaybackState>,
    changed: Condvar,
    processing: AtomicBool,
    running: AtomicBool,
}

pub struct AudioPlayer {
    stream: Option<cpal::Stream>,
    shared: Arc<SharedPlayback>,
    worker: Option<JoinHandle<()>>,
    output_rate: u32,
}

impl AudioPlayer {
    pub fn new() -> Result<Self, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| "Nessun dispositivo di uscita audio disponibile.".to_string())?;
        let supported = device
            .default_output_config()
            .map_err(|error| format!("Impossibile leggere l'uscita audio: {error}"))?;
        let configuration = supported.config();
        let shared = Arc::new(SharedPlayback {
            state: Mutex::new(PlaybackState {
                configuration: None,
                revision: 0,
                blocks: VecDeque::new(),
                buffered_frames: 0,
                position: 0.0,
                volume: 0.8,
                playing: false,
                exhausted: false,
                midi_score: Arc::new(MidiScore::new(&[])),
                midi_mode: false,
                midi_revision: 0,
                preview: None,
            }),
            changed: Condvar::new(),
            processing: AtomicBool::new(false),
            running: AtomicBool::new(true),
        });
        let midi_renderer = MidiRenderer::new(configuration.sample_rate.0)?;
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => {
                build_stream::<f32>(&device, &configuration, shared.clone(), midi_renderer)
            }
            cpal::SampleFormat::I16 => {
                build_stream::<i16>(&device, &configuration, shared.clone(), midi_renderer)
            }
            cpal::SampleFormat::U16 => {
                build_stream::<u16>(&device, &configuration, shared.clone(), midi_renderer)
            }
            format => {
                return Err(format!(
                    "Formato dell'uscita audio non supportato: {format}"
                ));
            }
        }
        .map_err(|error| format!("Impossibile avviare l'uscita audio: {error}"))?;
        stream
            .play()
            .map_err(|error| format!("Impossibile attivare l'uscita audio: {error}"))?;
        let worker_shared = shared.clone();
        let worker = thread::Builder::new()
            .name("audio-wsola".into())
            .spawn(move || render_audio(worker_shared))
            .map_err(|error| format!("Impossibile avviare il motore audio: {error}"))?;
        Ok(Self {
            stream: Some(stream),
            shared,
            worker: Some(worker),
            output_rate: configuration.sample_rate.0,
        })
    }

    pub fn load(&mut self, clip: &AudioClip) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let speed = state
            .configuration
            .as_ref()
            .map_or(1.0, |configuration| configuration.speed);
        state.configuration = Some(RenderConfiguration {
            samples: clip.samples.clone(),
            sample_rate: clip.sample_rate,
            output_rate: self.output_rate,
            duration: clip.duration,
            speed,
            start: 0.0,
            loop_range: None,
        });
        state.position = 0.0;
        state.playing = false;
        state.midi_score = Arc::new(MidiScore::new(&[]));
        state.preview = None;
        self.invalidate(&mut state);
    }

    pub fn play(&mut self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.configuration.is_none() {
            return;
        }
        if state.exhausted && state.blocks.is_empty() {
            state.position = 0.0;
            self.invalidate(&mut state);
        }
        if !state.playing {
            state.midi_revision = state.midi_revision.wrapping_add(1);
        }
        state.playing = true;
        self.shared.changed.notify_one();
    }

    pub fn pause(&mut self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.playing = false;
        state.midi_revision = state.midi_revision.wrapping_add(1);
    }

    pub fn toggle(&mut self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn seek(&mut self, seconds: f64) {
        if !seconds.is_finite() {
            return;
        }
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(configuration) = &state.configuration {
            state.position = seconds.clamp(0.0, configuration.duration);
            self.invalidate(&mut state);
        }
    }

    pub fn set_speed(&mut self, speed: f32) {
        if !speed.is_finite() {
            return;
        }
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(configuration) = &mut state.configuration {
            let speed = speed.clamp(0.25, 1.5);
            if (configuration.speed - speed).abs() > 0.0001 {
                configuration.speed = speed;
                self.invalidate(&mut state);
            }
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        if volume.is_finite() {
            self.shared
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .volume = volume.clamp(0.0, 1.0);
        }
    }

    pub fn set_midi_notes(&mut self, notes: &[Note]) {
        let score = Arc::new(MidiScore::new(notes));
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.midi_score = score;
        state.midi_revision = state.midi_revision.wrapping_add(1);
    }

    pub fn set_midi_mode(&mut self, enabled: bool) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.midi_mode != enabled {
            state.midi_mode = enabled;
            self.invalidate(&mut state);
        }
    }

    pub fn preview_note(&mut self, pitch: u8, program: u8, is_drum: bool, duration: f64) {
        if pitch > 127 || !duration.is_finite() {
            return;
        }
        self.shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .preview = Some(PreviewNote {
            pitch,
            program,
            is_drum,
            duration,
        });
    }

    pub fn set_loop(&mut self, range: Option<(f64, f64)>) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(configuration) = &mut state.configuration {
            configuration.loop_range = range.and_then(|(start, end)| {
                if !start.is_finite() || !end.is_finite() {
                    return None;
                }
                let start = start.clamp(0.0, configuration.duration);
                let end = end.clamp(0.0, configuration.duration);
                (end - start >= 0.02).then_some((start, end))
            });
            self.invalidate(&mut state);
        }
    }

    pub fn position(&self) -> f64 {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .position
    }

    pub fn is_playing(&self) -> bool {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .playing
    }

    pub fn is_processing(&self) -> bool {
        self.shared.processing.load(Ordering::Relaxed)
    }

    fn invalidate(&self, state: &mut PlaybackState) {
        if let Some(configuration) = &mut state.configuration {
            configuration.start = state.position;
        }
        state.revision = state.revision.wrapping_add(1);
        state.midi_revision = state.midi_revision.wrapping_add(1);
        state.blocks.clear();
        state.buffered_frames = 0;
        state.exhausted = false;
        self.shared
            .processing
            .store(!state.midi_mode, Ordering::Relaxed);
        self.shared.changed.notify_one();
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        self.stream.take();
        self.shared.running.store(false, Ordering::Relaxed);
        self.shared.changed.notify_all();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn build_stream<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    configuration: &cpal::StreamConfig,
    shared: Arc<SharedPlayback>,
    mut midi_renderer: MidiRenderer,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let channels = configuration.channels as usize;
    let callback_shared = shared.clone();
    device.build_output_stream(
        configuration,
        move |output: &mut [T], _| {
            let mut state = callback_shared
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let midi_enabled = state.midi_mode && state.playing;
            let midi_position =
                state
                    .configuration
                    .as_ref()
                    .map_or(state.position, |configuration| {
                        MidiTiming {
                            sample_rate: configuration.output_rate,
                            speed: configuration.speed,
                            duration: configuration.duration,
                            loop_range: configuration.loop_range,
                        }
                        .normalize_position(state.position)
                    });
            midi_renderer.synchronize(
                &state.midi_score,
                state.midi_revision,
                midi_enabled,
                midi_position,
            );
            if let Some(preview) = state.preview.take() {
                midi_renderer.preview(preview);
            }
            for frame in output.chunks_mut(channels) {
                let mut stereo = [0.0_f32; 2];
                if state.midi_mode && state.playing {
                    if let Some(configuration) = &state.configuration {
                        let timing = MidiTiming {
                            sample_rate: configuration.output_rate,
                            speed: configuration.speed,
                            duration: configuration.duration,
                            loop_range: configuration.loop_range,
                        };
                        let (rendered, position, finished) =
                            midi_renderer.transport_frame(state.position, timing);
                        stereo = rendered;
                        state.position = position;
                        if finished {
                            state.playing = false;
                            state.exhausted = true;
                            state.midi_revision = state.midi_revision.wrapping_add(1);
                        }
                    }
                } else if state.playing {
                    if let Some(block) = state.blocks.front_mut() {
                        let index = block.consumed * 2;
                        stereo.copy_from_slice(&block.samples[index..index + 2]);
                        block.consumed += 1;
                        let frames = block.samples.len() / 2;
                        let position = block.start
                            + (block.end - block.start) * block.consumed as f64 / frames as f64;
                        let complete = block.consumed == frames;
                        state.position = position;
                        state.buffered_frames -= 1;
                        if complete {
                            state.blocks.pop_front();
                        }
                    } else if state.exhausted {
                        state.playing = false;
                    }
                }
                let preview = midi_renderer.preview_frame();
                stereo[0] += preview[0];
                stereo[1] += preview[1];
                let volume = state.volume;
                for (channel, sample) in frame.iter_mut().enumerate() {
                    let amplitude = if channels == 1 {
                        (stereo[0] + stereo[1]) * 0.5
                    } else if channel < 2 {
                        stereo[channel]
                    } else {
                        0.0
                    };
                    *sample = T::from_sample((amplitude * volume).clamp(-1.0, 1.0));
                }
            }
            callback_shared.changed.notify_one();
        },
        move |_| {
            let mut state = shared
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.playing = false;
            shared.processing.store(false, Ordering::Relaxed);
        },
        None,
    )
}

fn render_audio(shared: Arc<SharedPlayback>) {
    let mut renderer = None;
    let mut revision = u64::MAX;
    while shared.running.load(Ordering::Relaxed) {
        let mut state = shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.midi_mode {
            shared.processing.store(false, Ordering::Relaxed);
            let _ = shared
                .changed
                .wait_timeout(state, Duration::from_millis(100));
            continue;
        }
        let configuration = match &state.configuration {
            Some(configuration) => configuration.clone(),
            None => {
                let _ = shared
                    .changed
                    .wait_timeout(state, Duration::from_millis(100));
                continue;
            }
        };
        if revision != state.revision {
            revision = state.revision;
            drop(state);
            renderer = Some(TimeStretcher::new(configuration));
            continue;
        }
        let target_frames = (configuration.output_rate as f64 * BUFFER_SECONDS) as usize;
        if state.buffered_frames >= target_frames || state.exhausted {
            let _ = shared
                .changed
                .wait_timeout(state, Duration::from_millis(20));
            continue;
        }
        drop(state);
        let block = renderer.as_mut().and_then(TimeStretcher::next_block);
        state = shared
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if revision != state.revision {
            continue;
        }
        if let Some(block) = block {
            state.buffered_frames += block.samples.len() / 2;
            state.blocks.push_back(block);
            if state.blocks.len() >= 3 {
                shared.processing.store(false, Ordering::Relaxed);
            }
        } else {
            state.exhausted = true;
            shared.processing.store(false, Ordering::Relaxed);
        }
    }
}

struct PlaybackSource {
    samples: Arc<Vec<f32>>,
    ratio: f64,
    frame_count: usize,
    loop_frames: Option<(i64, i64)>,
    coefficients: Option<Vec<[f32; FILTER_TAPS]>>,
}

impl PlaybackSource {
    fn new(configuration: &RenderConfiguration) -> Self {
        let frame_count = configuration.samples.len() / 2;
        let loop_frames = configuration.loop_range.and_then(|(start, end)| {
            let start = (start * configuration.sample_rate as f64).ceil() as i64;
            let end =
                ((end * configuration.sample_rate as f64).ceil() as i64).min(frame_count as i64);
            (end > start).then_some((start, end))
        });
        Self {
            samples: configuration.samples.clone(),
            ratio: configuration.sample_rate as f64 / configuration.output_rate as f64,
            frame_count,
            loop_frames,
            coefficients: (configuration.sample_rate != configuration.output_rate)
                .then(|| filter_coefficients(configuration.sample_rate, configuration.output_rate)),
        }
    }

    fn sample(&self, output_frame: f64, channel: usize) -> f32 {
        let position = self.native_position(output_frame);
        let center = position.floor() as i64;
        let Some(coefficients) = &self.coefficients else {
            return self.frame_sample(center, channel);
        };
        let phase = ((position - center as f64) * FILTER_PHASES as f64) as usize;
        let mut sample = 0.0;
        for (tap, &weight) in coefficients[phase.min(FILTER_PHASES - 1)]
            .iter()
            .enumerate()
        {
            let index = center + tap as i64 - FILTER_TAPS as i64 / 2 + 1;
            sample += self.frame_sample(index, channel) * weight;
        }
        sample
    }

    fn correlation_sample(&self, output_frame: f64, channel: usize) -> f32 {
        let position = self.native_position(output_frame);
        let index = position.floor() as i64;
        let fraction = (position - index as f64) as f32;
        let first = self.frame_sample(index, channel);
        let second = self.frame_sample(index + 1, channel);
        first + (second - first) * fraction
    }

    fn native_position(&self, output_frame: f64) -> f64 {
        let position = output_frame * self.ratio;
        match self.loop_frames {
            Some((start, end)) => {
                start as f64 + (position - start as f64).rem_euclid((end - start) as f64)
            }
            None => position,
        }
    }

    fn frame_sample(&self, frame: i64, channel: usize) -> f32 {
        let frame = match self.loop_frames {
            Some((start, end)) => start + (frame - start).rem_euclid(end - start),
            None => frame,
        };
        if frame < 0 || frame >= self.frame_count as i64 {
            return 0.0;
        }
        self.samples[frame as usize * 2 + channel]
    }
}

struct TimeStretcher {
    source: PlaybackSource,
    configuration: RenderConfiguration,
    position: f64,
    hop: usize,
    search: usize,
    overlap: Vec<f32>,
    overlap_reference: Vec<f32>,
    correlation_channel: usize,
    initialized: bool,
}

impl TimeStretcher {
    fn new(configuration: RenderConfiguration) -> Self {
        let hop = (configuration.output_rate as f64 * 0.02).round().max(1.0) as usize;
        let position = configuration.start * configuration.output_rate as f64;
        Self {
            source: PlaybackSource::new(&configuration),
            position,
            hop,
            search: hop / 2,
            overlap: vec![0.0; hop * 2],
            overlap_reference: vec![0.0; hop.div_ceil(8)],
            correlation_channel: 0,
            initialized: false,
            configuration,
        }
    }

    fn next_block(&mut self) -> Option<RenderedBlock> {
        let rate = self.configuration.output_rate as f64;
        let mut end = self.configuration.duration * rate;
        if let Some((start, loop_end)) = self.configuration.loop_range {
            end = loop_end * rate;
            if self.position < start * rate || self.position >= end - 1e-5 {
                self.position = start * rate;
            }
        }
        if self.position >= end - 1e-5 {
            return None;
        }
        let speed = self.configuration.speed as f64;
        let frames = self
            .hop
            .min(((end - self.position) / speed).ceil() as usize);
        if frames == 0 {
            return None;
        }
        let start = self.position / rate;
        let candidate = if self.initialized && (speed - 1.0).abs() > 0.0001 {
            self.aligned_position()
        } else {
            self.position
        };
        let mut samples = Vec::with_capacity(frames * 2);
        for frame in 0..frames {
            let weight = (0.5 - 0.5 * (PI * frame as f64 / self.hop as f64).cos()) as f32;
            for channel in 0..2 {
                let current = self.source.sample(candidate + frame as f64, channel);
                let sample = if self.initialized && (speed - 1.0).abs() > 0.0001 {
                    self.overlap[frame * 2 + channel] * (1.0 - weight) + current * weight
                } else {
                    current
                };
                samples.push(sample);
            }
        }
        if (speed - 1.0).abs() > 0.0001 {
            for frame in 0..self.hop {
                let position = candidate + (self.hop + frame) as f64;
                self.overlap[frame * 2] = self.source.sample(position, 0);
                self.overlap[frame * 2 + 1] = self.source.sample(position, 1);
            }
            let energy = self
                .overlap
                .chunks_exact(2)
                .fold([0.0; 2], |mut energy, frame| {
                    energy[0] += frame[0] * frame[0];
                    energy[1] += frame[1] * frame[1];
                    energy
                });
            self.correlation_channel = usize::from(energy[1] > energy[0]);
            for (index, value) in self.overlap_reference.iter_mut().enumerate() {
                *value = self.overlap[index * 16 + self.correlation_channel];
            }
        }
        self.initialized = true;
        self.position = (self.position + frames as f64 * speed).min(end);
        Some(RenderedBlock {
            samples,
            consumed: 0,
            start,
            end: self.position / rate,
        })
    }

    fn aligned_position(&self) -> f64 {
        let (region_start, region_end) = self.configuration.loop_range.map_or(
            (
                0,
                (self.configuration.duration * self.configuration.output_rate as f64).ceil() as i64
                    - 1,
            ),
            |(start, end)| {
                (
                    (start * self.configuration.output_rate as f64).ceil() as i64,
                    (end * self.configuration.output_rate as f64).ceil() as i64 - 1,
                )
            },
        );
        let nominal = (self.position.round() as i64).clamp(region_start, region_end);
        let lower = (nominal - self.search as i64).max(region_start);
        let upper = (nominal + self.search as i64).min(region_end);
        let energy = self
            .overlap_reference
            .iter()
            .map(|sample| sample * sample)
            .sum::<f32>();
        if energy < 1e-8 {
            return self.position;
        }
        let mut best_position = nominal;
        let mut best_score = f32::NEG_INFINITY;
        for candidate in (lower..=upper).step_by(8) {
            let score = self.correlation_score(candidate, nominal, energy);
            if score > best_score {
                best_score = score;
                best_position = candidate;
            }
        }
        let refined_start = (best_position - 7).max(lower);
        let refined_end = (best_position + 7).min(upper);
        for candidate in refined_start..=refined_end {
            let score = self.correlation_score(candidate, nominal, energy);
            if score > best_score {
                best_score = score;
                best_position = candidate;
            }
        }
        best_position as f64
    }

    fn correlation_score(&self, candidate: i64, nominal: i64, overlap_energy: f32) -> f32 {
        let mut dot = 0.0;
        let mut energy = 0.0;
        for (index, &previous) in self.overlap_reference.iter().enumerate() {
            let current = self.source.correlation_sample(
                candidate as f64 + (index * 8) as f64,
                self.correlation_channel,
            );
            dot += previous * current;
            energy += current * current;
        }
        dot / (overlap_energy * energy).sqrt().max(1e-12)
            - (candidate - nominal).abs() as f32 / self.hop as f32 * 0.001
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_clip(sample_rate: u32, frequency: f64, seconds: f64) -> AudioClip {
        let samples = (0..(sample_rate as f64 * seconds) as usize)
            .flat_map(|frame| {
                let sample =
                    (2.0 * PI * frequency * frame as f64 / sample_rate as f64).sin() as f32 * 0.5;
                [sample, sample]
            })
            .collect();
        AudioClip {
            path: PathBuf::new(),
            title: "Sine".into(),
            samples: Arc::new(samples),
            sample_rate,
            duration: seconds,
            waveform: Vec::new(),
        }
    }

    fn configuration(clip: &AudioClip, speed: f32) -> RenderConfiguration {
        RenderConfiguration {
            samples: clip.samples.clone(),
            sample_rate: clip.sample_rate,
            output_rate: clip.sample_rate,
            duration: clip.duration,
            speed,
            start: 0.0,
            loop_range: None,
        }
    }

    #[test]
    fn slowdown_preserves_pitch_and_changes_duration() {
        let clip = sine_clip(24_000, 440.0, 1.0);
        for speed in [0.25, 0.5, 0.75, 1.0, 1.5] {
            let mut stretcher = TimeStretcher::new(configuration(&clip, speed));
            let mut output = Vec::new();
            while let Some(block) = stretcher.next_block() {
                output.extend(block.samples.chunks_exact(2).map(|frame| frame[0]));
            }
            let expected_frames = (clip.sample_rate as f64 / speed as f64).ceil() as usize;
            assert!(output.len().abs_diff(expected_frames) <= 1);
            let start = clip.sample_rate as usize / 10;
            let end = output.len() - clip.sample_rate as usize / 10;
            let crossings = output[start..end]
                .windows(2)
                .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
                .count();
            let frequency = crossings as f64 * clip.sample_rate as f64 / (end - start) as f64;
            assert!(
                (frequency - 440.0).abs() < 3.0,
                "speed {speed}: {frequency}"
            );
        }
    }

    #[test]
    fn mono_resampling_rejects_aliases() {
        let audible = sine_clip(48_000, 1_000.0, 0.2).mono_16khz();
        let ultrasonic = sine_clip(48_000, 12_000.0, 0.2).mono_16khz();
        let rms = |samples: &[f32]| {
            let samples = &samples[100..samples.len() - 100];
            (samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32)
                .sqrt()
        };
        assert_eq!(audible.len(), 3200);
        assert!((rms(&audible) - 0.35355).abs() < 0.003);
        assert!(rms(&ultrasonic) < 0.002);
    }

    #[test]
    fn silent_loop_excludes_external_audio_at_every_speed() {
        let mut clip = sine_clip(24_000, 440.0, 0.5);
        let start = 0.2;
        let end = 0.3;
        let samples = Arc::make_mut(&mut clip.samples);
        for (index, frame) in samples.chunks_exact_mut(2).enumerate() {
            let seconds = index as f64 / clip.sample_rate as f64;
            let amplitude = if (start..end).contains(&seconds) {
                0.0
            } else {
                1.0
            };
            frame.fill(amplitude);
        }
        for speed in [0.25, 0.5, 0.75, 1.0, 1.25, 1.5] {
            for output_rate in [24_000, 32_000] {
                let mut settings = configuration(&clip, speed);
                settings.output_rate = output_rate;
                settings.start = start;
                settings.loop_range = Some((start, end));
                let mut stretcher = TimeStretcher::new(settings);
                for _ in 0..45 {
                    let block = stretcher.next_block().unwrap();
                    assert!(block.start >= start && block.end <= end);
                    assert!(
                        block.samples.iter().all(|sample| *sample == 0.0),
                        "external audio entered loop at speed {speed}, output {output_rate}"
                    );
                }
            }
        }
    }

    #[test]
    fn loop_overlap_and_correlation_ignore_external_impulses() {
        let mut isolated = sine_clip(24_000, 440.0, 0.5);
        let start = 0.200013;
        let end = 0.227017;
        let first = (start * isolated.sample_rate as f64).ceil() as usize;
        let after = (end * isolated.sample_rate as f64).ceil() as usize;
        let samples = Arc::make_mut(&mut isolated.samples);
        samples[..first * 2].fill(0.0);
        samples[after * 2..].fill(0.0);
        let mut contaminated = isolated.clone();
        let samples = Arc::make_mut(&mut contaminated.samples);
        for frame in [first - 1, after, after + 1, after + 8] {
            samples[frame * 2..frame * 2 + 2].fill(1.0);
        }
        for speed in [0.25, 0.5, 0.75, 1.0, 1.25, 1.5] {
            let mut settings = configuration(&isolated, speed);
            settings.start = start;
            settings.loop_range = Some((start, end));
            let mut reference = TimeStretcher::new(settings.clone());
            settings.samples = contaminated.samples.clone();
            let mut actual = TimeStretcher::new(settings);
            for _ in 0..24 {
                let expected = reference.next_block().unwrap();
                let observed = actual.next_block().unwrap();
                assert_eq!(expected.samples, observed.samples, "speed {speed}");
                assert_eq!(expected.start, observed.start);
                assert_eq!(expected.end, observed.end);
            }
        }
    }

    #[test]
    fn renderer_starts_at_seek_and_wraps_loop() {
        let clip = sine_clip(24_000, 440.0, 1.0);
        let mut settings = configuration(&clip, 0.5);
        settings.start = 0.35;
        settings.loop_range = Some((0.2, 0.4));
        let mut stretcher = TimeStretcher::new(settings);
        let first = stretcher.next_block().unwrap();
        assert!((first.start - 0.35).abs() < 1e-9);
        let mut wrapped = false;
        for _ in 0..50 {
            let block = stretcher.next_block().unwrap();
            assert!(block.start >= 0.2 - 1e-9 && block.end <= 0.4 + 1e-9);
            if (block.start - 0.2).abs() < 1e-9 {
                wrapped = true;
            }
        }
        assert!(wrapped);
    }
}
