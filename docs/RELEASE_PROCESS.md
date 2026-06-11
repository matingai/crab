# Release Process

Crab is currently pre-stable. Releases should be clear about what changed and honest about
compatibility risk.

## Pre-Release Checklist

- Run root Rust checks:

  ```bash
  cargo fmt --check
  cargo test --locked
  cargo run -- doctor
  cargo metadata --no-deps --format-version 1
  scripts/package-release.sh
  ```

- Run desktop checks when the desktop shell changed:

  ```bash
  cd desktop-shell
  npm ci
  npm run build
  npm run tauri:release -- --bundles dmg
  ```

- Review open-source hygiene:

  ```bash
  git status --short --untracked-files=all
  rg --hidden -i "(api[_-]?key|secret|token|password|bearer|authorization|private[_-]?key)"
  ```

- Update `CHANGELOG.md`.
- Update screenshots if the visible desktop shell changed.
- Confirm README quick-start commands still match the CLI.
- Confirm `docs/INSTALL.md` still matches the current binary name and install path.
- Tag the release only after the release commit is pushed. Tags matching `v*` trigger
  `.github/workflows/release.yml`, which builds CLI archives for macOS arm64, macOS Intel,
  Linux x64, and Windows x64, plus desktop installers for macOS and Windows, then
  publishes a prerelease with assets attached.

## Release Archives

The release workflow builds these assets:

Desktop installers:

- `crab-desktop-vX.Y.Z-aarch64-apple-darwin.dmg`
- `crab-desktop-vX.Y.Z-x86_64-apple-darwin.dmg`
- `crab-desktop-vX.Y.Z-x86_64-pc-windows-msvc-setup.exe`
- matching `.sha256` checksum files

CLI archives:

- `crab-vX.Y.Z-aarch64-apple-darwin.tar.gz`
- `crab-vX.Y.Z-x86_64-apple-darwin.tar.gz`
- `crab-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz`
- `crab-vX.Y.Z-x86_64-pc-windows-msvc.zip`
- matching `.sha256` checksum files

Use `scripts/package-release.sh` for a local single-platform archive before tagging.

Each archive should contain:

- the `crab` or `crab.exe` binary;
- `README.md` and `README.zh-CN.md`;
- `LICENSE`;
- `docs/INSTALL.md` and `docs/INSTALL.zh-CN.md`;
- `docs/QUICKSTART.md` and `docs/QUICKSTART.zh-CN.md`;
- `scripts/install.sh` and `scripts/install.ps1`.

## Desktop Installer Notes

Desktop installers are built from `desktop-shell/src-tauri` with Tauri:

- macOS builds publish unsigned DMGs for Apple Silicon and Intel.
- Windows builds publish an unsigned NSIS setup `.exe`.
- The 0.1.x installers are preview builds; expect Gatekeeper or SmartScreen warnings until
  Apple Developer ID, notarization, and Windows Authenticode signing are wired into CI.

Keep desktop versions aligned across root `Cargo.toml`, `desktop-shell/package.json`,
`desktop-shell/src-tauri/Cargo.toml`, and `desktop-shell/src-tauri/tauri.conf.json`.

## Versioning Notes

- `0.1.x`: active prototype, breaking changes can happen with clear notes.
- `0.2.x`: expected once runtime paths, core CLI shape, and bridge events are less volatile.
- `1.0.0`: should wait until the tool boundary, event protocol, local state layout, and
  extension story are stable enough for downstream users.

## Release Notes Shape

Good release notes should include:

- one-paragraph project-level summary;
- agent loop and delegation changes;
- tool or desktop shell changes;
- breaking changes and migration notes;
- security or privacy notes;
- known limitations.
