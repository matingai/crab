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
  Linux x64, and Windows x64, then publishes a prerelease with assets attached.

## Release Archives

The release workflow builds these assets:

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
