# FFmpeg binaries (not committed)

This folder receives the FFmpeg/FFprobe executables bundled with Konvert.
They are **not** stored in Git (they are large); fetch them with:

```bash
# Windows
powershell -ExecutionPolicy Bypass -File scripts/setup-ffmpeg.ps1

# Linux / macOS (prepared for phase 2)
bash scripts/setup-ffmpeg.sh
```

The script downloads the build pinned in `scripts/ffmpeg-manifest.json`, verifies its
SHA-256 and copies only the needed files into `ffmpeg/`. This happens at development
time only: the installed application never downloads anything.
