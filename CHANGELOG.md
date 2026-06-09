# Changelog

All notable public-facing changes to Crab should be documented here.

This project follows a lightweight form of [Keep a Changelog](https://keepachangelog.com/)
and uses semantic versioning once stable release boundaries are established.

## Unreleased

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
