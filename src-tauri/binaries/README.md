# FFmpeg binaries and OCR models (not committed)

This folder receives the FFmpeg/FFprobe executables and the OCR models bundled with
Konvert. They are **not** stored in Git (they are large); fetch them with:

```bash
npm run setup:ffmpeg
npm run setup:ocr
```

`setup:ffmpeg` runs `scripts/setup-ffmpeg.ps1` on Windows and `scripts/setup-ffmpeg.sh` on
Linux and macOS. The script downloads the build pinned in `scripts/ffmpeg-manifest.json`,
verifies its SHA-256 and copies only the needed files into `ffmpeg/`.

`setup:ocr` runs `scripts/setup-ocr.mjs` on every platform: it downloads the PaddleOCR
PP-OCRv5 models (ONNX, Apache-2.0) pinned in `scripts/ocr-manifest.json`, verifies their
SHA-256 and copies them into `ocr/`.

This happens at development time only: the installed application never downloads anything.

Windows and Linux use the LGPL builds of BtbN/FFmpeg-Builds. macOS (Apple Silicon) uses
a build made by `scripts/build-ffmpeg-macos.sh` with the same libraries, published in
the FFmpeg mirror release together with its sources.
