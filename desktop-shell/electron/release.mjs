import { spawnSync } from "node:child_process";
import process from "node:process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const rootDir = path.resolve(__dirname, "..");

const build = spawnSync("npm", ["run", "build"], {
  cwd: rootDir,
  stdio: "inherit",
  shell: true,
});

if (build.status !== 0) {
  process.exit(build.status ?? 1);
}

const electron = spawnSync("npx", ["electron", "./electron/main.mjs"], {
  cwd: rootDir,
  stdio: "inherit",
  shell: true,
  env: {
    ...process.env,
    HERMES_ELECTRON_MODE: "production",
  },
});

process.exit(electron.status ?? 0);
