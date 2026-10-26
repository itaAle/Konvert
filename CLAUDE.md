# Konvert: context for continuing development

> Context for coding agents (Claude Code) and for anyone resuming the work from another computer.
> This file is versioned in the public repository: **never** put secrets, names, personal emails,
> machine paths or other private data in it.

## What it is
Konvert is an Apple-style audio/video/image converter for 64-bit Windows 10/11, macOS 11+ on
Apple Silicon and Linux (x86_64 and arm64).
It uses Tauri 2.11, React 19 with TypeScript, and bundles FFmpeg/FFprobe.
It works **entirely offline, on the user's computer**.
**Public** repository: https://github.com/itaAle/Konvert (branch `main`).
Releases ship an NSIS installer plus a `.sha256` file: `v0.1.0` is published; the owner publishes `v0.2.0` (pictures).
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
  - `formats.rs`: declarative catalog with `TargetFormat`, `FORMATS` (FormatSpec), `RULES`, `available_targets`, `is_offered`. Pictures are `FormatKind::Image` with a single `ImageCodec` (JPG, PNG, WebP, AVIF).
  - `probe.rs`: ffprobe (including `-show_stream_groups`) → `MediaInfo`.
    - Still pictures (`image2`/`*_pipe` demuxers, HEIF/AVIF brands) are `MediaKind::Image` with `PictureInfo` (alpha, AVIF alpha plane, HEIC grid, EXIF); animated GIF/APNG/WebP stay videos.
    - For non-HEIF pictures a second ffprobe reads the first frame (`apply_first_frame_json`): EXIF orientation and EXIF presence. Stored dimensions are the displayed (already rotated) ones.
    - FFmpeg applies the rotation (EXIF or `irot`) by itself before the filters; `[0:g:N]` in a filter graph returns the reassembled HEIC grid.
  - `request.rs` → `plan.rs` (`plan_conversion`, pure) → `command.rs` (`build_from_plan`, pure FFmpeg arguments) → `convert.rs` (execution).
  - `estimate.rs`: size estimate and warnings.
  - Other modules: `naming.rs` (`name (n).ext`, never overwrite), `workspace.rs` (`%TEMP%\Konvert\run-…` with a lock and cleanup after crashes), `process.rs` (kill-on-close Job Object and `CREATE_NO_WINDOW`), `progress.rs`, `disk.rs`, `binaries.rs`, `error.rs`.
  - Stable error codes in `error.rs`: `engine_missing`, `input_not_found`, `output_unavailable`, `not_media`, `corrupt_input`, `unsupported_conversion`, `disk_full`, `permission_denied`, `cancelled`, `engine_failed`, `io`. Each has an i18n message.
  - FFmpeg always runs with `-protocol_whitelist file`, and inputs are passed as `file:` URLs.
  - Pictures: `plan.rs::plan_picture` → `ImagePlan` (always re-encoded); `command.rs::picture_args` (one filter graph: grid/alphamerge, scale, white background for JPG, alpha plane for AVIF; encoders mjpeg, png, libwebp, libaom-av1 still-picture; muxers `image2pipe`/`webp`/`avif`).
  - EXIF: FFmpeg keeps it only when converting to PNG (from JPEG/PNG/WebP, not from HEIF); otherwise the `MetadataDropped` warning is shown.
  - Tests: unit tests in the modules and `tests/engine.rs` (16 integration tests with real FFmpeg). `tests/fixtures/mod.rs` builds JPEGs with EXIF and grid HEIC files in memory (HEVC tiles made with libkvazaar).
- `crates/konvert-cli/`: development CLI (`npm run cli -- targets|plan|convert …`), not shipped.
- `src-tauri/`: thin IPC layer.
  - `commands.rs` holds 20 commands, also listed in `build.rs::COMMANDS` and `capabilities/default.json`: **a new command must be added in all three places**.
  - Other files: `jobs.rs` (sequential queue, `job-update` event), `prefs.rs`, `debug_log.rs` (redacted log, off by default), `launch.rs` (`LaunchFiles` queue for files from argv, the second instance and macOS `RunEvent::Opened`), `lib.rs` (single instance, Mica, close confirmation for × and ⌘Q via `ExitRequested`).
  - `tauri.conf.json`: strict CSP, `freezePrototype`, WebView2 flags without networking, per-user NSIS with `windows/installer-hooks.nsh` ("Open with" via `OpenWithProgids`; on Windows do NOT use Tauri's `fileAssociations`, which take over the default program).
  - `tauri.macos.conf.json` (.app/.dmg, hardened runtime, `fileAssociations` with rank `Alternate`) and `tauri.linux.conf.json` (.deb/.rpm/AppImage, MIME types) are merged over it; both use an opaque window. They may not touch security settings (`check:privacy`).
  - Tests keep the Windows installer's and the macOS/Linux associations' extensions identical to the file picker's `MEDIA_EXTENSIONS`.
- `src/`: React, Tailwind v4 (tokens via `data-scheme`/`data-vibrancy`), Motion, bundled Inter font, lucide icons.
  - `lib/api.ts` (IPC) and `lib/mock.ts` (fake backend, only for `npm run dev` in the browser).
  - `state/queue.ts`: pure queue reducer, with tests.
  - `i18n/it.ts` and `en.ts`: typed; a test checks that the keys match.
  - `components/`: DropScreen, FormatGrid, QualityPicker, AdvancedPanel, PlanSummary, ConvertingScreen, DoneScreen, SettingsSheet, …
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
- Available encoders: libopenh264 (H.264), libvpx/libvpx-vp9, libsvtav1, mpeg4, gif, libmp3lame, aac, libopus, libvorbis, flac, wmav2.
- The build also has picture decoders and encoders (mjpeg, png, libwebp, libaom-av1, libkvazaar, …): check them with `ffmpeg -encoders`.
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
- `target/release/konvert.pdb` contains the user's path: never upload `target/` or `.pdb` files.
- FFmpeg 9: a 1×1 crop of 4:2:0 YUV fails (convert to rgba first); `-map 0:g:0` on a HEIC gives a single tile, `[0:g:0]` in a filter graph gives the whole photo.

## Current status
- 2026-10-26: **v0.2.0** on `main` (commits `c437948` to `0e52cd9`, pushed): pictures in core, app, ui and docs; "Open with" fix for .aifc/.alac/.qt/.wave with a test keeping the installer and file picker extensions aligned; version 0.2.0.
  Installer `Konvert_0.2.0_x64-setup.exe`, SHA-256 `5cce9fe6ebf043392bc54c41cb4dac2cb56352b298ae9b578b69cebe7e614b67` (`release/` is ignored by Git: rebuild it with `npm run release`). The owner creates the tag and the GitHub release.
- Picture support:
  Input JPG/PNG/WebP/AVIF/HEIC/HEIF/BMP/TIFF, output JPG/PNG/WebP/AVIF (HEIC output is impossible: FFmpeg has no HEIF muxer).
  Owner's decisions: EXIF handled as FFmpeg does (kept only in PNG), BMP/TIFF included, HEIC included.
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

## Backlog (next work, by priority)
1. **Bug fix: stretched portrait videos** (listed in the README as an upcoming fix, at the owner's request).
   - A phone video with a 90° rotation (display matrix) that gets resized (GIF or "Maximum resolution") comes out stretched: `plan.rs::output_size` uses the coded dimensions, while FFmpeg rotates before `scale`.
   - Reproduced: 320×240 with rotation 90 and `--height 120` → 160×120 instead of 90×120.
   - Fix: read the rotation from the stream's `side_data_list` in `probe.rs` (as already done for pictures) and swap width and height.
2. Finish the macOS/Linux rollout (see Current status), and macOS window vibrancy (needs `macOSPrivateApi`).
3. Hardware encoders NVENC/QSV/AMF/h264_mf/VideoToolbox, with automatic detection.
4. FFmpeg built without any network support on Windows and Linux (macOS already is).
5. Code signing of the Windows installer; Developer ID signing + notarization on macOS (add the `APPLE_*` secrets, the workflow is ready).
6. Maybe: keep EXIF in JPG/WebP/AVIF too (FFmpeg does not write it: it would have to be copied by hand, with the orientation reset).

## Workflow for every feature
1. Short plan for the owner.
2. Implementation: core with tests first, then IPC, then the UI with i18n.
3. Green `npm test`.
4. Update the README (in English) and, if dependencies change, `THIRD_PARTY_LICENSES`.
5. Small commits.
6. Summary for the owner.
7. Push only on request.
8. For a new release: bump the version in `package.json`, `Cargo.toml` and `src-tauri/tauri.conf.json` (they must match), then `npm run release`.
