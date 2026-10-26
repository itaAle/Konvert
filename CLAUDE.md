# Konvert: contesto per continuare lo sviluppo

> Contesto per gli agenti (Claude Code) e per chi riprende il lavoro da un altro PC. È versionato nella repo pubblica:
> **mai** scriverci segreti, nomi, email personali, percorsi della macchina o altri dati privati.

## Cos'è
Konvert è un convertitore audio/video/immagini per Windows 10/11 a 64 bit, in stile Apple.
Usa Tauri 2.11, React 19 con TypeScript e FFmpeg/FFprobe inclusi.
Lavora **interamente in locale**.
Repo **pubblica**: https://github.com/itaAle/Konvert (branch `main`).
Release con installer NSIS e file `.sha256`: `v0.1.0` pubblicata; `v0.2.0` (foto) la pubblica il proprietario.
La pre-release `ffmpeg-n9.0.2-36` è il mirror della build FFmpeg e dei suoi sorgenti (richiesto dalla LGPL).

## Regole vincolanti (dal proprietario)
- **Privacy assoluta: zero connessioni di rete a runtime.** Niente telemetria, updater, CDN, crash reporting, fetch o plugin di rete. In caso di dubbio scegli sempre l'opzione che non tocca la rete.
  `npm run check:privacy` fallisce se nel grafo delle dipendenze desktop compaiono crate o pacchetti HTTP/TLS/WebSocket/telemetria, se la CSP viene allentata, se l'installer scaricherebbe qualcosa o se nel codice compaiono URL esterni non autorizzati.
- **Repo pubblica:**
  - mai segreti, percorsi assoluti della macchina, dati personali o file pesanti nella cronologia;
  - **mai committare i binari di FFmpeg/FFprobe**: sono ignorati da Git e si scaricano con `npm run setup:ffmpeg`.
- **Identità Git:** la config locale della repo è già impostata su `Ale <118929447+itaAle@users.noreply.github.com>`. Non usare email personali.
- **Commit** piccoli in stile Conventional Commits (`feat(core): …`, `feat(ui): …`, `docs: …`), in inglese, con il trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Mai fare push senza che il proprietario lo chieda.** I force push li esegue lui (la modalità automatica li blocca).
- **Testi visibili su GitHub** (README, commit, note delle release) **in inglese**. L'interfaccia è bilingue it/en.
- Il proprietario parla italiano e vuole:
  - un piano prima di implementare;
  - un riassunto a ogni milestone;
  - build e test verdi prima di ogni commit.
- **Non cancellare le preferenze reali dell'utente** (`%APPDATA%\app.konvert.desktop\preferences.json`): `outputDir` può puntare a una sua cartella reale, quindi le conversioni di prova dall'app finirebbero lì. Nei test usa una destinazione esplicita (CLI `--out-dir`, test d'integrazione).

## Architettura
Workspace Cargo (`Cargo.toml`, resolver 3, toolchain fissata a 1.99.1 in `rust-toolchain.toml`):

- `crates/konvert-core/`: dominio puro, senza Tauri.
  - `formats.rs`: catalogo dichiarativo con `TargetFormat`, `FORMATS` (FormatSpec), `RULES`, `available_targets`, `is_offered`. Le immagini sono `FormatKind::Image` con un solo `ImageCodec` (JPG, PNG, WebP, AVIF).
  - `probe.rs`: ffprobe (`-show_stream_groups` incluso) → `MediaInfo`.
    - Le immagini statiche (demuxer `image2`/`*_pipe`, marchi HEIF/AVIF) sono `MediaKind::Image` con `PictureInfo` (alfa, piano alfa AVIF, griglia HEIC, EXIF); GIF/APNG/WebP animati restano video.
    - Per le immagini non HEIF un secondo ffprobe legge il primo fotogramma (`apply_first_frame_json`): orientamento EXIF e presenza degli EXIF. Le dimensioni salvate sono quelle visualizzate (già ruotate).
    - FFmpeg applica da solo la rotazione (EXIF o `irot`) prima dei filtri; `[0:g:N]` in un filter graph restituisce la griglia HEIC ricomposta.
  - `request.rs` → `plan.rs` (`plan_conversion`, puro) → `command.rs` (`build_from_plan`, argomenti FFmpeg puri) → `convert.rs` (esecuzione).
  - `estimate.rs`: stima della dimensione e avvisi.
  - Altri moduli: `naming.rs` (`nome (n).ext`, mai sovrascrivere), `workspace.rs` (`%TEMP%\Konvert\run-…` con lock e pulizia dopo crash), `process.rs` (Job Object kill-on-close e `CREATE_NO_WINDOW`), `progress.rs`, `disk.rs`, `binaries.rs`, `error.rs`.
  - Codici di errore stabili in `error.rs`: `engine_missing`, `input_not_found`, `output_unavailable`, `not_media`, `corrupt_input`, `unsupported_conversion`, `disk_full`, `permission_denied`, `cancelled`, `engine_failed`, `io`. Ognuno ha un messaggio i18n.
  - FFmpeg gira sempre con `-protocol_whitelist file` e input passati come URL `file:`.
  - Immagini: `plan.rs::plan_picture` → `ImagePlan` (sempre ricodifica); `command.rs::picture_args` (un filter graph: griglia/alphamerge, scale, sfondo bianco per JPG, piano alfa per AVIF; encoder mjpeg, png, libwebp, libaom-av1 still-picture; muxer `image2pipe`/`webp`/`avif`).
  - EXIF: FFmpeg li conserva solo verso PNG (da JPEG/PNG/WebP, non da HEIF); negli altri casi c'è l'avviso `MetadataDropped`.
  - Test: unit test nei moduli e `tests/engine.rs` (16 test di integrazione con FFmpeg reale). `tests/fixtures/mod.rs` crea in memoria JPEG con EXIF e HEIC a griglia (tasselli HEVC fatti con libkvazaar).
- `crates/konvert-cli/`: CLI di sviluppo (`npm run cli -- targets|plan|convert …`), non distribuita.
- `src-tauri/`: strato IPC sottile.
  - `commands.rs` contiene 20 comandi, elencati anche in `build.rs::COMMANDS` e `capabilities/default.json`: **un comando nuovo va aggiunto in tutti e tre i posti**.
  - Altri file: `jobs.rs` (coda sequenziale, evento `job-update`), `prefs.rs`, `debug_log.rs` (log redatto, spento di default), `launch.rs` (file passati via argv e "Apri con"), `lib.rs` (single-instance, Mica, conferma alla chiusura).
  - `tauri.conf.json`: CSP restrittiva, `freezePrototype`, flag di WebView2 senza rete, NSIS per utente con `windows/installer-hooks.nsh` ("Apri con" via `OpenWithProgids`; NON usare le `fileAssociations` di Tauri, che sottraggono il programma predefinito).
- `src/`: React, Tailwind v4 (token via `data-scheme`/`data-vibrancy`), Motion, font Inter incluso, icone lucide.
  - `lib/api.ts` (IPC) e `lib/mock.ts` (backend finto, solo per `npm run dev` nel browser).
  - `state/queue.ts`: reducer puro della coda, con test.
  - `i18n/it.ts` e `en.ts`: tipizzati, un test verifica che le chiavi coincidano.
  - `components/`: DropScreen, FormatGrid, QualityPicker, AdvancedPanel, PlanSummary, ConvertingScreen, DoneScreen, SettingsSheet, …
- `src/bindings/`: tipi TypeScript **generati** da ts-rs durante `cargo test`. Se cambi un tipo Rust esportato, rigenerali e committali.
- `scripts/`:
  - `ffmpeg-manifest.json`: build BtbN `win64-lgpl-shared` n9.0.2-36, SHA-256 fissato, URL upstream più il mirror.
  - `setup-ffmpeg.ps1` / `.sh`.
  - `check-privacy.mjs`.
  - `generate-licenses.mjs` (rigenera `THIRD_PARTY_LICENSES`; va rilanciato se cambiano le dipendenze).
  - `monitor-network.ps1`.
  - `build-release.ps1` (7 passi: versione, npm ci, FFmpeg, controlli e test, build con `--remap-path-prefix`, verifica che l'exe non contenga percorsi locali, copia in `release/` con SHA-256).

## FFmpeg e licenze
- Build LGPL-3.0-or-later, senza GPL né nonfree: **niente libx264/libx265/fdk-aac**.
- Encoder disponibili: libopenh264 (H.264), libvpx/libvpx-vp9, libsvtav1, mpeg4, gif, libmp3lame, aac, libopus, libvorbis, flac, wmav2.
- Nella build ci sono anche decoder e encoder di immagini (mjpeg, png, libwebp, ecc.): verificali con `ffmpeg -encoders`.
- I brevetti H.264 non sono coperti dalla licenza BSD di OpenH264: il README lo dice già.
- Una nuova libreria Rust o npm deve avere una licenza compatibile e passare il controllo privacy. Poi esegui `npm run licenses`.

## Comandi
```
npm install && npm run setup:ffmpeg   # prima volta
npm run tauri dev                      # app in sviluppo
npm run dev                            # solo UI nel browser, con mock
npm test                               # privacy + licenze + Vitest + cargo test (tutto deve passare)
npm run release                        # installer + .sha256 in release/
```
Nota per la shell degli agenti: `cargo` potrebbe non essere nel PATH, quindi anteponi `$env:USERPROFILE\.cargo\bin`.

## Riprendere da un altro PC
1. Prerequisiti (Windows): Node.js 20+, rustup (la toolchain si installa da sola da `rust-toolchain.toml`), Visual Studio Build Tools con "Desktop development with C++", WebView2.
2. `git clone https://github.com/itaAle/Konvert`, poi `npm install` e `npm run setup:ffmpeg`.
3. Imposta l'identità Git **solo per la repo**: `git config user.name "Ale"` e `git config user.email "118929447+itaAle@users.noreply.github.com"`.
4. `npm test` deve essere tutto verde prima di iniziare.

## Note pratiche per gli agenti
- La modalità automatica blocca `powershell -ExecutionPolicy Bypass -File …` lanciato dall'agente: esegui il codice PowerShell (per esempio UI Automation) inline. `npm run release` invece funziona.
- Prova dell'app reale senza file picker: `npx tauri build --debug --no-bundle`, poi avvia `target/debug/konvert.exe` passando il file come argomento (stesso percorso di "Apri con") e leggi la finestra con UI Automation (interroga due volte per svegliare l'albero di accessibilità di WebView2).
- Interfaccia nel browser: `npm run dev` (porta 1420) con il backend finto; per l'anteprima degli agenti crea `.claude/launch.json` (ignorato da Git) con una configurazione `konvert-ui` che lancia `npm run dev` sulla porta 1420; `?files=a.jpg|b.mp4` sceglie cosa restituisce il file picker.
- Drag & drop non automatizzabile: chiedi al proprietario di provarlo a mano.
- `target/release/konvert.pdb` contiene il percorso utente: mai caricare `target/` o i `.pdb`.
- FFmpeg 9: il ritaglio 1×1 di YUV 4:2:0 fallisce (converti prima in rgba); `-map 0:g:0` su un HEIC dà un solo tassello, `[0:g:0]` in un filter graph dà la foto intera.

## Stato attuale
- 2026-10-26: **v0.2.0** su `main` (commit da `c437948` a `0e52cd9`, pushati): foto in core, app, ui e docs; fix "Apri con" per .aifc/.alac/.qt/.wave con un test che tiene allineate le estensioni dell'installer e del file picker; versione 0.2.0.
  Installer `Konvert_0.2.0_x64-setup.exe`, SHA-256 `5cce9fe6ebf043392bc54c41cb4dac2cb56352b298ae9b578b69cebe7e614b67` (`release/` è ignorata da Git: si rigenera con `npm run release`). Tag e release GitHub li fa il proprietario.
- Supporto immagini:
  Ingresso JPG/PNG/WebP/AVIF/HEIC/HEIF/BMP/TIFF, uscita JPG/PNG/WebP/AVIF (HEIC in uscita impossibile: FFmpeg non ha un muxer HEIF).
  Decisioni del proprietario: EXIF lasciati come fa FFmpeg (solo PNG), BMP/TIFF inclusi, HEIC incluso.
- Test verdi: 26 Vitest, 118 unit test del core, 16 di integrazione, 16 dell'app.
- Rete verificata a zero: nessun socket con il build release e con l'app installata.
- Verificati: installer, "Apri con", istanza singola, preferenze mantenute alla reinstallazione.
- Non ancora verificati a mano:
  - casella "Delete the application data" nella disinstallazione;
  - avviso con WebView2 mancante;
  - SmartScreen su un altro PC;
  - avviso di spazio su disco insufficiente;
  - installer 0.2.0: aggiornamento sopra la 0.1.0, "Apri con" sulle foto, foto HEIC vere da iPhone (i test usano HEIC sintetici).

## Backlog (prossime funzionalità, in ordine di priorità)
1. **Bug fix: video verticali deformati** (scritto nel README come futuro bug fix, su richiesta del proprietario).
   - Un video del telefono con rotazione 90° (display matrix) e ridimensionato (GIF o "Risoluzione massima") esce deformato: `plan.rs::output_size` usa le dimensioni codificate, mentre FFmpeg ruota prima dello `scale`.
   - Riprodotto: 320×240 con rotazione 90 e `--height 120` → 160×120 invece di 90×120.
   - Correzione: leggere la rotazione dal `side_data_list` dello stream in `probe.rs` (come già per le immagini) e scambiare larghezza e altezza.
2. Versioni Linux e macOS (il manifest ha già una voce linux non testata).
3. Encoder hardware NVENC/QSV/AMF/h264_mf, con rilevamento automatico.
4. FFmpeg compilato senza alcun supporto di rete.
5. Firma del codice dell'installer.
6. Possibile: conservare gli EXIF anche in JPG/WebP/AVIF (FFmpeg non li scrive: andrebbero copiati a mano, con orientamento azzerato).

## Flusso per ogni feature
1. Piano breve al proprietario.
2. Implementazione: prima il core con i test, poi l'IPC, poi la UI con i18n.
3. `npm test` verde.
4. Aggiornare il README (in inglese) e, se cambiano le dipendenze, `THIRD_PARTY_LICENSES`.
5. Commit piccoli.
6. Riassunto al proprietario.
7. Push solo su richiesta.
8. Per una nuova release: alzare la versione in `package.json`, `Cargo.toml` e `src-tauri/tauri.conf.json` (devono coincidere), poi `npm run release`.
