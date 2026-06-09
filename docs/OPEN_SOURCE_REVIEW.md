# Open-source Privacy Review

This note records the privacy and repository-hygiene review performed while preparing the
project for public release. It is not a substitute for a dedicated secret scanner, but it
documents the highest-risk areas and the cleanup already applied.

## Scope Reviewed

- Tracked source files and documentation.
- Untracked local artifacts visible in the working tree.
- Git file history for obvious sensitive filenames.
- Current `HEAD` and all commits for common credential patterns such as OpenAI-style keys,
  GitHub tokens, Slack tokens, AWS access keys, Google API keys, and private key headers.

## Cleanup Applied

- Replaced personal placeholder values in the desktop auth form with neutral examples.
- Removed tracked generated sample report artifacts from `desktop-shell/`.
- Added ignore rules for `.env`, local agent data, desktop shell local data, generated
  reports, generated decks, and generated document files.
- Rewrote the root README as a professional English open-source README.
- Added a professional Chinese README at `README.zh-CN.md`.
- Added the MIT license.
- Rebuilt the repository into a single clean root commit with neutral author and committer
  metadata.
- Removed the previous local `origin` remote because it pointed at a personal account path.
- Set repository-local Git author defaults to neutral maintainer metadata for future
  commits.

## Current Findings

- No hard-coded real API key, private key, GitHub token, Slack token, AWS key, or Google API
  key was found by the pattern scans that were run.
- No direct author name, author email, or local absolute author path was found in the
  tracked working-tree text after cleanup. Generic words such as `author` still appear as
  document metadata fields in code.
- Local runtime data exists under `.hermes-agent-rs/` and `desktop-shell/.hermes-agent-rs/`.
  These directories are ignored, but they may contain sessions, logs, archives, provider
  settings, and model outputs. Keep them out of public commits.
- Generated research and presentation artifacts exist under `desktop-shell/doc/`,
  `desktop-shell/slides/`, and root-level generated document/deck files. The new ignore
  rules prevent accidental future commits, but local files may still remain on disk.
- The local Git history now contains one root commit with neutral author metadata. No old
  author metadata or old generated sample document paths are reachable from current refs.
- If the previous history was already pushed to a remote, that remote must be replaced or
  force-pushed with the clean history before publishing.

## Recommended Pre-release Checklist

1. Add a new public remote and push only the rebuilt clean history.
2. Run a dedicated scanner such as `gitleaks` or `trufflehog` against the final repository.
3. Run `cargo fmt --check`, `cargo test`, and the relevant desktop-shell checks.
4. Review all untracked files with `git status --short --untracked-files=all` before the
   first public commit.
5. Do not commit local `.env` files, `.hermes-agent-rs/`, desktop shell runtime state,
   generated documents, generated slides, or model outputs.

## Commands Used During This Review

```bash
rg --hidden -i "(api[_-]?key|secret|token|password|bearer|authorization|private[_-]?key)"
rg --hidden "<local-path-and-loopback-url-patterns>"
git log --all --name-only --pretty=format:
git grep -n -I -E "(sk-[A-Za-z0-9_-]{20,}|gh[pousr]_[A-Za-z0-9_]{20,}|xox[baprs]-[A-Za-z0-9-]{20,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{20,}|-----BEGIN (RSA|DSA|EC|OPENSSH|PRIVATE) KEY-----)" HEAD -- .
```
