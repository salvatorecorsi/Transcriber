# Changelog

## 2026-10-13 — Transcriber! 0.2.0: Real Book, MIDI e accordi

### Richiesta
Rinominare l'applicazione Transcriber!, adottare una grafica bianca e nera ispirata al Real Book e aggiungere riproduzione MIDI e ascolto delle singole note al clic. Riconoscere gli accordi con le relative qualità e rendere disponibile il programma su GitHub pubblico, con eseguibile facilmente utilizzabile su un altro PC.

### Modifiche effettuate
- `src/design.rs`, `src/app.rs`, `assets/fonts/PetalumaScript.otf`, `assets/fonts/Petaluma.otf`, `assets/fonts/OFL-Petaluma.txt`: tema bianco e nero, tipografia Petaluma, simboli musicali SMuFL e pulsanti neri con bordi leggermente irregolari.
- `src/midi.rs`, `src/audio.rs`, `src/app.rs`, `assets/sounds/GeneralUser-GS.sf2`, `assets/sounds/LICENSE-GeneralUser.txt`: sintetizzatore RustySynth con SoundFont incorporato, selezione Audio/MIDI sulla parte corrente, gestione di velocità e loop e anteprima delle note senza spostare la riproduzione.
- `src/harmony.rs`, `src/score.rs`, `src/tablature.rs`, `src/app.rs`: riconoscimento conservativo degli accordi dalle note del mix completo, qualità, estensioni e bassi alternativi; simboli disattivabili nello spartito e nella tablatura e clic sulle note per l'ascolto MIDI.
- `src/runtime.rs`, `src/transcription.rs`, `.cargo/config.toml`: percorsi indipendenti dalla macchina di sviluppo, installazione del motore AI con verifica SHA-256, accelerazione Vulkan con ripiego CPU e runtime MSVC statico.
- `Cargo.toml`, `Cargo.lock`, `src/main.rs`, `tools/build.ps1`, `tools/package.ps1`: nome Transcriber!, versione 0.2.0 e preparazione dell'eseguibile autonomo e del pacchetto ZIP completo per Windows.
- `.gitignore`, `.github/workflows/windows.yml`, `README.md`, `assets/licenses/THIRD-PARTY.md`, `assets/licenses/LICENSE-RustySynth.txt`, `assets/licenses/LICENSE-muscriptor.txt`, `assets/licenses/LICENSE-ggml.txt`, `assets/licenses/LICENSE-pffft.txt`, `assets/licenses/LICENSE-MuScriptor-model.txt`, `assets/licenses/NOTICE-MuScriptor-model.md`: esclusione di audio, cache e modelli dal repository, verifiche automatiche Windows, istruzioni di installazione e licenze delle dipendenze.

### Note
- Superati 51 test e Clippy con warning trattati come errori; verificata l'uscita MIDI reale tramite CPAL a 0,5×, con loop e anteprima che mantiene invariata la posizione.
- Gli accordi sono stime basate su sovrapposizioni temporali stabili; la semplice presenza di più note non viene automaticamente interpretata come un accordo.
- Pubblicati i sorgenti e la release v0.2.0 su https://github.com/salvatorecorsi/Transcriber, con eseguibile autonomo e pacchetto ZIP completo scaricabili senza login. L'eseguibile incorpora font e sintetizzatore; la trascrizione richiede il motore AI installabile dall'app o incluso nel pacchetto completo.

---

## 2026-10-13 — Switch tra spartito e tablatura

### Richiesta
Aggiungere uno switch per scegliere tra spartito e tablatura, con etichette in italiano.

### Modifiche effettuate
- `src/tablature.rs`: tablatura per chitarra a sei corde e basso a quattro corde in accordatura standard, fino al tasto 24; scelta della diteggiatura tramite programmazione dinamica con anticipazione delle note e gestione dei conflitti tra note sostenute.
- `src/tablature.rs`: durate e continuazioni tra battute, indicazione delle note non assegnabili, cursore di riproduzione, seek al clic, inseguimento e cache; larghezza dinamica delle battute e scroll orizzontale per evitare sovrapposizioni nelle griglie dense.
- `src/app.rs`, `src/main.rs`: pulsanti “Spartito” e “Tablatura”, preferenza persistente e caricamento compatibile con le impostazioni esistenti.
- `README.md`: documentazione della nuova visualizzazione.

### Note
- La diteggiatura è dedotta dalla trascrizione; gli eventi originali restano invariati.
- Superati 24 test e Clippy con warning trattati come errori. Verificata visivamente la tablatura sul brano completo di Rosenwinkel e aggiornata la versione eseguibile.

---

## 2026-10-13 — Corsivo Transcribe desktop in Rust

### Richiesta
Creare un'applicazione simile a Transcribe!, con riproduzione, pausa e rallentamento dei file audio, aggiungendo trascrizione musicale e spartito selezionabile per strumento tramite tecnologie di NeuralNote. Usare Rust e trascrivere il file `audio/Kurt Rosenwinkels Break.mp3`.

### Modifiche effettuate
- `Cargo.toml`, `Cargo.lock`, `src/main.rs`, `src/app.rs`: applicazione desktop egui, caricamento audio, waveform con zoom, scorciatoie, loop A/B e preferenze persistenti per file.
- `src/audio.rs`: decodifica Symphonia, riproduzione CPAL e rallentamento WSOLA con intonazione conservata, velocità da 0,25× a 1,5×; corretto l'audio che fuoriusciva dai limiti del loop.
- `src/transcription.rs`: trascrizione locale asincrona, avanzamento, annullamento e cache tramite processo separato.
- `src/score.rs`: spartito per strumento, tempo, metro, quantizzazione, posizione del primo movimento e inseguimento della riproduzione.
- `src/export.rs`: esportazione MIDI e MusicXML.
- `engine/bridge.cpp`, `engine/CMakeLists.txt`, `engine/LICENSE.muscriptor`: integrazione del motore MuScriptor usato da NeuralNote mediante bridge C++.
- `tools/setup-engine.ps1`, `tools/build.ps1`, `README.md`, `.gitignore`: configurazione e compilazione, dipendenze fissate con verifica hash, documentazione e esclusione degli artefatti generati.

### Note
- Superati 15 test audio, notazione ed esportazione e Clippy con warning trattati come errori; verificata anche l'uscita audio reale a 0,5× e con loop.
- Motori CPU e Vulkan compilati e verificati su un campione di 15 secondi, con 451 note rilevate. Trascrizione integrale del brano di 1092,68 secondi completata sulla RTX 3050: 5849 eventi, dei quali 5847 nel gruppo chitarra acustica e 2 nelle percussioni.
- Salvati JSON, MIDI e MusicXML della chitarra. Verificata la chiave all'ottava bassa per la chitarra, mantenendo invariati i pitch originali; corretto il tema grafico e verificata la finestra mediante screenshot.

---
