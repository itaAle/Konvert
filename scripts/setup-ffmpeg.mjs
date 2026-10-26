#!/usr/bin/env node
// `npm run setup:ffmpeg`: runs the setup script of the current platform.
// Windows: scripts/setup-ffmpeg.ps1; Linux and macOS: scripts/setup-ffmpeg.sh.
// Extra arguments are forwarded (e.g. `npm run setup:ffmpeg -- --force`).

import { spawnSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);

const [cmd, cmdArgs] =
  process.platform === "win32"
    ? [
        "powershell",
        [
          "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", join(here, "setup-ffmpeg.ps1"),
          ...args.map((a) => (a === "--force" ? "-Force" : a)),
        ],
      ]
    : ["bash", [join(here, "setup-ffmpeg.sh"), ...args]];

const result = spawnSync(cmd, cmdArgs, { stdio: "inherit" });
if (result.error) {
  console.error(result.error.message);
  process.exit(1);
}
process.exit(result.status ?? 1);
