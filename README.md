# Konvert
Audio and video file converter with a modern interface.

Konvert converte file audio e video da un formato all'altro, **interamente sul tuo
computer**: nessuna connessione di rete, nessuna telemetria, nessun account.

> Stato: in sviluppo (milestone 1 di 5). Prima piattaforma: Windows 10/11.

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
npm test                  # controllo privacy + test Rust (unitari e di integrazione)
npm run check:privacy     # solo il controllo privacy
npm run cli -- convert "C:\percorso\brano.wav" --to mp3 --preset balanced
```

`konvert-cli` è uno strumento di sviluppo che usa lo stesso motore dell'app; non viene
installato con Konvert.

### Struttura

```
crates/konvert-core/   logica di dominio in Rust: analisi, builder dei comandi FFmpeg,
                       esecuzione, nomi univoci, file temporanei (senza dipendenze da Tauri)
crates/konvert-cli/    CLI di sviluppo sopra konvert-core
src-tauri/             app Tauri: strato IPC sottile, configurazione, permessi
src/                   interfaccia React + TypeScript
scripts/               setup di FFmpeg, controllo privacy
```

## Licenza

Konvert è distribuito con licenza [MIT](LICENSE). FFmpeg è distribuito separatamente
con licenza LGPL v3 o successiva: dettagli in `THIRD_PARTY_LICENSES` (in arrivo).
