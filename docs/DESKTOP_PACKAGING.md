# Desktop Packaging

Crab ships both a CLI and a desktop shell. The CLI archives are still the best fit for
developers, servers, and scripted workflows. The desktop installers are for users who want
to download an app, open it, choose a workspace, and inspect the agent loop visually.

## Release Assets

Tagged releases build these desktop installer assets:

| Platform | Asset |
| --- | --- |
| macOS Apple Silicon | `crab-desktop-vX.Y.Z-aarch64-apple-darwin.dmg` |
| macOS Intel | `crab-desktop-vX.Y.Z-x86_64-apple-darwin.dmg` |
| Windows x64 | `crab-desktop-vX.Y.Z-x86_64-pc-windows-msvc-setup.exe` |

Each installer has a matching `.sha256` checksum file. CLI archives are published beside
the installers for users who prefer terminal-first workflows.

## Local Build

Install dependencies once:

```bash
cd desktop-shell
npm install
```

Build a macOS DMG:

```bash
cd ..
scripts/package-desktop.sh
```

Build a Windows setup installer from Windows:

```powershell
cd ..
bash scripts/package-desktop.sh
```

The helper writes release-ready assets into `dist/`, for example
`crab-desktop-v0.1.4-aarch64-apple-darwin.dmg`, plus a matching `.sha256` checksum. Tauri's
native bundle output remains under `desktop-shell/src-tauri/target/release/bundle/`.

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
share the same stable asset names and SHA-256 checksum behavior.

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

## Versioning

The desktop package version should stay aligned across:

- root `Cargo.toml`;
- `desktop-shell/package.json`;
- `desktop-shell/src-tauri/Cargo.toml`;
- `desktop-shell/src-tauri/tauri.conf.json`.

Before tagging, run the release checklist in [Release Process](RELEASE_PROCESS.md).
