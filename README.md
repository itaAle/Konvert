# Konvert
Audio and video file converter with a modern interface.

Konvert converte file audio e video da un formato all'altro, **interamente sul tuo
computer**: nessuna connessione di rete, nessuna telemetria, nessun account.

Piattaforma: Windows 10/11 (64 bit). Linux e macOS sono previsti in una fase successiva.

## Come si usa

1. Trascina uno o più file audio o video nella finestra (oppure **Scegli file…**, Ctrl+O,
   o tasto destro sul file → **Apri con** → **Konvert**).
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

## Formati

Konvert propone solo le conversioni sensate per il file che gli dai:

| File in ingresso  | Formati proposti                                                        |
| ----------------- | ----------------------------------------------------------------------- |
| Audio             | MP3, M4A (AAC), WAV, FLAC, OGG (Vorbis), OPUS, WMA, AIFF                |
| Video con audio   | MP4, MKV, MOV, WebM, AVI, GIF e **solo audio** in tutti i formati audio |
| Video senza audio | MP4, MKV, MOV, WebM, AVI, GIF                                           |

- Se i flussi sono già compatibili con il formato scelto (es. MP4 H.264/AAC → MKV),
  vengono copiati **senza ricodifica**: istantaneo e senza perdita.
- Avvisi chiari quando la conversione perde qualità o non può recuperarla
  (es. MP3 → WAV) e quando si perdono copertina, sottotitoli o tracce audio extra.
- Metadati e copertina conservati dove il formato lo permette (MP3, M4A, FLAC).
- Video H.264 con OpenH264, VP9 per WebM, AV1 (SVT-AV1) tra le opzioni avanzate.

La tabella dei formati è dichiarativa: `crates/konvert-core/src/formats.rs`.

## Installazione

1. Scarica dalla pagina [Releases](https://github.com/itaAle/Konvert/releases) l'installer
   `Konvert_<versione>_x64-setup.exe` e il file `Konvert_<versione>_x64-setup.exe.sha256`.
2. **Verifica il checksum** (vedi sotto) prima di aprire l'installer.
3. Avvia l'installer: installa Konvert solo per il tuo utente (nessun permesso di
   amministratore), crea la voce nel menu Start e aggiunge Konvert al menu **Apri con**
   dei formati supportati, senza cambiare i programmi predefiniti.

Konvert usa Microsoft Edge WebView2, già presente in Windows 11 e in Windows 10
aggiornato. Se manca, l'installer lo segnala: installalo dal sito Microsoft
("WebView2 Runtime"). L'installer non scarica mai nulla da Internet.

### Verificare il checksum SHA-256

Il file `.sha256` contiene l'impronta dell'installer. In PowerShell, nella cartella dei
download:

```powershell
(Get-FileHash .\Konvert_0.1.0_x64-setup.exe -Algorithm SHA256).Hash
Get-Content .\Konvert_0.1.0_x64-setup.exe.sha256
```

Le due impronte devono coincidere (maiuscole e minuscole non contano). In alternativa:
`certutil -hashfile Konvert_0.1.0_x64-setup.exe SHA256`. Su Linux/macOS:
`sha256sum -c Konvert_0.1.0_x64-setup.exe.sha256`. Se non coincidono, non avviare il file.

### Avviso di Windows SmartScreen

Finché l'installer non è firmato con un certificato di code signing, Windows SmartScreen
può mostrare **"Windows ha protetto il PC"** (app non riconosciuta). Per procedere in
sicurezza:

1. scarica l'installer **solo** dalla pagina Releases di questo repository;
2. verifica il checksum come descritto sopra;
3. nella finestra di SmartScreen scegli **Ulteriori informazioni** → **Esegui comunque**.

Il codice sorgente è pubblico: chiunque può controllarlo e ricompilare l'installer
(vedi [Creare una release](#creare-una-release)).

## Aggiornamenti

Konvert **non controlla e non scarica aggiornamenti** (né automaticamente né su
richiesta). Per aggiornare:

1. in **Impostazioni → Informazioni** trovi la versione installata e il link
   **Pagina dei rilasci**, che si apre nel browser solo quando lo clicchi;
2. scarica il nuovo installer, verifica il checksum e installalo sopra la versione
   precedente.

Le preferenze vengono mantenute. Se l'installer propone di disinstallare prima la
versione precedente, lascia **non selezionata** la casella "Cancella i dati
dell'applicazione".

## Disinstallazione

Da **Impostazioni di Windows → App → App installate → Konvert → Disinstalla**.
La casella **Cancella i dati dell'applicazione** (non selezionata di default) elimina
anche le preferenze e l'eventuale registro di debug. Le voci "Apri con" vengono rimosse.

## Privacy

Konvert funziona interamente sul tuo computer. Nessun dato lascia il dispositivo.

**Rete**
- Nessuna connessione di rete, neanche all'avvio: niente telemetria, analytics, crash
  reporting, controllo aggiornamenti, licenze o account.
- Font, icone e librerie sono inclusi nell'app: nessun CDN o servizio esterno.
- Content Security Policy restrittiva (`default-src 'self'`, connessioni solo verso l'IPC
  interno), nessun plugin Tauri di rete (updater, http, …), interfaccia WebView2 avviata
  con le funzioni di rete in background disattivate.
- FFmpeg e FFprobe sono eseguiti con `-protocol_whitelist file`: possono aprire solo file
  locali, mai indirizzi di rete.
- L'unico indirizzo web noto all'app è la pagina dei rilasci, che si apre nel browser
  solo se la clicchi.

**Dati sul disco**
- Nessuna cronologia di file convertiti, nomi, percorsi o anteprime.
- Le uniche impostazioni salvate sono le preferenze (lingua, aspetto, qualità predefinita,
  cartella di destinazione, modalità debug) in un file JSON leggibile:
  `%APPDATA%\app.konvert.desktop\preferences.json`. Nessun identificativo di installazione
  o del dispositivo.
- I file in lavorazione stanno in una cartella temporanea dedicata (`%TEMP%\Konvert`) e
  vengono eliminati al termine, all'annullamento o, dopo un crash, all'avvio successivo.
- Il registro di debug è **disattivato di default**. Se lo attivi in Impostazioni, resta
  in `%LOCALAPPDATA%\app.konvert.desktop\logs\konvert.log`, senza nomi né percorsi dei
  file (sostituiti da segnaposto), e non viene mai inviato.

**Come verificarlo**
- **Resource Monitor** (`resmon`) → scheda **Rete**: durante l'uso, i processi
  `konvert.exe`, `msedgewebview2.exe` e `ffmpeg.exe` non compaiono tra i processi con
  attività di rete.
- **Wireshark**: con Konvert in uso non compare traffico generato dall'app.
- Lo script `scripts/monitor-network.ps1` osserva i socket di Konvert e di tutti i suoi
  processi figli (WebView2, FFmpeg):
  `powershell -ExecutionPolicy Bypass -File scripts/monitor-network.ps1 -Seconds 60`
  mentre usi l'app (exit code 0 = nessuna connessione).
- `npm run check:privacy` fallisce se nel grafo delle dipendenze compaiono librerie
  HTTP/TLS/WebSocket o di telemetria, se la CSP viene allentata, se l'installer
  scaricherebbe qualcosa o se nel codice compaiono indirizzi esterni non autorizzati.

Tutte le dipendenze e le loro licenze sono elencate in
[`THIRD_PARTY_LICENSES`](THIRD_PARTY_LICENSES): nessuna effettua traffico di rete in Konvert.

## Funzionalità future

Idee raccolte per i prossimi aggiornamenti:

- **Immagini**: conversione di file JPG/JPEG, PNG, WebP, AVIF (oggi i file immagine
  vengono rifiutati perché Konvert gestisce solo audio e video).
- Versioni per **Linux** e **macOS**.
- **Accelerazione hardware** del video (NVIDIA NVENC, Intel Quick Sync, AMD AMF),
  con rilevamento automatico.
- FFmpeg compilato **senza alcun supporto di rete** (oggi la rete è bloccata a runtime).
- Installer **firmato** con certificato di code signing (niente avviso SmartScreen).

## Sviluppo

### Prerequisiti (Windows)

- [Node.js](https://nodejs.org/) 20 o successivo (con npm)
- [Rust](https://rustup.rs/) (la versione esatta è fissata in `rust-toolchain.toml`:
  rustup la installa da solo)
- Visual Studio Build Tools con il carico di lavoro **Sviluppo di applicazioni desktop con C++**
- WebView2 Runtime

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
repository. Il download avviene **solo in fase di sviluppo**: l'app non scarica mai nulla.

### Comandi

```powershell
npm run tauri dev         # avvia l'app in modalità sviluppo
npm run dev               # solo interfaccia nel browser, con un backend simulato (design)
npm test                  # privacy, licenze, test interfaccia (Vitest) e test Rust
npm run check:privacy     # solo il controllo privacy
npm run licenses          # rigenera THIRD_PARTY_LICENSES
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
src-tauri/             app Tauri: comandi IPC, coda, preferenze, registro, permessi,
                       configurazione e hook dell'installer (windows/installer-hooks.nsh)
src/                   interfaccia React + TypeScript (Tailwind, Motion)
src/bindings/          tipi TypeScript generati dai tipi Rust (ts-rs, durante `cargo test`)
scripts/               setup di FFmpeg, controllo privacy, monitor di rete, licenze, release
```

## Creare una release

```powershell
npm run release
```

Lo script `scripts/build-release.ps1`:

1. controlla che la versione coincida in `package.json`, `Cargo.toml` e
   `src-tauri/tauri.conf.json`;
2. installa le dipendenze npm esatte (`npm ci`) e la build di FFmpeg verificata;
3. esegue controllo privacy, controllo licenze e tutti i test;
4. compila l'installer NSIS con i percorsi locali rimappati e verifica che l'eseguibile
   non contenga il nome utente o cartelle della macchina di build;
5. copia in `release/` l'installer e il suo file `.sha256`.

Tutti gli input sono fissati (toolchain Rust, `Cargo.lock`, `package-lock.json`, archivio
FFmpeg con hash). La prima build scarica una volta gli strumenti NSIS usati da Tauri.
Pubblica sulla pagina Releases sia l'installer sia il file `.sha256`.

## Licenze

Konvert è distribuito con licenza [MIT](LICENSE).

Include FFmpeg (build BtbN `win64-lgpl-shared`), distribuito con licenza **LGPL v3 o
successiva**. FFmpeg gira come programma separato e i suoi file sono installati senza
modifiche nella cartella `ffmpeg` dell'installazione, dove possono essere sostituiti. La
build non contiene componenti GPL (nessun libx264/libx265) né non liberi. Il commit esatto
dei sorgenti di FFmpeg e la ricetta di build sono indicati in
[`THIRD_PARTY_LICENSES`](THIRD_PARTY_LICENSES) e in `scripts/ffmpeg-manifest.json`.

**Brevetti H.264**: il video H.264 è codificato con OpenH264, la cui licenza BSD copre
solo il codice sorgente. **I brevetti H.264 non sono coperti dalla licenza BSD di
OpenH264** (la copertura brevettuale di Cisco vale solo per i binari OpenH264 distribuiti
da Cisco stessa). Alcuni codec (H.264, AAC) possono essere soggetti a brevetti in alcuni
paesi: chi usa o ridistribuisce Konvert è responsabile delle eventuali licenze necessarie.

Tutte le dipendenze, con licenza e testo completo, sono in
[`THIRD_PARTY_LICENSES`](THIRD_PARTY_LICENSES) (incluso anche nell'installazione e
consultabile da **Impostazioni → Informazioni → Licenze di terze parti**).
