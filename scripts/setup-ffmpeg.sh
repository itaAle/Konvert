#!/usr/bin/env bash
# Downloads the pinned LGPL FFmpeg/FFprobe build, verifies its SHA-256 and installs
# it into src-tauri/binaries/ffmpeg (bundled by Tauri as a resource).
#
# Development/build-time only: the installed Konvert app never downloads anything.
# The archive URL and its SHA-256 are pinned in scripts/ffmpeg-manifest.json.
#
# Usage: bash scripts/setup-ffmpeg.sh [--force]
#
# Requirements: curl, sha256sum (or shasum), tar/unzip, node (to read the manifest).
# Platforms: Linux x86_64/arm64 and macOS on Apple Silicon. Windows uses
# scripts/setup-ffmpeg.ps1 (`npm run setup:ffmpeg` picks the right script).
# Written for the bash 3.2 that ships with macOS (no mapfile).

set -euo pipefail

FORCE=0
[[ "${1:-}" == "--force" ]] && FORCE=1

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
MANIFEST="$SCRIPT_DIR/ffmpeg-manifest.json"
DOWNLOAD_DIR="$REPO_ROOT/.ffmpeg-download"
DEST_DIR="$REPO_ROOT/src-tauri/binaries/ffmpeg"

case "$(uname -s)-$(uname -m)" in
  Linux-x86_64)               PLATFORM="linux-x86_64" ;;
  Linux-aarch64|Linux-arm64)  PLATFORM="linux-aarch64" ;;
  Darwin-arm64)               PLATFORM="macos-aarch64" ;;
  MINGW*-x86_64|MSYS*-x86_64) PLATFORM="windows-x86_64" ;;
  *) echo "Unsupported platform: $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

# Read a value from the manifest (keeps the script free of a jq dependency).
manifest() {
  node -e '
    const m = require(process.argv[1]);
    const v = process.argv[2].split(".").reduce((o, k) => (o == null ? o : o[k]), m);
    if (v == null) process.exit(1);
    console.log(Array.isArray(v) ? v.join("\n") : v);
  ' "$MANIFEST" "$1"
}

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1
  else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

EXPECTED="$(manifest "platforms.$PLATFORM.sha256" | tr 'A-F' 'a-f')"
if [[ -z "$EXPECTED" ]]; then
  echo "No pinned FFmpeg build for $PLATFORM yet: see scripts/build-ffmpeg-macos.sh." >&2
  exit 1
fi
ARCHIVE_TYPE="$(manifest "platforms.$PLATFORM.archive")"
BIN_SUBDIR="$(manifest "platforms.$PLATFORM.binDir")"
URLS=();     while IFS= read -r l; do URLS+=("$l"); done < <(manifest "platforms.$PLATFORM.urls")
FILES=();    while IFS= read -r l; do FILES+=("$l"); done < <(manifest "platforms.$PLATFORM.files")
REQUIRED=(); while IFS= read -r l; do REQUIRED+=("$l"); done < <(manifest requiredEncoders)
ARCHIVE="$DOWNLOAD_DIR/$(basename "${URLS[0]}")"

mkdir -p "$DOWNLOAD_DIR"

# 1. Download (or reuse a cached copy whose hash matches).
if [[ $FORCE -eq 1 || ! -f "$ARCHIVE" || "$(sha256 "$ARCHIVE")" != "$EXPECTED" ]]; then
  ok=0
  for url in "${URLS[@]}"; do
    echo "Downloading $url"
    if curl --fail --location --silent --show-error --output "$ARCHIVE" "$url"; then ok=1; break; fi
    echo "warning: download failed" >&2
  done
  [[ $ok -eq 1 ]] || { echo "Could not download the FFmpeg archive; update scripts/ffmpeg-manifest.json." >&2; exit 1; }
else
  echo "Using cached archive $(basename "$ARCHIVE")"
fi

# 2. Verify integrity before extracting anything.
ACTUAL="$(sha256 "$ARCHIVE")"
if [[ "$ACTUAL" != "$EXPECTED" ]]; then
  rm -f "$ARCHIVE"
  printf 'SHA-256 mismatch\n  expected: %s\n  actual:   %s\nThe file was deleted.\n' "$EXPECTED" "$ACTUAL" >&2
  exit 1
fi
echo "SHA-256 verified: $ACTUAL"

# 3. Extract into a fresh, isolated directory.
EXTRACT_DIR="$(mktemp -d "$DOWNLOAD_DIR/extract-XXXXXX")"
trap 'rm -rf "$EXTRACT_DIR"' EXIT
case "$ARCHIVE_TYPE" in
  zip)    unzip -q "$ARCHIVE" -d "$EXTRACT_DIR" ;;
  tar.xz) tar -xJf "$ARCHIVE" -C "$EXTRACT_DIR" ;;
  *)      echo "Unknown archive type: $ARCHIVE_TYPE" >&2; exit 1 ;;
esac
TOP_DIR="$(find "$EXTRACT_DIR" -mindepth 1 -maxdepth 1 -type d | head -n1)"
BIN_DIR="$TOP_DIR/$BIN_SUBDIR"

# 4. Install only what Konvert needs.
rm -rf "$DEST_DIR"
mkdir -p "$DEST_DIR"
shopt -s nullglob
for pattern in "${FILES[@]}"; do
  matches=("$BIN_DIR"/$pattern)
  [[ ${#matches[@]} -gt 0 ]] || { echo "No file matching '$pattern' in the archive." >&2; exit 1; }
  cp "${matches[@]}" "$DEST_DIR/"
done
[[ -f "$TOP_DIR/LICENSE.txt" ]] && cp "$TOP_DIR/LICENSE.txt" "$DEST_DIR/LICENSE.txt"
chmod +x "$DEST_DIR"/ffmpeg* "$DEST_DIR"/ffprobe*
{
  echo "FFmpeg build bundled with Konvert"
  echo "version:  $(manifest ffmpegVersion)"
  echo "license:  $(manifest license)"
  echo "archive:  $(basename "$ARCHIVE")"
  echo "sha256:   $EXPECTED"
  echo "source:   $(manifest source.ffmpegCommit)"
  echo "recipe:   $(manifest source.buildRecipe)"
} > "$DEST_DIR/BUILD-INFO.txt"

# 5. Smoke test: the binaries run and provide every encoder Konvert relies on.
EXE=""; [[ "$PLATFORM" == windows-* ]] && EXE=".exe"
FFMPEG="$DEST_DIR/ffmpeg$EXE"
"$DEST_DIR/ffprobe$EXE" -hide_banner -version >/dev/null
ENCODERS="$("$FFMPEG" -hide_banner -encoders)"
missing=()
for enc in "${REQUIRED[@]}"; do
  grep -Eq "^ *[VAS][A-Z.]{5} +${enc} " <<<"$ENCODERS" || missing+=("$enc")
done
if [[ ${#missing[@]} -gt 0 ]]; then
  echo "The FFmpeg build lacks required encoders: ${missing[*]}" >&2
  exit 1
fi

echo
echo "OK: $("$FFMPEG" -hide_banner -version | head -n1)"
echo "Installed into src-tauri/binaries/ffmpeg; all required encoders present."
