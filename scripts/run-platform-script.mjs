#!/usr/bin/env node
// Runs scripts/<name>.ps1 on Windows and scripts/<name>.sh on Linux and macOS,
// forwarding the remaining arguments. Used by `npm run setup:ffmpeg` and
// `npm run release`, e.g. `npm run release -- --skip-tests`.
// Long options are translated for PowerShell: --skip-tests → -SkipTests.

import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const [name, ...args] = process.argv.slice(2);
if (!name) {
  console.error("Usage: node scripts/run-platform-script.mjs <script name> [args...]");
  process.exit(2);
}

const toPowerShell = (arg) =>
  arg.startsWith("--")
    ? "-" + arg.slice(2).replace(/(^|-)([a-z])/g, (_, __, c) => c.toUpperCase())
    : arg;

const [cmd, cmdArgs] =
  process.platform === "win32"
    ? [
        "powershell",
        ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(here, `${name}.ps1`), ...args.map(toPowerShell)],
      ]
    : ["bash", [join(here, `${name}.sh`), ...args]];

const result = spawnSync(cmd, cmdArgs, { stdio: "inherit" });
if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
process.exit(result.status ?? 1);
