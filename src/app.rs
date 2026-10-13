use crate::audio::{self, AudioClip, AudioPlayer};
use crate::design;
use crate::design::Icon;
use crate::export;
use crate::harmony::{self, Chord};
use crate::runtime::{self, InstallEvent};
use crate::score::{self, ScoreSettings};
use crate::tablature;
use crate::transcription::{self, Note, Transcription, TranscriptionEvent};
use crate::waveform;
use eframe::egui::{self, Align, Align2, Color32, FontId, Pos2, RichText, Sense, Stroke, Vec2};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

const BACKGROUND: Color32 = Color32::WHITE;
const PANEL: Color32 = Color32::WHITE;
const TEXT: Color32 = Color32::BLACK;

type LoadedAudio = Result<(AudioClip, f64), String>;

#[derive(serde::Serialize, serde::Deserialize)]
struct PracticeSettings {
    #[serde(default)]
    midi_mode: bool,
    #[serde(default = "default_chords_visible")]
    show_chords: bool,
    #[serde(default)]
    tablature: bool,
    #[serde(default)]
    selected_instrument: Option<(u8, bool)>,
    bpm: f64,
    beats_per_bar: u8,
    subdivisions: u8,
    offset: f64,
    follow: bool,
    speed: f32,
    volume: f32,
    loop_start: f64,
    loop_end: f64,
    loop_enabled: bool,
}

pub struct TranscribeApp {
    midi_mode: bool,
    midi_loaded_instrument: Option<(u8, bool)>,
    show_chords: bool,
    chords: Vec<Chord>,
    install_receiver: Option<mpsc::Receiver<InstallEvent>>,
    install_cancelled: Arc<AtomicBool>,
    installing: bool,
    show_licenses: bool,
    show_instrument_settings: bool,
    show_score_settings: bool,
    preview_menu: bool,
    tablature: bool,
    clip: Option<AudioClip>,
    player: Option<AudioPlayer>,
    library: Vec<PathBuf>,
    loaded_receiver: Option<mpsc::Receiver<LoadedAudio>>,
    transcription_receiver: Option<mpsc::Receiver<TranscriptionEvent>>,
    cancelled: Arc<AtomicBool>,
    transcription: Option<Transcription>,
    selected_instrument: Option<(u8, bool)>,
    score_settings: ScoreSettings,
    speed: f32,
    volume: f32,
    loop_start: f64,
    loop_end: f64,
    loop_enabled: bool,
    waveform_zoom: f64,
    waveform_start: f64,
    conditioning: usize,
    progress: f32,
    status: String,
    loading: bool,
    busy: bool,
    tap_times: Vec<Instant>,
    screenshot_path: Option<PathBuf>,
    screenshot_requested: bool,
    started_at: Instant,
}

impl TranscribeApp {
    pub fn new(creation: &eframe::CreationContext<'_>) -> Self {
        design::configure(&creation.egui_ctx);
        let arguments: Vec<String> = std::env::args().collect();
        let screenshot_path = arguments
            .iter()
            .position(|argument| argument == "--screenshot")
            .and_then(|index| arguments.get(index + 1))
            .map(PathBuf::from);
        let preview = screenshot_path
            .as_ref()
            .and_then(|_| {
                arguments
                    .iter()
                    .position(|argument| argument == "--preview")
                    .and_then(|index| arguments.get(index + 1))
            })
            .map(String::as_str);
        let mut application = Self {
            midi_mode: false,
            midi_loaded_instrument: None,
            show_chords: true,
            chords: Vec::new(),
            install_receiver: None,
            install_cancelled: Arc::new(AtomicBool::new(false)),
            installing: false,
            show_licenses: false,
            show_instrument_settings: preview == Some("instruments"),
            show_score_settings: preview == Some("reading"),
            preview_menu: preview == Some("menu"),
            tablature: false,
            clip: None,
            player: None,
            library: audio_library(),
            loaded_receiver: None,
            transcription_receiver: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            transcription: None,
            selected_instrument: None,
            score_settings: ScoreSettings::default(),
            speed: 1.0,
            volume: 0.8,
            loop_start: 0.0,
            loop_end: 0.0,
            loop_enabled: false,
            waveform_zoom: 1.0,
            waveform_start: 0.0,
            conditioning: 0,
            progress: 0.0,
            status: "Pronto".into(),
            loading: false,
            busy: false,
            tap_times: Vec::new(),
            screenshot_path,
            screenshot_requested: false,
            started_at: Instant::now(),
        };
        let requested_audio = arguments
            .iter()
            .position(|argument| argument == "--audio")
            .and_then(|index| arguments.get(index + 1))
            .map(PathBuf::from)
            .map(|path| {
                if path.is_relative() {
                    runtime::application_root().join(path)
                } else {
                    path
                }
            });
        if let Some(path) = requested_audio.or_else(|| application.library.first().cloned()) {
            application.open_audio(path);
        }
        application
    }

    fn open_audio(&mut self, path: PathBuf) {
        self.save_practice();
        self.cancelled.store(true, Ordering::Relaxed);
        self.transcription_receiver = None;
        self.busy = false;
        if let Some(player) = self.player.as_mut() {
            player.pause();
        }
        self.transcription = transcription::load_cached(&path);
        self.chords = self
            .transcription
            .as_ref()
            .map_or_else(Vec::new, |transcription| {
                harmony::analyze_chords(&transcription.notes)
            });
        self.midi_loaded_instrument = None;
        self.choose_initial_instrument();
        self.clip = None;
        self.loading = true;
        self.status = "Apertura audio...".into();
        let (sender, receiver) = mpsc::channel();
        self.loaded_receiver = Some(receiver);
        std::thread::spawn(move || {
            let loaded = audio::load_audio(&path).map(|clip| {
                let bpm = estimate_tempo(&clip);
                (clip, bpm)
            });
            let _ = sender.send(loaded);
        });
    }

    fn choose_initial_instrument(&mut self) {
        let instruments = self.instruments();
        self.selected_instrument = instruments
            .iter()
            .filter(|(program, drums, _)| !*drums && (24..=31).contains(program))
            .max_by_key(|(_, _, count)| *count)
            .or_else(|| instruments.first())
            .map(|(program, drums, _)| (*program, *drums));
    }

    fn poll_workers(&mut self) {
        if let Some(result) = self
            .loaded_receiver
            .as_ref()
            .and_then(|receiver| receiver.try_recv().ok())
        {
            self.loaded_receiver = None;
            self.loading = false;
            match result {
                Ok((clip, bpm)) => {
                    if self.player.is_none() {
                        match AudioPlayer::new() {
                            Ok(player) => self.player = Some(player),
                            Err(error) => self.status = error,
                        }
                    }
                    if let Some(player) = self.player.as_mut() {
                        player.load(&clip);
                        player.set_speed(self.speed);
                        player.set_volume(self.volume);
                        player.set_loop(None);
                        self.status = if self.transcription.is_some() {
                            "Trascrizione salvata caricata"
                        } else {
                            "Audio pronto · avvia la trascrizione"
                        }
                        .into();
                    }
                    self.loop_start = 0.0;
                    self.loop_end = clip.duration.min(15.0);
                    self.loop_enabled = false;
                    self.waveform_start = 0.0;
                    self.waveform_zoom = 1.0;
                    self.score_settings.bpm = bpm;
                    self.score_settings.offset = 0.0;
                    self.clip = Some(clip);
                    self.restore_practice();
                    if self.transcription.is_none() {
                        self.midi_mode = false;
                    }
                }
                Err(error) => self.status = error,
            }
        }
        let events: Vec<_> = self
            .transcription_receiver
            .as_ref()
            .map(|receiver| receiver.try_iter().collect())
            .unwrap_or_default();
        for event in events {
            match event {
                TranscriptionEvent::Progress(progress, status) => {
                    self.progress = progress;
                    self.status = status;
                }
                TranscriptionEvent::Finished(transcription) => {
                    self.chords = harmony::analyze_chords(&transcription.notes);
                    self.midi_loaded_instrument = None;
                    self.transcription = Some(transcription);
                    self.choose_initial_instrument();
                    self.progress = 1.0;
                    self.busy = false;
                    self.status = "Trascrizione completata e salvata".into();
                    self.transcription_receiver = None;
                }
                TranscriptionEvent::Failed(error) => {
                    self.busy = false;
                    self.status = error;
                    self.transcription_receiver = None;
                }
            }
        }
        let install_events: Vec<_> = self
            .install_receiver
            .as_ref()
            .map(|receiver| receiver.try_iter().collect())
            .unwrap_or_default();
        for event in install_events {
            match event {
                InstallEvent::Progress(progress, status) => {
                    self.progress = progress;
                    self.status = status;
                }
                InstallEvent::Finished => {
                    self.installing = false;
                    self.install_receiver = None;
                    self.status = "Motore AI installato. Puoi trascrivere l'audio.".into();
                }
                InstallEvent::FinishedWithWarning(message) => {
                    self.installing = false;
                    self.install_receiver = None;
                    self.status = message;
                }
                InstallEvent::Failed(error) => {
                    self.installing = false;
                    self.install_receiver = None;
                    self.status = error;
                }
            }
        }
    }

    fn start_engine_installation(&mut self) {
        self.install_cancelled = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        self.install_receiver = Some(receiver);
        self.installing = true;
        self.progress = 0.0;
        self.status = "Preparazione del motore AI...".into();
        runtime::spawn_installation(sender, self.install_cancelled.clone());
    }

    fn sync_midi_track(&mut self) {
        if self.clip.is_none() {
            return;
        }
        if self.midi_loaded_instrument != self.selected_instrument {
            let notes = self.selected_notes();
            if let Some(player) = self.player.as_mut() {
                player.set_midi_notes(&notes);
            }
            self.midi_loaded_instrument = self.selected_instrument;
        }
        if let Some(player) = self.player.as_mut() {
            player.set_midi_mode(self.midi_mode && self.selected_instrument.is_some());
        }
    }

    fn instruments(&self) -> Vec<(u8, bool, usize)> {
        let mut instruments = Vec::new();
        if let Some(transcription) = &self.transcription {
            for note in &transcription.notes {
                if let Some((_, _, count)) = instruments
                    .iter_mut()
                    .find(|(program, drums, _)| *program == note.program && *drums == note.is_drum)
                {
                    *count += 1;
                } else {
                    instruments.push((note.program, note.is_drum, 1));
                }
            }
        }
        instruments.sort_by_key(|(program, drums, _)| (*drums, *program));
        instruments
    }

    fn selected_notes(&self) -> Vec<Note> {
        match (&self.transcription, self.selected_instrument) {
            (Some(transcription), Some((program, drums))) => transcription
                .notes
                .iter()
                .filter(|note| note.program == program && note.is_drum == drums)
                .cloned()
                .collect(),
            _ => Vec::new(),
        }
    }

    fn start_transcription(&mut self) {
        let Some(clip) = &self.clip else { return };
        if !transcription::engine_ready() {
            self.status = "Installa il motore AI dal pulsante in alto.".into();
            return;
        }
        let clip = clip.clone();
        let instruments = match self.conditioning {
            1 => vec!["clean_electric_guitar".into()],
            2 => vec![
                "clean_electric_guitar".into(),
                "acoustic_bass".into(),
                "acoustic_piano".into(),
                "drums".into(),
            ],
            _ => Vec::new(),
        };
        self.cancelled = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = mpsc::channel();
        self.transcription_receiver = Some(receiver);
        self.busy = true;
        self.progress = 0.0;
        self.status = "Preparazione audio per MuScriptor...".into();
        let cancelled = self.cancelled.clone();
        std::thread::spawn(move || {
            let samples = clip.mono_16khz();
            transcription::spawn_transcription(
                clip.path,
                samples,
                clip.duration,
                instruments,
                sender,
                cancelled,
            );
        });
    }

    fn update_loop(&mut self) {
        if self.loop_end - self.loop_start < 0.02 {
            self.loop_enabled = false;
        }
        if let Some(player) = self.player.as_mut() {
            player.set_loop(
                self.loop_enabled
                    .then_some((self.loop_start, self.loop_end)),
            );
        }
    }

    fn save_practice(&self) {
        let Some(clip) = &self.clip else {
            return;
        };
        let settings = PracticeSettings {
            midi_mode: self.midi_mode,
            show_chords: self.show_chords,
            tablature: self.tablature,
            selected_instrument: self.selected_instrument,
            bpm: self.score_settings.bpm,
            beats_per_bar: self.score_settings.beats_per_bar,
            subdivisions: self.score_settings.subdivisions,
            offset: self.score_settings.offset,
            follow: self.score_settings.follow,
            speed: self.speed,
            volume: self.volume,
            loop_start: self.loop_start,
            loop_end: self.loop_end,
            loop_enabled: self.loop_enabled,
        };
        let path = transcription::cache_path(&clip.path).with_extension("practice.json");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(serialized) = serde_json::to_vec_pretty(&settings) {
            let _ = std::fs::write(path, serialized);
        }
    }

    fn restore_practice(&mut self) {
        let Some(clip) = &self.clip else {
            return;
        };
        let path = transcription::cache_path(&clip.path).with_extension("practice.json");
        let settings = std::fs::read(path)
            .ok()
            .and_then(|contents| serde_json::from_slice::<PracticeSettings>(&contents).ok());
        if let Some(settings) = settings {
            self.midi_mode = settings.midi_mode;
            self.show_chords = settings.show_chords;
            self.tablature = settings.tablature;
            if let Some((program, drums)) = settings.selected_instrument
                && self
                    .instruments()
                    .iter()
                    .any(|(candidate, is_drum, _)| *candidate == program && *is_drum == drums)
            {
                self.selected_instrument = Some((program, drums));
            }
            self.score_settings.bpm = settings.bpm.clamp(30.0, 300.0);
            self.score_settings.beats_per_bar = settings.beats_per_bar.clamp(3, 7);
            self.score_settings.subdivisions = settings.subdivisions.clamp(2, 6);
            self.score_settings.offset = settings.offset.clamp(0.0, clip.duration);
            self.score_settings.follow = settings.follow;
            self.speed = settings.speed.clamp(0.25, 1.5);
            self.volume = settings.volume.clamp(0.0, 1.0);
            self.loop_start = settings.loop_start.clamp(0.0, clip.duration);
            self.loop_end = settings.loop_end.clamp(0.0, clip.duration);
            self.loop_enabled = settings.loop_enabled;
            if let Some(player) = self.player.as_mut() {
                player.set_midi_mode(self.midi_mode);
                player.set_speed(self.speed);
                player.set_volume(self.volume);
            }
            self.update_loop();
        }
    }

    fn seek(&mut self, position: f64) {
        if let Some(player) = self.player.as_mut() {
            player.seek(position);
        }
    }

    fn position(&self) -> f64 {
        self.player.as_ref().map_or(0.0, AudioPlayer::position)
    }

    fn keyboard(&mut self, context: &egui::Context) {
        if context.wants_keyboard_input()
            || self.show_instrument_settings
            || self.show_score_settings
            || self.show_licenses
            || self.loading
            || self.clip.is_none()
        {
            return;
        }
        let position = self.position();
        if context.input(|input| input.key_pressed(egui::Key::Space))
            && let Some(player) = self.player.as_mut()
        {
            player.toggle();
        }
        if context.input(|input| input.key_pressed(egui::Key::ArrowLeft)) {
            self.seek((position - 5.0).max(0.0));
        }
        if context.input(|input| input.key_pressed(egui::Key::ArrowRight)) {
            self.seek(position + 5.0);
        }
        if context.input(|input| input.key_pressed(egui::Key::Home)) {
            self.seek(0.0);
        }
        if context.input(|input| input.key_pressed(egui::Key::I)) {
            (self.loop_start, self.loop_end) = keyboard_loop_range(
                self.loop_start,
                self.loop_end,
                position,
                self.clip.as_ref().map_or(0.0, |clip| clip.duration),
                true,
            );
            self.update_loop();
        }
        if context.input(|input| input.key_pressed(egui::Key::O)) {
            (self.loop_start, self.loop_end) = keyboard_loop_range(
                self.loop_start,
                self.loop_end,
                position,
                self.clip.as_ref().map_or(0.0, |clip| clip.duration),
                false,
            );
            self.loop_enabled = self.loop_end > self.loop_start;
            self.update_loop();
        }
        if context.input(|input| input.key_pressed(egui::Key::L)) {
            self.loop_enabled = !self.loop_enabled;
            self.update_loop();
        }
    }

    fn open_audio_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Audio", &["mp3", "wav", "flac", "ogg", "m4a", "aiff"])
            .pick_file()
        {
            if !self.library.contains(&path) {
                self.library.push(path.clone());
            }
            self.open_audio(path);
        }
    }

    fn sidebar(&mut self, context: &egui::Context) {
        egui::SidePanel::left("library")
            .exact_width(244.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(22.0))
            .show(context, |ui| {
                ui.add_space(8.0);
                ui.label(RichText::new("Transcriber!").size(31.0).color(TEXT));
                ui.add_space(32.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Libreria").size(20.0));
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        if design::icon_button(ui, Icon::Folder, "Apri audio", false).clicked() {
                            self.open_audio_dialog();
                        }
                    });
                });
                ui.add_space(12.0);
                for path in self.library.clone() {
                    let selected = self.clip.as_ref().is_some_and(|clip| clip.path == path);
                    let title = path.file_stem().unwrap_or_default().to_string_lossy();
                    let response = ui.add_sized(
                        [ui.available_width(), 48.0],
                        design::button(&title).selected(selected).wrap(),
                    );
                    if response.clicked() && !selected && !self.loading {
                        self.open_audio(path);
                    }
                }
                ui.add_space(24.0);
                ui.separator();
                ui.add_space(18.0);
                ui.label(RichText::new("Parti").size(20.0));
                ui.add_space(10.0);
                let instruments = self.instruments();
                if instruments.is_empty() {
                    ui.label(RichText::new("Nessuna parte trascritta").size(13.0));
                }
                for (program, drums, _) in instruments {
                    let selected = self.selected_instrument == Some((program, drums));
                    let name = score::instrument_name(program, drums);
                    let label =
                        RichText::new(name).color(if selected { Color32::WHITE } else { TEXT });
                    if ui
                        .add_sized(
                            [ui.available_width(), 36.0],
                            egui::Button::new(label).selected(selected),
                        )
                        .clicked()
                    {
                        self.selected_instrument = Some((program, drums));
                    }
                }
            });
    }

    fn transport(&mut self, context: &egui::Context) {
        let active = self.busy || self.installing || self.loading;
        let idle_status = matches!(
            self.status.as_str(),
            "Pronto"
                | "Trascrizione salvata caricata"
                | "Audio pronto · avvia la trascrizione"
                | "Trascrizione completata e salvata"
                | "Motore AI installato. Puoi trascrivere l'audio."
        );
        let show_status = active || !idle_status;
        egui::TopBottomPanel::bottom("transport")
            .exact_height(if self.busy || self.installing {
                120.0
            } else if show_status {
                100.0
            } else {
                68.0
            })
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(30, 16)),
            )
            .show(context, |ui| {
                ui.horizontal(|ui| {
                    let playing = self.player.as_ref().is_some_and(AudioPlayer::is_playing);
                    let enabled = self.clip.is_some() && self.player.is_some();
                    ui.add_enabled_ui(enabled, |ui| {
                        if design::icon_button(
                            ui,
                            Icon::Backward,
                            "Indietro di 5 secondi · ←",
                            false,
                        )
                        .clicked()
                        {
                            self.seek((self.position() - 5.0).max(0.0));
                        }
                        if design::icon_button(
                            ui,
                            if playing { Icon::Pause } else { Icon::Play },
                            if playing {
                                "Pausa · Spazio"
                            } else {
                                "Riproduci · Spazio"
                            },
                            false,
                        )
                        .clicked()
                            && let Some(player) = self.player.as_mut()
                        {
                            player.toggle();
                        }
                        if design::icon_button(ui, Icon::Forward, "Avanti di 5 secondi · →", false)
                            .clicked()
                        {
                            self.seek(self.position() + 5.0);
                        }
                    });
                    ui.add_space(10.0);
                    ui.add_sized(
                        [98.0, 36.0],
                        egui::Label::new(
                            RichText::new(time_label(self.position()))
                                .size(23.0)
                                .monospace(),
                        ),
                    );
                    ui.add_space(12.0);
                    if design::icon_button(
                        ui,
                        Icon::Audio,
                        "Ascolta la registrazione",
                        !self.midi_mode,
                    )
                    .clicked()
                    {
                        self.midi_mode = false;
                        if let Some(player) = self.player.as_mut() {
                            player.set_midi_mode(false);
                        }
                    }
                    ui.add_enabled_ui(
                        self.selected_instrument.is_some() && self.player.is_some(),
                        |ui| {
                            if design::icon_button(
                                ui,
                                Icon::Midi,
                                "Ascolta la parte in MIDI",
                                self.midi_mode,
                            )
                            .clicked()
                            {
                                self.midi_mode = true;
                                if let Some(player) = self.player.as_mut() {
                                    player.set_midi_mode(true);
                                }
                            }
                        },
                    );
                    ui.add_space(18.0);
                    design::icon_label(ui, Icon::Speed, "Velocità · intonazione invariata");
                    ui.scope(|ui| {
                        ui.spacing_mut().slider_width = 130.0;
                        if design::slider(
                            ui,
                            egui::Slider::new(&mut self.speed, 0.25..=1.5)
                                .step_by(0.05)
                                .show_value(false),
                        )
                        .changed()
                            && let Some(player) = self.player.as_mut()
                        {
                            player.set_speed(self.speed);
                        }
                    });
                    let speed = ui
                        .add_sized(
                            [64.0, 36.0],
                            egui::DragValue::new(&mut self.speed)
                                .range(0.25..=1.5)
                                .speed(0.01)
                                .suffix("×")
                                .fixed_decimals(2),
                        )
                        .on_hover_text("Velocità di riproduzione");
                    if speed.changed()
                        && let Some(player) = self.player.as_mut()
                    {
                        player.set_speed(self.speed);
                    }
                    ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                        ui.scope(|ui| {
                            ui.spacing_mut().slider_width = 110.0;
                            if design::slider(
                                ui,
                                egui::Slider::new(&mut self.volume, 0.0..=1.0).show_value(false),
                            )
                            .changed()
                                && let Some(player) = self.player.as_mut()
                            {
                                player.set_volume(self.volume);
                            }
                        });
                        design::icon_label(ui, Icon::Volume, "Volume");
                    });
                });
                if show_status {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&self.status).size(12.0));
                        if self.busy || self.installing {
                            ui.add(
                                egui::ProgressBar::new(self.progress)
                                    .desired_width(150.0)
                                    .show_percentage(),
                            );
                            if design::icon_button(ui, Icon::Cancel, "Annulla", false).clicked() {
                                if self.installing {
                                    self.install_cancelled.store(true, Ordering::Relaxed);
                                } else {
                                    self.cancelled.store(true, Ordering::Relaxed);
                                }
                                self.status = "Annullamento...".into();
                            }
                        }
                        if self.player.as_ref().is_some_and(AudioPlayer::is_processing) {
                            ui.spinner();
                        }
                    });
                }
            });
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let menu_button = design::icon_button(ui, Icon::Menu, "Menu", false);
                if self.preview_menu {
                    egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&menu_button));
                    self.preview_menu = false;
                }
                egui::Popup::menu(&menu_button)
                    .width(280.0)
                    .show(|ui| self.action_menu(ui));
                let title = self
                    .clip
                    .as_ref()
                    .map_or("Il tuo prossimo ascolto", |clip| clip.title.as_str());
                left_heading(ui, title, 31.0, 42.0);
            });
        });
        ui.add_space(14.0);
    }

    fn action_menu(&mut self, ui: &mut egui::Ui) {
        let available = !self.busy && !self.installing && !self.loading;
        let engine_ready = transcription::engine_ready();
        let caption = if !engine_ready {
            "Installa AI"
        } else if self.transcription.is_some() {
            "Ritrascrivi audio"
        } else {
            "Trascrivi audio"
        };
        if menu_action(
            ui,
            Icon::Transcribe,
            caption,
            available && (self.clip.is_some() || !engine_ready),
        ) {
            if engine_ready {
                self.start_transcription();
            } else {
                self.start_engine_installation();
            }
        }
        if menu_action(ui, Icon::Instruments, "Modalità strumenti…", available) {
            self.show_instrument_settings = true;
        }
        ui.separator();
        if menu_action(
            ui,
            Icon::ExportMidi,
            "Esporta MIDI",
            self.transcription.is_some(),
        ) {
            self.export_midi();
        }
        if menu_action(
            ui,
            Icon::ExportScore,
            "Esporta MusicXML",
            self.selected_instrument.is_some(),
        ) {
            self.export_score();
        }
        ui.separator();
        if menu_action(ui, Icon::Settings, "Impostazioni di lettura…", true) {
            self.show_score_settings = true;
        }
        if menu_action(ui, Icon::Info, "Font e licenze…", true) {
            self.show_licenses = true;
        }
    }

    fn change_waveform_zoom(&mut self, zoom: f64) {
        self.waveform_zoom = zoom.clamp(1.0, 16.0);
        if let Some(clip) = &self.clip {
            let window = clip.duration / self.waveform_zoom;
            self.waveform_start =
                (self.position() - window * 0.25).clamp(0.0, (clip.duration - window).max(0.0));
        }
    }

    fn waveform(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.add_enabled_ui(self.clip.is_some(), |ui| {
                    if design::icon_button(
                        ui,
                        Icon::Repeat,
                        "Ripeti selezione · L",
                        self.loop_enabled,
                    )
                    .clicked()
                    {
                        self.loop_enabled = !self.loop_enabled;
                        self.update_loop();
                    }
                    if design::icon_button(ui, Icon::ZoomIn, "Ingrandisci waveform", false)
                        .clicked()
                    {
                        self.change_waveform_zoom(self.waveform_zoom * 2.0);
                    }
                    if design::icon_button(ui, Icon::Fit, "Mostra tutto il brano", false).clicked()
                    {
                        self.change_waveform_zoom(1.0);
                    }
                    if design::icon_button(ui, Icon::ZoomOut, "Riduci waveform", false).clicked() {
                        self.change_waveform_zoom(self.waveform_zoom * 0.5);
                    }
                });
            });
        });
        ui.add_space(10.0);
        let position = self.position();
        let action = waveform::show(
            ui,
            self.clip.as_ref(),
            position,
            self.waveform_zoom,
            &mut self.waveform_start,
            &mut self.loop_start,
            &mut self.loop_end,
            self.loop_enabled,
        );
        if action.loop_changed {
            if action.loop_dragged {
                self.loop_enabled = true;
            }
            self.update_loop();
        }
        if let Some(position) = action.seek {
            self.seek(position);
        }
    }

    fn score_panel(&mut self, ui: &mut egui::Ui) {
        let notes = self.selected_notes();
        let tablature_supported = tablature::supports_tablature(&notes);
        let show_tablature = self.tablature && tablature_supported;
        ui.add_space(24.0);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if design::icon_button(ui, Icon::Settings, "Impostazioni di lettura", false)
                    .clicked()
                {
                    self.show_score_settings = true;
                }
                ui.add_enabled_ui(tablature_supported, |ui| {
                    if design::icon_button(ui, Icon::Tablature, "Tablatura", show_tablature)
                        .clicked()
                    {
                        self.tablature = true;
                    }
                });
                if design::icon_button(ui, Icon::Score, "Spartito", !show_tablature).clicked() {
                    self.tablature = false;
                }
                let title = self.selected_instrument.map_or_else(
                    || "Trascrizione",
                    |(program, drums)| score::instrument_name(program, drums),
                );
                left_heading(ui, title, 23.0, 36.0);
            });
        });
        ui.add_space(14.0);
        let position = self.position();
        if notes.is_empty() {
            let (rect, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), ui.available_height().max(180.0)),
                Sense::hover(),
            );
            let painter = ui.painter_at(rect);
            for index in 0..5 {
                let y = rect.center().y - 32.0 + index as f32 * 12.0;
                painter.line_segment(
                    [
                        Pos2::new(rect.left() + 30.0, y),
                        Pos2::new(rect.right() - 30.0, y),
                    ],
                    Stroke::new(0.7, Color32::BLACK),
                );
            }
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                if self.busy {
                    "La trascrizione è in corso"
                } else {
                    "Dall'ascolto alla pagina."
                },
                FontId::proportional(27.0),
                Color32::BLACK,
            );
        } else {
            let chords = if self.show_chords {
                self.chords.as_slice()
            } else {
                &[]
            };
            let action = if show_tablature {
                tablature::render_tablature(ui, &notes, &self.score_settings, position, chords)
            } else {
                score::render_score(ui, &notes, &self.score_settings, position, chords)
            };
            match action {
                Some(score::NotationAction::Seek(seconds)) => self.seek(seconds),
                Some(score::NotationAction::Preview { pitch, duration }) => {
                    if let Some(player) = self.player.as_mut() {
                        let (program, drums) = self.selected_instrument.unwrap_or((0, false));
                        player.preview_note(pitch, program, drums, duration.clamp(0.08, 1.5));
                    }
                }
                None => {}
            }
        }
    }

    fn instrument_settings(&mut self, context: &egui::Context) {
        if !self.show_instrument_settings {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("instrument_settings"))
            .frame(egui::Frame::popup(&context.style()).inner_margin(24.0))
            .show(context, |ui| {
                ui.set_width(450.0);
                modal_heading(ui, "Modalità strumenti");
                ui.add_space(14.0);
                for (index, label, description) in [
                    (
                        0,
                        "Automatico",
                        "Riconosce le parti presenti nella registrazione.",
                    ),
                    (
                        1,
                        "Chitarra elettrica",
                        "Concentra il riconoscimento sulla chitarra elettrica.",
                    ),
                    (
                        2,
                        "Quartetto jazz",
                        "Chitarra, contrabbasso, pianoforte e batteria.",
                    ),
                ] {
                    ui.radio_value(&mut self.conditioning, index, label);
                    ui.label(RichText::new(description).size(14.0));
                    ui.add_space(14.0);
                }
                ui.label(
                    RichText::new("La scelta si applica alla prossima trascrizione.").size(13.0),
                );
            });
        if response.should_close() {
            self.show_instrument_settings = false;
        }
    }

    fn tap_tempo(&mut self) {
        let now = Instant::now();
        if self
            .tap_times
            .last()
            .is_some_and(|last| now.duration_since(*last).as_secs_f64() > 2.0)
        {
            self.tap_times.clear();
        }
        self.tap_times.push(now);
        if self.tap_times.len() > 6 {
            self.tap_times.remove(0);
        }
        if self.tap_times.len() > 1 {
            let interval = now.duration_since(self.tap_times[0]).as_secs_f64()
                / (self.tap_times.len() - 1) as f64;
            self.score_settings.bpm = (60.0 / interval).clamp(30.0, 300.0);
        }
    }

    fn reading_settings(&mut self, context: &egui::Context) {
        if !self.show_score_settings {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("reading_settings"))
            .frame(egui::Frame::popup(&context.style()).inner_margin(24.0))
            .show(context, |ui| {
                ui.set_width(450.0);
                modal_heading(ui, "Impostazioni di lettura");
                ui.add_space(16.0);
                egui::Grid::new("reading_controls")
                    .num_columns(2)
                    .spacing(Vec2::new(20.0, 16.0))
                    .show(ui, |ui| {
                        ui.label("Tempo");
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [90.0, 36.0],
                                egui::DragValue::new(&mut self.score_settings.bpm)
                                    .range(30.0..=300.0)
                                    .speed(0.2)
                                    .suffix(" BPM")
                                    .fixed_decimals(1),
                            );
                            if design::icon_button(ui, Icon::Tap, "Batti il tempo", false).clicked()
                            {
                                self.tap_tempo();
                            }
                        });
                        ui.end_row();
                        ui.label("Metro");
                        egui::ComboBox::from_id_salt("meter")
                            .selected_text(format!("{}/4", self.score_settings.beats_per_bar))
                            .show_ui(ui, |ui| {
                                for beats in [3, 4, 5, 6, 7] {
                                    ui.selectable_value(
                                        &mut self.score_settings.beats_per_bar,
                                        beats,
                                        format!("{beats}/4"),
                                    );
                                }
                            });
                        ui.end_row();
                        ui.label("Griglia");
                        egui::ComboBox::from_id_salt("grid")
                            .selected_text(match self.score_settings.subdivisions {
                                2 => "Ottavi",
                                3 => "Terzine",
                                6 => "Sestine",
                                _ => "Sedicesimi",
                            })
                            .show_ui(ui, |ui| {
                                for (division, label) in [
                                    (2, "Ottavi"),
                                    (3, "Terzine"),
                                    (4, "Sedicesimi"),
                                    (6, "Sestine"),
                                ] {
                                    ui.selectable_value(
                                        &mut self.score_settings.subdivisions,
                                        division,
                                        label,
                                    );
                                }
                            });
                        ui.end_row();
                        ui.label("Primo battito");
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                [90.0, 36.0],
                                egui::DragValue::new(&mut self.score_settings.offset)
                                    .range(
                                        0.0..=self
                                            .clip
                                            .as_ref()
                                            .map_or(600.0, |clip| clip.duration),
                                    )
                                    .speed(0.01)
                                    .suffix(" s")
                                    .fixed_decimals(2),
                            );
                            if design::icon_button(
                                ui,
                                Icon::Downbeat,
                                "Usa la posizione di ascolto",
                                false,
                            )
                            .clicked()
                            {
                                self.score_settings.offset = self.position();
                            }
                        });
                        ui.end_row();
                    });
                ui.add_space(18.0);
                ui.checkbox(&mut self.score_settings.follow, "Segui ascolto");
                ui.checkbox(&mut self.show_chords, "Mostra accordi");
            });
        if response.should_close() {
            self.show_score_settings = false;
        }
    }

    fn export_midi(&mut self) {
        let Some(transcription) = &self.transcription else {
            return;
        };
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("transcription.mid")
            .add_filter("MIDI", &["mid"])
            .save_file()
        {
            self.status = match export::write_midi(&path, transcription, self.score_settings.bpm) {
                Ok(()) => format!("MIDI salvato: {}", path.display()),
                Err(error) => error,
            };
        }
    }

    fn export_score(&mut self) {
        let notes = self.selected_notes();
        let title = self
            .clip
            .as_ref()
            .map_or("Trascrizione", |clip| clip.title.as_str());
        if let Some(path) = rfd::FileDialog::new()
            .set_file_name("score.musicxml")
            .add_filter("MusicXML", &["musicxml"])
            .save_file()
        {
            self.status = match export::write_musicxml(&path, &notes, &self.score_settings, title) {
                Ok(()) => format!("Spartito salvato: {}", path.display()),
                Err(error) => error,
            };
        }
    }
}

impl eframe::App for TranscribeApp {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_workers();
        self.keyboard(context);
        self.sidebar(context);
        self.sync_midi_track();
        self.transport(context);
        if self.show_licenses {
            egui::Window::new("Font e licenze").open(&mut self.show_licenses).default_width(620.0).vscroll(true).show(context, |ui| {
                ui.label("Transcriber! · Petaluma Script / Petaluma, Steinberg · GeneralUser GS, S. Christian Collins · RustySynth, Nobuyuki Tanaka.");
                ui.label("MuScriptor: Kyutai e Mirelo. Il modello AI è CC BY-NC 4.0; i pesi hanno una licenza distinta dal codice dell'app.");
                ui.hyperlink_to("Progetto e download", runtime::RELEASE_URL);
                ui.separator();
                ui.label(include_str!("../assets/fonts/OFL-Petaluma.txt"));
                ui.separator();
                ui.label(include_str!("../assets/sounds/LICENSE-GeneralUser.txt"));
                ui.separator();
                ui.label(include_str!("../assets/licenses/THIRD-PARTY.md"));
                for license in [
                    include_str!("../assets/licenses/LICENSE-muscriptor.txt"),
                    include_str!("../assets/licenses/LICENSE-ggml.txt"),
                    include_str!("../assets/licenses/LICENSE-pffft.txt"),
                    include_str!("../assets/licenses/LICENSE-RustySynth.txt"),
                    include_str!("../assets/licenses/LICENSE-MuScriptor-model.txt"),
                    include_str!("../assets/licenses/NOTICE-MuScriptor-model.md"),
                ] {
                    ui.separator();
                    ui.label(license);
                }
            });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BACKGROUND)
                    .inner_margin(egui::Margin::symmetric(30, 26)),
            )
            .show(context, |ui| {
                self.header(ui);
                self.waveform(ui);
                self.score_panel(ui);
            });
        self.instrument_settings(context);
        self.reading_settings(context);
        if let Some(path) = &self.screenshot_path {
            for event in context.input(|input| input.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    let bytes: Vec<u8> = image
                        .pixels
                        .iter()
                        .flat_map(|pixel| pixel.to_array())
                        .collect();
                    let _ = image::save_buffer(
                        path,
                        &bytes,
                        image.width() as u32,
                        image.height() as u32,
                        image::ColorType::Rgba8,
                    );
                    context.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            if !self.loading
                && !self.screenshot_requested
                && self.started_at.elapsed() > Duration::from_secs(2)
            {
                self.screenshot_requested = true;
                context.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                    egui::UserData::default(),
                ));
            }
        }
        if self.busy
            || self.installing
            || self.loading
            || self.player.as_ref().is_some_and(AudioPlayer::is_playing)
            || self.screenshot_path.is_some()
        {
            context.request_repaint_after(Duration::from_millis(25));
        } else {
            context.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_practice();
        self.cancelled.store(true, Ordering::Relaxed);
        self.install_cancelled.store(true, Ordering::Relaxed);
        if let Some(player) = self.player.as_mut() {
            player.pause();
        }
    }
}

fn default_chords_visible() -> bool {
    true
}

fn keyboard_loop_range(
    start: f64,
    end: f64,
    position: f64,
    duration: f64,
    move_start: bool,
) -> (f64, f64) {
    let minimum_span = 0.05_f64.min(duration);
    if move_start {
        let start = position.clamp(0.0, duration - minimum_span);
        let end = if end < start + minimum_span {
            start + 15.0
        } else {
            end
        };
        (start, end.clamp(start + minimum_span, duration))
    } else {
        let end = position.clamp(minimum_span, duration);
        (start.clamp(0.0, end - minimum_span), end)
    }
}

fn menu_action(ui: &mut egui::Ui, icon: Icon, label: &str, enabled: bool) -> bool {
    ui.add_enabled_ui(enabled, |ui| {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
        });
        let mut row = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(rect.shrink(4.0))
                .layout(egui::Layout::left_to_right(Align::Center)),
        );
        design::icon_label(&mut row, icon, label);
        row.label(label);
        if response.hovered() {
            ui.painter()
                .rect_stroke(rect, 0.0, Stroke::new(1.0, TEXT), egui::StrokeKind::Inside);
        }
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.clicked() {
            ui.close();
        }
        response.clicked()
    })
    .inner
}

fn left_heading(ui: &mut egui::Ui, title: &str, size: f32, height: f32) {
    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), height),
        egui::Layout::left_to_right(Align::Center),
        |ui| {
            ui.add(egui::Label::new(RichText::new(title).size(size).strong()).truncate())
                .on_hover_text(title);
        },
    );
}

fn modal_heading(ui: &mut egui::Ui, title: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(25.0).strong());
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            if design::icon_button(ui, Icon::Cancel, "Chiudi · Esc", false).clicked() {
                ui.close();
            }
        });
    });
}

fn time_label(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let minutes = (seconds / 60.0).floor() as u64;
    format!("{minutes:02}:{:04.1}", seconds % 60.0)
}

fn audio_library() -> Vec<PathBuf> {
    let directory = runtime::library_directory();
    let mut paths: Vec<_> = std::fs::read_dir(directory)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|extension| {
                matches!(
                    extension.to_string_lossy().to_lowercase().as_str(),
                    "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aiff"
                )
            })
        })
        .collect();
    paths.sort();
    paths
}

fn estimate_tempo(clip: &AudioClip) -> f64 {
    let window_frames = (clip.sample_rate as usize / 100).max(1);
    let maximum_samples = (clip.sample_rate as usize * 90 * 2).min(clip.samples.len());
    let energies: Vec<f64> = clip.samples[..maximum_samples]
        .chunks(window_frames * 2)
        .map(|chunk| {
            chunk
                .iter()
                .map(|sample| (*sample as f64).powi(2))
                .sum::<f64>()
                / chunk.len() as f64
        })
        .collect();
    let flux: Vec<f64> = energies
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).max(0.0))
        .collect();
    let mut best = (0.0, 120.0);
    for bpm in 70..=180 {
        let lag = (6000.0 / bpm as f64).round() as usize;
        if flux.len() <= lag {
            continue;
        }
        let correlation = flux[..flux.len() - lag]
            .iter()
            .zip(&flux[lag..])
            .map(|(left, right)| left * right)
            .sum::<f64>();
        if correlation > best.0 {
            best = (correlation, bpm as f64);
        }
    }
    best.1
}

fn export_artifacts(clip: &AudioClip, transcription: &Transcription) -> Result<(), String> {
    let bpm = estimate_tempo(clip);
    let export_path = transcription::cache_path(&clip.path).with_extension("mid");
    export::write_midi(&export_path, transcription, bpm)?;
    let mut guitar_counts = [0usize; 8];
    for note in &transcription.notes {
        if !note.is_drum && (24..=31).contains(&note.program) {
            guitar_counts[(note.program - 24) as usize] += 1;
        }
    }
    if let Some((index, count)) = guitar_counts
        .iter()
        .enumerate()
        .max_by_key(|(_, count)| *count)
        && *count > 0
    {
        let program = index as u8 + 24;
        let notes: Vec<_> = transcription
            .notes
            .iter()
            .filter(|note| !note.is_drum && note.program == program)
            .cloned()
            .collect();
        let settings = ScoreSettings {
            bpm,
            ..Default::default()
        };
        let score_path = transcription::cache_path(&clip.path).with_extension("guitar.musicxml");
        export::write_musicxml(&score_path, &notes, &settings, &clip.title)?;
    }
    Ok(())
}

pub fn run_command_line() -> bool {
    let arguments: Vec<String> = std::env::args().collect();
    let Some(mode) = arguments.get(1) else {
        return false;
    };
    if mode == "--install-ai" {
        let (sender, receiver) = mpsc::channel();
        runtime::spawn_installation(sender, Arc::new(AtomicBool::new(false)));
        for event in receiver {
            match event {
                InstallEvent::Progress(progress, message) => {
                    println!("{:.1}% {message}", progress * 100.0);
                }
                InstallEvent::Finished => return true,
                InstallEvent::FinishedWithWarning(message) => {
                    println!("{message}");
                    return true;
                }
                InstallEvent::Failed(error) => {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
            }
        }
        std::process::exit(1);
    }
    if !matches!(
        mode.as_str(),
        "--transcribe" | "--inspect" | "--audio-smoke" | "--midi-smoke" | "--export-cache"
    ) {
        return false;
    }
    let path = arguments
        .get(2)
        .map(PathBuf::from)
        .or_else(|| audio_library().first().cloned());
    let Some(path) = path else {
        eprintln!("Nessun file audio");
        std::process::exit(1);
    };
    let clip = match audio::load_audio(&path) {
        Ok(clip) => clip,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    println!(
        "{} · {:.2}s · {}Hz · tempo stimato {} BPM",
        clip.title,
        clip.duration,
        clip.sample_rate,
        estimate_tempo(&clip)
    );
    if mode == "--inspect" {
        if let Some(transcription) = transcription::load_cached(&path) {
            let chords = harmony::analyze_chords(&transcription.notes);
            println!(
                "{} note · {} accordi stimati",
                transcription.notes.len(),
                chords.len()
            );
            for chord in chords.iter().take(12) {
                println!("{:.2}–{:.2}s {}", chord.onset, chord.offset, chord.symbol);
            }
        }
        return true;
    }
    if mode == "--export-cache" {
        let transcription = transcription::load_cached(&path).unwrap_or_else(|| {
            eprintln!("Trascrizione salvata assente");
            std::process::exit(1);
        });
        if let Err(error) = export_artifacts(&clip, &transcription) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        println!("MIDI e spartito aggiornati dalla trascrizione salvata");
        return true;
    }
    if mode == "--audio-smoke" {
        let mut player = AudioPlayer::new().unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(1)
        });
        player.load(&clip);
        player.set_volume(0.2);
        player.set_speed(0.5);
        player.seek(5.0);
        player.set_loop(Some((5.0, 5.4)));
        player.play();
        std::thread::sleep(Duration::from_millis(1200));
        player.pause();
        println!(
            "Audio output OK · posizione {:.3}s · loop 5.0–5.4 · velocità 0.5×",
            player.position()
        );
        return true;
    }
    if mode == "--midi-smoke" {
        let transcription = transcription::load_cached(&path).unwrap_or_else(|| {
            eprintln!("Trascrizione salvata assente");
            std::process::exit(1);
        });
        let mut player = AudioPlayer::new().unwrap_or_else(|error| {
            eprintln!("{error}");
            std::process::exit(1);
        });
        player.load(&clip);
        player.set_volume(0.2);
        player.set_midi_notes(&transcription.notes);
        player.set_midi_mode(true);
        player.set_speed(0.5);
        player.seek(5.0);
        player.set_loop(Some((5.0, 5.4)));
        player.play();
        std::thread::sleep(Duration::from_millis(1200));
        player.pause();
        let position = player.position();
        assert!((5.0..5.4).contains(&position));
        player.preview_note(64, 24, false, 0.25);
        std::thread::sleep(Duration::from_millis(350));
        assert_eq!(player.position(), position);
        println!("MIDI e preview OK · posizione {position:.3}s · loop 5.0–5.4 · velocità 0.5×");
        return true;
    }
    let (sender, receiver) = mpsc::channel();
    let samples = clip.mono_16khz();
    transcription::spawn_transcription(
        path,
        samples,
        clip.duration,
        Vec::new(),
        sender,
        Arc::new(AtomicBool::new(false)),
    );
    for event in receiver {
        match event {
            TranscriptionEvent::Progress(progress, message) => {
                println!("{:.1}% {message}", progress * 100.0)
            }
            TranscriptionEvent::Finished(transcription) => {
                println!(
                    "Completata: {} note · {}",
                    transcription.notes.len(),
                    transcription.engine
                );
                if let Err(error) = export_artifacts(&clip, &transcription) {
                    eprintln!("{error}");
                    std::process::exit(1);
                }
                return true;
            }
            TranscriptionEvent::Failed(error) => {
                eprintln!("{error}");
                std::process::exit(1);
            }
        }
    }
    eprintln!("Il motore ha interrotto la trascrizione");
    std::process::exit(1);
}

#[cfg(test)]
mod tests {
    use super::{PracticeSettings, keyboard_loop_range, menu_action};
    use crate::design::Icon;
    use eframe::egui::{self, Pos2, Rect, Vec2};

    #[test]
    fn keyboard_markers_keep_the_requested_position_and_valid_loop() {
        assert_eq!(
            keyboard_loop_range(0.0, 15.0, 100.0, 200.0, true),
            (100.0, 115.0)
        );
        assert_eq!(
            keyboard_loop_range(100.0, 115.0, 110.0, 200.0, false),
            (100.0, 110.0)
        );
        let (start, end) = keyboard_loop_range(100.0, 110.0, 50.0, 200.0, false);
        assert_eq!(end, 50.0);
        assert!(start < end);
        assert_eq!(
            keyboard_loop_range(0.0, 15.0, 200.0, 200.0, true),
            (199.95, 200.0)
        );
    }

    #[test]
    fn menu_icon_click_runs_only_enabled_actions() {
        for enabled in [true, false] {
            let context = egui::Context::default();
            let pointer = Pos2::new(20.0, 24.0);
            let mut clicked = false;
            for press in [None, Some(true), Some(false)] {
                let mut input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 200.0))),
                    ..Default::default()
                };
                input.events.push(egui::Event::PointerMoved(pointer));
                if let Some(pressed) = press {
                    input.events.push(egui::Event::PointerButton {
                        pos: pointer,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                let _ = context.run(input, |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        clicked |=
                            menu_action(ui, Icon::Instruments, "Modalità strumenti", enabled);
                    });
                });
            }
            assert_eq!(clicked, enabled);
        }
    }

    #[test]
    fn existing_practice_settings_default_to_score_view() {
        let contents = r#"{"bpm":174,"beats_per_bar":4,"subdivisions":4,"offset":0,"follow":true,"speed":1,"volume":0.8,"loop_start":0,"loop_end":15,"loop_enabled":false}"#;
        let mut settings: PracticeSettings = serde_json::from_str(contents).unwrap();
        assert!(!settings.tablature);
        settings.tablature = true;
        let saved = serde_json::to_vec(&settings).unwrap();
        let restored: PracticeSettings = serde_json::from_slice(&saved).unwrap();
        assert!(restored.tablature);
        assert_eq!(restored.bpm, 174.0);
    }
}
