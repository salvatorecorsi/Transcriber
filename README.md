# Transcriber!

Workstation desktop in Rust per studiare registrazioni, rallentarle senza cambiare intonazione e trascriverle in spartito, tablatura e MIDI. Interfaccia bianca e nera ispirata ai jazz Real Book, font Petaluma Script e pulsanti con il bordo irregolare di una fotocopia.

## Scarica e avvia

Scarica [Transcriber.exe](https://github.com/salvatorecorsi/Transcriber/releases/download/v0.2.0/Transcriber.exe), aprilo e premi **Installa AI**. Il primo avvio scarica il motore e il modello MuScriptor small, circa 200 MB più il motore. Poi la trascrizione funziona offline. Non servono account GitHub, Rust, Python, compilatori o un'installazione separata dei font. Riproduzione audio e sintesi MIDI sono già disponibili nell'eseguibile.

Per avere tutto pronto anche su un computer senza connessione, scarica [Transcriber-Windows-x64.zip](https://github.com/salvatorecorsi/Transcriber/releases/download/v0.2.0/Transcriber-Windows-x64.zip), estrai l'intera cartella e apri `Transcriber.exe`. Il pacchetto comprende motore CPU, accelerazione Vulkan, modello e licenze. Il file `portable.flag` fa salvare le impostazioni e le trascrizioni nella cartella estratta.

Requisiti: Windows 10 o 11 a 64 bit, uscita audio disponibile e un processore x64 con AVX e F16C. La CPU è sufficiente. Quando il driver offre Vulkan, l'app preferisce la GPU; se il motore GPU non parte, riprova con la CPU. La velocità di trascrizione dipende dall'hardware e dalla durata del brano.

Tutti i download sono disponibili nelle [release pubbliche](https://github.com/salvatorecorsi/Transcriber/releases). `SHA256SUMS` contiene le impronte dei file distribuiti; l'installer dell'app verifica anche motore e modello prima di usarli.

## Studiare un brano

Apri un file audio dall'interfaccia. Il player offre play/pausa, seek sulla waveform, zoom, volume, loop A–B e velocità da 0,25× a 1,5× mantenendo l'intonazione. Spazio alterna play/pausa, le frecce spostano di cinque secondi e Home torna all'inizio. I imposta A, O imposta B e L attiva il loop.

**Trascrivi audio** esegue MuScriptor sul computer. Puoi lasciare il riconoscimento automatico, limitare il modello alla chitarra elettrica o indicare il quartetto chitarra/contrabbasso/piano/batteria. **Annulla** interrompe il processo. I file audio restano sul computer e non vengono caricati sul server del modello.

Seleziona la parte riconosciuta per mostrarne le note. **Spartito / Tablatura** cambia la vista; la tablatura usa accordatura standard E–A–D–G–B–E per chitarra e E–A–D–G per basso, fino al tasto 24. Corde e tasti sono stimati considerando note simultanee, note sostenute e continuità della posizione. Le note non assegnabili sono indicate con `?`. La chitarra usa la chiave di violino all'ottava bassa, conservando l'altezza sonora originale.

**Audio / MIDI** permette di ascoltare la registrazione oppure la parte selezionata con i suoni General MIDI incorporati. Il clic su una nota dello spartito o della tablatura ne riproduce subito l'anteprima MIDI. Loop, posizione e velocità restano sincronizzati con la visualizzazione. La scelta della parte cambia la notazione e l'ascolto MIDI; l'audio originale rimane il mix della registrazione.

Le sigle degli **accordi** sono inferite dalle note simultanee e sostenute riconosciute nell'intera trascrizione. Comprendono qualità maggiori, minori, diminuite, aumentate, sospese, settime ed estensioni jazz. Sono una lettura armonica automatica: un voicing senza fondamentale o una nota estranea può avere più interpretazioni.

BPM, Tap, metro, quantizzazione e posizione del primo battito regolano la lettura ritmica; i tempi MIDI rilevati dal modello rimangono originali. Con **Segui ascolto** lo spartito scorre durante la riproduzione; disattivalo per consultarlo liberamente. Il clic in un punto libero del pentagramma sposta l'ascolto.

**Esporta MIDI** salva tutte le parti riconosciute. **MusicXML** salva la parte selezionata per correggere e impaginare la trascrizione in un editor di spartiti. Note, ritmica, accordi e diteggiature restano risultati automatici da rifinire musicalmente.

## Salvataggi

Con il solo eseguibile, motore, modello e salvataggi sono in `%LOCALAPPDATA%\Transcriber`. Nel pacchetto completo con `portable.flag`, restano accanto all'eseguibile. La cartella `transcriptions` contiene note salvate e impostazioni per i singoli audio. Una cartella `audio` accanto all'app può servire da libreria iniziale; la distribuzione è vuota e non include registrazioni o trascrizioni degli utenti.

Per usare una posizione diversa, imposta `TRANSCRIBER_DATA_DIR`. Per forzare il motore CPU, imposta `TRANSCRIBER_TRANSCRIPTION_BACKEND=cpu`.

## Compilazione

Serve Rust stable con target Windows MSVC. Le dipendenze Rust sono fissate in `Cargo.lock`; font e SoundFont si trovano in `assets` e vengono incorporati nell'eseguibile. La libreria runtime MSVC viene collegata staticamente.

```powershell
./tools/build.ps1
```

Lo script produce `Transcriber.exe`. Per preparare anche il motore AI dai sorgenti:

```powershell
./tools/setup-engine.ps1
./tools/setup-engine.ps1 -Vulkan
```

Il primo comando prepara il motore CPU. Il secondo prepara anche il motore Vulkan e richiede 7-Zip e un driver che fornisca Vulkan. Lo script recupera la toolchain e una revisione fissata di `muscriptor.cpp`, verifica le impronte SHA-256 e compila il piccolo adattatore in `engine/bridge.cpp`. Queste dipendenze di compilazione servono solo allo sviluppatore.

```powershell
cargo test
cargo clippy --all-targets -- -D warnings
./tools/package.ps1
```

Il pacchetto di release comprende esclusivamente l'app, i due motori originali verificati, il modello, istruzioni e avvisi di licenza. `./tools/package.ps1 -SkipBuild` usa l'eseguibile già compilato. Gli asset sono generati in `dist`.

La UI, la decodifica, il resampling, il playback, WSOLA, la sintesi MIDI, il riconoscimento degli accordi, la quantizzazione e gli export sono Rust. Il motore di inferenza è il port C++/ggml di MuScriptor usato da NeuralNote e comunica con l'app tramite JSON in un processo separato.

La modalità `--install-ai` prepara motore e modello senza aprire la finestra. `--transcribe "percorso/audio.mp3"` trascrive e salva JSON, MIDI e MusicXML della chitarra, se riconosciuta; `--export-cache "percorso/audio.mp3"` rigenera gli export dalle note salvate.

`--inspect "percorso/audio.mp3"` controlla la decodifica e, quando esiste una trascrizione salvata, mostra il numero di note, il numero di accordi stimati e le prime sigle con i tempi. `--audio-smoke "percorso/audio.mp3"` verifica l'uscita audio; `--midi-smoke "percorso/audio.mp3"` verifica playback MIDI, loop, rallentamento e anteprima della nota su una trascrizione già salvata. `--screenshot "percorso/immagine.png"` acquisisce la finestra e termina.

## Motore, font e suoni

[NeuralNote](https://github.com/DamRsn/NeuralNote) usa [muscriptor.cpp](https://github.com/DamRsn/muscriptor.cpp), port di [MuScriptor](https://github.com/muscriptor/muscriptor) di Mirelo e Kyutai. Il codice del motore è MIT. I [pesi MuScriptor GGUF](https://huggingface.co/DamRsn/muscriptor-gguf) sono **CC BY-NC 4.0, per uso non commerciale**, e conservano le condizioni di utilizzo pubblicate dagli autori.

[Petaluma e Petaluma Script](https://github.com/steinbergmedia/petaluma) di Steinberg sono SIL OFL 1.1; [GeneralUser GS](https://schristiancollins.com/generaluser.php) di S. Christian Collins usa la propria licenza GeneralUser GS 2.0. Il sintetizzatore [RustySynth](https://github.com/sinshu/rustysynth) è MIT. Avvisi, attribuzioni e testi delle licenze sono disponibili nell'app e in [assets/licenses](assets/licenses/THIRD-PARTY.md); gli originali dei font e del SoundFont sono nelle rispettive cartelle `assets/fonts` e `assets/sounds`.
