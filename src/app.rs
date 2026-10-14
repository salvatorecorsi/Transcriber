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
use eframe::egui::{
    self, Align, Align2, Color32, Pos2, Rect, RichText, Sense, Shape, Stroke, Vec2,
};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

const TEXT: Color32 = design::INK;

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
    show_library: bool,
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
    waveform_follow_reset: bool,
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
            show_library: preview == Some("library"),
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
            waveform_follow_reset: false,
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
                    self.waveform_follow_reset = true;
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
            self.waveform_follow_reset = true;
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
            || self.show_library
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

    fn library_modal(&mut self, context: &egui::Context) {
        if !self.show_library {
            return;
        }
        let mut selected_path = None;
        let mut browse = false;
        let response = egui::Modal::new(egui::Id::new("audio_library"))
            .frame(egui::Frame::popup(&context.style()).inner_margin(24.0))
            .show(context, |ui| {
                ui.set_width(480.0);
                modal_heading(ui, "Le registrazioni");
                ui.add_space(18.0);
                if self.library.is_empty() {
                    ui.label(
                        RichText::new("Apri un file audio per iniziare.").color(design::MUTED),
                    );
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(360.0)
                        .show(ui, |ui| {
                            for path in &self.library {
                                let selected =
                                    self.clip.as_ref().is_some_and(|clip| &clip.path == path);
                                let title = path.file_stem().unwrap_or_default().to_string_lossy();
                                let response = ui
                                    .add_enabled(
                                        !self.loading,
                                        egui::Button::new(RichText::new(title.as_ref()).color(
                                            if selected { design::BLUE } else { design::INK },
                                        ))
                                        .fill(if selected {
                                            design::PAPER
                                        } else {
                                            design::WHITE
                                        })
                                        .stroke(Stroke::NONE)
                                        .corner_radius(6.0)
                                        .min_size(Vec2::new(ui.available_width(), 42.0))
                                        .wrap(),
                                    )
                                    .on_hover_text(path.to_string_lossy());
                                if response.clicked() {
                                    selected_path = Some(path.clone());
                                }
                            }
                        });
                }
                ui.add_space(18.0);
                browse = ui
                    .add_enabled(!self.loading, design::button("Apri file audio…"))
                    .clicked();
            });
        if response.should_close() || selected_path.is_some() || browse {
            self.show_library = false;
        }
        if let Some(path) = selected_path
            && self.clip.as_ref().is_none_or(|clip| clip.path != path)
        {
            self.open_audio(path);
        }
        if browse {
            self.open_audio_dialog();
        }
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
        let wide = context.content_rect().width() >= 1250.0;
        let height = if wide { 94.0 } else { 139.0 } + if show_status { 38.0 } else { 0.0 };
        egui::TopBottomPanel::bottom("transport")
            .exact_height(height)
            .frame(
                egui::Frame::new()
                    .fill(design::PAPER)
                    .inner_margin(egui::Margin::symmetric(32, 15)),
            )
            .show(context, |ui| {
                let rect = workstation_rect(ui.available_rect_before_wrap());
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(13.0, 10.0);
                    if wide {
                        ui.horizontal(|ui| {
                            self.playback_controls(ui);
                            ui.add_space(8.0);
                            self.position_readout(ui);
                            ui.add_space(8.0);
                            self.speed_controls(ui, true);
                            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                self.volume_control(ui, 82.0);
                                ui.add_space(10.0);
                                self.source_controls(ui);
                            });
                        });
                    } else {
                        ui.horizontal(|ui| {
                            self.playback_controls(ui);
                            ui.add_space(10.0);
                            self.position_readout(ui);
                            if rect.width() >= 720.0 {
                                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                    self.source_controls(ui);
                                });
                            }
                        });
                        ui.horizontal(|ui| {
                            self.speed_controls(ui, true);
                            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                                self.volume_control(ui, 72.0);
                                if rect.width() < 720.0 {
                                    self.source_controls(ui);
                                }
                            });
                        });
                    }
                    if show_status {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&self.status).size(12.0).color(design::MUTED));
                            if self.busy || self.installing {
                                ui.add(
                                    egui::ProgressBar::new(self.progress)
                                        .desired_width(150.0)
                                        .show_percentage(),
                                );
                                if design::round_icon_button(
                                    ui,
                                    Icon::Cancel,
                                    "Annulla",
                                    false,
                                    28.0,
                                )
                                .clicked()
                                {
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
            });
    }

    fn playback_controls(&mut self, ui: &mut egui::Ui) {
        let playing = self.player.as_ref().is_some_and(AudioPlayer::is_playing);
        ui.add_enabled_ui(self.clip.is_some() && self.player.is_some(), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                if design::round_icon_button(
                    ui,
                    if playing { Icon::Pause } else { Icon::Play },
                    if playing {
                        "Pausa · Spazio"
                    } else {
                        "Riproduci · Spazio"
                    },
                    false,
                    51.0,
                )
                .clicked()
                    && let Some(player) = self.player.as_mut()
                {
                    player.toggle();
                }
                ui.add_space(5.0);
                if design::round_icon_button(
                    ui,
                    Icon::Backward,
                    "Indietro di 5 secondi · ←",
                    false,
                    36.0,
                )
                .clicked()
                {
                    self.seek((self.position() - 5.0).max(0.0));
                }
                if design::round_icon_button(
                    ui,
                    Icon::Forward,
                    "Avanti di 5 secondi · →",
                    false,
                    36.0,
                )
                .clicked()
                {
                    self.seek(self.position() + 5.0);
                }
                if design::round_icon_button(
                    ui,
                    Icon::Repeat,
                    "Ripeti selezione · L",
                    self.loop_enabled,
                    36.0,
                )
                .clicked()
                {
                    self.loop_enabled = !self.loop_enabled;
                    self.update_loop();
                }
            });
        });
    }

    fn position_readout(&self, ui: &mut egui::Ui) {
        ui.add_sized(
            [136.0, 51.0],
            egui::Label::new(
                RichText::new(time_label(self.position()))
                    .font(design::number_font(25.0))
                    .color(design::INK),
            ),
        );
    }

    fn speed_controls(&mut self, ui: &mut egui::Ui, slider_visible: bool) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 7.0;
            ui.label(RichText::new("Velocità").size(10.0).color(design::MUTED));
            let previous = self.speed;
            if slider_visible {
                ui.scope(|ui| {
                    ui.spacing_mut().slider_width = 94.0;
                    design::slider(
                        ui,
                        egui::Slider::new(&mut self.speed, 0.25..=1.5)
                            .step_by(0.05)
                            .show_value(false),
                    )
                    .on_hover_text("Velocità · intonazione invariata");
                });
            }
            ui.scope(|ui| {
                ui.style_mut().override_font_id = Some(design::number_font(12.0));
                ui.add_sized(
                    [56.0, 30.0],
                    egui::DragValue::new(&mut self.speed)
                        .range(0.25..=1.5)
                        .speed(0.01)
                        .suffix("×")
                        .fixed_decimals(2),
                )
                .on_hover_text("Velocità di riproduzione");
            });
            egui::Frame::new()
                .fill(design::WHITE)
                .corner_radius(7.0)
                .inner_margin(3.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        for (speed, label) in [(0.5, "½×"), (0.75, "¾×"), (1.0, "1×")] {
                            let selected = (self.speed - speed).abs() < 0.005;
                            if ui
                                .add_sized(
                                    [36.0, 28.0],
                                    egui::Button::new(
                                        RichText::new(label).font(design::number_font(12.0)).color(
                                            if selected {
                                                design::WHITE
                                            } else {
                                                design::MUTED
                                            },
                                        ),
                                    )
                                    .fill(if selected {
                                        design::BLUE
                                    } else {
                                        design::WHITE
                                    })
                                    .stroke(Stroke::NONE)
                                    .corner_radius(5.0),
                                )
                                .clicked()
                            {
                                self.speed = speed;
                            }
                        }
                    });
                });
            if (self.speed - previous).abs() > f32::EPSILON
                && let Some(player) = self.player.as_mut()
            {
                player.set_speed(self.speed);
            }
        });
    }

    fn source_controls(&mut self, ui: &mut egui::Ui) {
        ui.allocate_ui_with_layout(
            Vec2::new(165.0, 36.0),
            egui::Layout::left_to_right(Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if design::tab_button(ui, Icon::Audio, "Audio", !self.midi_mode).clicked() {
                    self.midi_mode = false;
                    if let Some(player) = self.player.as_mut() {
                        player.set_midi_mode(false);
                    }
                }
                ui.add_enabled_ui(
                    self.selected_instrument.is_some() && self.player.is_some(),
                    |ui| {
                        if design::tab_button(ui, Icon::Midi, "MIDI", self.midi_mode).clicked() {
                            self.midi_mode = true;
                            if let Some(player) = self.player.as_mut() {
                                player.set_midi_mode(true);
                            }
                        }
                    },
                );
            },
        );
    }

    fn volume_control(&mut self, ui: &mut egui::Ui, width: f32) {
        ui.allocate_ui_with_layout(
            Vec2::new(width + 29.0, 36.0),
            egui::Layout::left_to_right(Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 7.0;
                design::icon_label(ui, Icon::Volume, "Volume");
                ui.scope(|ui| {
                    ui.spacing_mut().slider_width = width;
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
            },
        );
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 49.0), Sense::hover());
        let wordmark = ui.painter().layout_no_wrap(
            "Transcriber".into(),
            design::strong_font(26.0),
            design::INK,
        );
        let origin = Pos2::new(rect.left(), rect.center().y - wordmark.size().y * 0.5);
        let mark_position = origin + Vec2::new(wordmark.size().x, 0.0);
        ui.painter().galley(origin, wordmark, design::INK);
        ui.painter().text(
            mark_position,
            Align2::LEFT_TOP,
            "!",
            design::strong_font(26.0),
            design::BLUE,
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    Pos2::new(rect.right() - 92.0, rect.top()),
                    rect.right_bottom(),
                ))
                .layout(egui::Layout::right_to_left(Align::Center)),
            |ui| {
                let menu_button = design::round_icon_button(ui, Icon::Menu, "Menu", false, 40.0);
                if self.preview_menu {
                    egui::Popup::open_id(ui.ctx(), egui::Popup::default_response_id(&menu_button));
                    self.preview_menu = false;
                }
                egui::Popup::menu(&menu_button)
                    .width(280.0)
                    .show(|ui| self.action_menu(ui));
                ui.add_space(4.0);
                if design::round_icon_button(ui, Icon::Folder, "Apri audio e libreria", false, 40.0)
                    .clicked()
                {
                    self.show_library = true;
                }
            },
        );
        ui.painter().line_segment(
            [
                rect.left_bottom() + Vec2::new(0.0, 8.0),
                rect.right_bottom() + Vec2::new(0.0, 8.0),
            ],
            Stroke::new(1.0, Color32::from_gray(214)),
        );
        ui.add_space(if ui.ctx().content_rect().height() < 800.0 {
            16.0
        } else {
            28.0
        });
        self.session_heading(ui);
    }

    fn session_heading(&mut self, ui: &mut egui::Ui) {
        let compact = ui.ctx().content_rect().height() < 800.0;
        let separate_selector = ui.available_width() < 790.0;
        let title_height = if compact { 92.0 } else { 113.0 };
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(
                ui.available_width(),
                if separate_selector {
                    149.0
                } else {
                    title_height
                },
            ),
            Sense::hover(),
        );
        let title = self
            .clip
            .as_ref()
            .map_or("Il tuo prossimo ascolto", |clip| clip.title.as_str());
        let title_width = if separate_selector {
            rect.width()
        } else {
            (rect.width() - 274.0).max(200.0)
        };
        let title_rect = Rect::from_min_size(
            rect.min,
            Vec2::new(
                title_width,
                if separate_selector {
                    90.0
                } else {
                    title_height
                },
            ),
        );
        ui.interact(title_rect, ui.id().with("recording_title"), Sense::hover())
            .on_hover_text(self.clip.as_ref().map_or_else(
                || title.to_owned(),
                |clip| clip.path.to_string_lossy().into_owned(),
            ));
        let mut title_size = if !compact && rect.width() > 1100.0 {
            52.0
        } else {
            43.0
        };
        let mut heading = ui.painter().layout_no_wrap(
            title.into(),
            design::display_font(title_size),
            design::INK,
        );
        while heading.size().x > title_width - 4.0 && title_size > 26.0 {
            title_size -= 1.0;
            heading = ui.painter().layout_no_wrap(
                title.into(),
                design::display_font(title_size),
                design::INK,
            );
        }
        if heading.size().x > title_width - 4.0 {
            let mut visible_title = title.to_owned();
            while visible_title.pop().is_some() {
                heading = ui.painter().layout_no_wrap(
                    format!("{visible_title}…"),
                    design::display_font(title_size),
                    design::INK,
                );
                if heading.size().x <= title_width - 4.0 {
                    break;
                }
            }
        }
        let paper_rect = Rect::from_min_size(
            title_rect.min,
            Vec2::new(
                (heading.size().x + 14.0).max(104.0).min(title_width),
                (32.0 + heading.size().y + 8.0).min(title_rect.height()),
            ),
        );
        design::paint_paper(ui.painter(), paper_rect.intersect(rect));
        let label_rect =
            Rect::from_min_size(title_rect.min + Vec2::new(2.0, 5.0), Vec2::new(90.0, 23.0));
        ui.painter().add(Shape::convex_polygon(
            vec![
                label_rect.left_top() + Vec2::new(0.0, 1.0),
                label_rect.right_top() + Vec2::new(-2.0, -1.0),
                label_rect.right_bottom(),
                label_rect.left_bottom() + Vec2::new(1.0, 2.0),
            ],
            design::INK,
            Stroke::NONE,
        ));
        ui.painter().text(
            label_rect.center(),
            Align2::CENTER_CENTER,
            "Registrazione",
            egui::FontId::proportional(11.0),
            design::WHITE,
        );
        let title_painter = ui
            .painter()
            .with_clip_rect(title_rect.intersect(ui.clip_rect()));
        let heading_origin = title_rect.min + Vec2::new(0.0, 32.0);
        let underline_width = (heading.size().x * 0.3).min(148.0);
        let underline_left = heading_origin.x + heading.size().x - underline_width;
        let underline_top = heading_origin.y + heading.size().y - 9.0;
        title_painter.add(Shape::convex_polygon(
            vec![
                Pos2::new(underline_left - 2.0, underline_top + 2.0),
                Pos2::new(underline_left + underline_width + 4.0, underline_top),
                Pos2::new(underline_left + underline_width + 6.0, underline_top + 7.0),
                Pos2::new(underline_left, underline_top + 9.0),
            ],
            design::CORAL,
            Stroke::NONE,
        ));
        title_painter.galley(heading_origin, heading, design::INK);
        let selector_rect = if separate_selector {
            Rect::from_min_size(rect.min + Vec2::new(0.0, 94.0), Vec2::new(244.0, 48.0))
        } else {
            Rect::from_min_size(
                Pos2::new(rect.right() - 244.0, rect.top() + 23.0),
                Vec2::new(244.0, 60.0),
            )
        };
        self.part_selector(ui, selector_rect);
    }

    fn part_selector(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let instruments = self.instruments();
        let response = ui.interact(
            rect,
            ui.id().with("selected_part"),
            if instruments.is_empty() {
                Sense::hover()
            } else {
                Sense::click()
            },
        );
        ui.painter().rect_filled(rect, 8.0, design::WHITE);
        ui.painter().circle_filled(
            Pos2::new(rect.left() + 27.0, rect.center().y),
            17.0,
            design::PAPER,
        );
        let mut symbol = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_center_size(
            Pos2::new(rect.left() + 27.0, rect.center().y),
            Vec2::splat(22.0),
        )));
        design::icon_label(&mut symbol, Icon::Score, "Parte visualizzata");
        ui.painter().text(
            Pos2::new(rect.left() + 54.0, rect.center().y - 12.0),
            Align2::LEFT_CENTER,
            "Parte",
            egui::FontId::proportional(10.0),
            design::MUTED,
        );
        let name = self
            .selected_instrument
            .map_or("Nessuna parte trascritta", |(program, drums)| {
                score::instrument_name(program, drums)
            });
        ui.painter().text(
            Pos2::new(rect.left() + 54.0, rect.center().y + 7.0),
            Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(14.0),
            design::INK,
        );
        ui.painter().add(Shape::line(
            vec![
                Pos2::new(rect.right() - 22.0, rect.center().y - 2.0),
                Pos2::new(rect.right() - 17.0, rect.center().y + 3.0),
                Pos2::new(rect.right() - 12.0, rect.center().y - 2.0),
            ],
            Stroke::new(1.5, design::INK),
        ));
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, !instruments.is_empty(), name)
        });
        egui::Popup::menu(&response).width(260.0).show(|ui| {
            for (program, drums, _) in instruments {
                if ui
                    .selectable_value(
                        &mut self.selected_instrument,
                        Some((program, drums)),
                        score::instrument_name(program, drums),
                    )
                    .clicked()
                {
                    ui.close();
                }
            }
        });
        response
            .on_hover_text("Scegli la parte da leggere e ascoltare in MIDI")
            .on_hover_cursor(if self.selected_instrument.is_some() {
                egui::CursorIcon::PointingHand
            } else {
                egui::CursorIcon::Default
            });
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

    fn waveform(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("La registrazione").size(12.0));
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.label(
                    RichText::new(format!("{:.1}×", self.waveform_zoom))
                        .font(design::number_font(11.0))
                        .color(design::MUTED),
                )
                .on_hover_text("Zoom: Ctrl + rotella · trascina per spostarti nel brano");
            });
        });
        ui.add_space(2.0);
        let position = self.position();
        let action = waveform::show(
            ui,
            self.clip.as_ref(),
            position,
            &mut self.waveform_zoom,
            &mut self.waveform_start,
            &mut self.loop_start,
            &mut self.loop_end,
            self.loop_enabled,
            std::mem::take(&mut self.waveform_follow_reset),
        );
        if action.loop_changed {
            if action.loop_dragged {
                self.loop_enabled = true;
            }
            self.update_loop();
        }
        if let Some(position) = action.seek {
            self.seek(position);
            self.waveform_follow_reset = false;
        }
        ui.add_space(if ui.ctx().content_rect().height() < 800.0 {
            10.0
        } else {
            17.0
        });
    }

    fn score_panel(&mut self, ui: &mut egui::Ui) {
        let notes = self.selected_notes();
        let tablature_supported = tablature::supports_tablature(&notes);
        let show_tablature = self.tablature && tablature_supported;
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), ui.available_height().max(130.0)),
            Sense::hover(),
        );
        let painter = ui.painter_at(rect.expand2(Vec2::new(0.0, 5.0)));
        painter.rect_filled(
            rect.translate(Vec2::new(0.0, 4.0))
                .shrink2(Vec2::new(1.0, 0.0)),
            0.0,
            Color32::from_gray(230),
        );
        painter.rect_filled(
            rect.translate(Vec2::new(0.0, 3.0))
                .shrink2(Vec2::new(1.0, 0.0)),
            0.0,
            design::WHITE,
        );
        painter.rect_filled(rect, 0.0, design::WHITE);
        let inner_rect = rect.shrink2(Vec2::new(16.0, 7.0));
        let mut paper = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner_rect)
                .layout(egui::Layout::top_down(Align::Min)),
        );
        paper.set_clip_rect(rect);
        self.score_toolbar(&mut paper, tablature_supported, show_tablature);
        paper.add_space(6.0);
        let position = self.position();
        if notes.is_empty() {
            let empty_rect = paper.available_rect_before_wrap();
            for index in 0..5 {
                let y = empty_rect.center().y - 18.0 + index as f32 * 10.0;
                painter.line_segment(
                    [
                        Pos2::new(empty_rect.left() + 34.0, y),
                        Pos2::new(empty_rect.right() - 34.0, y),
                    ],
                    Stroke::new(0.7, design::STAFF_COLOR),
                );
            }
            let message = if self.busy {
                "La trascrizione è in corso"
            } else if self.clip.is_none() {
                "Un ascolto. Una pagina nuova."
            } else {
                "Dall'ascolto alla pagina."
            };
            let caption =
                painter.layout_no_wrap(message.into(), design::display_font(27.0), design::INK);
            let caption_rect =
                Rect::from_center_size(empty_rect.center(), caption.size() + Vec2::new(28.0, 14.0));
            painter.rect_filled(caption_rect, 0.0, design::WHITE);
            painter.galley(
                empty_rect.center() - caption.size() * 0.5,
                caption,
                design::INK,
            );
        } else {
            let chords = if self.show_chords {
                self.chords.as_slice()
            } else {
                &[]
            };
            let action = if show_tablature {
                tablature::render_tablature(
                    &mut paper,
                    &notes,
                    &self.score_settings,
                    position,
                    chords,
                )
            } else {
                score::render_score(&mut paper, &notes, &self.score_settings, position, chords)
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
        let fold = 15.0;
        painter.add(Shape::convex_polygon(
            vec![
                Pos2::new(rect.right() - fold, rect.bottom()),
                rect.right_bottom(),
                Pos2::new(rect.right(), rect.bottom() - fold),
            ],
            design::PAPER,
            Stroke::NONE,
        ));
        painter.line_segment(
            [
                Pos2::new(rect.right() - fold, rect.bottom()),
                Pos2::new(rect.right(), rect.bottom() - fold),
            ],
            Stroke::new(1.0, Color32::from_gray(226)),
        );
    }

    fn score_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        tablature_supported: bool,
        show_tablature: bool,
    ) {
        let narrow = ui.available_width() < 620.0;
        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), if narrow { 86.0 } else { 48.0 }),
            egui::Layout::top_down(Align::Min),
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 5.0;
                    if design::tab_button(ui, Icon::Score, "Spartito", !show_tablature).clicked() {
                        self.tablature = false;
                    }
                    ui.add_enabled_ui(tablature_supported, |ui| {
                        if design::tab_button(ui, Icon::Tablature, "Tablatura", show_tablature)
                            .clicked()
                        {
                            self.tablature = true;
                        }
                    });
                    if !narrow {
                        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                            self.reading_controls(ui)
                        });
                    }
                });
                if narrow {
                    ui.horizontal(|ui| self.reading_controls(ui));
                }
            },
        );
        let divider = ui.cursor().top() + 1.0;
        ui.painter().line_segment(
            [
                Pos2::new(ui.max_rect().left(), divider),
                Pos2::new(ui.max_rect().right(), divider),
            ],
            Stroke::new(1.0, design::PAPER),
        );
    }

    fn reading_controls(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.x = 8.0;
        if design::round_icon_button(ui, Icon::Settings, "Impostazioni di lettura", false, 32.0)
            .clicked()
        {
            self.show_score_settings = true;
        }
        if design::round_icon_button(
            ui,
            Icon::Follow,
            "Segui ascolto",
            self.score_settings.follow,
            32.0,
        )
        .clicked()
        {
            self.score_settings.follow = !self.score_settings.follow;
        }
        if design::round_icon_button(ui, Icon::Chords, "Mostra accordi", self.show_chords, 32.0)
            .clicked()
        {
            self.show_chords = !self.show_chords;
        }
        if ui
            .add(
                egui::Button::new(
                    RichText::new(format!("{}/4", self.score_settings.beats_per_bar))
                        .font(design::number_font(12.0))
                        .color(design::INK),
                )
                .frame(false)
                .min_size(Vec2::new(42.0, 32.0)),
            )
            .on_hover_text("Metro e griglia")
            .clicked()
        {
            self.show_score_settings = true;
        }
        ui.label(RichText::new("BPM").size(9.0).color(design::MUTED));
        ui.scope(|ui| {
            ui.style_mut().override_font_id = Some(design::number_font(14.0));
            ui.add_sized(
                [55.0, 32.0],
                egui::DragValue::new(&mut self.score_settings.bpm)
                    .range(30.0..=300.0)
                    .speed(0.2)
                    .fixed_decimals(0),
            )
            .on_hover_text("Tempo della trascrizione");
        });
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
        self.sync_midi_track();
        self.transport(context);
        if self.show_licenses {
            egui::Window::new("Font e licenze").open(&mut self.show_licenses).default_width(620.0).vscroll(true).show(context, |ui| {
                ui.label("Transcriber! · Apfel Grotezk, Collletttivo · Fraunces, Undercase Type · Monaspace, GitHub · Petaluma Script / Petaluma, Steinberg · GeneralUser GS, S. Christian Collins · RustySynth, Nobuyuki Tanaka.");
                ui.label("MuScriptor: Kyutai e Mirelo. Il modello AI è CC BY-NC 4.0; i pesi hanno una licenza distinta dal codice dell'app.");
                ui.hyperlink_to("Progetto e download", runtime::RELEASE_URL);
                ui.separator();
                ui.label(include_str!("../assets/fonts/OFL-Petaluma.txt"));
                ui.separator();
                ui.label(include_str!("../assets/fonts/OFL-Apfel-Grotezk.txt"));
                ui.separator();
                ui.label(include_str!("../assets/fonts/OFL-Monaspace.txt"));
                ui.separator();
                ui.label(include_str!("../assets/fonts/OFL-Fraunces.txt"));
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
                    .fill(design::PAPER)
                    .inner_margin(egui::Margin::symmetric(32, 12)),
            )
            .show(context, |ui| {
                let rect = workstation_rect(ui.available_rect_before_wrap());
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    self.header(ui);
                    self.waveform(ui);
                    self.score_panel(ui);
                });
            });
        self.library_modal(context);
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

fn workstation_rect(rect: Rect) -> Rect {
    let width = rect.width().min(1440.0);
    Rect::from_min_size(
        Pos2::new(rect.center().x - width * 0.5, rect.top()),
        Vec2::new(width, rect.height()),
    )
}

fn modal_heading(ui: &mut egui::Ui, title: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).font(design::strong_font(25.0)));
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
