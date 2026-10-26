# Konvert
Audio, video and image file converter with a modern interface.

Konvert converts audio, video and image files from one format to another **entirely on
your computer**: no network connections, no telemetry, no account.

Platforms: Windows 10/11 (64-bit), macOS 11 or later on Apple Silicon (M1 and later),
Linux x86_64. Linux arm64 packages are coming in a later release.

## How to use it

1. Drag one or more audio, video or image files into the window (or use **Choose files…**,
   Ctrl+O / ⌘O, or right-click the file → **Open with** → **Konvert**).
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
If you close the window (or quit with ⌘Q) during a conversion, Konvert asks for
confirmation.

macOS-style interface in English or Italian, with light/dark theme (automatic or manual)
and the Mica effect on Windows 11.

## Formats

Konvert only offers the conversions that make sense for the file you give it:

| Input file | Formats offered |
| --- | --- |
| Audio | MP3, M4A (AAC), ALAC (M4A), WAV, FLAC, OGG (Vorbis), OPUS, WMA, AIFF; more formats: AAC, AC3, E-AC3, MP2, CAF (ALAC), MKA |
| Video with audio | MP4, MKV, MOV, WebM, AVI, GIF, animated WebP; more formats: TS, MPG (MPEG-2), 3GP, FLV, APNG. Also **audio only** in every audio format and **frames** as JPG, PNG or WebP |
| Video without audio | The same, without the audio formats |
| Picture (JPG/JPEG, PNG, WebP, AVIF, HEIC/HEIF, BMP, TIFF, ICO) | JPG, PNG, WebP, AVIF; more formats: BMP, TIFF, GIF, ICO |

Less common formats are behind **More formats**, so the usual choices stay in front.

- When the streams are already compatible with the chosen format (e.g. MP4 H.264/AAC →
  MKV, TS or FLV), they are copied **without re-encoding**: instant and lossless.
- Clear warnings when a conversion loses quality or cannot bring it back (e.g. MP3 → WAV)
  and when cover art, subtitles or extra audio tracks are dropped.
- Metadata and cover art are kept where the format allows it (MP3, M4A, ALAC, FLAC).
- H.264 video with OpenH264, VP9 for WebM, AV1 (SVT-AV1) among the advanced options; MPG
  uses MPEG-2 video with MP2 audio, like DVDs.
- ALAC and AAC both use the `.m4a` extension: the cards say which is which
  (**ALAC · M4A · Lossless**). CAF holds Apple Lossless (FFmpeg cannot write AAC or Opus
  into CAF). MKA keeps the original soundtrack whenever it can.
- Animated WebP and APNG work like GIF (frame rate and size limits, no sound) and keep
  transparency; APNG is lossless, so its files can get very large.
- Videos recorded with the phone upright keep their proportions when they are resized.

Pictures from a video (**Frames**):

- **One frame**: choose the moment with a slider (the middle of the video by default).
- **A series**: one picture every 0.5 s to 1 min, or every frame. The pictures go into a new
  folder named after the video (`Holiday/Holiday-001.jpg`, `Holiday-002.jpg`, …), never
  over an existing one; Konvert tells you how many pictures to expect.

Pictures:

- Photos are turned the right way up (EXIF orientation, HEIC rotation), and HEIC photos from
  phones, stored as a grid of tiles, are put back together.
- Pictures keep their size unless you choose a maximum size under **Advanced**.
- Transparency is kept in PNG, WebP, AVIF, TIFF, GIF and ICO; in JPG and BMP, transparent
  areas become white.
- **ICO** makes a Windows icon with the standard sizes 16, 32, 48 and 256 px (only the ones
  that fit the picture, never enlarged); a picture that is not square is centered on a
  transparent square.
- GIF pictures have at most 256 colors: fine for logos, not for photos.
- EXIF data (camera, date, location) is kept when a JPEG, PNG or WebP picture becomes a PNG;
  the other conversions leave it out, and Konvert tells you so before converting.
- Animated GIF, WebP and PNG files are treated as videos.

The format table is declarative: `crates/konvert-core/src/formats.rs`.

## Installation

From the [Releases](https://github.com/itaAle/Konvert/releases) page, download the
package for your system **and its `.sha256` file**, then **verify the checksum** (see
below) before opening the package.

| System                 | Package                                        |
| ---------------------- | ---------------------------------------------- |
| Windows 10/11 (64-bit) | `Konvert_<version>_x64-setup.exe`              |
| macOS, Apple Silicon   | `Konvert_<version>_aarch64.dmg`                |
| Debian, Ubuntu, Mint…  | `Konvert_<version>_amd64.deb`                  |
| Fedora, openSUSE…      | `Konvert-<version>-1.x86_64.rpm`               |
| Any Linux distribution | `Konvert_<version>_amd64.AppImage`             |

The Linux packages are for x86_64 (64-bit Intel/AMD) computers. Packages for arm64
(`_arm64.deb`, `.aarch64.rpm`, `_aarch64.AppImage`) are coming in a later release.

**Windows.** Run the installer: it installs Konvert for your user only (no administrator
rights), creates the Start menu entry and adds Konvert to the **Open with** menu of the
supported formats, without changing your default programs. Konvert uses Microsoft Edge
WebView2, which is already part of Windows 11 and of an up-to-date Windows 10. If it is
missing, the installer tells you: install the "WebView2 Runtime" from the Microsoft
website. The installer never downloads anything.

**macOS.** Open the `.dmg` and drag Konvert into **Applications**. Konvert appears in
Finder's **Open With** menu for the supported formats, without becoming the default app.
The app is not notarized by Apple yet (see [macOS Gatekeeper warning](#macos-gatekeeper-warning)).

**Linux.** Install the `.deb` (`sudo apt install ./Konvert_<version>_amd64.deb`) or the
`.rpm` (`sudo dnf install ./Konvert-<version>-1.x86_64.rpm`): the package manager adds the
system libraries Konvert needs (WebKitGTK) and Konvert appears in the applications menu and
in the **Open with** menu of the supported formats. The AppImage needs no installation:
make it executable (`chmod +x Konvert_*.AppImage`) and run it.

### Verifying the SHA-256 checksum

The `.sha256` file contains the fingerprint of the package. On Windows, in PowerShell, in
your downloads folder:

```powershell
(Get-FileHash .\Konvert_0.3.0_x64-setup.exe -Algorithm SHA256).Hash
Get-Content .\Konvert_0.3.0_x64-setup.exe.sha256
```

The two fingerprints must match (upper/lower case does not matter). Alternatively:
`certutil -hashfile Konvert_0.3.0_x64-setup.exe SHA256`. On Linux, in the folder with
both files: `sha256sum -c <package>.sha256`; on macOS: `shasum -a 256 -c <package>.sha256`.
If they do not match, do not open the file.

### Windows SmartScreen warning

Until the installer is signed with a code-signing certificate, Windows SmartScreen may
show **"Windows protected your PC"** (unrecognized app). To proceed safely:

1. download the installer **only** from the Releases page of this repository;
2. verify the checksum as described above;
3. in the SmartScreen window choose **More info** → **Run anyway**.

The source code is public: anyone can review it and rebuild the installer
(see [Building a release](#building-a-release)).

### macOS Gatekeeper warning

Konvert for macOS is signed ad hoc but not notarized (that needs a paid Apple Developer
account), so the first time you open it macOS says that it **cannot verify** Konvert. To
proceed safely:

1. download the `.dmg` **only** from the Releases page of this repository and verify the
   checksum as described above;
2. open Konvert once and close the warning (do not move it to the Bin);
3. go to **System Settings → Privacy & Security**, scroll down and click **Open Anyway**
   next to the message about Konvert, then confirm.

macOS remembers the choice: later launches open normally. Alternatively, in Terminal:
`xattr -dr com.apple.quarantine /Applications/Konvert.app`.

## Updates

Konvert **does not check for or download updates** (neither automatically nor on
request). To update:

1. **Settings → About** shows the installed version and the **Releases page** link, which
   opens in your browser only when you click it;
2. download the new package, verify the checksum and install it over the previous
   version (macOS: replace the app in Applications; AppImage: replace the file).

Your preferences are kept. On Windows, if the installer offers to uninstall the previous
version first, leave the "Delete the application data" checkbox **unchecked**.

## Uninstalling

- **Windows**: **Settings → Apps → Installed apps → Konvert → Uninstall**. The **Delete the
  application data** checkbox (unchecked by default) also removes your preferences and the
  debug log, if any. The "Open with" entries are removed.
- **macOS**: move Konvert from Applications to the Bin. Preferences and log, if you want
  to remove them too: `~/Library/Application Support/app.konvert.desktop` and
  `~/Library/Logs/app.konvert.desktop`.
- **Linux**: `sudo apt remove konvert` or `sudo dnf remove konvert` (AppImage: delete the
  file). Preferences and log: `~/.config/app.konvert.desktop` and
  `~/.local/share/app.konvert.desktop`.

## Privacy

Konvert works entirely on your computer. No data leaves your device.

**Network**
- No network connections, not even at startup: no telemetry, analytics, crash
  reporting, update checks, licensing or accounts.
- Fonts, icons and libraries are bundled with the app: no CDN or external service.
- Strict Content Security Policy (`default-src 'self'`, connections only to the internal
  IPC), no networking Tauri plugin (updater, http, …), WebView2 interface (Windows) started
  with its background networking features disabled.
- FFmpeg and FFprobe run with `-protocol_whitelist file`: they can open local files only,
  never network addresses. On macOS FFmpeg is also built without any network protocol.
- The only web address the app knows is the releases page, which opens in your browser
  only if you click it.

**Data on disk**
- No history of converted files, names, paths or thumbnails.
- The only saved settings are your preferences (language, appearance, default quality,
  destination folder, debug mode) in a readable JSON file, `preferences.json`, in
  `%APPDATA%\app.konvert.desktop` (Windows), `~/Library/Application Support/app.konvert.desktop`
  (macOS) or `~/.config/app.konvert.desktop` (Linux). No installation or device identifier.
- Files being processed live in a dedicated temporary folder (`Konvert` in the system
  temporary folder) and are deleted when the job ends, when it is cancelled or, after a
  crash, at the next launch.
- The debug log is **off by default**. If you turn it on in Settings, it stays in
  `konvert.log` (`%LOCALAPPDATA%\app.konvert.desktop\logs` on Windows,
  `~/Library/Logs/app.konvert.desktop` on macOS, `~/.local/share/app.konvert.desktop/logs`
  on Linux), without file names or paths (replaced by placeholders), and it is never sent
  anywhere.

**How to verify it**
- **Resource Monitor** (`resmon`) → **Network** tab on Windows, **Activity Monitor** →
  **Network** on macOS: while you use the app, Konvert, its web view and FFmpeg do not
  appear among the processes with network activity.
- **Wireshark**: with Konvert in use, no traffic generated by the app shows up.
- The script `scripts/monitor-network.ps1` (Windows) watches the sockets of Konvert and of
  all its child processes (WebView2, FFmpeg):
  `powershell -ExecutionPolicy Bypass -File scripts/monitor-network.ps1 -Seconds 60`
  while you use the app (exit code 0 = no connection). On Linux and macOS,
  `bash scripts/monitor-network.sh --seconds 60 <path of the konvert executable>` does the
  same with strace (Linux) or lsof (macOS); the CI runs it on every build.
- `npm run check:privacy` fails if HTTP/TLS/WebSocket or telemetry libraries appear in the
  dependency graph, if the CSP is loosened, if the installer would download anything or
  if unauthorized external addresses appear in the code.

Every dependency and its license is listed in
[`THIRD_PARTY_LICENSES`](THIRD_PARTY_LICENSES): none of them makes network traffic in Konvert.

## Future features

Ideas collected for upcoming updates:

- **Hardware-accelerated** video (NVIDIA NVENC, Intel Quick Sync, AMD AMF, Apple
  VideoToolbox), with automatic detection.
- FFmpeg built **without any network support** on Windows and Linux too (today the network
  is blocked at runtime there; the macOS build already has no network protocol).
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

### Prerequisites (macOS, Apple Silicon)

- Xcode command line tools (`xcode-select --install`), Node.js 20 or later, rustup.

### Prerequisites (Linux)

- Node.js 20 or later, rustup, and the libraries Tauri needs. On Debian/Ubuntu:

```bash
sudo apt install build-essential libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libxdo-dev patchelf file
```

### Setup

```bash
npm install
npm run setup:ffmpeg      # downloads FFmpeg/FFprobe (LGPL) and verifies the SHA-256
```

`npm run setup:ffmpeg` runs `scripts/setup-ffmpeg.ps1` on Windows and
`scripts/setup-ffmpeg.sh` on Linux and macOS. It downloads the FFmpeg build pinned in
`scripts/ffmpeg-manifest.json`, verifies its SHA-256 hash and copies only the needed files
into `src-tauri/binaries/ffmpeg/`. The binaries are not in the repository. The download
happens **at development time only**: the app never downloads anything.

Windows and Linux use the LGPL builds of [BtbN/FFmpeg-Builds](https://github.com/BtbN/FFmpeg-Builds).
No LGPL build exists for macOS on Apple Silicon, so `scripts/build-ffmpeg-macos.sh` makes
one from source, with the same library versions and `--disable-network`. The
**FFmpeg for macOS** workflow runs it; its archive and the archive of its sources are
published in the FFmpeg mirror release, and the manifest pins the archive's SHA-256.

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
npm run cli -- convert "C:\path\video.mp4" --to png --every 2   # a series of frames
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
                       configuration (tauri.conf.json, plus tauri.macos/linux.conf.json
                       for those bundles) and installer hooks (windows/installer-hooks.nsh)
src/                   React + TypeScript interface (Tailwind, Motion)
src/bindings/          TypeScript types generated from the Rust types (ts-rs, during `cargo test`)
scripts/               FFmpeg setup and macOS build, privacy check, network monitors,
                       licenses, release
.github/workflows/     CI on Windows, macOS and Linux; release packages; macOS FFmpeg build
```

## Building a release

```bash
npm run release
```

On Windows it runs `scripts/build-release.ps1`, on Linux and macOS
`scripts/build-release.sh`. Both:

1. check that the version matches in `package.json`, `Cargo.toml` and
   `src-tauri/tauri.conf.json`;
2. install the exact npm dependencies (`npm ci`) and the verified FFmpeg build;
3. run the privacy check, the license check and every test;
4. build the packages (NSIS installer; `.dmg`; `.deb`, `.rpm` and AppImage) with local
   paths remapped and verify that the executable contains neither the user name nor
   folders of the build machine (and, on Linux and macOS, that FFmpeg is executable inside
   every package);
5. copy the packages and their `.sha256` files into `release/`.

Every input is pinned (Rust toolchain, `Cargo.lock`, `package-lock.json`, FFmpeg archive
with its hash). The first build downloads the NSIS or AppImage tools used by Tauri once.

The macOS and Linux packages are built by the **Release packages** GitHub Actions workflow
(started by hand), which also runs the network monitor on the finished app. On macOS it
signs FFmpeg and the app ad hoc. If the repository secrets listed at the top of
`.github/workflows/release.yml` exist (Apple Developer ID certificate and account), it
signs with the Developer ID and notarizes the app and the `.dmg` instead. Publish every package with its `.sha256` file on the
Releases page.

## Licenses

Konvert is released under the [MIT](LICENSE) license.

It bundles FFmpeg (BtbN builds `win64-lgpl-shared`, `linux64-lgpl` and `linuxarm64-lgpl`;
on macOS the build made by `scripts/build-ffmpeg-macos.sh`), distributed under the **LGPL
version 3 or later**. FFmpeg runs as a separate program and its files are installed
unmodified in the `ffmpeg` folder of the application, where they can be replaced. The build contains no
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
