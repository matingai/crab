# Desktop Packaging

Crab ships both a CLI and a desktop shell. The CLI archives are still the best fit for
developers, servers, and scripted workflows. The desktop installers are for users who want
to download an app, open it, choose a workspace, and inspect the agent loop visually.

Chinese version: [桌面安装包](DESKTOP_PACKAGING.zh-CN.md).

## User Install Path

Most users should not build Crab from source. For a tagged release, send them to
[GitHub Releases](https://github.com/matingai/crab/releases) and have them download one
of the desktop assets:

- macOS Apple Silicon: `crab-desktop-vX.Y.Z-aarch64-apple-darwin.dmg`
- macOS Intel: `crab-desktop-vX.Y.Z-x86_64-apple-darwin.dmg`
- Windows x64: `crab-desktop-vX.Y.Z-x86_64-pc-windows-msvc-setup.exe`

The intended install experience is deliberately ordinary:

- macOS: open the DMG and drag Crab into Applications.
- Windows: run the setup `.exe`.

The current 0.1.x installers are unsigned preview builds. Keep that note visible in
release notes until macOS notarization and Windows Authenticode signing are wired in.

## Release Assets

Tagged releases build these desktop installer assets:

| Platform | Asset |
| --- | --- |
| macOS Apple Silicon | `crab-desktop-vX.Y.Z-aarch64-apple-darwin.dmg` |
| macOS Intel | `crab-desktop-vX.Y.Z-x86_64-apple-darwin.dmg` |
| Windows x64 | `crab-desktop-vX.Y.Z-x86_64-pc-windows-msvc-setup.exe` |

Each installer has a matching `.sha256` checksum file. CLI archives are published beside
the installers for users who prefer terminal-first workflows. Desktop installers also get
a small `.json` release manifest with the asset name, version, target triple, bundle type,
and SHA-256 checksum. The manifest is intended for download pages, mirrors, and future
installer discovery without scraping release notes.

## Local Build

Install dependencies once:

```bash
cd desktop-shell
npm install
```

Build the installer for the current platform:

```bash
cd desktop-shell
npm run package:desktop
```

Build a macOS DMG on macOS:

```bash
cd desktop-shell
npm run package:dmg
```

Build a Windows setup installer from Windows:

```bash
cd desktop-shell
npm run package:exe
```

The helper writes release-ready assets into `dist/`, for example
`crab-desktop-v0.1.4-aarch64-apple-darwin.dmg`, plus matching `.sha256` and `.json`
metadata files. Tauri's native bundle output remains under
`desktop-shell/src-tauri/target/release/bundle/`.

Set `CRAB_TARGET` when you want the helper to pass an explicit Tauri target and label the
asset with that target triple:

```bash
CRAB_TARGET=aarch64-apple-darwin scripts/package-desktop.sh
```

The helper also accepts explicit flags:

```bash
scripts/package-desktop.sh --target aarch64-apple-darwin --bundle dmg --version v0.1.4
```

CI uses this mode for every desktop matrix entry. Explicit targets write Tauri output
under `desktop-shell/src-tauri/target/<target>/release/bundle/`.

For direct Tauri debugging, run the bundler from `desktop-shell/`:

```bash
npm run tauri:release -- --bundles dmg
```

Use `--bundles nsis` on Windows.

## CI Build

`.github/workflows/release.yml` builds desktop installers on native GitHub-hosted runners:

- `macos-14` for Apple Silicon DMG.
- `macos-15-intel` for Intel Mac DMG.
- `windows-2025` for Windows x64 NSIS setup `.exe`.

The release workflow uses `scripts/package-desktop.sh` so local packaging and CI packaging
share the same stable asset names, SHA-256 checksum behavior, and release manifest shape.

Release assets should include, for each desktop installer:

- the installer itself (`.dmg` or setup `.exe`);
- a sibling `.sha256` checksum file;
- a sibling `.json` manifest file.

## Signing And Trust

The current 0.1.x desktop installers are unsigned preview builds. They are usable for
testing, but operating systems may warn users:

- macOS may show Gatekeeper warnings because the DMG is not signed and notarized yet.
- Windows may show SmartScreen warnings because the setup executable is not code-signed yet.

A production-grade desktop release should add:

- Apple Developer ID signing.
- macOS notarization and stapling.
- Windows Authenticode signing.
- A documented checksum verification path for every installer.

## Checksum Verification

For macOS or Linux:

```bash
shasum -a 256 -c crab-desktop-vX.Y.Z-aarch64-apple-darwin.dmg.sha256
```

For Windows PowerShell:

```powershell
Get-FileHash .\crab-desktop-vX.Y.Z-x86_64-pc-windows-msvc-setup.exe -Algorithm SHA256
Get-Content .\crab-desktop-vX.Y.Z-x86_64-pc-windows-msvc-setup.exe.sha256
```

The two SHA-256 values should match before the installer is opened.

## Versioning

The desktop package version should stay aligned across:

- root `Cargo.toml`;
- `desktop-shell/package.json`;
- `desktop-shell/src-tauri/Cargo.toml`;
- `desktop-shell/src-tauri/tauri.conf.json`.

Before tagging, run the release checklist in [Release Process](RELEASE_PROCESS.md).
