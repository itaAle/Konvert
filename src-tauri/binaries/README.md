# FFmpeg binaries (not committed)

This folder receives the FFmpeg/FFprobe executables bundled with Konvert.
They are **not** stored in Git (they are large); fetch them with:

```bash
npm run setup:ffmpeg
```

It runs `scripts/setup-ffmpeg.ps1` on Windows and `scripts/setup-ffmpeg.sh` on Linux
and macOS. The script downloads the build pinned in `scripts/ffmpeg-manifest.json`,
verifies its SHA-256 and copies only the needed files into `ffmpeg/`. This happens at
development time only: the installed application never downloads anything.

Windows and Linux use the LGPL builds of BtbN/FFmpeg-Builds. macOS (Apple Silicon) uses
a build made by `scripts/build-ffmpeg-macos.sh` with the same libraries, published in
the FFmpeg mirror release together with its sources.
