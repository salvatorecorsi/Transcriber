# Changelog

## 2026-10-14 — Transcriber! 0.4.0: interfaccia editoriale in Rust

### Richiesta
Portare nell'applicazione Rust il mockup approvato, ispirato ai ritagli di International Anthem e alla palette del sito personale. Eliminare i pulsanti di zoom della waveform e usare Ctrl + rotella.

### Modifiche effettuate
- `src/design.rs`, `assets/fonts/ApfelGrotezk-Regular.ttf`, `assets/fonts/ApfelGrotezk-Mittel.ttf`, `assets/fonts/MonaspaceNeon-Regular.ttf`, `assets/fonts/Fraunces-Black.ttf`: tema editoriale con titoli Fraunces su carta ritagliata, interfaccia Apfel Grotezk, numeri Monaspace e palette bianca/grigia con blu `#333fff` e corallo `#e95b38`; mantenuta Petaluma per la notazione musicale.
- `src/app.rs`: libreria audio in modale, selettore della parte accanto al titolo, pannello spartito/tablatura su carta bianca, controlli allineati e player compatto; mantenuti menu, impostazioni, trascrizione, esportazioni, ascolto MIDI e anteprima delle note.
- `src/waveform.rs`, `src/app.rs`: zoom da 1× a 64× con Ctrl + rotella ancorato al punto sotto il puntatore, senza pulsanti +/−; estremi del loop blu e corallo trascinabili. Il seek esplicito ripristina l'inseguimento della posizione dopo lo zoom manuale.
- `src/score.rs`, `src/tablature.rs`: struttura della notazione grigia, note e cifre nere, cursore blu e distinzione tra font musicali e numerici; recuperati i 30 pixel ancora riservati al vecchio footer per rendere visibili tutte le sei corde nella finestra compatta.
- `src/main.rs`: dimensioni iniziali e minime della finestra adattate alla nuova composizione.
- `Cargo.toml`, `Cargo.lock`, `src/runtime.rs`, `README.md`, `tools/package.ps1`: versione 0.4.0, documentazione dei controlli e inclusione delle licenze dei font nel pacchetto.
- `assets/fonts/OFL-Apfel-Grotezk.txt`, `assets/fonts/OFL-Monaspace.txt`, `assets/fonts/OFL-Fraunces.txt`, `assets/licenses/THIRD-PARTY.md`: licenze e attribuzioni dei nuovi font incorporati nell'eseguibile.

### Note
- Superati 63 test e Clippy con warning trattati come errori, incluse regressioni per zoom ancorato, priorità dei gesti sul loop e ripristino dell'inseguimento dopo il seek.
- Verificati tramite screenshot nativi spartito e tablatura a 1440×900 e 1000×700, spartito a 1280×720 e tutte le sei corde nel viewport compatto. Menu, libreria, modalità degli strumenti e impostazioni di lettura controllati a 1280×720.
- Motore AI e separazione delle tracce non modificati; quest'ultima resta esclusa dal progetto.
- Generato il pacchetto e verificato l'avvio dell'eseguibile autonomo e dello ZIP estratto fuori dal workspace, con font incorporati. L'eseguibile propone l'installazione AI; il pacchetto completo riconosce motori e modello inclusi. Lo ZIP non contiene audio dell'utente né trascrizioni.
- Release in preparazione; nessuna pubblicazione registrata in questa voce.

---

## 2026-10-14 — Mockup editoriale della nuova interfaccia

### Richiesta
Preparare un mockup HTML non funzionante prima di intervenire sull'app Rust, ispirato ai ritagli e alla stampa di International Anthem e ai colori del sito personale.

### Modifiche effettuate
- `mockup/index.html`: prototipo statico con titolo su ritaglio, waveform, estremi del loop colorati, spartito grigio con note nere e comandi a icona; trasporto sempre visibile su desktop. Rimossi i pulsanti di zoom della waveform, mantenendo l'indicatore 1× con tooltip “Zoom: Ctrl + rotella”.
- `mockup/mockup.css`: composizione editoriale, superfici di carta e adattamento alle diverse larghezze con palette `#f5f5f5`, `#000000`, `#333fff` e `#e95b38`.
- `mockup/assets/fonts/`: font locali Fraunces, Apfel Grotezk, Monaspace e Petaluma, con licenze e fonti in `SOURCES.txt`.

### Note
- Verificato visivamente a 1280×720 e 1024×768; controllati assenza di overflow a 375 pixel e caricamento dei font senza errori.
- Il prototipo non modifica il codice Rust e non costituisce una release. La separazione AI degli strumenti resta esclusa dal progetto; nessuna pubblicazione effettuata.
- Lo zoom con Ctrl + rotella è previsto per la futura UI Rust; nel mockup resta soltanto un'indicazione statica, senza JavaScript.

---

## 2026-10-13 — Transcriber! 0.3.1: leggibilità della notazione

### Richiesta
Migliorare la leggibilità della tablatura, eliminare gli indicatori di durata e continuità e rendere grigia la struttura dello spartito.

### Modifiche effettuate
- `src/design.rs`, `src/score.rs`, `src/app.rs`: colore grigio condiviso per pentagrammi, stanghette e tagli addizionali, compreso lo spartito vuoto; note, accordi e cursore di riproduzione restano neri.
- `src/tablature.rs`: cifre Petaluma da 17 pixel centrate sulla geometria effettiva, distanza tra corde di 18 pixel e maschere disegnate prima di tutte le cifre per evitare il taglio dei tasti negli accordi a sei corde.
- `src/tablature.rs`: rimossi segmenti di durata, frecce di continuità e calcoli grafici degli estremi; i tasti continuati restano tra parentesi. Griglia, battiti inattivi e numeri di battuta sono grigi; cifre, accordi e cursore sono neri.
- `Cargo.toml`, `Cargo.lock`, `src/runtime.rs`, `README.md`: aggiornamento della versione e della documentazione alla 0.3.1.

### Note
- Le durate MIDI originali e la gestione delle note sostenute nell'assegnazione delle corde restano invariate.
- Superati 59 test e Clippy con warning trattati come errori, incluso un test di regressione per accordi a sei corde con tasti a due cifre e continuazioni. Verificati spartito e tablatura tramite screenshot e un campione locale dell'accordo Bbmaj9/D tratto dal brano di Rosenwinkel.
- Pubblicata la release v0.3.1 su https://github.com/salvatorecorsi/Transcriber/releases/tag/v0.3.1, con eseguibile autonomo e ZIP completo; verificati tutti i download senza login, i digest SHA256 dei cinque asset e la build Windows su GitHub Actions.

---

## 2026-10-13 — Transcriber! 0.3.0: interfaccia compatta e loop trascinabili

### Richiesta
Ripulire e allineare l'interfaccia, sostituire i testi dei pulsanti con icone e rimuovere le informazioni audio superflue. Gestire il loop con due stanghette colorate trascinabili e raccogliere trascrizione, esportazioni e modalità degli strumenti nel menu in alto a destra.

### Modifiche effettuate
- `src/design.rs`, `src/app.rs`: pulsanti a icona vettoriale con dimensioni uniformi di 36 pixel, tooltip ed etichette accessibili; trasporto compatto con selezione Audio/MIDI, riproduzione, salti, velocità e volume.
- `src/waveform.rs`, `src/app.rs`, `src/main.rs`: maniglie rosse e blu trascinabili per gli estremi del loop, priorità del trascinamento sul seek, vincoli sugli estremi e inseguimento sospeso durante l'interazione; rimossa la riga di controlli Loop A/B e mantenute le scorciatoie con un pulsante di attivazione a icona.
- `src/app.rs`: menu in alto a destra per trascrizione o installazione AI, esportazioni MIDI e MusicXML, modalità degli strumenti, impostazioni di lettura e licenze; modali dedicate alle modalità Automatico/Chitarra/Quartetto e a BPM, tap tempo, metro, griglia, primo battito, inseguimento e accordi.
- `src/app.rs`, `src/score.rs`, `src/tablature.rs`: toolbar compatta per spartito, tablatura e impostazioni; eliminate informazioni su frequenza di campionamento, stereo e durata totale, intestazioni duplicate, suggerimenti permanenti ed esportazioni in linea; informazioni sulle note non assegnabili disponibili nel tooltip.
- `src/runtime.rs`, `Cargo.toml`, `Cargo.lock`, `README.md`, `tools/package.ps1`: aggiornamento alla versione 0.3.0, documentazione dei nuovi controlli e adeguamento del pacchetto di distribuzione.

### Note
- Superati 58 test e Clippy con warning trattati come errori; verificato il clic reale egui sull'icona delle voci del menu, inclusi i comandi disabilitati. Gli screenshot definitivi confermano titoli a sinistra, icone allineate, righe del menu interamente cliccabili e le due modali.
- Verificata la compatibilità delle scorciatoie I/O e dei salvataggi precedenti: gli estremi restano validi senza spostare l'ascolto e la normalizzazione delle vecchie impostazioni non viene interpretata come trascinamento né attiva il loop.
- Pubblicata la release v0.3.0 su https://github.com/salvatorecorsi/Transcriber/releases/tag/v0.3.0, con eseguibile autonomo e ZIP completo; verificati tutti i download senza login e superata la build Windows su GitHub Actions.

---

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
