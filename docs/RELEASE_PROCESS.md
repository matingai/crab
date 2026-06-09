# Release Process

Crab is currently pre-stable. Releases should be clear about what changed and honest about
compatibility risk.

## Pre-Release Checklist

- Run root Rust checks:

  ```bash
  cargo fmt --check
  cargo test --locked
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
- Tag the release only after the release commit is pushed.

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
