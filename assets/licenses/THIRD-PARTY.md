# Componenti di terze parti

Transcriber! incorpora Petaluma/Petaluma Script e GeneralUser GS senza modifiche. Il motore AI è distribuito separatamente o nel pacchetto completo; i pesi MuScriptor hanno una licenza distinta dal codice.

| Componente | Autori / origine | Licenza | Avviso completo |
| --- | --- | --- | --- |
| muscriptor.cpp | Damien Ronssin; port di MuScriptor, Mirelo e Kyutai | MIT | `LICENSE-muscriptor.txt` |
| ggml | The ggml authors | MIT | `LICENSE-ggml.txt` |
| PFFFT / FFTPACK | Julien Pommier; UCAR / NCAR | BSD-style | `LICENSE-pffft.txt` |
| MuScriptor small, conversione GGUF f16 | Mirelo e Kyutai; conversione di Damien Ronssin | CC BY-NC 4.0 | `LICENSE-MuScriptor-model.txt`, `NOTICE-MuScriptor-model.md` |
| Petaluma / Petaluma Script | Steinberg Media Technologies GmbH | SIL Open Font License 1.1 | `OFL-Petaluma.txt` |
| GeneralUser GS 2.0.3 | S. Christian Collins | GeneralUser GS License 2.0 | `LICENSE-GeneralUser.txt` |
| RustySynth | Nobuyuki Tanaka | MIT | `LICENSE-RustySynth.txt` |

Font e banco sonoro sono incorporati nell'eseguibile. I relativi avvisi sono accessibili anche nel pannello Licenze dell'app.

Il modello originale è stato convertito da safetensors a GGUF con pesi float16, senza riaddestramento o fine-tuning. Transcriber! usa il file GGUF originale senza modificarlo.

Motore: <https://github.com/DamRsn/muscriptor.cpp>, revisione `c8adc019677ed3479fa3c22c1a117d662382f347`.

Modello: <https://huggingface.co/DamRsn/muscriptor-gguf>, revisione `d7045f94e8b19427f4ff9542975035e66596e51c`, file `v1/muscriptor-small-f16.gguf`, SHA-256 `925f55af65a20ebc4f8b45ceaf095a12b72493d436cb112623cd0041a1af23d4`.

Progetto originale MuScriptor: <https://github.com/muscriptor/muscriptor>, <https://huggingface.co/MuScriptor>. Paper: <https://arxiv.org/abs/2607.08168v1>.

Petaluma: <https://github.com/steinbergmedia/petaluma>.

GeneralUser GS: <https://schristiancollins.com/generaluser.php>.

I pesi MuScriptor sono destinati all'uso non commerciale. L'attribuzione e le condizioni supplementari pubblicate dagli autori sono conservate integralmente in `NOTICE-MuScriptor-model.md`.

Il pacchetto non include registrazioni musicali degli utenti né trascrizioni preesistenti.
