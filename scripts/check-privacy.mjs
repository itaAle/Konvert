#!/usr/bin/env node
// Privacy guard for Konvert. Fails (exit code 1) if the app could gain network
// capabilities. Run with `npm run check:privacy` (also part of `npm test`).
//
// Checks:
//  1. Rust: the runtime dependency graph of the app, resolved for every desktop
//     target, contains no HTTP/TLS/WebSocket client or telemetry crate.
//     (Cargo.lock is target-independent and lists mobile-only crates such as
//     `reqwest`, which Tauri uses only on Android/iOS; `cargo tree --target`
//     shows what is really compiled for each platform.)
//  2. npm: no production dependency is a network client, network-capable Tauri
//     plugin or analytics/crash-reporting SDK.
//  3. Tauri config: strict CSP, no updater, hardened WebView flags, and the
//     frontend capabilities grant no shell/http/fs/opener permission.
//  4. Sources: no external URLs in frontend or Rust code (documentation links
//     in comments and the allow-listed releases page excepted) and no direct
//     use of network APIs.

import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync, statSync } from "node:fs";
import { extname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = fileURLToPath(new URL("..", import.meta.url));

/** URLs that may appear in code: opened in the system browser only on user click. */
const ALLOWED_URL_PREFIXES = ["https://github.com/itaAle/Konvert/releases"];

const DESKTOP_TARGETS = [
  "x86_64-pc-windows-msvc",
  "aarch64-pc-windows-msvc",
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
  "x86_64-apple-darwin",
  "aarch64-apple-darwin",
];

const BANNED_CRATES = new Set([
  "reqwest", "hyper", "hyper-util", "hyper-tls", "hyper-rustls", "h2", "h3", "quinn",
  "ureq", "isahc", "curl", "curl-sys", "attohttpc", "surf", "awc", "actix-web", "axum",
  "warp", "tungstenite", "tokio-tungstenite", "async-tungstenite", "websocket",
  "rustls", "tokio-rustls", "native-tls", "tokio-native-tls", "openssl", "openssl-sys",
  "trust-dns-resolver", "hickory-resolver", "lettre",
]);
const BANNED_CRATE_PREFIXES = [
  "sentry", "opentelemetry", "posthog", "mixpanel", "segment", "aptabase",
  "tauri-plugin-http", "tauri-plugin-updater", "tauri-plugin-websocket",
  "tauri-plugin-upload", "tauri-plugin-aptabase", "tauri-plugin-analytics",
];

const BANNED_NPM = new Set([
  "axios", "node-fetch", "cross-fetch", "isomorphic-fetch", "whatwg-fetch", "got", "ky",
  "superagent", "request", "socket.io-client", "ws", "firebase", "logrocket",
  "@tauri-apps/plugin-http", "@tauri-apps/plugin-updater", "@tauri-apps/plugin-websocket",
  "@tauri-apps/plugin-upload", "@vercel/analytics", "react-ga", "react-ga4",
]);
const BANNED_NPM_PREFIXES = [
  "@sentry/", "@segment/", "@amplitude/", "@datadog/", "@bugsnag/", "posthog", "mixpanel",
];

/** Permission identifiers the frontend may be granted. */
const ALLOWED_PERMISSION = /^(core:(event|window|app|webview|path):[\w-]+|allow-[\w-]+|dialog:[\w-]+)$/;

const failures = [];
const passed = [];
const fail = (msg) => failures.push(msg);

// --- 1. Rust runtime dependencies --------------------------------------------

function checkRustDependencies() {
  for (const target of DESKTOP_TARGETS) {
    let out;
    try {
      out = execFileSync(
        "cargo",
        ["tree", "-q", "-p", "konvert", "-e", "normal", "--target", target,
         "--prefix", "none", "--format", "{p}"],
        { cwd: ROOT, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
      );
    } catch (e) {
      fail(`cargo tree failed for ${target}: ${e.stderr || e.message}`);
      continue;
    }
    const crates = new Set(out.split(/\r?\n/).filter(Boolean).map((l) => l.split(" ")[0]));
    const banned = [...crates].filter(
      (c) => BANNED_CRATES.has(c) || BANNED_CRATE_PREFIXES.some((p) => c.startsWith(p)),
    );
    if (banned.length) fail(`Rust (${target}): network-capable crates: ${banned.join(", ")}`);
    else passed.push(`Rust ${target}: ${crates.size} runtime crates, none network-capable`);
  }
}

// --- 2. npm production dependencies ------------------------------------------

function checkNpmDependencies() {
  const lockPath = join(ROOT, "package-lock.json");
  if (!existsSync(lockPath)) return fail("package-lock.json not found (run npm install)");
  const lock = JSON.parse(readFileSync(lockPath, "utf8"));
  const prod = Object.entries(lock.packages ?? {})
    .filter(([key, meta]) => key.startsWith("node_modules/") && !meta.dev && !meta.devOptional)
    .map(([key]) => key.slice(key.lastIndexOf("node_modules/") + "node_modules/".length));
  const banned = prod.filter(
    (n) => BANNED_NPM.has(n) || BANNED_NPM_PREFIXES.some((p) => n.startsWith(p)),
  );
  if (banned.length) fail(`npm: network/analytics packages in production deps: ${banned.join(", ")}`);
  else passed.push(`npm: ${prod.length} production packages, none network-capable`);
}

// --- 3. Tauri configuration --------------------------------------------------

function parseCsp(csp) {
  if (!csp) return null;
  if (typeof csp === "object") {
    return Object.fromEntries(
      Object.entries(csp).map(([k, v]) => [k, Array.isArray(v) ? v : String(v).split(/\s+/)]),
    );
  }
  return Object.fromEntries(
    csp.split(";").map((d) => d.trim()).filter(Boolean)
      .map((d) => { const [k, ...v] = d.split(/\s+/); return [k, v]; }),
  );
}

function checkCsp(name, csp, extraAllowedOrigins = []) {
  const directives = parseCsp(csp);
  if (!directives) return fail(`tauri.conf.json: ${name} is missing`);
  if (directives["default-src"]?.join(" ") !== "'self'") {
    fail(`tauri.conf.json: ${name} default-src must be exactly 'self'`);
  }
  const allowedOrigins = ["http://ipc.localhost", ...extraAllowedOrigins];
  const allowedConnect = new Set(["ipc:", ...allowedOrigins]);
  for (const token of directives["connect-src"] ?? []) {
    if (!allowedConnect.has(token)) fail(`tauri.conf.json: ${name} connect-src allows ${token}`);
  }
  for (const [directive, tokens] of Object.entries(directives)) {
    for (const token of tokens) {
      const remote = /^(https?|wss?|ftp):/i.test(token) && !allowedOrigins.includes(token);
      if (token === "*" || remote || /^(https?|wss?):$/i.test(token)) {
        fail(`tauri.conf.json: ${name} ${directive} allows remote source ${token}`);
      }
    }
  }
}

function checkTauriConfig() {
  const confPath = join(ROOT, "src-tauri", "tauri.conf.json");
  const conf = JSON.parse(readFileSync(confPath, "utf8"));
  const security = conf.app?.security ?? {};
  const before = failures.length;

  checkCsp("csp", security.csp);
  checkCsp("devCsp", security.devCsp ?? security.csp, ["ws://localhost:1420"]);
  if (security.dangerousDisableAssetCspModification) {
    fail("tauri.conf.json: dangerousDisableAssetCspModification must not be set");
  }
  if (conf.plugins?.updater || conf.bundle?.createUpdaterArtifacts) {
    fail("tauri.conf.json: the updater must not be configured");
  }
  for (const win of conf.app?.windows ?? []) {
    if (!String(win.additionalBrowserArgs ?? "").includes("--disable-background-networking")) {
      fail(`tauri.conf.json: window "${win.label}" lacks --disable-background-networking`);
    }
  }
  // The installer must not download anything (WebView2 bootstrapper modes do).
  const webview = conf.bundle?.windows?.webviewInstallMode?.type ?? "downloadBootstrapper";
  if (!["skip", "offlineInstaller"].includes(webview)) {
    fail(`tauri.conf.json: webviewInstallMode "${webview}" downloads WebView2 during installation`);
  }
  // On Windows Tauri's fileAssociations make Konvert the default app; "Open with" is
  // done in the installer hooks. macOS and Linux declare theirs in their own config.
  if (conf.bundle?.fileAssociations?.length) {
    fail("tauri.conf.json: fileAssociations would change the user's default apps");
  }
  if (failures.length === before) passed.push("Tauri config: strict CSP, no updater, hardened WebView, offline installer");

  checkPlatformConfigs();

  const capDir = join(ROOT, "src-tauri", "capabilities");
  for (const file of readdirSync(capDir).filter((f) => f.endsWith(".json"))) {
    const cap = JSON.parse(readFileSync(join(capDir, file), "utf8"));
    for (const perm of cap.permissions ?? []) {
      const id = typeof perm === "string" ? perm : perm.identifier;
      if (!ALLOWED_PERMISSION.test(id)) fail(`capabilities/${file}: permission "${id}" is not allowed`);
    }
  }
  if (!failures.some((f) => f.startsWith("capabilities/"))) {
    passed.push("Capabilities: only Konvert commands, events, window and dialogs");
  }
}

/**
 * Platform files (tauri.<platform>.conf.json) are merged over tauri.conf.json:
 * they may change windows and bundles, never the security settings checked above.
 */
function checkPlatformConfigs() {
  const dir = join(ROOT, "src-tauri");
  const files = readdirSync(dir).filter((f) => /^tauri\.\w+\.conf\.json$/.test(f));
  const before = failures.length;
  for (const file of files) {
    const conf = JSON.parse(readFileSync(join(dir, file), "utf8"));
    const platform = file.split(".")[1];
    if (conf.app?.security) fail(`${file}: security settings belong in tauri.conf.json only`);
    if (conf.plugins?.updater || conf.bundle?.createUpdaterArtifacts) {
      fail(`${file}: the updater must not be configured`);
    }
    if (conf.bundle?.windows) fail(`${file}: Windows bundle settings belong in tauri.conf.json`);
    const associations = conf.bundle?.fileAssociations ?? [];
    if (platform === "windows" && associations.length) {
      fail(`${file}: fileAssociations would change the user's default apps`);
    }
    // macOS: "Alternate" lists Konvert in "Open With" without making it the default app.
    for (const a of platform === "macos" ? associations : []) {
      if (a.rank !== "Alternate") fail(`${file}: fileAssociations "${a.name}" must have rank "Alternate"`);
    }
  }
  if (files.length && failures.length === before) {
    passed.push(`Platform configs (${files.join(", ")}): no security override, no updater`);
  }
}

// --- 4. Source code ----------------------------------------------------------

function walk(dir, exts, out = []) {
  if (!existsSync(dir)) return out;
  for (const name of readdirSync(dir)) {
    if (name === "node_modules" || name === "target" || name === "gen") continue;
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path, exts, out);
    else if (exts.includes(extname(name))) out.push(path);
  }
  return out;
}

/** Heuristic: is the text at `index` inside a comment on this line? */
function inComment(line, index) {
  const before = line.slice(0, index);
  const head = before.trimStart();
  if (/^(\/\/|\/\*|\*|#|<!--)/.test(head)) return true;
  return /(^|\s|\{)(\/\/|\/\*)/.test(before);
}

const URL_RE = /\b(?:https?|wss?|ftp):\/\/[^\s'"`)<>\]]+/gi;
const FRONTEND_NET_API = /\bfetch\s*\(|XMLHttpRequest|new\s+WebSocket\b|new\s+EventSource\b|sendBeacon|importScripts\s*\(/;
const RUST_NET_API = /\bstd::net\b|\bTcpStream\b|\bTcpListener\b|\bUdpSocket\b|tokio::net\b/;

function checkSources() {
  const frontend = [
    ...walk(join(ROOT, "src"), [".ts", ".tsx", ".js", ".jsx", ".css", ".html", ".json"]),
    join(ROOT, "index.html"),
  ];
  // Integration tests (`crates/*/tests/`) are not shipped and deliberately use
  // URLs to check that they are refused, so they are not scanned.
  const rust = [
    ...walk(join(ROOT, "src-tauri", "src"), [".rs"]),
    ...walk(join(ROOT, "crates"), [".rs"]).filter((f) => !f.split(sep).includes("tests")),
  ];
  let urls = 0;
  for (const file of [...frontend, ...rust]) {
    const rel = relative(ROOT, file).split(sep).join("/");
    const isRust = file.endsWith(".rs");
    readFileSync(file, "utf8").split(/\r?\n/).forEach((line, i) => {
      for (const m of line.matchAll(URL_RE)) {
        if (inComment(line, m.index)) continue;
        if (ALLOWED_URL_PREFIXES.some((p) => m[0].startsWith(p))) { urls++; continue; }
        fail(`${rel}:${i + 1}: external URL ${m[0]}`);
      }
      const api = isRust ? RUST_NET_API : FRONTEND_NET_API;
      if (api.test(line) && !inComment(line, line.search(api))) {
        fail(`${rel}:${i + 1}: network API usage: ${line.trim()}`);
      }
    });
  }
  if (!failures.some((f) => /: (external URL|network API)/.test(f))) {
    passed.push(`Sources: ${frontend.length + rust.length} files, no external URLs (${urls} allow-listed) or network APIs`);
  }
}

checkRustDependencies();
checkNpmDependencies();
checkTauriConfig();
checkSources();

for (const p of passed) console.log(`  ok   ${p}`);
if (failures.length) {
  for (const f of failures) console.error(`  FAIL ${f}`);
  console.error(`\nPrivacy check failed (${failures.length} problem${failures.length > 1 ? "s" : ""}).`);
  process.exit(1);
}
console.log("\nPrivacy check passed: no network capability found.");
