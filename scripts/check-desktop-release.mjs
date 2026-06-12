#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");

const errors = [];
const warnings = [];

main();

function main() {
  const rootCargoVersion = readTomlValue("Cargo.toml", "version");
  const tauriCargoVersion = readTomlValue("desktop-shell/src-tauri/Cargo.toml", "version");
  const desktopPackage = readJson("desktop-shell/package.json");
  const tauriConfig = readJson("desktop-shell/src-tauri/tauri.conf.json");

  checkVersions({
    rootCargoVersion,
    desktopPackageVersion: desktopPackage.version,
    tauriCargoVersion,
    tauriConfigVersion: tauriConfig.version,
  });
  checkDesktopPackage(desktopPackage);
  checkTauriConfig(tauriConfig);
  checkReleaseWorkflow();
  checkPackagingScripts();
  printReport(rootCargoVersion);
}

function checkVersions(versions) {
  const entries = Object.entries(versions);
  const values = new Set(entries.map(([, value]) => value));
  if (values.size !== 1) {
    errors.push(
      "Desktop release versions are not aligned:\n" +
        entries.map(([label, value]) => `  - ${label}: ${value}`).join("\n"),
    );
  }
}

function checkDesktopPackage(packageJson) {
  const scripts = packageJson.scripts ?? {};
  requireScript(scripts, "package:desktop", "package-desktop.mjs");
  requireScript(scripts, "package:dmg", "--bundle dmg");
  requireScript(scripts, "package:exe", "--bundle nsis");
  requireScript(scripts, "release:check", "check-desktop-release.mjs");
}

function checkTauriConfig(config) {
  requireEqual(config.productName, "Crab", "Tauri productName");
  requireEqual(config.identifier, "com.matingai.crab.desktop", "Tauri identifier");
  requireEqual(config.build?.frontendDist, "../out", "Tauri frontendDist");
  requireEqual(config.bundle?.active, true, "Tauri bundle.active");
  requireNonEmpty(config.bundle?.publisher, "Tauri bundle.publisher");
  requireNonEmpty(config.bundle?.shortDescription, "Tauri bundle.shortDescription");
  requireNonEmpty(config.bundle?.longDescription, "Tauri bundle.longDescription");

  if (!bundleTargetsInclude(config.bundle?.targets, "dmg")) {
    errors.push("Tauri bundle targets must include dmg or be set to all.");
  }
  if (!bundleTargetsInclude(config.bundle?.targets, "nsis")) {
    errors.push("Tauri bundle targets must include nsis or be set to all.");
  }
  if (!config.bundle?.macOS?.minimumSystemVersion) {
    warnings.push("Tauri macOS minimumSystemVersion is not set.");
  }
  if (!config.bundle?.windows?.nsis) {
    errors.push("Tauri Windows NSIS bundle configuration is missing.");
  }

  for (const icon of config.bundle?.icon ?? []) {
    requireFile(path.join("desktop-shell/src-tauri", icon), `Tauri icon ${icon}`);
  }
}

function checkReleaseWorkflow() {
  const workflowPath = ".github/workflows/release.yml";
  const workflow = readText(workflowPath);
  requireContains(workflow, "scripts/package-desktop.sh", "release workflow desktop packaging step");
  requireContains(workflow, "aarch64-apple-darwin", "release workflow macOS Apple Silicon target");
  requireContains(workflow, "x86_64-apple-darwin", "release workflow macOS Intel target");
  requireContains(workflow, "x86_64-pc-windows-msvc", "release workflow Windows x64 target");
  requireContains(workflow, "bundles: dmg", "release workflow DMG bundle");
  requireContains(workflow, "bundles: nsis", "release workflow NSIS bundle");
  requireContains(workflow, "dist/*.dmg", "release workflow DMG artifact upload");
  requireContains(workflow, "dist/*.exe", "release workflow EXE artifact upload");
  requireContains(workflow, "dist/*.json", "release workflow manifest artifact upload");
}

function checkPackagingScripts() {
  const helper = readText("scripts/package-desktop.mjs");
  requireFile("scripts/package-desktop.sh", "desktop shell wrapper");
  requireContains(helper, "CRAB_DESKTOP_BUNDLE", "desktop package bundle override");
  requireContains(helper, "CRAB_TARGET", "desktop package target override");
  requireContains(helper, ".sha256", "desktop package checksum output");
  requireContains(helper, ".json", "desktop package manifest output");
  requireContains(helper, "-setup", "Windows setup asset suffix");
}

function printReport(version) {
  const releaseVersion = `v${version}`;
  const plannedAssets = [
    `crab-desktop-${releaseVersion}-aarch64-apple-darwin.dmg`,
    `crab-desktop-${releaseVersion}-x86_64-apple-darwin.dmg`,
    `crab-desktop-${releaseVersion}-x86_64-pc-windows-msvc-setup.exe`,
  ];

  console.log("Crab desktop release preflight");
  console.log(`Version: ${releaseVersion}`);
  console.log("Planned desktop assets:");
  for (const asset of plannedAssets) {
    console.log(`  - ${asset}`);
    console.log(`  - ${asset}.sha256`);
    console.log(`  - ${asset}.json`);
  }

  if (warnings.length > 0) {
    console.log("\nWarnings:");
    for (const warning of warnings) {
      console.log(`  - ${warning}`);
    }
  }

  if (errors.length > 0) {
    console.error("\nFailures:");
    for (const error of errors) {
      console.error(`  - ${error}`);
    }
    process.exit(1);
  }

  console.log("\nOK: desktop DMG/EXE packaging is release-ready.");
}

function readJson(relativePath) {
  return JSON.parse(readText(relativePath));
}

function readText(relativePath) {
  const filePath = path.join(repoRoot, relativePath);
  if (!fs.existsSync(filePath)) {
    errors.push(`${relativePath} is missing.`);
    return "";
  }
  return fs.readFileSync(filePath, "utf8");
}

function readTomlValue(relativePath, key) {
  const text = readText(relativePath);
  const match = text.match(new RegExp(`^\\s*${escapeRegExp(key)}\\s*=\\s*"([^"]+)"`, "m"));
  if (!match) {
    errors.push(`Unable to read ${key} from ${relativePath}.`);
    return "";
  }
  return match[1];
}

function requireFile(relativePath, label) {
  if (!fs.existsSync(path.join(repoRoot, relativePath))) {
    errors.push(`${label} is missing at ${relativePath}.`);
  }
}

function requireScript(scripts, name, expectedText) {
  const command = scripts[name];
  if (!command) {
    errors.push(`desktop-shell/package.json is missing script '${name}'.`);
    return;
  }
  if (!command.includes(expectedText)) {
    errors.push(`desktop-shell/package.json script '${name}' should include '${expectedText}'.`);
  }
}

function requireEqual(actual, expected, label) {
  if (actual !== expected) {
    errors.push(`${label} should be '${expected}', got '${actual}'.`);
  }
}

function requireNonEmpty(value, label) {
  if (typeof value !== "string" || value.trim() === "") {
    errors.push(`${label} must be set.`);
  }
}

function requireContains(text, expected, label) {
  if (!text.includes(expected)) {
    errors.push(`${label} should contain '${expected}'.`);
  }
}

function bundleTargetsInclude(targets, expected) {
  return targets === "all" || (Array.isArray(targets) && targets.includes(expected));
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}
