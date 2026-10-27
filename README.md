# Konvert
Convert audio, video, images and PDFs entirely on your computer, with a modern interface.

Konvert converts between many audio, video and image formats, takes frames from videos,
turns photos into PDFs and PDF pages into pictures, splits, turns and rearranges the pages
of a PDF, **compresses** files in their own format (down to a size you choose) and lets you
keep or remove EXIF data and locations. Everything happens **on your computer**: no network
connections, no telemetry, no account.

Platforms: Windows 10/11 (64-bit), macOS 11 or later on Apple Silicon (M1 and later),
Linux x86_64 and arm64.

## How to use it

1. Drag one or more audio, video, image or PDF files into the window (or use **Choose files…**,
   Ctrl+O / ⌘O, or right-click the file → **Open with** → **Konvert**).
2. Konvert analyzes them and offers only the formats that make sense (for several files:
   the ones they all have in common).
3. Pick the format and the quality (**Best quality**, **Balanced**, **Small file**): the
   estimated size and any warnings appear right away. Under **Advanced** you can adjust
   bitrate, sample rate, channels, resolution, frame rate and codec (for pictures: quality
   and maximum size).
4. Press **Convert**: progress for each file and overall, time remaining, and **Cancel**
   at any moment. When it is done you can open the files or show them in their folder.

To make files smaller instead, pick **Compress** above the formats (see
[Compress](#compress)).

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
| Picture (JPG/JPEG, PNG, WebP, AVIF, HEIC/HEIF, BMP, TIFF, ICO, SVG, JPEG XL¹) | JPG, PNG, WebP, AVIF, **PDF**; more formats: BMP, TIFF, GIF, ICO, JPEG XL¹ |
| PDF document | Its **pages as pictures** (PNG, JPG, WebP); a **new PDF** with pages taken out, turned or moved, or **split** into several PDFs; together with other PDFs or pictures, **one PDF** |

¹ JPEG XL needs FFmpeg's libjxl, which the Windows and Linux builds have and the macOS
build does not: Konvert checks the engine and offers JPEG XL only where it works.

Less common formats are behind **More formats**, so the usual choices stay in front.

- When the streams are already compatible with the chosen format (e.g. MP4 H.264/AAC →
  MKV, TS or FLV), they are copied **without re-encoding**: instant and lossless.
- Clear warnings when a conversion loses quality or cannot bring it back (e.g. MP3 → WAV)
  and when cover art, tags, subtitles or extra audio tracks are dropped.
- Tags (title, artist, album…) are kept wherever the format has room for them; cover art
  in MP3, M4A, ALAC, FLAC, AIFF, OGG and Opus. Raw AC3, E-AC3 and MP2 files cannot hold
  tags, and Konvert says so.
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
- **EXIF data** (camera, date, location) is kept in JPG, PNG, WebP and AVIF, from JPEG, PNG,
  WebP, HEIC and AVIF photos: Konvert copies it itself (FFmpeg does not), sets the
  orientation to normal (the pixels are already turned) and leaves out the old thumbnail.
  Other formats leave it out, and Konvert tells you so before converting.
- **Remove the location**: when a photo or a video records where it was made (GPS data,
  phone video location), a switch lets you leave it out of the converted file. It is off
  by default; when on, the coordinates are overwritten, not just unlinked.
- **SVG** drawings are drawn by Konvert (with resvg, the same on every system) and then
  converted like any picture. They scale without loss: **Maximum size** can also enlarge
  them, and an icon gets every size up to 256 px.
- Animated GIF, WebP and PNG files are treated as videos.
- Camera RAW files (DNG, CR2, NEF…) are not supported: FFmpeg cannot develop them and no
  small, permissively licensed alternative does it well enough.

PDF documents:

- **Pictures → PDF**: one picture per page, on A4 or Letter paper (orientation automatic or
  chosen) or on a page shaped like the photo, with no, small or normal margins. Photos are
  turned the right way up; the quality setting decides the picture quality and resolution
  on the page (**Best quality**: full size, **Balanced**: 300 dpi, **Small file**: 150 dpi).
  Several photos go into **one PDF** in the order of the list (or, if you turn that off,
  one PDF each).
- **PDF → pictures**: every page or only some (`1-3, 5, 8-`), as PNG, JPG or WebP at 72 to
  600 dpi, drawn on white. One page is saved next to the PDF (`Report-page-05.png`),
  several go into a new folder (`Report/Report-page-01.png`, …); the word between name and
  number follows the language of the interface (`page`, `pagina`).
- **Edit a PDF** (choose **PDF** for a single PDF): its pages appear as thumbnails, drawn on
  your computer. Drag a page to move it, turn it left or right, take it out (or put it
  back), or keep only some pages (`1-3, 5, 8-`); the keyboard works too (arrow keys,
  Ctrl + arrow keys to move, R to turn, Delete to take out, S to split after a page).
  The result is a new PDF; the original is never changed.
- **Split a PDF**: one new PDF at every page, every 2, 3, 4, 5, 10 or 20 pages, or where you
  click the scissors between two pages, saved in a new folder named after the document
  (`Report/Report-pages-01-03.pdf`, `Report/Report-page-04.pdf`, …). Edits and splits
  combine: take out, turn and move pages, then split the result.
- **Combine**: PDFs and pictures become one PDF, in the order of the list (move the files
  up and down), named after the first file. Pages keep their size and rotation.
- Pages are copied as they are, never re-compressed: only what they use (fonts, pictures)
  goes into the new PDF, so a few pages taken from a large document make a small file.
  Links to pages that are left out are removed. Bookmarks, form fields, attachments and
  the document properties of the original PDFs are not carried over (Konvert says so
  when a PDF has bookmarks or a form).
- PDFs created by Konvert contain no author, title, dates, file names or paths: only
  "Konvert" as the producing program.
- PDFs that need a password to be opened are not supported: Konvert says so and does not
  ask for passwords.
- Pages are drawn by hayro, a PDF renderer written in Rust: very unusual PDFs may look
  slightly different than in other readers.

The format table is declarative: `crates/konvert-core/src/formats.rs`.

## Compress

**Compress** makes a smaller copy of each file **in its own format**, for example a PDF that
is too large for an upload form. It works on several files at once, even of different kinds.

- **Light**, **Strong** (recommended) or **Maximum**: from hardly visible changes to the
  smallest file that is still readable (Maximum lowers the resolution too).
- **Stay under a size** (in MB): Konvert tries stronger settings until the file fits. If it
  cannot get there, it keeps the smallest result and says so.
- The copy is saved as `name-compressed.ext` (`name-compresso.ext` in Italian) and is
  **never larger than the original**: if nothing can be gained, no file is written.

| File | How it gets smaller |
|------|---------------------|
| PDF | Pictures are scaled to the resolution they have on paper (about 200, 144 or 96 dpi) and stored again: photos as JPEG, screenshots and charts without loss or with 256 colors, gray pictures without color channels. Unused objects, duplicates, metadata and page thumbnails go; the rest is recompressed and packed into object streams. **Text, drawings, links, bookmarks and forms stay as they are.** |
| PDF, only if you allow it | When nothing else is small enough, pages become pictures: the text can no longer be selected or searched, links and forms are lost. Off by default. |
| JPG, WebP, AVIF, JPEG XL¹ | Lower quality; Strong limits the long side to 2560 px, Maximum to 1600 px (and leaves out the EXIF data). |
| PNG | Every pixel kept, then 256 colors (Strong, unless the picture has soft transparency; Maximum). |
| Video | Lower bit rate; Strong limits the picture to 1080p, Maximum to 720p at 30 frames per second with mono sound. With a size limit, the bit rate comes from the duration. |
| Animated GIF, WebP, APNG | Narrower, fewer frames per second. |
| Audio | Lower bit rate (mono at Maximum); WAV, FLAC, AIFF and ALAC stay lossless: 16 bits, then a lower sample rate and mono. |

HEIC, BMP, TIFF, ICO and SVG files cannot be compressed in their own format: convert them
instead (for example HEIC → JPG).

## Installation

From the [Releases](https://github.com/itaAle/Konvert/releases) page, download the zip for
your system. Each zip holds the package **and its `.sha256` file**: extract both and
**verify the checksum** (see below) before opening the package.

| System                 | Download                                | Package inside                         |
| ---------------------- | --------------------------------------- | -------------------------------------- |
| Windows 10/11 (64-bit) | `Konvert_<version>_Windows_x64.zip`     | `Konvert_<version>_x64-setup.exe`      |
| macOS, Apple Silicon   | `Konvert_<version>_macOS_arm.zip`       | `Konvert_<version>_aarch64.dmg`        |
| Debian, Ubuntu, Mint…  | `Konvert_<version>_Linux_x64_deb.zip`   | `Konvert_<version>_amd64.deb`          |
| Fedora, openSUSE…      | `Konvert_<version>_Linux_x64_rpm.zip`   | `Konvert-<version>-1.x86_64.rpm`       |
| Any Linux distribution | `Konvert_<version>_Linux_x64_AppImage.zip` | `Konvert_<version>_amd64.AppImage`  |

For Linux on arm64 computers (e.g. Raspberry Pi 4/5 with a 64-bit system) take the
`Linux_arm` zips instead: `_arm64.deb`, `.aarch64.rpm` and `_aarch64.AppImage` packages.

**Windows.** Run the installer: it installs Konvert for your user only (no administrator
rights), creates the Start menu entry and adds Konvert to the **Open with** menu of the
supported formats, without changing your default programs. Konvert uses Microsoft Edge
WebView2, which is already part of Windows 11 and of an up-to-date Windows 10. If it is
missing, the installer tells you: install the "WebView2 Runtime" from the Microsoft
website. The installer never downloads anything.

**macOS.** Open the `.dmg` and drag Konvert into **Applications**. Konvert appears in
Finder's **Open With** menu for the supported formats, without becoming the default app.
The app is not notarized by Apple yet (see [macOS Gatekeeper warning](#macos-gatekeeper-warning)).

**Linux.** Install the `.deb` (`sudo apt install ./Konvert_<version>_amd64.deb`, or
`_arm64.deb`) or the `.rpm` (`sudo dnf install ./Konvert-<version>-1.x86_64.rpm`, or
`.aarch64.rpm`): the package manager adds the
system libraries Konvert needs (WebKitGTK) and Konvert appears in the applications menu and
in the **Open with** menu of the supported formats. The AppImage needs no installation:
make it executable (`chmod +x Konvert_*.AppImage`) and run it.

### Verifying the SHA-256 checksum

The `.sha256` file contains the fingerprint of the package. On Windows, in PowerShell, in
your downloads folder:

```powershell
(Get-FileHash .\Konvert_0.5.0_x64-setup.exe -Algorithm SHA256).Hash
Get-Content .\Konvert_0.5.0_x64-setup.exe.sha256
```

The two fingerprints must match (upper/lower case does not matter). Alternatively:
`certutil -hashfile Konvert_0.5.0_x64-setup.exe SHA256`. On Linux, in the folder with
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
- SVG drawings are drawn by a library without network code, and a drawing may not even
  read other files: pictures it names are ignored, only pictures embedded in it are drawn.
- PDF files are read and written by Rust libraries without network code (hayro, lopdf):
  links, scripts, forms and embedded files in a PDF are never opened or run.
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
                       EXIF, SVG and PDF handling, unique names, temporary files
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
