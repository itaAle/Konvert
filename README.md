# Konvert
Audio, video and image file converter with a modern interface.

Konvert converts audio, video and image files from one format to another **entirely on
your computer**: no network connections, no telemetry, no account.

Platform: Windows 10/11 (64-bit). Linux and macOS are planned for a later phase.

## How to use it

1. Drag one or more audio, video or image files into the window (or use **Choose files…**,
   Ctrl+O, or right-click the file → **Open with** → **Konvert**).
2. Konvert analyzes them and offers only the formats that make sense (for several files:
   the ones they all have in common).
3. Pick the format and the quality (**Best quality**, **Balanced**, **Small file**): the
   estimated size and any warnings appear right away. Under **Advanced** you can adjust
   bitrate, sample rate, channels, resolution, frame rate and codec (for pictures: quality
   and maximum size).
4. Press **Convert**: progress for each file and overall, time remaining, and **Cancel**
   at any moment. When it is done you can open the files or show them in their folder.

Converted files are saved next to the original (or in a folder of your choice) and never
overwrite other files (`song.mp3` already exists? → `song (1).mp3`).
If you close the window during a conversion, Konvert asks for confirmation.

macOS-style interface in English or Italian, with light/dark theme (automatic or manual)
and the Mica effect on Windows 11.

## Formats

Konvert only offers the conversions that make sense for the file you give it:

| Input file                                         | Formats offered                                                       |
| -------------------------------------------------- | --------------------------------------------------------------------- |
| Audio                                              | MP3, M4A (AAC), WAV, FLAC, OGG (Vorbis), OPUS, WMA, AIFF              |
| Video with audio                                   | MP4, MKV, MOV, WebM, AVI, GIF, and **audio only** in every audio format |
| Video without audio                                | MP4, MKV, MOV, WebM, AVI, GIF                                         |
| Picture (JPG/JPEG, PNG, WebP, AVIF, HEIC/HEIF, BMP, TIFF) | JPG, PNG, WebP, AVIF                                           |

- When the streams are already compatible with the chosen format (e.g. MP4 H.264/AAC →
  MKV), they are copied **without re-encoding**: instant and lossless.
- Clear warnings when a conversion loses quality or cannot bring it back (e.g. MP3 → WAV)
  and when cover art, subtitles or extra audio tracks are dropped.
- Metadata and cover art are kept where the format allows it (MP3, M4A, FLAC).
- H.264 video with OpenH264, VP9 for WebM, AV1 (SVT-AV1) among the advanced options.

Pictures:

- Photos are turned the right way up (EXIF orientation, HEIC rotation), and HEIC photos from
  phones, stored as a grid of tiles, are put back together.
- Pictures keep their size unless you choose a maximum size under **Advanced**.
- Transparency is kept in PNG, WebP and AVIF; in JPG, transparent areas become white.
- EXIF data (camera, date, location) is kept when a JPEG, PNG or WebP picture becomes a PNG;
  the other conversions leave it out, and Konvert tells you so before converting.
- Animated GIF, WebP and PNG files are treated as videos.

The format table is declarative: `crates/konvert-core/src/formats.rs`.

## Installation

1. From the [Releases](https://github.com/itaAle/Konvert/releases) page, download the
   installer `Konvert_<version>_x64-setup.exe` and the file
   `Konvert_<version>_x64-setup.exe.sha256`.
2. **Verify the checksum** (see below) before opening the installer.
3. Run the installer: it installs Konvert for your user only (no administrator rights),
   creates the Start menu entry and adds Konvert to the **Open with** menu of the
   supported formats, without changing your default programs.

Konvert uses Microsoft Edge WebView2, which is already part of Windows 11 and of an
up-to-date Windows 10. If it is missing, the installer tells you: install the
"WebView2 Runtime" from the Microsoft website. The installer never downloads anything.

### Verifying the SHA-256 checksum

The `.sha256` file contains the fingerprint of the installer. In PowerShell, in your
downloads folder:

```powershell
(Get-FileHash .\Konvert_0.2.0_x64-setup.exe -Algorithm SHA256).Hash
Get-Content .\Konvert_0.2.0_x64-setup.exe.sha256
```

The two fingerprints must match (upper/lower case does not matter). Alternatively:
`certutil -hashfile Konvert_0.2.0_x64-setup.exe SHA256`. On Linux/macOS:
`sha256sum -c Konvert_0.2.0_x64-setup.exe.sha256`. If they do not match, do not run the file.

### Windows SmartScreen warning

Until the installer is signed with a code-signing certificate, Windows SmartScreen may
show **"Windows protected your PC"** (unrecognized app). To proceed safely:

1. download the installer **only** from the Releases page of this repository;
2. verify the checksum as described above;
3. in the SmartScreen window choose **More info** → **Run anyway**.

The source code is public: anyone can review it and rebuild the installer
(see [Building a release](#building-a-release)).

## Updates

Konvert **does not check for or download updates** (neither automatically nor on
request). To update:

1. **Settings → About** shows the installed version and the **Releases page** link, which
   opens in your browser only when you click it;
2. download the new installer, verify the checksum and install it over the previous
   version.

Your preferences are kept. If the installer offers to uninstall the previous version
first, leave the "Delete the application data" checkbox **unchecked**.

## Uninstalling

From **Windows Settings → Apps → Installed apps → Konvert → Uninstall**.
The **Delete the application data** checkbox (unchecked by default) also removes your
preferences and the debug log, if any. The "Open with" entries are removed.

## Privacy

Konvert works entirely on your computer. No data leaves your device.

**Network**
- No network connections, not even at startup: no telemetry, analytics, crash
  reporting, update checks, licensing or accounts.
- Fonts, icons and libraries are bundled with the app: no CDN or external service.
- Strict Content Security Policy (`default-src 'self'`, connections only to the internal
  IPC), no networking Tauri plugin (updater, http, …), WebView2 interface started with
  its background networking features disabled.
- FFmpeg and FFprobe run with `-protocol_whitelist file`: they can open local files only,
  never network addresses.
- The only web address the app knows is the releases page, which opens in your browser
  only if you click it.

**Data on disk**
- No history of converted files, names, paths or thumbnails.
- The only saved settings are your preferences (language, appearance, default quality,
  destination folder, debug mode) in a readable JSON file:
  `%APPDATA%\app.konvert.desktop\preferences.json`. No installation or device identifier.
- Files being processed live in a dedicated temporary folder (`%TEMP%\Konvert`) and are
  deleted when the job ends, when it is cancelled or, after a crash, at the next launch.
- The debug log is **off by default**. If you turn it on in Settings, it stays in
  `%LOCALAPPDATA%\app.konvert.desktop\logs\konvert.log`, without file names or paths
  (replaced by placeholders), and it is never sent anywhere.

**How to verify it**
- **Resource Monitor** (`resmon`) → **Network** tab: while you use the app, the
  `konvert.exe`, `msedgewebview2.exe` and `ffmpeg.exe` processes do not appear among the
  processes with network activity.
- **Wireshark**: with Konvert in use, no traffic generated by the app shows up.
- The script `scripts/monitor-network.ps1` watches the sockets of Konvert and of all its
  child processes (WebView2, FFmpeg):
  `powershell -ExecutionPolicy Bypass -File scripts/monitor-network.ps1 -Seconds 60`
  while you use the app (exit code 0 = no connection).
- `npm run check:privacy` fails if HTTP/TLS/WebSocket or telemetry libraries appear in the
  dependency graph, if the CSP is loosened, if the installer would download anything or
  if unauthorized external addresses appear in the code.

Every dependency and its license is listed in
[`THIRD_PARTY_LICENSES`](THIRD_PARTY_LICENSES): none of them makes network traffic in Konvert.

## Future features

Ideas collected for upcoming updates:

- **Bug fix: portrait videos.** Videos recorded with the phone upright come out stretched
  when Konvert resizes them (GIF, or **Maximum resolution** under **Advanced**): the new
  size is computed without the rotation stored in the video.
- **Linux** and **macOS** versions.
- **Hardware-accelerated** video (NVIDIA NVENC, Intel Quick Sync, AMD AMF), with
  automatic detection.
- FFmpeg built **without any network support** (today the network is blocked at runtime).
- **Signed** installer with a code-signing certificate (no SmartScreen warning).

## Development

### Prerequisites (Windows)

- [Node.js](https://nodejs.org/) 20 or later (with npm)
- [Rust](https://rustup.rs/) (the exact version is pinned in `rust-toolchain.toml`:
  rustup installs it by itself)
- Visual Studio Build Tools with the **Desktop development with C++** workload
- WebView2 Runtime

With winget:

```powershell
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools --override "--wait --passive --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

### Setup

```powershell
npm install
npm run setup:ffmpeg      # downloads FFmpeg/FFprobe (LGPL) and verifies the SHA-256
```

The script `scripts/setup-ffmpeg.ps1` (or `scripts/setup-ffmpeg.sh` on Linux/macOS)
downloads the FFmpeg build pinned in `scripts/ffmpeg-manifest.json`, verifies its SHA-256
hash and copies only the needed files into `src-tauri/binaries/ffmpeg/`. The binaries are
not in the repository. The download happens **at development time only**: the app never
downloads anything.

### Commands

```powershell
npm run tauri dev         # start the app in development mode
npm run dev               # interface only, in the browser, with a mocked backend (design)
npm test                  # privacy, licenses, interface tests (Vitest) and Rust tests
npm run check:privacy     # privacy check only
npm run licenses          # regenerate THIRD_PARTY_LICENSES
npm run cli -- targets "C:\path\video.mp4"              # formats offered
npm run cli -- plan "C:\path\video.mp4" --to webm         # decisions, warnings, estimate
npm run cli -- convert "C:\path\song.wav" --to mp3 --preset balanced
npm run cli -- convert "C:\path\photo.heic" --to jpg --max-size 1920
```

`konvert-cli` is a development tool that uses the same engine as the app; it is not
installed with Konvert.

### Structure

```
crates/konvert-core/   domain logic in Rust (no Tauri dependency): probing, format
                       catalog, planning, estimates, FFmpeg command builder, execution,
                       unique names, temporary files
crates/konvert-cli/    development CLI on top of konvert-core
src-tauri/             Tauri app: IPC commands, queue, preferences, log, permissions,
                       configuration and installer hooks (windows/installer-hooks.nsh)
src/                   React + TypeScript interface (Tailwind, Motion)
src/bindings/          TypeScript types generated from the Rust types (ts-rs, during `cargo test`)
scripts/               FFmpeg setup, privacy check, network monitor, licenses, release
```

## Building a release

```powershell
npm run release
```

The script `scripts/build-release.ps1`:

1. checks that the version matches in `package.json`, `Cargo.toml` and
   `src-tauri/tauri.conf.json`;
2. installs the exact npm dependencies (`npm ci`) and the verified FFmpeg build;
3. runs the privacy check, the license check and every test;
4. builds the NSIS installer with local paths remapped and verifies that the executable
   contains neither the user name nor folders of the build machine;
5. copies the installer and its `.sha256` file into `release/`.

Every input is pinned (Rust toolchain, `Cargo.lock`, `package-lock.json`, FFmpeg archive
with its hash). The first build downloads the NSIS tools used by Tauri once.
Publish both the installer and the `.sha256` file on the Releases page.

## Licenses

Konvert is released under the [MIT](LICENSE) license.

It bundles FFmpeg (BtbN build `win64-lgpl-shared`), distributed under the **LGPL version 3
or later**. FFmpeg runs as a separate program and its files are installed unmodified in
the `ffmpeg` folder of the installation, where they can be replaced. The build contains no
GPL components (no libx264/libx265) and no non-free ones. The exact FFmpeg source commit
and the build recipe are listed in [`THIRD_PARTY_LICENSES`](THIRD_PARTY_LICENSES) and in
`scripts/ffmpeg-manifest.json`.

**H.264 patents**: H.264 video is encoded with OpenH264, whose BSD license covers the
source code only. **H.264 patents are not covered by the BSD license of OpenH264**
(Cisco's patent coverage applies only to the OpenH264 binaries distributed by Cisco
itself). Some codecs (H.264, HEVC, AAC) may be subject to patents in some countries: whoever
uses or redistributes Konvert is responsible for any licenses that may be required.

Every dependency, with its license and full text, is in
[`THIRD_PARTY_LICENSES`](THIRD_PARTY_LICENSES) (also included in the installation and
viewable from **Settings → About → Third-party licenses**).
