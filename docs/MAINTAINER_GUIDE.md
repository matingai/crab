# Maintainer Guide

This guide is for keeping the public Crab repository coherent as it grows.

## Project Positioning

Describe Crab as:

- a Rust-native local agent runtime;
- a goal-state centered agent loop;
- a governed tool registry;
- a desktop shell and bridge surface;
- a local-first workspace automation system.

Avoid describing it as a finished production assistant or a generic ChatGPT clone. The
project is more interesting when the agent loop and runtime architecture stay visible.

## Triage Priorities

1. Security, privacy, and unsafe tool behavior.
2. Reproducible crashes, data loss, or broken quick-start paths.
3. Agent loop regressions: goal tracking, context pressure, recovery, delegation, and
   evidence handling.
4. Desktop shell regressions that block normal workflows.
5. Documentation and examples that help contributors understand the architecture.

## Labels

Use `.github/labels.yml` as the canonical label list. If syncing manually, keep these label
families:

- `type:*` for the shape of the work;
- `area:*` for the subsystem;
- `needs triage`, `blocked`, `help wanted`, and `good first issue` for workflow state.

## Public Hygiene

- Keep local runtime paths ignored.
- Prefer demo data for screenshots.
- Avoid personal emails, private account names, private API keys, local absolute paths, and
  proprietary documents in docs or tests.
- When in doubt, link to `docs/OPEN_SOURCE_REVIEW.md` before publishing a release.

## Documentation Rhythm

When the agent loop changes, update `docs/AGENT_LOOP.md`.
When runtime boundaries or state layout change, update `docs/ARCHITECTURE.md`.
When public commands change, update both root READMEs.
When the project direction changes, update `ROADMAP.md` and `docs/FUTURE_VISION.md`.
