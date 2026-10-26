#!/usr/bin/env bash
# Builds the LGPL FFmpeg/FFprobe used by Konvert on macOS (Apple Silicon).
#
# No upstream project publishes an LGPL arm64 build for macOS, so Konvert builds its
# own. The external libraries are the ones of the Windows/Linux builds (BtbN
# FFmpeg-Builds, same tag as scripts/ffmpeg-manifest.json) at the same commits, so
# the three platforms behave the same. Every source is pinned to a commit (or SVN
# revision) and verified after checkout.
#
# The result is static: ffmpeg and ffprobe link only macOS system libraries.
# Unlike the BtbN builds it is configured with --disable-network: the binaries
# cannot open any network protocol at all.
#
# Development/CI only (.github/workflows/ffmpeg-macos.yml). Outputs in <out-dir>:
#   ffmpeg-<version>-macos-arm64-lgpl.tar.xz          binaries (bin/) + LICENSE.txt
#   ffmpeg-<version>-macos-arm64-lgpl-sources.tar.xz  every source + this script (LGPL)
# Upload both to the FFmpeg mirror release, then pin the first one's SHA-256 in
# scripts/ffmpeg-manifest.json (platforms.macos-aarch64).
#
# Usage: bash scripts/build-ffmpeg-macos.sh [out-dir]   (default: .ffmpeg-build/out)
# Requirements: Xcode command line tools and
#   brew install nasm cmake meson ninja pkg-config autoconf automake libtool subversion

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"
WORK="$REPO_ROOT/.ffmpeg-build"
OUT="${1:-$WORK/out}"
SRC="$WORK/src"
BUILD="$WORK/build"
PREFIX="$WORK/prefix"

[[ "$(uname -s)-$(uname -m)" == "Darwin-arm64" ]] || { echo "Run this on an Apple Silicon Mac." >&2; exit 1; }

# FFmpeg: the commit of the version pinned in the manifest (n9.0.2-36-g6882969ce9).
FFMPEG_REPO="https://github.com/FFmpeg/FFmpeg.git"
FFMPEG_COMMIT="6882969ce9fd2aa6aa1d0008b2bfe1c838aac11d"
FFMPEG_VERSION="n9.0.2-36-g6882969ce9"

# name|repository|commit — from BtbN/FFmpeg-Builds scripts.d at autobuild-2026-10-25-13-00.
LIBS=(
  "ogg|https://github.com/xiph/ogg.git|06a5e0262cdc28aa4ae6797627a783b5010440f0"
  "vorbis|https://github.com/xiph/vorbis.git|1b75110b5a2754ba1931d82dd83cb822b266a21d"
  "opus|https://github.com/xiph/opus.git|503d81b138d76621aae4b12786e90de48aa8db3a"
  "vpx|https://chromium.googlesource.com/webm/libvpx|5e680f30801d03c21078f8c4b772464752516211"
  "svtav1|https://gitlab.com/AOMediaCodec/SVT-AV1.git|5ed9ffbcb5086da23f30fe9bc5b00145358462da"
  "openh264|https://github.com/cisco/openh264.git|8b2d28faade10d74d99ac80e199aef664c9c5a3b"
  "aom|https://aomedia.googlesource.com/aom|b4ce2cd097b00921b0327bfb5f07b5f8361ade99"
  "dav1d|https://code.videolan.org/videolan/dav1d.git|9711965b60bb692ae24004659acf61f5c7d9ed61"
  "webp|https://chromium.googlesource.com/webm/libwebp|5c168cd23b1872969c38e34b01b04dcf29849288"
  "kvazaar|https://github.com/ultravideo/kvazaar.git|2b06691bb5844404c0e703f12a5d7b0fee914ec7"
)
LAME_SVN="https://svn.code.sf.net/p/lame/svn/trunk/lame"
LAME_REV="6835"

export MACOSX_DEPLOYMENT_TARGET="11.0"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig"
export CFLAGS="-O2 -arch arm64"
export CXXFLAGS="-O2 -arch arm64"
JOBS="$(sysctl -n hw.ncpu)"
CMAKE_COMMON=(-G Ninja -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX="$PREFIX"
  -DCMAKE_PREFIX_PATH="$PREFIX" -DCMAKE_INSTALL_LIBDIR=lib -DCMAKE_OSX_ARCHITECTURES=arm64
  -DBUILD_SHARED_LIBS=OFF -DCMAKE_POLICY_VERSION_MINIMUM=3.5)
MESON_COMMON=(--prefix="$PREFIX" --libdir=lib --buildtype=release --default-library=static)

# Shallow checkout of exactly one commit, verified.
fetch() { # name repo commit
  local dir="$SRC/$1"
  if [[ -d "$dir/.git" && "$(git -C "$dir" rev-parse HEAD)" == "$3" ]]; then return; fi
  rm -rf "$dir" && mkdir -p "$dir"
  git -C "$dir" init -q
  git -C "$dir" fetch -q --depth 1 "$2" "$3"
  git -C "$dir" -c advice.detachedHead=false checkout -q FETCH_HEAD
  [[ "$(git -C "$dir" rev-parse HEAD)" == "$3" ]] || { echo "$1: commit mismatch" >&2; exit 1; }
}

cmake_build() { # name [cmake options...]
  local name="$1"; shift
  cmake -S "$SRC/$name" -B "$BUILD/$name" "${CMAKE_COMMON[@]}" "$@"
  cmake --build "$BUILD/$name" -j "$JOBS"
  cmake --install "$BUILD/$name"
}

meson_build() { # name [meson options...]
  local name="$1"; shift
  meson setup --reconfigure "$BUILD/$name" "$SRC/$name" "${MESON_COMMON[@]}" "$@" \
    || meson setup "$BUILD/$name" "$SRC/$name" "${MESON_COMMON[@]}" "$@"
  meson compile -C "$BUILD/$name"
  meson install -C "$BUILD/$name"
}

echo "==> Fetching sources"
mkdir -p "$SRC" "$BUILD" "$PREFIX" "$OUT"
for entry in "${LIBS[@]}"; do IFS='|' read -r name repo commit <<<"$entry"; fetch "$name" "$repo" "$commit"; done
fetch ffmpeg "$FFMPEG_REPO" "$FFMPEG_COMMIT"
if [[ ! -d "$SRC/lame" ]]; then svn checkout -q "$LAME_SVN@$LAME_REV" "$SRC/lame"; fi
[[ "$(svn info --show-item revision "$SRC/lame")" == "$LAME_REV" ]] || { echo "lame: revision mismatch" >&2; exit 1; }

echo "==> Building libraries"
cmake_build ogg -DBUILD_TESTING=OFF -DINSTALL_DOCS=OFF
cmake_build vorbis -DBUILD_TESTING=OFF
cmake_build opus -DOPUS_BUILD_PROGRAMS=OFF -DOPUS_BUILD_TESTING=OFF -DBUILD_TESTING=OFF
cmake_build svtav1 -DBUILD_APPS=OFF -DBUILD_TESTING=OFF -DSVT_AV1_LTO=OFF
cmake_build aom -DENABLE_TESTS=0 -DENABLE_EXAMPLES=0 -DENABLE_TOOLS=0 -DENABLE_DOCS=0 \
  -DCONFIG_TUNE_VMAF=0 -DCONFIG_AV1_DECODER=1
cmake_build webp -DWEBP_BUILD_ANIM_UTILS=OFF -DWEBP_BUILD_CWEBP=OFF -DWEBP_BUILD_DWEBP=OFF \
  -DWEBP_BUILD_GIF2WEBP=OFF -DWEBP_BUILD_IMG2WEBP=OFF -DWEBP_BUILD_VWEBP=OFF \
  -DWEBP_BUILD_WEBPINFO=OFF -DWEBP_BUILD_WEBPMUX=OFF -DWEBP_BUILD_EXTRAS=OFF \
  -DWEBP_BUILD_LIBWEBPMUX=ON
meson_build openh264 -Dtests=disabled
meson_build dav1d -Denable_tools=false -Denable_tests=false

( cd "$SRC/kvazaar" && ./autogen.sh >/dev/null && mkdir -p "$BUILD/kvazaar" && cd "$BUILD/kvazaar" \
  && "$SRC/kvazaar/configure" --prefix="$PREFIX" --disable-shared --enable-static \
  && make -j "$JOBS" && make install )

( cd "$SRC/lame" && ./configure --prefix="$PREFIX" --disable-shared --enable-static \
    --disable-frontend --disable-gtktest --disable-dependency-tracking \
  && make -j "$JOBS" && make install )

( mkdir -p "$BUILD/vpx" && cd "$BUILD/vpx" \
  && "$SRC/vpx/configure" --prefix="$PREFIX" --target=arm64-darwin20-gcc --enable-static \
    --disable-shared --enable-pic --enable-vp9-highbitdepth --disable-examples --disable-tools \
    --disable-docs --disable-unit-tests \
  && make -j "$JOBS" && make install )

echo "==> Building FFmpeg"
mkdir -p "$BUILD/ffmpeg"
( cd "$BUILD/ffmpeg" && "$SRC/ffmpeg/configure" \
    --prefix="$PREFIX" --pkg-config-flags=--static --extra-version=konvert \
    --extra-cflags="-I$PREFIX/include" --extra-ldflags="-L$PREFIX/lib" --extra-libs="-lc++" \
    --enable-version3 --disable-gpl --disable-nonfree \
    --enable-static --disable-shared --disable-autodetect --disable-network \
    --disable-ffplay --disable-doc --disable-debug \
    --enable-zlib --enable-iconv --enable-pthreads \
    --enable-libaom --enable-libdav1d --enable-libkvazaar --enable-libmp3lame \
    --enable-libopenh264 --enable-libopus --enable-libsvtav1 --enable-libvorbis \
    --enable-libvpx --enable-libwebp \
  && make -j "$JOBS" && make install )

echo "==> Checking the binaries"
for exe in ffmpeg ffprobe; do
  # Only system libraries may be linked: nothing from Homebrew or the build prefix.
  if otool -L "$PREFIX/bin/$exe" | tail -n +2 | grep -vE '^\s+(/usr/lib/|/System/Library/)'; then
    echo "$exe links non-system libraries (listed above)" >&2; exit 1
  fi
done
if "$PREFIX/bin/ffmpeg" -hide_banner -protocols | grep -qE '^\s+(https?|tcp|udp|tls)$'; then
  echo "ffmpeg still has network protocols" >&2; exit 1
fi

echo "==> Packaging"
NAME="ffmpeg-$FFMPEG_VERSION-macos-arm64-lgpl"
STAGE="$WORK/stage"
rm -rf "$STAGE" && mkdir -p "$STAGE/$NAME/bin" "$STAGE/$NAME-sources"
cp "$PREFIX/bin/ffmpeg" "$PREFIX/bin/ffprobe" "$STAGE/$NAME/bin/"
cp "$SRC/ffmpeg/COPYING.LGPLv3" "$STAGE/$NAME/LICENSE.txt"
"$PREFIX/bin/ffmpeg" -hide_banner -buildconf > "$STAGE/$NAME/BUILDCONF.txt"
for dir in "$SRC"/*; do
  name="$(basename "$dir")"
  if [[ -d "$dir/.git" ]]; then
    git -C "$dir" archive --format=tar --prefix="$name/" HEAD | tar -x -C "$STAGE/$NAME-sources"
  else
    rsync -a --exclude .svn "$dir/" "$STAGE/$NAME-sources/$name/"
  fi
done
cp "$0" "$STAGE/$NAME-sources/build-ffmpeg-macos.sh"
# Owner 0:0 so the archives do not carry the CI user's name.
for item in "$NAME" "$NAME-sources"; do
  tar -C "$STAGE" --uid 0 --gid 0 -cf - "$item" | xz -9 -T0 > "$OUT/$item.tar.xz"
done
( cd "$OUT" && shasum -a 256 "$NAME.tar.xz" "$NAME-sources.tar.xz" | tee SHA256SUMS )
echo "OK: archives in $OUT"
