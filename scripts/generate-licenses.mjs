#!/usr/bin/env node
// Generates THIRD_PARTY_LICENSES: every component shipped with Konvert, its
// license and the full license texts (deduplicated).
//
//   node scripts/generate-licenses.mjs           write the file
//   node scripts/generate-licenses.mjs --check   fail if the file is outdated
//
// Sources: the Rust runtime dependency graph of the app for every shipped
// platform (`cargo tree -e normal`), the npm production packages (package-lock.json),
// the npm build tools whose code ends up in the bundle, and FFmpeg (manifest).

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const OUTPUT = join(ROOT, "THIRD_PARTY_LICENSES");
/** Shipped platforms: Rust target → label shown for crates not used everywhere. */
const TARGETS = {
  "x86_64-pc-windows-msvc": "Windows",
  "aarch64-apple-darwin": "macOS",
  "x86_64-unknown-linux-gnu": "Linux",
  "aarch64-unknown-linux-gnu": "Linux",
};
/** Build tools whose output is part of the shipped frontend bundle. */
const BUNDLED_BUILD_TOOLS = ["tailwindcss", "vite"];
const LICENSE_FILE = /^(licen[cs]e|copying|notice|unlicense)([-._].*)?$/i;

const ffmpeg = JSON.parse(readFileSync(join(ROOT, "scripts", "ffmpeg-manifest.json"), "utf8"));
const appVersion = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8")).version;

function licenseTexts(dir) {
  if (!dir || !existsSync(dir)) return [];
  return readdirSync(dir)
    .filter((f) => LICENSE_FILE.test(f))
    .sort()
    .map((f) => readFileSync(join(dir, f), "utf8").replace(/\r\n/g, "\n").trim());
}

// --- Rust ------------------------------------------------------------------------
function rustPackages() {
  const metadata = JSON.parse(
    execFileSync("cargo", ["metadata", "--format-version", "1"], {
      cwd: ROOT,
      encoding: "utf8",
      maxBuffer: 256 * 1024 * 1024,
    }),
  );
  const byNameVersion = new Map(metadata.packages.map((p) => [`${p.name} ${p.version}`, p]));
  /** "name version" → platforms whose build contains the crate. */
  const platforms = new Map();
  for (const [target, label] of Object.entries(TARGETS)) {
    const tree = execFileSync(
      "cargo",
      ["tree", "-q", "-p", "konvert", "-e", "normal,no-proc-macro", "--target", target, "--prefix", "none", "--format", "{p}"],
      { cwd: ROOT, encoding: "utf8" },
    );
    for (const line of tree.split(/\r?\n/)) {
      const [name, v] = line.trim().split(" ");
      if (!name || !v) continue;
      const key = `${name} ${v.replace(/^v/, "")}`;
      if (!platforms.has(key)) platforms.set(key, new Set());
      platforms.get(key).add(label);
    }
  }
  const platformCount = new Set(Object.values(TARGETS)).size;
  const packages = [];
  for (const [key, labels] of platforms) {
    const meta = byNameVersion.get(key);
    // Konvert's own crates are covered by LICENSE.
    if (!meta || meta.source == null) continue;
    packages.push({
      name: meta.name,
      version: meta.version,
      license: meta.license ?? (meta.license_file ? "see license file" : "unknown"),
      repository: meta.repository ?? "",
      texts: licenseTexts(dirname(meta.manifest_path)),
      note: labels.size < platformCount ? `${[...labels].sort().join(", ")} only` : "",
    });
  }
  return packages.sort((a, b) => a.name.localeCompare(b.name) || a.version.localeCompare(b.version));
}

// --- npm -------------------------------------------------------------------------
function npmPackages() {
  const lock = JSON.parse(readFileSync(join(ROOT, "package-lock.json"), "utf8"));
  const packages = [];
  for (const [key, meta] of Object.entries(lock.packages ?? {})) {
    if (!key.startsWith("node_modules/")) continue;
    const name = key.slice(key.lastIndexOf("node_modules/") + "node_modules/".length);
    const bundledTool = BUNDLED_BUILD_TOOLS.includes(name) && key === `node_modules/${name}`;
    if ((meta.dev || meta.devOptional) && !bundledTool) continue;
    const dir = join(ROOT, key);
    let pkg = {};
    try {
      pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
    } catch {}
    const repo = typeof pkg.repository === "string" ? pkg.repository : pkg.repository?.url ?? "";
    packages.push({
      name,
      version: meta.version,
      license: meta.license ?? pkg.license ?? "unknown",
      repository: repo.replace(/^git\+/, "").replace(/\.git$/, ""),
      texts: licenseTexts(dir),
      note: bundledTool ? "build tool, code included in the bundle" : "",
    });
  }
  return packages.sort((a, b) => a.name.localeCompare(b.name));
}

// --- Output ----------------------------------------------------------------------
function table(packages) {
  const rows = packages.map((p) => `  ${`${p.name} ${p.version}`.padEnd(46)} ${p.license}${p.note ? `  (${p.note})` : ""}`);
  return rows.join("\n");
}

function render(rust, npm) {
  // Deduplicate identical license texts across packages.
  const texts = new Map();
  for (const p of [...rust, ...npm]) {
    for (const text of p.texts) {
      if (!texts.has(text)) texts.set(text, []);
      texts.get(text).push(`${p.name} ${p.version}`);
    }
  }
  const missing = [...rust, ...npm].filter((p) => p.texts.length === 0).map((p) => `${p.name} ${p.version} (${p.license})`);
  const rule = "=".repeat(78);

  return `KONVERT ${appVersion} - THIRD-PARTY SOFTWARE NOTICES
${rule}

Konvert is released under the MIT License (see LICENSE). It bundles the
third-party software listed below. Generated by scripts/generate-licenses.mjs;
do not edit by hand.

No component makes network connections in Konvert. Konvert performs no network
activity at all: scripts/check-privacy.mjs fails the build if an HTTP, TLS,
WebSocket or telemetry library enters the dependency graph, and
scripts/monitor-network.ps1 shows that the running app opens no socket. See the
"Privacy" section of README.md.


1. FFmpeg
${rule}

Files:      in the "ffmpeg" folder: ffmpeg and ffprobe (ffmpeg.exe,
            ffprobe.exe and the libav*/libsw* DLLs on Windows)
License:    GNU Lesser General Public License, version 3 or later
            (${ffmpeg.license}); full text in ffmpeg/LICENSE.txt
Version:    ${ffmpeg.ffmpegVersion}
Builds:     Windows and Linux: BtbN FFmpeg-Builds, variants win64-lgpl-shared,
            linux64-lgpl and linuxarm64-lgpl
            ${ffmpeg.source.release}
            macOS (Apple Silicon): built by scripts/build-ffmpeg-macos.sh of the
            Konvert repository, with the same library versions and without
            any network protocol (--disable-network)
Website:    https://ffmpeg.org/

How Konvert uses FFmpeg
    FFmpeg runs as a separate program (child process); Konvert does not link
    to it. The files are installed unmodified in the "ffmpeg" folder of the
    application and can be replaced with any compatible build (same file
    names), as the LGPL allows.

Build configuration
    Configured without --enable-gpl and without --enable-nonfree: no GPL-only
    component (no libx264, libx265, libxvid) and no non-free one (no
    libfdk-aac). It uses --enable-version3, hence LGPL version 3 or later.
    \`ffmpeg -version\` prints the exact configuration. Libraries relevant to
    Konvert, each under its own LGPL-compatible license: LAME (MP3, LGPL),
    libopus (BSD), libvorbis (BSD), libvpx (BSD), SVT-AV1 (BSD-3-Clause-Clear),
    OpenH264 (BSD-2-Clause), libaom (BSD-2-Clause), dav1d (BSD-2-Clause),
    libwebp (BSD-3-Clause), kvazaar (BSD-3-Clause).

Corresponding source code
    - FFmpeg at the exact commit: ${ffmpeg.source.ffmpegCommit}
    - Build recipe pinning every library version and the configure flags:
      ${ffmpeg.source.buildRecipe} (Windows, Linux) and
      scripts/build-ffmpeg-macos.sh (macOS)
    - Copy of the binaries and of the source archives in the Konvert
      repository: ${ffmpeg.source.mirror}
    - The archive and its SHA-256 are pinned in scripts/ffmpeg-manifest.json
      of the Konvert repository (https://github.com/itaAle/Konvert).
    On request, the maintainers will provide a copy of the corresponding
    source code through the Konvert repository's issue tracker.

Patents
    Some codecs (for example H.264/AVC and AAC) may be covered by patents in
    some countries. The BSD license of OpenH264 covers its source code only
    and grants NO license to H.264 patents (Cisco's patent coverage applies
    only to the OpenH264 binaries that Cisco itself distributes). Users and
    distributors are responsible for any patent licensing required where
    they live.


2. Fonts and system components
${rule}

Inter (font, bundled in the app)      SIL Open Font License 1.1
    https://rsms.me/inter/ - packaged as @fontsource-variable/inter (below).

Microsoft Edge WebView2 Runtime       Microsoft Software License Terms
    Windows. Part of the system; used to display the interface, not
    distributed with Konvert. Konvert starts it with background networking,
    component updates and SmartScreen lookups disabled.

WKWebView (WebKit)                    Part of macOS
    macOS. Used to display the interface, not distributed with Konvert.

WebKitGTK, GTK, GLib and their dependencies   LGPL-2.1-or-later and others
    Linux. The .deb and .rpm packages use the copies installed by the
    distribution. The AppImage contains unmodified, dynamically linked copies
    taken from Ubuntu 22.04 LTS; their source code is available from the
    Ubuntu archive (https://packages.ubuntu.com/jammy/) and, on request,
    through the Konvert repository's issue tracker.


3. Rust libraries (${rust.length}, compiled into the Konvert executable)
${rule}

${table(rust)}


4. JavaScript libraries (${npm.length}, included in the interface bundle)
${rule}

${table(npm)}
${missing.length ? `\nThese packages ship no license file; their license (from their metadata) is one\nof the standard texts reproduced in section 5 (MIT, Apache-2.0, BSD-3-Clause):\n${missing.map((m) => `  ${m}`).join("\n")}\n` : ""}

5. License texts
${rule}

Each text below is reproduced as distributed by the packages listed above it.

${[...texts]
  .map(([text, owners]) => `${"-".repeat(78)}\nUsed by: ${owners.join(", ")}\n${"-".repeat(78)}\n\n${text}\n`)
  .join("\n")}`;
}

const content = render(rustPackages(), npmPackages());
if (process.argv.includes("--check")) {
  const current = existsSync(OUTPUT) ? readFileSync(OUTPUT, "utf8").replace(/\r\n/g, "\n") : "";
  if (current !== content) {
    console.error("THIRD_PARTY_LICENSES is outdated: run `node scripts/generate-licenses.mjs`.");
    process.exit(1);
  }
  console.log("THIRD_PARTY_LICENSES is up to date.");
} else {
  writeFileSync(OUTPUT, content);
  console.log(`Wrote THIRD_PARTY_LICENSES (${(content.length / 1024).toFixed(0)} KB).`);
}
