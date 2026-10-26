#!/usr/bin/env bash
# Release build of Konvert for macOS (Apple Silicon) and Linux: the packages plus
# their SHA-256 checksums in release/. Windows uses scripts/build-release.ps1;
# `npm run release` picks the right script.
#
# Every input is pinned: Rust toolchain (rust-toolchain.toml), Cargo.lock,
# package-lock.json (npm ci) and the FFmpeg archive (SHA-256 verified).
#
#   1. Checks that the version is the same in package.json, Cargo.toml and tauri.conf.json.
#   2. Installs the exact npm dependencies (npm ci).
#   3. Installs the pinned FFmpeg build (macOS: signs it for the hardened runtime).
#   4. Runs the privacy check, the third-party license check and every test.
#   5. Builds the packages with local paths remapped.
#   6. Verifies that the executable contains no local path and that FFmpeg is
#      executable inside every package (macOS: signature and notarization).
#   7. Copies the packages to release/ and writes <package>.sha256.
#
# Packages: macOS .dmg; Linux .deb, .rpm and .AppImage (x86_64 or arm64, the
# architecture of the build machine).
#
# macOS signing and notarization are optional and use these environment variables:
#   APPLE_SIGNING_IDENTITY  "Developer ID Application: Name (TEAMID)", in the keychain
#   APPLE_ID, APPLE_PASSWORD (app-specific password), APPLE_TEAM_ID
# Without them (no Apple Developer account) the app is signed ad hoc and not
# notarized: it runs on Apple Silicon, but Gatekeeper asks the user to allow it
# once (see "Installation" in README.md).
#
# Network is used only here, at build time (npm, FFmpeg archive, the AppImage
# tools that Tauri downloads, Apple's notary service). The app never connects.
#
# Usage: bash scripts/build-release.sh [--skip-tests]

set -euo pipefail

SKIP_TESTS=0
[[ "${1:-}" == "--skip-tests" ]] && SKIP_TESTS=1

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"
OS="$(uname -s)"
STEPS=7
step() { printf '\n==> [%s/%s] %s\n' "$1" "$STEPS" "$2"; }

step 1 'Version'
PKG_VERSION="$(node -p 'require("./package.json").version')"
CONF_VERSION="$(node -p 'require("./src-tauri/tauri.conf.json").version')"
CARGO_VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)"
if [[ "$PKG_VERSION" != "$CONF_VERSION" || "$PKG_VERSION" != "$CARGO_VERSION" ]]; then
  echo "Version mismatch: package.json $PKG_VERSION, tauri.conf.json $CONF_VERSION, Cargo.toml $CARGO_VERSION" >&2
  exit 1
fi
echo "Konvert $PKG_VERSION"

step 2 'npm ci'
npm ci --no-audit --no-fund

step 3 'FFmpeg'
bash scripts/setup-ffmpeg.sh
SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-}"
if [[ "$OS" == Darwin ]]; then
  if [[ -z "$SIGNING_IDENTITY" ]]; then
    echo "warning: APPLE_SIGNING_IDENTITY is not set: ad hoc signature, no notarization." >&2
    SIGNING_IDENTITY="-"
    export APPLE_SIGNING_IDENTITY="-"
  fi
  # Tauri notarizes whenever these variables exist, even empty (CI passes missing
  # secrets as empty strings): keep them only for a Developer ID build.
  if [[ "$SIGNING_IDENTITY" == "-" || -z "${APPLE_ID:-}" ]]; then
    unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID
  fi
  # Tauri signs the app, not the executables among its resources: sign FFmpeg first
  # (hardened runtime and secure timestamp, both required for notarization).
  # An ad hoc signature cannot carry a secure timestamp.
  timestamp="--timestamp"; [[ "$SIGNING_IDENTITY" == "-" ]] && timestamp="--timestamp=none"
  for exe in src-tauri/binaries/ffmpeg/ffmpeg src-tauri/binaries/ffmpeg/ffprobe; do
    codesign --force --options runtime "$timestamp" --sign "$SIGNING_IDENTITY" "$exe"
  done
fi

step 4 'Privacy, licenses and tests'
node scripts/check-privacy.mjs
node scripts/generate-licenses.mjs --check
if [[ $SKIP_TESTS -eq 0 ]]; then
  npx vitest run
  cargo test --workspace
fi

step 5 'Build (local paths remapped)'
CARGO_HOME_DIR="${CARGO_HOME:-$HOME/.cargo}"
RUSTUP_HOME_DIR="${RUSTUP_HOME:-$HOME/.rustup}"
# CARGO_ENCODED_RUSTFLAGS (0x1F-separated) keeps paths with spaces intact.
CARGO_ENCODED_RUSTFLAGS="$(printf '%s\x1f%s\x1f%s' \
  "--remap-path-prefix=$CARGO_HOME_DIR=/cargo" \
  "--remap-path-prefix=$RUSTUP_HOME_DIR=/rustup" \
  "--remap-path-prefix=$REPO_ROOT=/konvert")"
export CARGO_ENCODED_RUSTFLAGS
npx tauri build
unset CARGO_ENCODED_RUSTFLAGS
BUNDLE="target/release/bundle"

step 6 'Checks on the packages'
needles=("$HOME" "$REPO_ROOT" "$CARGO_HOME_DIR")
USER_NAME="${USER:-$(id -un)}"
[[ ${#USER_NAME} -ge 4 ]] && needles+=("/$USER_NAME/")
for needle in "${needles[@]}"; do
  if grep -aqF "$needle" target/release/konvert; then
    echo "target/release/konvert contains the local path $needle" >&2
    exit 1
  fi
done
echo "OK: the executable contains no user name or local folder."

PACKAGES=()
if [[ "$OS" == Darwin ]]; then
  APP="$BUNDLE/macos/Konvert.app"
  codesign --verify --deep --strict --verbose=2 "$APP"
  for exe in "$APP/Contents/Resources/ffmpeg/ffmpeg" "$APP/Contents/Resources/ffmpeg/ffprobe"; do
    [[ -x "$exe" ]] || { echo "$exe is not executable" >&2; exit 1; }
    codesign --verify --strict "$exe"
  done
  for dmg in "$BUNDLE"/dmg/*.dmg; do
    if [[ "$SIGNING_IDENTITY" != "-" && -n "${APPLE_ID:-}" ]]; then
      # Tauri notarized and stapled the app; notarize the disk image too, so that
      # opening it shows no warning either.
      codesign --force --timestamp --sign "$SIGNING_IDENTITY" "$dmg"
      xcrun notarytool submit "$dmg" --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" \
        --team-id "$APPLE_TEAM_ID" --wait
      xcrun stapler staple "$dmg"
      xcrun stapler validate "$APP"
      spctl --assess --type execute --verbose "$APP"
    fi
    PACKAGES+=("$dmg")
  done
else
  for deb in "$BUNDLE"/deb/*.deb; do
    listing="$(dpkg-deb -c "$deb")"
    for exe in ffmpeg ffprobe; do
      grep -qE "^-rwxr-xr-x .*/ffmpeg/$exe$" <<<"$listing" \
        || { echo "$deb: ffmpeg/$exe is missing or not executable" >&2; exit 1; }
    done
    PACKAGES+=("$deb")
  done
  for rpm_file in "$BUNDLE"/rpm/*.rpm; do
    if command -v rpm >/dev/null; then
      listing="$(rpm -qlvp "$rpm_file")"
      for exe in ffmpeg ffprobe; do
        grep -qE "^-rwxr-xr-x .*/ffmpeg/$exe$" <<<"$listing" \
          || { echo "$rpm_file: ffmpeg/$exe is missing or not executable" >&2; exit 1; }
      done
    else
      echo "warning: rpm is not installed, $rpm_file not inspected" >&2
    fi
    PACKAGES+=("$rpm_file")
  done
  for appimage in "$BUNDLE"/appimage/*.AppImage; do
    extract="$(mktemp -d)"
    ( cd "$extract" && "$REPO_ROOT/$appimage" --appimage-extract >/dev/null )
    for exe in ffmpeg ffprobe; do
      found="$(find "$extract/squashfs-root" -path "*/ffmpeg/$exe" -type f | head -n1)"
      [[ -n "$found" && -x "$found" ]] \
        || { echo "$appimage: ffmpeg/$exe is missing or not executable" >&2; exit 1; }
    done
    rm -rf "$extract"
    PACKAGES+=("$appimage")
  done
fi
echo "OK: FFmpeg is present and executable in every package."

step 7 'Packages and checksums'
mkdir -p release
for pkg in "${PACKAGES[@]}"; do
  name="$(basename "$pkg")"
  cp "$pkg" "release/$name"
  ( cd release && { command -v sha256sum >/dev/null && sha256sum "$name" || shasum -a 256 "$name"; } > "$name.sha256" )
  echo "release/$name"
  cat "release/$name.sha256"
done
