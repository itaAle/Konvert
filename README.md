# Konvert
Audio and video file converter with a modern interface.

Konvert converte file audio e video da un formato all'altro, **interamente sul tuo
computer**: nessuna connessione di rete, nessuna telemetria, nessun account.

> Stato: in sviluppo (milestone 4 di 5). Prima piattaforma: Windows 10/11.

## Come si usa

1. Trascina uno o più file audio o video nella finestra (oppure **Scegli file…**, o Ctrl+O).
2. Konvert li analizza e propone solo i formati sensati (per più file: quelli comuni a tutti).
3. Scegli il formato e la qualità (**Massima qualità**, **Bilanciata**, **File piccolo**):
   vedi subito la dimensione stimata e gli eventuali avvisi. In **Avanzate** puoi
   regolare bitrate, frequenza, canali, risoluzione, fotogrammi al secondo e codec.
4. Premi **Converti**: avanzamento per ogni file e totale, tempo rimanente e **Annulla**
   in ogni momento. A fine lavoro puoi aprire i file o mostrarli nella cartella.

I file convertiti vengono salvati accanto all'originale (oppure in una cartella a scelta)
e non ne sovrascrivono mai altri (`brano.mp3` esiste già? → `brano (1).mp3`).
Se chiudi la finestra durante una conversione, Konvert chiede conferma.

Interfaccia in stile macOS, in italiano o inglese, con tema chiaro/scuro (automatico o
a scelta) ed effetto Mica su Windows 11.

### Impostazioni (Ctrl+,)

- **Lingua**, **Aspetto**, **Qualità predefinita**, **Cartella di destinazione**.
- **Modalità debug** (disattivata di default): scrive un registro tecnico locale per
  diagnosticare problemi, senza nomi né percorsi dei file. Si può mostrare o eliminare.
- **Informazioni**: versione di Konvert e di FFmpeg, impegni sulla privacy, link alla
  pagina dei rilasci (si apre nel browser solo quando lo clicchi).

Le preferenze sono l'unico dato che Konvert conserva: un file JSON leggibile in
`%APPDATA%\app.konvert.desktop\preferences.json`. Il registro di debug, se attivato, è in
`%LOCALAPPDATA%\app.konvert.desktop\logs\konvert.log`.

## Formati

Konvert propone solo le conversioni sensate per il file che gli dai:

| File in ingresso  | Formati proposti                                                        |
| ----------------- | ----------------------------------------------------------------------- |
| Audio             | MP3, M4A (AAC), WAV, FLAC, OGG (Vorbis), OPUS, WMA, AIFF                |
| Video con audio   | MP4, MKV, MOV, WebM, AVI, GIF e **solo audio** in tutti i formati audio |
| Video senza audio | MP4, MKV, MOV, WebM, AVI, GIF                                           |

- Tre preset in linguaggio comune: **Massima qualità**, **Bilanciata**, **File piccolo**,
  più opzioni avanzate (bitrate, frequenza, canali, risoluzione, fps, codec).
- Se i flussi sono già compatibili con il formato scelto (es. MP4 H.264/AAC → MKV),
  vengono copiati **senza ricodifica**: istantaneo e senza perdita.
- Avvisi chiari quando la conversione perde qualità o non può recuperarla
  (es. MP3 → WAV) e quando si perdono copertina, sottotitoli o tracce audio extra.
- Metadati e copertina conservati dove il formato lo permette (MP3, M4A, FLAC).
- Video H.264 con OpenH264, VP9 per WebM, AV1 (SVT-AV1) tra le opzioni avanzate.

La tabella dei formati è dichiarativa: `crates/konvert-core/src/formats.rs`.

## Sviluppo

### Prerequisiti (Windows)

- [Node.js](https://nodejs.org/) 20 o successivo (con npm)
- [Rust](https://rustup.rs/) stabile (toolchain `x86_64-pc-windows-msvc`)
- Visual Studio Build Tools con il carico di lavoro **Sviluppo di applicazioni desktop con C++**
- WebView2 Runtime (già presente su Windows 11 e su Windows 10 aggiornato)

Con winget:

```powershell
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

### Setup

```powershell
npm install
npm run setup:ffmpeg      # scarica FFmpeg/FFprobe (LGPL), ne verifica lo SHA-256
```

Lo script `scripts/setup-ffmpeg.ps1` (o `scripts/setup-ffmpeg.sh` su Linux/macOS) scarica
la build di FFmpeg fissata in `scripts/ffmpeg-manifest.json`, ne verifica l'hash SHA-256 e
copia solo i file necessari in `src-tauri/binaries/ffmpeg/`. I binari non sono nel
repository. Il download avviene **solo in fase di sviluppo**: l'app installata non scarica
mai nulla.

### Comandi

```powershell
npm run tauri dev         # avvia l'app in modalità sviluppo
npm run dev               # solo interfaccia nel browser, con un backend simulato (design)
npm test                  # controllo privacy + test interfaccia (Vitest) + test Rust
npm run check:privacy     # solo il controllo privacy
npm run cli -- targets "C:\percorso\video.mp4"          # formati proposti
npm run cli -- plan "C:\percorso\video.mp4" --to webm     # decisioni, avvisi, stima
npm run cli -- convert "C:\percorso\brano.wav" --to mp3 --preset balanced
```

`konvert-cli` è uno strumento di sviluppo che usa lo stesso motore dell'app; non viene
installato con Konvert.

### Struttura

```
crates/konvert-core/   logica di dominio in Rust (senza dipendenze da Tauri): analisi,
                       catalogo formati, pianificazione, stima, builder dei comandi FFmpeg,
                       esecuzione, nomi univoci, file temporanei
crates/konvert-cli/    CLI di sviluppo sopra konvert-core
src-tauri/             app Tauri: strato IPC sottile, configurazione, permessi
src/                   interfaccia React + TypeScript
src/bindings/          tipi TypeScript generati dai tipi Rust (ts-rs, durante `cargo test`)
scripts/               setup di FFmpeg, controllo privacy
```

## Licenza

Konvert è distribuito con licenza [MIT](LICENSE). FFmpeg è distribuito separatamente
con licenza LGPL v3 o successiva: dettagli in `THIRD_PARTY_LICENSES` (in arrivo).
