# Konvert: context for continuing development

> Context for coding agents (Claude Code) and for anyone resuming the work from another computer.
> This file is versioned in the public repository: **never** put secrets, names, personal emails,
> machine paths or other private data in it.

## What it is
Konvert is an Apple-style audio/video/image/PDF converter for 64-bit Windows 10/11, macOS 11+ on
Apple Silicon and Linux (x86_64 and arm64).
It uses Tauri 2.11, React 19 with TypeScript, and bundles FFmpeg/FFprobe.
It works **entirely offline, on the user's computer**.
**Public** repository: https://github.com/itaAle/Konvert (branch `main`).
Releases ship a Windows NSIS installer, macOS/Linux packages and a `.sha256` file for each: `v0.1.0` to `v0.4.0` are published (0.4.0: more formats, EXIF, SVG, PDF; first release with Linux arm64); `v0.5.0` (PDF page tools) is next.
The `ffmpeg-n9.0.2-36` pre-release mirrors the FFmpeg build and its sources (required by the LGPL).

## Binding rules (from the owner)
- **Absolute privacy: zero network connections at runtime.** No telemetry, updater, CDN, crash reporting, fetch or network plugins. When in doubt, always pick the option that does not touch the network.
  `npm run check:privacy` fails if HTTP/TLS/WebSocket/telemetry crates or packages appear in the desktop dependency graph, if the CSP is loosened, if the installer would download anything, or if unauthorized external URLs appear in the code.
- **Public repository:**
  - never secrets, absolute machine paths, personal data or heavy files in the history;
  - **never commit the FFmpeg/FFprobe binaries**: they are ignored by Git and downloaded with `npm run setup:ffmpeg`.
- **Git identity:** the repository's local config uses `Ale <118929447+itaAle@users.noreply.github.com>`. Never use personal emails.
- **Commits:** small, Conventional Commits style (`feat(core): …`, `feat(ui): …`, `docs: …`), in English, with the trailer `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
- **Never push unless the owner asks.** Force pushes are run by the owner (auto mode blocks them).
- **Text visible on GitHub** (README, this file, commits, release notes) is **in English**. The app UI is bilingual (it/en).
- The owner speaks Italian (answer in Italian) and wants:
  - a plan before implementing;
  - a summary at every milestone;
  - green build and tests before every commit.
- **Do not delete the user's real preferences** (`%APPDATA%\app.konvert.desktop\preferences.json`): `outputDir` may point to a real folder of theirs, so test conversions from the app would end up there. In tests use an explicit destination (CLI `--out-dir`, integration tests).

## Architecture
Cargo workspace (`Cargo.toml`, resolver 3, toolchain pinned to 1.99.1 in `rust-toolchain.toml`):

- `crates/konvert-core/`: pure domain logic, no Tauri.
  - `formats.rs`: declarative catalog with `TargetFormat`, `FORMATS` (FormatSpec), `RULES`, `available_targets`, `is_offered`. Pictures are `FormatKind::Image` with a single `ImageCodec` (JPG, PNG, WebP, AVIF, BMP, TIFF, GIF, ICO).
    - A format's id is its serialized `TargetFormat` name (`TargetFormat::id`/`from_id`, CLI `--to alac`). Formats may share an extension (ALAC/AAC `.m4a`, still/animated WebP and GIF): `source_format(media, input)` uses the extension, the kind of content and `detect_codecs`.
    - `secondary: true` formats are shown under "More formats" in the UI.
    - Groups (`TargetGroup`): video, audio, audioOnly, image, frames (pictures from a video), pages (pictures of PDF pages), document (PDF). Animations (GIF, animated WebP, APNG) share the GIF limits (`VideoCodec::is_animation`).
  - `probe.rs`: ffprobe (including `-show_stream_groups`) → `MediaInfo`.
    - Still pictures (`image2`/`*_pipe` demuxers, HEIF/AVIF brands) are `MediaKind::Image` with `PictureInfo` (alpha, AVIF alpha plane, HEIC grid, EXIF); animated GIF/APNG/WebP stay videos.
    - For non-HEIF pictures a second ffprobe reads the first frame (`apply_first_frame_json`): EXIF orientation and EXIF presence. Stored dimensions are the displayed (already rotated) ones.
    - FFmpeg applies the rotation (EXIF or `irot`) by itself before the filters; `[0:g:N]` in a filter graph returns the reassembled HEIC grid.
    - Video streams also report their displayed size: a quarter-turn display matrix swaps width and height, so resized portrait videos keep their proportions.
  - `request.rs` → `plan.rs` (`plan_conversion`, pure) → `command.rs` (`build_from_plan`, pure FFmpeg arguments) → `convert.rs` (execution).
  - `estimate.rs`: size estimate and warnings.
  - New modules: `exif.rs`, `svg.rs`, `metadata.rs`, `pdf.rs` (see below).
  - Other modules: `naming.rs` (`name (n).ext`, never overwrite), `workspace.rs` (`%TEMP%\Konvert\run-…` with a lock and cleanup after crashes), `process.rs` (kill-on-close Job Object and `CREATE_NO_WINDOW`), `progress.rs`, `disk.rs`, `binaries.rs`, `error.rs`.
  - Stable error codes in `error.rs`: `engine_missing`, `input_not_found`, `output_unavailable`, `not_media`, `corrupt_input`, `unsupported_conversion`, `invalid_pages`, `pdf_encrypted`, `disk_full`, `permission_denied`, `cancelled`, `no_gain` (a smaller copy would not be smaller), `engine_failed`, `io`. Each has an i18n message.
  - FFmpeg always runs with `-protocol_whitelist file`, and inputs are passed as `file:` URLs.
  - Pictures: `plan.rs::plan_picture` → `plan_still` → `ImagePlan` (always re-encoded); `command.rs::picture_args` (one filter graph: grid/alphamerge, scale, white background for JPG/BMP, alpha plane for AVIF, palette for GIF; encoders mjpeg, png, libwebp, libaom-av1 still-picture, bmp, tiff, gif; muxers `image2pipe`/`webp`/`avif`/`gif`/`ico`).
    - ICO: `ImagePlan::icons` (16/32/48/256 px up to the picture's size, each fitted into a transparent square), one PNG stream per size into the `ico` muxer (`command.rs::icon_args`). `.ico` inputs are pictures (largest stream; the first-frame probe selects that stream by index).
    - Frames from videos: `plan_frames` → `ConversionPlan::frames` (`FramePlan`). One frame: `-ss` before `-i`, clamped inside the video. A series: `fps=1/N` (or every frame with `-fps_mode passthrough`) into the `image2` muxer, written in `workspace.job_dir(id)` (`%` escaped in the pattern), then `naming::move_series` creates a new folder `name[ (n)]` with `name-001.ext`…; `ConversionOutcome::files` > 1 means `output` is a folder.
  - EXIF (`exif.rs`, no dependency): `extract` reads the TIFF block from JPEG/PNG/WebP/HEIF/AVIF, `prepare` sets Orientation to 1, drops IFD1 (thumbnail), writes the new pixel size and, for "remove location", zeroes the GPS directory and unlinks it; `insert` writes JPEG APP1, PNG eXIf, WebP EXIF (VP8/VP8L converted to VP8X) or an AVIF `Exif` item (iinf + iloc + iref cdsc, data offsets after `meta` shifted, payload in a new `mdat`). `convert.rs::finish_exif` runs after FFmpeg in the workspace (`ImagePlan::copy_exif`); if the copy fails and the location must go, `strip` removes all EXIF. The probe sets `PictureInfo::exif_copyable` and `MediaInfo::has_location` (GPS, or video `location*`/Apple ISO6709 tags). `little_exif` (no lossy WebP) and `img-parts` (no HEIF) were evaluated and rejected.
  - Location: `ConversionRequest::remove_location` → `ConversionPlan::remove_location` (only if the file has one); for audio/video, `-metadata location= -metadata location-eng= -metadata com.apple.quicktime.location.ISO6709=`.
  - Tags and covers: `FormatSpec::tags` (false for raw AC3/E-AC3/MP2 → `TagsDropped`), `cover_art` (attached picture: MP3, M4A, ALAC, FLAC, AIFF with `-write_id3v2 1`) and `cover_art_tag` (Ogg/Opus: `metadata.rs` builds a FLAC picture block, base64, in an `ffmetadata` file passed as a second input with `-map_metadata:s:a:0 1:g`).
  - Optional encoders: `FormatSpec::requires_encoder` (JPEG XL → libjxl); `engine_encoders` + `formats::supported_targets` filter what the app and the CLI offer (the macOS build has no libjxl). The picker/associations list `.jxl` everywhere but macOS (`LIBJXL_EXTENSIONS` in `commands.rs`).
  - SVG (`svg.rs`, resvg): detected before ffprobe, intrinsic size from usvg; `ImagePlan::render` is the drawing size (vector: `max_dimension` may enlarge, icons get every size); `convert.rs` renders a PNG in the workspace and FFmpeg encodes it. Never reads files named by the SVG (string href resolver disabled), catches panics, caps size at 16384 px / 100 MP. Test SVGs have no `xmlns` URL (the privacy check rejects URLs in code; usvg accepts elements without namespace).
  - PDF (`pdf.rs`: hayro 0.8 reads and draws, lopdf 0.46 without default features writes; pure Rust, no network): `.pdf` inputs are detected before ffprobe as `MediaKind::Document` with `DocumentInfo` (pages, first page size). Password-protected files → `pdf_encrypted` (hayro opens files with an empty user password). `FormatKind::Document` target `Pdf` (no muxer: written by Konvert).
    - Pictures → PDF: `plan_document` places the picture (`pdf::place`: A4/Letter/fit, orientation, margins 0/14/42 pt) and plans a JPEG (preset → full size, 300 or 150 dpi on paper); `convert.rs::run_document` encodes it with FFmpeg in the workspace, then `pdf::assemble`. PDF parts are merged page by page (inherited MediaBox/CropBox/Rotate/Resources copied; outlines, AcroForm, metadata and Info dropped; Info has only `Producer: Konvert`).
    - PDF → pictures: `plan_pages` (`DocumentPlan` pages/dpi, `pdf::parse_pages` `1-3, 5, 8-` → `invalid_pages`); `run_pages` renders on a blocking thread (channel of 2 pages, `AtomicBool` stop, panics caught, 600 dpi and 100 MP caps), PNG written directly, JPG/WebP through FFmpeg. One page → one file next to the PDF; several → `naming::move_series_named` folder `name/name-<label>-NN.ext` (label from `PdfOptions::page_label`, letters only).
    - Several files → one PDF: `CombinedJob` + `run_combined` (named after the first file), IPC `start_combined`.
    - PDF → PDF (page tools, one PDF): `PdfOptions::layout` (pages in the new order, each with a clockwise turn; left-out pages absent; `None` = untouched) and `split` (`none`/`every` + `split_every`/`after` + `split_after` positions). `plan_document` checks them (`pdf::check_layout`, `pdf::split_sizes` → `invalid_pages`) into `DocumentPlan` `pages`/`rotations`/`parts`; `DocumentInfo::extras` (bookmarks or a form) → `PdfExtrasDropped`.
    - `convert.rs::run_edit` → `pdf::edit` on a blocking thread (the source is loaded once, one `Writer` per part, cancellation between parts). `Writer::add_pages` copies only what the chosen pages reach, with new object numbers (no clashes, so merges use it too); references to left-out pages become null, named destinations become explicit, links to left-out pages are removed, page `Metadata`/`PieceInfo`/`LastModified` are dropped, turns add to the (inherited) `Rotate`. lopdf's `prune_objects` is quadratic: do not use it.
    - One document goes next to the others (`name.pdf`, never overwriting); a split goes into a folder `name/name-pages-01-03.pdf`, `name-page-04.pdf` (labels `page_label`/`pages_label`; positions instead of page numbers when pages were reordered).
    - Thumbnails: `pdf::thumbnails` (at most 48 pages per call, longest side ≤ 512 px, PNG `data:` URLs, which the CSP already allows), IPC `pdf_thumbnails`.
  - Compress ("Comprimi", a smaller copy in the file's own format): `ConversionRequest::compress` (`CompressOptions`: level light/strong/max, `max_bytes` limit, `pages_as_pictures`, file `label`) with `target` = the file's own format (`compress::compress_format`: PDF, JPG/PNG/WebP/AVIF/JXL, every video and audio format; not HEIC/BMP/TIFF/ICO/SVG). `plan_conversion` → `compress::plan`; `plan::plan_offered` is the planner without the "is offered" check (PNG→PNG, FLAC→FLAC are "pointless" targets otherwise).
    - Media: `compress::Search` turns the level into ordinary requests (preset Small + `AdvancedOptions`: picture quality/side, video bit rate from bits per pixel and codec efficiency, 1080p/720p, 30 fps, audio bit rate, mono, 16-bit/44.1/22.05 kHz for lossless) plus `Attempt::palette` (PNG 256 colors: palettegen/paletteuse in `picture_args`) and `drop_exif` (Max). With a limit: ladders (pictures: lower quality, then smaller sides; animations; lossless audio) or a bit rate from the duration corrected up to twice by the overshoot (video, lossy audio). `convert.rs::compress_media` keeps the first attempt that fits, else the smallest.
    - PDF (`pdf_compress.rs`, no FFmpeg): a hayro `Device` records the largest placed size of each image XObject (transform maps pixels to points); a second pass decodes each candidate once (`RasterImage::with_rgba`: every filter/color space, alpha from SMask/Mask) and stores it for every step (dpi 200/144/96/72/60/50/40): photos JPEG (`jpeg-encoder`, progressive, 4:2:0), lossless sources also try PNG-predictor Flate and median-cut 256 colors (JPEG must be 25% smaller to win), ≤256 colors or ≤16 grays → indexed Flate 1–8 bits, gray RGB → DeviceGray; kept only if 3% smaller. Skipped: masks, soft masks, `Matte`, JBIG2/CCITT. Then metadata/thumbnails/PieceInfo dropped, Info = `Producer: Konvert`, unreachable objects removed, duplicate streams merged, Flate streams recompressed (level 9), saved with object + xref streams; lopdf writes the xref stream uncompressed, so `pack_xref_stream` rewrites it in place (Flate + PNG Up predictor). `pages_as_pictures` (opt-in) renders pages to JPEG and keeps that document only if smaller.
    - Never larger: `run_compress` returns `no_gain` when the result is not smaller than the file; the copy is `name-<label>.ext` (`compressed`/`compresso`).
  - Tests: unit tests in the modules and `tests/engine.rs` (39 integration tests with real FFmpeg; the JPEG XL one skips itself without libjxl). `tests/fixtures/mod.rs` builds JPEGs with EXIF, grid HEIC files (HEVC tiles made with libkvazaar) and PDFs (plain, with personal metadata, encrypted, with a photo, with a heavy drawing) in memory.
- `crates/konvert-cli/`: development CLI (`npm run cli -- targets|plan|convert|combine …`, PDF options `--page-size/--orientation/--margin/--dpi/--pages`, smaller copies `--compress light|strong|max --max-mb N --pages-as-pictures` without `--to`), not shipped.
- `src-tauri/`: thin IPC layer.
  - `commands.rs` holds 22 commands (`start_combined` makes one PDF from several files, `pdf_thumbnails` draws page thumbnails), also handled in `lib.rs::generate_handler!`, listed in `build.rs::COMMANDS` and granted in `capabilities/default.json`: **a new command must be added in all of them** (a test compares the lists), and its `permissions/autogenerated/*.toml` committed.
  - Other files: `jobs.rs` (sequential queue, `job-update` event; `enqueue_combined` for one PDF from several files), `prefs.rs`, `debug_log.rs` (redacted log, off by default), `launch.rs` (`LaunchFiles` queue for files from argv, the second instance and macOS `RunEvent::Opened`), `lib.rs` (single instance, Mica, close confirmation for × and ⌘Q via `ExitRequested`).
  - `tauri.conf.json`: strict CSP, `freezePrototype`, WebView2 flags without networking, per-user NSIS with `windows/installer-hooks.nsh` ("Open with" via `OpenWithProgids`; on Windows do NOT use Tauri's `fileAssociations`, which take over the default program).
  - `tauri.macos.conf.json` (.app/.dmg, hardened runtime, `fileAssociations` with rank `Alternate`) and `tauri.linux.conf.json` (.deb/.rpm/AppImage, MIME types) are merged over it; both use an opaque window. They may not touch security settings (`check:privacy`).
  - Tests keep the Windows installer's and the macOS/Linux associations' extensions identical to the file picker's `MEDIA_EXTENSIONS`.
- `src/`: React, Tailwind v4 (tokens via `data-scheme`/`data-vibrancy`), Motion, bundled Inter font, lucide icons.
  - `lib/api.ts` (IPC) and `lib/mock.ts` (fake backend, only for `npm run dev` in the browser).
  - `state/queue.ts`: pure queue reducer, with tests. PDF: `pdf` options, `move` (order of the pages), `combines()` (PDF target with several files: always with a PDF among them, for photos unless "One PDF" is off); a combined job shares one `jobId` across items and `resultItems` shows it once. A lone PDF with target PDF is edited (`lonePdf`, `editsPdf`): `pages` (`state/pages.ts` `EditorPage`: order, turn, removed) and `cuts` (page numbers a split follows) become `layout`/`splitAfter` in `requestOf`, never for merged files; removing the PDF forgets them; `canStart` needs a change (`pdfChanged`, `editedDocuments`).
  - `i18n/it.ts` and `en.ts`: typed; a test checks that the keys match.
  - `components/`: DropScreen, FormatGrid ("More formats"), FramesPanel (one frame or a series), PdfPanel (paper, orientation, margins, one PDF), PagesPanel (dpi, page range), PageEditor (thumbnails loaded in view by `lib/useThumbnails.ts`; pointer-event drag, since Tauri's file drop disables HTML drag and drop inside the window on Windows; keys: arrows, Ctrl+arrows, R, Delete, S), SplitPanel (one PDF or split), FileList (move up/down when combining), ui/Segmented, QualityPicker, AdvancedPanel, PlanSummary, ConvertingScreen, DoneScreen (folder outputs), SettingsSheet, …
  - `state/queue.ts::commonTargets` intersects formats by group too: photos and videos never share a picture format (converted photo vs frame).
  - Compress: `FileAnalysis::compress` (the file's own format, `null` when it cannot be compressed) → `canCompress` (every file); `state.compress` (instead of a target) + `state.compression`; `requestFor(state, item)` gives each file its own request (`requestOf` is the shared one of a format). `CompressCard` (above "Convert to"; the same `layoutId` highlight as the format cards), `CompressPanel` (levels, "Stay under" limit in MB with a suggested common limit, PDF pages as pictures), DoneScreen shows the saving and when the limit was not reached. The mock: `?mock=nogain` simulates `no_gain`.
- `src/bindings/`: TypeScript types **generated** by ts-rs during `cargo test`. If you change an exported Rust type, regenerate and commit them.
- `scripts/`:
  - `ffmpeg-manifest.json`: FFmpeg n9.0.2-36 per platform with pinned SHA-256 (BtbN `win64-lgpl-shared`, `linux64-lgpl`, `linuxarm64-lgpl`; `macos-aarch64` is our own build), upstream URL plus the mirror.
  - `setup-ffmpeg.ps1` / `.sh` (the `.sh` must stay compatible with macOS bash 3.2: no `mapfile`, no empty arrays under `set -u`).
  - `build-ffmpeg-macos.sh`: static LGPL build for Apple Silicon, same library commits as BtbN, `--disable-network`; run by `.github/workflows/ffmpeg-macos.yml`.
  - `run-platform-script.mjs`: `npm run setup:ffmpeg` / `npm run release` run the `.ps1` on Windows, the `.sh` elsewhere.
  - `check-privacy.mjs`.
  - `generate-licenses.mjs` (regenerates `THIRD_PARTY_LICENSES` from the crates of every shipped platform; rerun it when dependencies or the version change).
  - `monitor-network.ps1` (Windows) and `monitor-network.sh` (strace on Linux, lsof on macOS).
  - `build-release.ps1` (Windows installer) and `build-release.sh` (macOS/Linux packages; signs FFmpeg for the hardened runtime, checks FFmpeg is executable in each package). 7 steps: version, npm ci, FFmpeg, checks and tests, build with `--remap-path-prefix`, check that the executable contains no local paths, copy to `release/` with SHA-256.
- `.github/`: `ci.yml` (npm test on Windows, macOS arm64, Linux x64/arm64 + network monitor of the app), `release.yml` (manual; .dmg signed ad hoc, or with Developer ID + notarization when the optional `APPLE_*` secrets listed in its header exist; Linux packages; as artifacts), `ffmpeg-macos.yml`, shared `actions/setup`. Actions are pinned by commit SHA.
- Process lifetime: Windows Job Object; Linux `PR_SET_PDEATHSIG`; macOS has no equivalent (an orphan after a crash only writes into the temp run dir, cleaned at next start).

## FFmpeg and licenses
- LGPL-3.0-or-later build, no GPL or nonfree parts: **no libx264/libx265/fdk-aac**.
- Available encoders: libopenh264 (H.264), libvpx/libvpx-vp9, libsvtav1, mpeg4, mpeg2video, gif, libwebp_anim, apng, libmp3lame, aac, alac, ac3, eac3, mp2, libopus, libvorbis, flac, wmav2.
- The build also has picture decoders and encoders (mjpeg, png, libwebp, libaom-av1, bmp, tiff, libkvazaar, …): check them with `ffmpeg -encoders`.
- Every encoder and muxer Konvert uses is native or in all three platform builds, except libjxl and librsvg: present in the BtbN Windows/Linux builds, missing from our macOS build.
- FFmpeg limits found while adding formats: the CAF muxer only takes ALAC/PCM (no AAC or Opus); the Ogg/Opus muxers reject attached pictures (cover art there needs a `METADATA_BLOCK_PICTURE` tag); an image2 output pattern treats `%` in folder names as a pattern, so it must be escaped as `%%`.
- H.264 patents are not covered by OpenH264's BSD license: the README already says so.
- A new Rust or npm library must have a compatible license and pass the privacy check. Then run `npm run licenses`.

## Commands
```
npm install && npm run setup:ffmpeg   # first time
npm run tauri dev                      # app in development mode
npm run dev                            # UI only, in the browser, with the mock backend
npm test                               # privacy + licenses + Vitest + cargo test (everything must pass)
npm run release                        # packages of this platform + .sha256 in release/
```
Note for agent shells: `cargo` may not be on the PATH, so prepend `$env:USERPROFILE\.cargo\bin`.
The owner only has Windows: macOS and Linux are built and tested on GitHub Actions. From Windows, `cargo check/clippy -p konvert-core --target aarch64-apple-darwin` (or `x86_64-unknown-linux-gnu`) works; the app crate cannot be cross-checked.

## Resuming from another computer
1. Prerequisites (Windows): Node.js 20+, rustup (the toolchain installs itself from `rust-toolchain.toml`), Visual Studio Build Tools with "Desktop development with C++", WebView2.
2. `git clone https://github.com/itaAle/Konvert`, then `npm install` and `npm run setup:ffmpeg`.
3. Set the Git identity **for the repository only**: `git config user.name "Ale"` and `git config user.email "118929447+itaAle@users.noreply.github.com"`.
4. `npm test` must be fully green before you start.

## Practical notes for agents
- Auto mode blocks `powershell -ExecutionPolicy Bypass -File …` launched by the agent: run PowerShell code (e.g. UI Automation) inline instead. `npm run release` works.
- Real-app check without the file picker: `npx tauri build --debug --no-bundle`, then start `target/debug/konvert.exe` with the file as argument (same path as "Open with") and read the window with UI Automation (query twice to wake up WebView2's accessibility tree).
- UI in the browser: `npm run dev` (port 1420) with the fake backend; for the agents' preview, create `.claude/launch.json` (ignored by Git) with a `konvert-ui` configuration running `npm run dev` on port 1420; `?files=a.jpg|b.mp4` sets what the file picker returns.
- Drag & drop cannot be automated: ask the owner to try it by hand.
- Before starting a conversion in the real app, read the destination from the path text of the "Save to" row, or from `preferences.json`: the row's × button is named "Accanto al file originale"/"Next to the original" even when a folder is set. With a folder set, conversions go there: do not start them.
- UI Automation: buttons with `aria-pressed` (the scissors) expose `TogglePattern`, not `InvokePattern`; cards and segments expose `SelectionItemPattern`; a page is focused with `SetFocus()` and driven with `SendKeys`.
- Reading the real app with UI Automation: find the window by enumerating the `konvert` process windows with class `Tauri Window` and `AutomationElement.FromHandle` (searching by name from the root fails); redefine the `Add-Type` helper in every PowerShell call and do not name a C# method `Main`.
- The browser pane captures only part of the page at device pixel ratio 2: read it with `read_page`/`get_page_text`, or shift `#root` with a CSS transform just for the screenshot.
- `target/release/konvert.pdb` contains the user's path: never upload `target/` or `.pdb` files.
- FFmpeg 9: a 1×1 crop of 4:2:0 YUV fails (convert to rgba first); `-map 0:g:0` on a HEIC gives a single tile, `[0:g:0]` in a filter graph gives the whole photo.

## Current status
- 2026-10-26: **v0.2.0** on `main` (commits `c437948` to `0e52cd9`, pushed): pictures in core, app, ui and docs; "Open with" fix for .aifc/.alac/.qt/.wave with a test keeping the installer and file picker extensions aligned; version 0.2.0.
  Installer `Konvert_0.2.0_x64-setup.exe`, SHA-256 `5cce9fe6ebf043392bc54c41cb4dac2cb56352b298ae9b578b69cebe7e614b67` (`release/` is ignored by Git: rebuild it with `npm run release`). The owner creates the tag and the GitHub release.
- Picture support:
  Input JPG/PNG/WebP/AVIF/HEIC/HEIF/BMP/TIFF, output JPG/PNG/WebP/AVIF (HEIC output is impossible: FFmpeg has no HEIF muxer).
  Owner's decisions: EXIF handled as FFmpeg does (kept only in PNG; superseded in phase B, Konvert now copies it), BMP/TIFF included, HEIC included.
- Green tests (Windows): 26 Vitest, 118 core unit tests, 16 integration tests, 17 app tests.
- Network verified at zero: no sockets with the release build and with the installed app.
- Verified: installer, "Open with", single instance, preferences kept on reinstall.
- Not yet verified by hand:
  - the "Delete the application data" checkbox on uninstall;
  - the warning when WebView2 is missing;
  - SmartScreen on another computer;
  - the low disk space warning;
  - 0.2.0 installer: upgrade over 0.1.0, "Open with" on pictures, real iPhone HEIC photos (tests use synthetic HEIC files).

- 2026-10-26: **v0.3.0, macOS (Apple Silicon) and Linux support** on `main`. Owner's decisions: macOS without an Apple Developer account for now (ad hoc signature, no notarization, Gatekeeper "Open Anyway" documented in the README); .deb + .rpm + AppImage; Linux x86_64 + arm64.
  - CI green on Windows x64, macOS arm64, Linux x64 and arm64, including the network monitor of the running app (no connection).
  - The macOS FFmpeg build is pinned and mirrored with its sources; the Linux archives are mirrored too (all hashes verified).
  - Release flow: the Windows installer comes from `npm run release` locally; the macOS/Linux packages come from the **Release packages** workflow artifacts (one artifact per package with its `.sha256`; attach the extracted files to the release, not the artifact zips); the owner creates the `v0.3.0` tag and the GitHub release.
  - v0.3.0 ships Windows, macOS arm64 and Linux x64 only: the first Linux arm64 packaging failed (missing `xdg-utils`, fixed). The owner will run **Release packages** with `platform: linux-arm64` later; then restore arm64 in the README (platforms line and installation table).
  - Not verified on real hardware yet: drag & drop and "Open With" on macOS/Linux, Gatekeeper, the Linux desktop integration.

- 2026-10-26: **more formats, phase A** on `main` (pushed 2026-10-27): the stretched portrait video bug is fixed; new outputs ALAC, AAC, AC3, E-AC3, MP2, CAF, MKA, TS, MPG, 3GP, FLV, animated WebP, APNG, BMP, TIFF, still GIF, ICO; frames from videos (one frame or a series in a folder); new inputs .eac3/.mp2/.caf/.apng/.ico.
  - Owner's decisions for the whole plan: runtime encoder detection (JPEG XL only where the engine has libjxl, so not on macOS), SVG through resvg on every platform, no RAW input, PDF in pure Rust (spike first; PDFium only after asking), frames as a "Frames" group in the format grid, series in a subfolder, "remove location" off by default.
  - Green tests (Windows): 28 Vitest, 139 core unit tests, 22 integration tests, 17 app tests.
  - Not verified by hand yet: the new formats in the installed app, "Open with" on the new extensions, drag & drop of videos into the Frames flow.

- 2026-10-26: **more formats, phase B** on `main` (pushed 2026-10-27): cover art in AIFF/OGG/Opus and a "tags dropped" warning; EXIF kept in JPG/PNG/WebP/AVIF (also from HEIC) with "Remove the location" (photos and videos); JPEG XL where FFmpeg has libjxl; SVG input through resvg (new dependency, licenses regenerated). No RAW (owner's decision).
  - Green tests (Windows): 29 Vitest, 159 core unit tests, 26 integration tests, 17 app tests; `cargo check -p konvert-core` passes for macOS and Linux targets.
  - Not verified by hand yet: real iPhone HEIC photos with GPS, EXIF read by Windows Explorer/Photos in the converted files, SVG files with system fonts and complex text.

- 2026-10-26: **more formats, phase C (PDF)** on `main` (pushed 2026-10-27): pictures → PDF (A4/Letter/fit, orientation, margins, one PDF or one each), PDF → PNG/JPG/WebP pictures (dpi, page range), PDFs and pictures combined into one PDF in the list order; `.pdf` in the picker and "Open with". New dependencies hayro and lopdf (licenses regenerated). Spike: hayro drew multilingual text, emoji, gradients, transparency and images correctly, about 10 ms per page.
  - Green tests (Windows): 32 Vitest, 173 core unit tests, 31 integration tests, 17 app tests; `cargo clippy -p konvert-core` is clean for `aarch64-apple-darwin`, `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`.
  - Not verified by hand yet: a conversion started from the app (agents checked analysis, plans and the UI there, but never convert into the owner's destination folder), real-world PDFs (scans, forms, large files), "Open with" on `.pdf`.

- 2026-10-27: **v0.4.0** (phases A, B and C): version bumped in `package.json`, `Cargo.toml` and `tauri.conf.json`; `npm run release` green (all checks and tests, no local paths in the binaries).
  Installer `Konvert_0.4.0_x64-setup.exe` (52 MB), SHA-256 `b7b2e6c60037f225b2f15af4b31c599bb67e6c1f77e3e578c69c0636a157352a` (rebuild with `npm run release`). The macOS/Linux packages come from the **Release packages** workflow run on the pushed version commit; the owner creates the `v0.4.0` tag and the GitHub release.
  - The owner installed and tried the Windows 0.4.0 installer: it works.
  - Published 2026-10-27 (tag `v0.4.0` on `d7d2d1e`) with all platforms, Linux arm64 included. Release assets are zips named `Konvert_<version>_<Windows_x64|macOS_arm|Linux_x64_deb|Linux_x64_rpm|Linux_x64_AppImage|Linux_arm_deb|Linux_arm_rpm|Linux_arm_AppImage>.zip`, each holding the package and its `.sha256` with their original names; the README installation table follows this scheme.

- 2026-10-27: **PDF page tools** on `main` (local commits, not pushed): choosing PDF for a single PDF opens a page editor (thumbnails drawn locally; drag, turn, take out, keep only `1-3, 5`; keyboard shortcuts) and a "Result" section (one PDF, or split at every page, every N pages or at the scissors between pages, into a folder). Pages are copied without re-compression and only with what they use; links to left-out pages are removed, named destinations kept; bookmarks and forms dropped with a warning. No new dependency; no network (thumbnails are local IPC returning `data:` URLs).
  - Green tests (Windows): 40 Vitest, 185 core unit tests, 34 integration tests, 18 app tests.
  - Verified: the UI in the browser preview with the mock (light and dark), the CLI on real PDFs (layout, rotation, split), and the real debug app with UI Automation and a real mouse: thumbnails over IPC (9 and 126 pages, lazy loading), drag inside WebView2, buttons, keys (R, Ctrl+arrows, Home/End), "keep only", split at scissors into a folder (pages, turns and metadata checked), no socket from Konvert or WebView2 (`monitor-network.ps1` logic). Not verified yet: real-world PDFs with forms/bookmarks/links, 1000+ page PDFs.
- 2026-10-27: **v0.5.0** (PDF page tools, readable drop-down lists in the dark theme): version bumped in `package.json`, `package-lock.json`, `Cargo.toml`/`Cargo.lock` and `tauri.conf.json`, licenses regenerated; version commit `31fe327` pushed, `npm run release` green (all checks and tests, no local paths in the binaries).
  Installer `Konvert_0.5.0_x64-setup.exe` (52 MB), SHA-256 `257123f51b78fc466106a72e1752fbbc6b37d521074d0c0cdca2f10013b2ce7f` (rebuild with `npm run release`). The Windows installer comes from `npm run release` locally; the macOS/Linux packages from the **Release packages** workflow run on the pushed version commit; the owner creates the `v0.5.0` tag and the GitHub release.
- 2026-10-27: **Compress** on `main` (local commits, not pushed). Owner's decisions: levels Light/Strong/Max plus an optional MB limit, never larger than the original, PDF compression in pure Rust, PDF pages become pictures only with an explicit switch (off by default). New dependency `jpeg-encoder` (licenses regenerated); `flate2` was already in the tree.
  - Measured on real PDFs (release CLI, about 1 s each): lecture slides 3.9 → 1.1 MB, slides with screenshots 2.3 → 1.5 MB, a summary with figures 1.5 → 0.5 MB; text stays selectable. Fonts are not subset yet (they can be half of a slide deck's size).
  - Verified: the UI in the browser preview with the mock (one PDF, mixed files, `no_gain`). Not verified yet: Compress in the real app, very large scanned PDFs, PDFs with forms.
  - Next (approved by the owner): DOCX → PDF and PDF → DOCX in pure Rust with the Typst engine (spike first: installer size and `check:privacy`).

## Backlog (next work, by priority)
1. **PDF**: page tools done (see Current status). Possible next steps, only after the owner's OK: keep bookmarks when pages are only turned or reordered, page tools on several PDFs combined (pages of every file in one editor).
2. macOS/Linux: verify on real hardware (see Current status), and macOS window vibrancy (needs `macOSPrivateApi`).
3. Hardware encoders NVENC/QSV/AMF/h264_mf/VideoToolbox, with automatic detection.
4. FFmpeg built without any network support on Windows and Linux (macOS already is).
5. Code signing of the Windows installer; Developer ID signing + notarization on macOS (add the `APPLE_*` secrets, the workflow is ready).

## Workflow for every feature
1. Short plan for the owner.
2. Implementation: core with tests first, then IPC, then the UI with i18n.
3. Green `npm test`.
4. Update the README (in English) and, if dependencies change, `THIRD_PARTY_LICENSES`.
5. Small commits.
6. Summary for the owner.
7. Push only on request.
8. For a new release: bump the version in `package.json`, `Cargo.toml` and `src-tauri/tauri.conf.json` (they must match), then `npm run release`.
