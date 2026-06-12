#!/usr/bin/env node
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
}

function main() {
  const args = process.argv.slice(2);
  const options = parseArgs(args);

  const version = normalizeVersion(
    options.version ?? process.env.CRAB_VERSION ?? `v${readRootCargoVersion()}`,
  );
  const targetWasExplicit = Boolean(options.target ?? process.env.CRAB_TARGET);
  const target = options.target ?? process.env.CRAB_TARGET ?? readRustHostTriple();
  const targetPlatform = resolveTargetPlatform(target);
  const bundle =
    options.bundle ?? process.env.CRAB_DESKTOP_BUNDLE ?? defaultBundleForTarget(targetPlatform);

  const bundleInfo = resolveBundleInfo(bundle);
  if (!isBundleCompatible(targetPlatform, bundleInfo.bundle)) {
    fail(
      `Bundle '${bundleInfo.bundle}' is not compatible with target '${target}'. ` +
        "Use dmg for macOS targets or nsis for Windows targets.",
    );
  }

  const desktopDir = path.join(repoRoot, "desktop-shell");
  const buildReleaseDir = targetWasExplicit
    ? path.join(desktopDir, "src-tauri", "target", target, "release")
    : path.join(desktopDir, "src-tauri", "target", "release");
  const nextEnvPath = path.join(desktopDir, "next-env.d.ts");
  const bundleOutputDir = path.join(buildReleaseDir, "bundle");
  const specificBundleDir = path.join(bundleOutputDir, bundleInfo.bundle);

  let nextEnvBackup = null;
  try {
    nextEnvBackup = backupNextEnv(nextEnvPath);
    fs.rmSync(specificBundleDir, { recursive: true, force: true });

    const tauriArgs = ["run", "tauri:release", "--"];
    if (targetWasExplicit) {
      tauriArgs.push("--target", target);
    }
    tauriArgs.push("--bundles", bundleInfo.bundle);

    runNpm(tauriArgs, desktopDir, {
      ...process.env,
      NEXT_TELEMETRY_DISABLED: process.env.NEXT_TELEMETRY_DISABLED ?? "1",
    });

    const installer = findInstaller(bundleOutputDir, bundleInfo.extension);
    if (!installer) {
      const files = listFiles(bundleOutputDir).slice(0, 60).join("\n");
      fail(
        `No .${bundleInfo.extension} installer found in ${bundleOutputDir}` +
          (files ? `\nFiles found:\n${files}` : ""),
      );
    }

    const distDir = path.join(repoRoot, "dist");
    fs.mkdirSync(distDir, { recursive: true });

    const assetName = `crab-desktop-${version}-${target}${bundleInfo.assetSuffix}.${bundleInfo.extension}`;
    const assetPath = path.join(distDir, assetName);
    fs.copyFileSync(installer, assetPath);

    const checksum = sha256File(assetPath);
    fs.writeFileSync(path.join(distDir, `${assetName}.sha256`), `${checksum}  ${assetName}\n`);
    fs.writeFileSync(
      path.join(distDir, `${assetName}.json`),
      `${JSON.stringify(
        {
          name: assetName,
          version,
          target,
          platform: bundleInfo.platform,
          bundle: bundleInfo.bundle,
          kind: "desktop-installer",
          file: assetName,
          sha256: checksum,
          unsigned_preview: true,
          install_hint:
            "Download this installer from the matching GitHub release and verify the .sha256 file before opening it.",
        },
        null,
        2,
      )}\n`,
    );

    console.log(`Created dist/${assetName}`);
    console.log(`Created dist/${assetName}.sha256`);
    console.log(`Created dist/${assetName}.json`);
  } finally {
    restoreNextEnv(nextEnvPath, nextEnvBackup);
  }
}

function parseArgs(argv) {
  const parsed = {};
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--bundle" || arg === "--bundles") {
      parsed.bundle = readValue(argv, ++index, arg);
    } else if (arg === "--target") {
      parsed.target = readValue(argv, ++index, arg);
    } else if (arg === "--version") {
      parsed.version = readValue(argv, ++index, arg);
    } else if (arg === "--help" || arg === "-h") {
      printHelpAndExit();
    } else {
      fail(`Unknown argument '${arg}'. Run scripts/package-desktop.sh --help for usage.`);
    }
  }
  return parsed;
}

function readValue(argv, index, flag) {
  const value = argv[index];
  if (!value || value.startsWith("--")) {
    fail(`Missing value for ${flag}`);
  }
  return value;
}

function printHelpAndExit() {
  console.log(`Usage: scripts/package-desktop.sh [--bundle dmg|nsis] [--target triple] [--version vX.Y.Z]

Environment variables:
  CRAB_DESKTOP_BUNDLE  dmg or nsis
  CRAB_TARGET          Rust target triple, for example aarch64-apple-darwin
  CRAB_VERSION         Release label used in the output asset name

Outputs:
  dist/crab-desktop-<version>-<target>.dmg
  dist/crab-desktop-<version>-<target>-setup.exe
  plus matching .sha256 and .json metadata files`);
  process.exit(0);
}

function normalizeVersion(value) {
  const trimmed = String(value).trim();
  if (!trimmed) {
    fail("Version cannot be empty");
  }
  return trimmed;
}

function readRootCargoVersion() {
  const cargoToml = fs.readFileSync(path.join(repoRoot, "Cargo.toml"), "utf8");
  const match = cargoToml.match(/^\s*version\s*=\s*"([^"]+)"/m);
  if (!match) {
    fail("Unable to read package version from Cargo.toml");
  }
  return match[1];
}

function readRustHostTriple() {
  const result = spawnSync("rustc", ["-vV"], {
    cwd: repoRoot,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  if (result.status !== 0) {
    fail(`Unable to resolve Rust host target:\n${result.stderr || result.stdout}`);
  }
  const hostLine = result.stdout.split(/\r?\n/).find((line) => line.startsWith("host:"));
  if (!hostLine) {
    fail("Unable to find host triple in rustc -vV output");
  }
  return hostLine.replace("host:", "").trim();
}

function resolveTargetPlatform(target) {
  if (target.includes("apple-darwin")) {
    return "macOS";
  }
  if (target.includes("windows")) {
    return "Windows";
  }
  fail(
    `Unsupported desktop installer target: ${target}\n` +
      "Desktop installers currently support macOS DMG and Windows NSIS setup EXE.",
  );
}

function defaultBundleForTarget(platform) {
  return platform === "Windows" ? "nsis" : "dmg";
}

function resolveBundleInfo(bundle) {
  switch (bundle) {
    case "dmg":
      return {
        bundle,
        extension: "dmg",
        assetSuffix: "",
        platform: "macOS",
      };
    case "nsis":
      return {
        bundle,
        extension: "exe",
        assetSuffix: "-setup",
        platform: "Windows",
      };
    default:
      fail("Unsupported Tauri desktop bundle: " + bundle + "\nUse dmg on macOS or nsis on Windows.");
  }
}

function isBundleCompatible(platform, bundleName) {
  return (
    (platform === "macOS" && bundleName === "dmg") ||
    (platform === "Windows" && bundleName === "nsis")
  );
}

function backupNextEnv(filePath) {
  if (!fs.existsSync(filePath)) {
    return { existed: false, backupPath: null };
  }
  const backupPath = path.join(os.tmpdir(), `crab-next-env-${Date.now()}-${process.pid}.d.ts`);
  fs.copyFileSync(filePath, backupPath);
  return { existed: true, backupPath };
}

function restoreNextEnv(filePath, backup) {
  if (!backup) {
    return;
  }
  if (backup.existed) {
    fs.copyFileSync(backup.backupPath, filePath);
    fs.rmSync(backup.backupPath, { force: true });
  } else {
    fs.rmSync(filePath, { force: true });
  }
}

function runNpm(args, cwd, env) {
  const npmBin = process.platform === "win32" ? "npm.cmd" : "npm";
  const result = spawnSync(npmBin, args, {
    cwd,
    env,
    stdio: "inherit",
  });
  if (result.status !== 0) {
    fail(`npm ${args.join(" ")} failed with exit code ${result.status ?? "unknown"}`);
  }
}

function findInstaller(root, extension) {
  return listFiles(root)
    .filter((file) => file.endsWith(`.${extension}`))
    .sort((left, right) => left.localeCompare(right))[0];
}

function listFiles(root) {
  if (!fs.existsSync(root)) {
    return [];
  }
  const files = [];
  const stack = [root];
  while (stack.length > 0) {
    const current = stack.pop();
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const entryPath = path.join(current, entry.name);
      if (entry.isDirectory()) {
        stack.push(entryPath);
      } else if (entry.isFile()) {
        files.push(entryPath);
      }
    }
  }
  return files;
}

function sha256File(filePath) {
  const hash = crypto.createHash("sha256");
  hash.update(fs.readFileSync(filePath));
  return hash.digest("hex");
}

function fail(message) {
  throw new Error(message);
}
