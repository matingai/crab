# Changelog

All notable public-facing changes to Crab should be documented here.

This project follows a lightweight form of [Keep a Changelog](https://keepachangelog.com/)
and uses semantic versioning once stable release boundaries are established.

## Unreleased

### Changed

- Added a conservative `computer_use` foundation for macOS Accessibility-backed native
  desktop automation: status checks, permission prompting, frontmost-app UI tree
  snapshots with compact state flags, read-only ref inspection, native-action-aware ref
  search, text appear/disappear wait polling, read-only ref readiness waits, pre-action
  ref guards, native action availability guards, snapshot-bound approval-gated
  focus/click/text/scroll actions, whitelisted native Accessibility actions, whitelisted
  non-text key pressing, docs, and `doctor` visibility. Arbitrary keyboard typing and
  broad app-control write actions remain intentionally disabled.
- Tool calls now pass through a local `tool_policy` preflight that protects common
  sensitive paths by default and can require approval or disable configured tools/path
  patterns before execution. Path-like arguments are now inspected recursively, including
  nested arrays/objects and camelCase aliases.
- Direct web-fetch tools now pass through `network_policy`, which blocks loopback,
  private, link-local, and metadata-style hosts by default unless local config allows
  them.
- The `terminal` tool now accepts a bounded `timeout_seconds` argument, matching the
  controlled timeout surface already available in `execute_code`.
- Tool observations, live previews, session timeline details, archive records, and saved
  assistant tool-call arguments now redact common credential patterns before persistence.
- Approval requests now store redacted display commands/reasons and use a stable command
  hash for approval matching, while preserving compatibility with legacy raw-command
  approval files.
- Tool outcomes now carry explicit error status across events, timeline entries, archive
  records, goal-state reconciliation, and the desktop shell; parallel batches report
  `completed_with_errors` when all tools finish but one or more fail.
- Tool completion events now include a stable `error_kind` for malformed JSON arguments,
  invalid tool arguments, tool-policy denials, approval denials, non-zero process exits,
  timeouts, and generic execution failures.
- Malformed JSON and typed tool-argument errors now include a compact
  `expected_arguments_schema` hint so the next model turn can repair the tool call with
  less guesswork.
- Tool and parallel-batch completion events now include elapsed duration, and the desktop
  shell surfaces that timing in the live execution timeline.
- Turn start events now include a turn id, resumed flag, input character count, and
  redacted user-input preview instead of exposing the full prompt in the event stream.
- Agent turns now emit a `turn_finished` summary event with status, elapsed duration,
  tool-call count, and a redacted response preview so UIs can show a clear run boundary.
- User-requested stops now emit a structured `turn_interrupted` event with turn id, runtime
  phase, reason, and redacted message before the legacy stop error is returned.
- Resumed approvals now emit a structured `approval_resolved` event with approval status,
  approved/denied state, tool id, execution mode, and parallel-batch metadata before the
  paused tool continues.
- Main and background model completion events now include status-aware elapsed duration,
  making retry, routing fallback, and auxiliary-model latency visible in the desktop shell.
- Main and background model completion events now include provider token usage when the
  model API returns it, and the desktop shell displays usage in the live event stream.
- Model request start events now expose safe request-shape metadata such as API mode,
  response-continuation usage, and temporary output budgets for live debugging.
- Model request recovery now emits a structured `model_recovery` event for output-budget
  reduction, retry backoff, and context-overflow compression with a redacted provider-error
  preview.
- Prompt context preparation now emits a structured `context_prepared` event with projected
  tokens, request budget, message counts, retained context blocks, trim labels, and elapsed
  preparation time without exposing raw prompt content.
- Prompt context preparation now also emits `context_sources_updated` with kept/clipped/
  skipped source metadata and compact redacted previews for optional context blocks.
- Session persistence now emits richer checkpoint metadata through `session_saved`,
  including turn id, history/timeline counts, pending approvals, response-continuation
  availability, and a compact path preview for desktop recovery visibility.
- Delegated worker tools now emit `delegate_run_updated` lifecycle events when worker runs
  are created and finalized, including child session ids, attempts, status, objective
  previews, and compact result previews.
- Goal-state writes now emit a structured `goal_state_updated` event for user-input seeds,
  tool observations, tool-result reconcile, and turn-end reconcile, exposing focus-goal
  metadata and counts without streaming the full working memory.
- Todo-state writes now emit a structured `todo_state_updated` event for goal-state sync,
  explicit todo-tool updates, and delegated worker step updates, exposing counts and
  redacted active previews.
- Solve-trace writes now emit a structured `solve_trace_updated` event for episode starts,
  tool/delegation steps, delegated decisions, and turn outcomes, exposing compact redacted
  previews and trace counters.
- Experience and meta-pattern writes now emit `learning_state_updated` events for episode
  distillation, pattern rebuilds, and model-assisted pattern summaries so the learning
  loop is visible without exposing full memory records.
- Distilled experience and meta-pattern context are now enabled by default for CLI and
  desktop runs when matching records exist, with `HERMES_RS_DISABLE_LEARNING_CONTEXT` and
  granular disable flags for workspaces that want to opt out.
- Context compression now emits a structured `context_compacted` event with before/after
  message counts, estimated tokens, summary usage, trigger reason, and pruned-tool-output
  counts without exposing the compacted summary body.
- Desktop installer packaging now passes explicit Tauri targets in CI and emits a sibling
  `.json` release manifest beside each DMG or Windows setup `.exe`, including version,
  target, bundle type, and SHA-256 metadata.
- Desktop installer packaging now has a cross-platform Node helper plus
  `npm run package:desktop`, `npm run package:dmg`, and `npm run package:exe` shortcuts for
  release-ready local builds.
- Desktop release packaging now has a fast preflight check that verifies version alignment,
  Tauri DMG/NSIS configuration, required icons, packaging helper behavior, and the GitHub
  Release workflow matrix before native installers are built.
- The English and Chinese READMEs now surface desktop DMG and Windows setup `.exe`
  downloads near the top of the project page, with release asset names and unsigned
  preview-build notes for first-time users.
- Subdirectory instruction discovery now returns root-to-leaf context stacks, tracks loaded
  hint files instead of permanently marking empty directories, and labels blocked context
  with the exact display path.
- `execute_code` now shares the terminal tool's destructive shell-risk checks and pauses
  for approval before running obvious dangerous inline or file-backed scripts.
- File mutation tools now protect Git paths with uncommitted changes by default and require
  an explicit `allow_dirty` argument before modifying user-owned dirty worktree content.

## 0.1.4

### Added

- Tauri desktop installer packaging for macOS DMG and Windows NSIS setup `.exe` release
  assets.
- Desktop packaging documentation covering release asset names, CI behavior, checksums,
  and unsigned preview-build limitations.

### Changed

- The desktop shell package, Tauri crate, and Tauri bundle metadata now align with the root
  Crab version.

## 0.1.3

### Added

- `crab doctor` local diagnostics for workspace, provider, shell-safety, toolchain, and
  release hygiene checks.
- One-command release installers for macOS/Linux and Windows PowerShell.
- English and Chinese quickstart guides for first-time users and demos.

### Changed

- Release archives now include bilingual README/install/quickstart docs and installer
  scripts alongside the CLI binary.

## 0.1.2

### Changed

- Release publishing now uses the GitHub CLI instead of a Node-based release action.

### Fixed

- Release checksum files now use archive basenames, so users can run `shasum -c` after
  downloading an archive and its `.sha256` file into the same directory.
- The CLI now exposes `crab --version` for package smoke tests and user diagnostics.

## 0.1.1

### Changed

- Release archives now build with current GitHub-hosted runner labels for macOS Intel,
  macOS Apple Silicon, Linux x64, and Windows x64.
- GitHub Actions workflows use Node 24-compatible action versions.

### Fixed

- PDFKit-backed PDF tests now skip gracefully on CI environments where Swift exists
  without Apple's PDFKit framework.
- Local command cancellation now terminates Unix process groups so shell/script child
  processes are cleaned up reliably.

### Added

- Public open-source packaging files, including contribution, security, support, conduct,
  issue, and pull request templates.
- CI and dependency update configuration for the public repository.
- Recommended GitHub labels, topics, and repository presentation guidance.
- Demo-ready examples, launch copy, and a public demo script for project promotion.
- A first-run desktop demo state that showcases Crab's goal loop, tool evidence, and
  worker delegation story before a workspace is loaded.
- A promotion playbook covering launch sequence, channel tactics, social copy, metrics,
  and objection handling.
- An agent-loop feedback issue template and public feedback label for launch-driven design
  discussion.
- English and Chinese FAQ pages plus README no-key trial sections for first-time visitors.
- English and Chinese install guides plus package metadata for public source installs.
- Release automation for macOS, Linux, and Windows CLI archives, plus a local package
  script for single-platform installable builds.

### Changed

- CI now runs the full locked Rust test suite after the existing test failures were fixed.

### Fixed

- Preserved context-compaction summaries when rebuilding retry requests after model context
  overflow errors.
- Kept short retry context injections from being dropped by the minimum context block
  budget.
- Added a `skills.include_bundled` configuration switch so tests and minimal local stores
  can opt out of repository-bundled skills without changing production defaults.

## 0.1.0

### Added

- Rust-native local agent runtime and CLI.
- Goal-state centered agent loop with persistent session state.
- Tool registry covering workspace files, Git, browser, PDF, Office, memory, skills, MCP,
  cron, and delegation-oriented workflows.
- Desktop shell built with Next.js and Electron, with Tauri scaffolding.
- Documentation for architecture, agent loop design, project overview, future vision, and
  open-source privacy review.
