#!/usr/bin/env node
// Downloads the OCR models pinned in scripts/ocr-manifest.json, verifies their
// SHA-256 and installs them into src-tauri/binaries/ocr. Development and build
// time only: the installed application never downloads anything.
//
//   node scripts/setup-ocr.mjs     (npm run setup:ocr)
//
// Downloads are kept in .ocr-download/ (ignored by Git) and reused when their
// hash still matches.

import { createHash } from "node:crypto";
import { copyFileSync, existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));
const manifest = JSON.parse(readFileSync(join(ROOT, "scripts", "ocr-manifest.json"), "utf8"));
const CACHE = join(ROOT, ".ocr-download");
const DEST = join(ROOT, "src-tauri", "binaries", "ocr");

const sha256 = (path) => createHash("sha256").update(readFileSync(path)).digest("hex");
const matches = (path, file) => existsSync(path) && sha256(path) === file.sha256;

async function download(name, file) {
  const cached = join(CACHE, name);
  if (matches(cached, file)) return cached;
  mkdirSync(CACHE, { recursive: true });
  for (const url of file.urls) {
    console.log(`Downloading ${url}`);
    try {
      const response = await fetch(url);
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const data = Buffer.from(await response.arrayBuffer());
      const hash = createHash("sha256").update(data).digest("hex");
      if (hash !== file.sha256) throw new Error(`SHA-256 mismatch: got ${hash}`);
      writeFileSync(`${cached}.part`, data);
      renameSync(`${cached}.part`, cached);
      return cached;
    } catch (error) {
      console.warn(`  failed: ${error.message}`);
    }
  }
  throw new Error(`Could not download ${name} from any of its URLs.`);
}

const files = Object.entries(manifest.files);
if (files.every(([name, file]) => matches(join(DEST, name), file))) {
  console.log(`OCR models already installed in src-tauri/binaries/ocr.`);
  process.exit(0);
}
rmSync(DEST, { recursive: true, force: true });
mkdirSync(DEST, { recursive: true });
for (const [name, file] of files) {
  copyFileSync(await download(name, file), join(DEST, name));
  console.log(`Installed ${name} (${(file.size / 1024 / 1024).toFixed(1)} MB, SHA-256 verified)`);
}
writeFileSync(
  join(DEST, "BUILD-INFO.txt"),
  [
    manifest.name,
    `License: ${manifest.license} (see THIRD_PARTY_LICENSES)`,
    `Models: ${manifest.source.models}`,
    `ONNX files: ${manifest.source.onnx}`,
    "",
    ...files.map(([name, file]) => `${name}  ${file.sha256}`),
    "",
  ].join("\n"),
);
