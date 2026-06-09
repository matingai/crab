# Badges, Topics, And Repository Packaging

This page keeps the public presentation of Crab consistent across GitHub, README files,
release notes, and community templates.

## Recommended Repository Description

Rust-native local agent runtime and desktop shell built around a goal-state agent loop,
worker delegation, governed tools, and local-first workspace state.

## Recommended GitHub Topics

Use concise topics that match the actual project surface:

```text
rust
ai-agent
agent-runtime
agent-loop
tool-calling
local-first
desktop-agent
electron
tauri
openai-compatible
mcp
automation
developer-tools
```

## Badge Policy

Badges should signal real properties of the repository. Prefer badges that link to useful
project surfaces instead of pure decoration.

Current badge groups:

- project health: stars, license, CI, last commit, issues, PRs welcome;
- runtime identity: Rust 2024, active 0.1.x, OpenAI-compatible models;
- architecture: agent loop, worker delegation, local-first runtime, desktop shell;
- community: contributing, security policy, roadmap, docs.

Avoid badges for claims that are not continuously true, such as production-ready,
enterprise-grade, audited, benchmark-leading, or formally verified.

## Suggested Social Preview

If GitHub social preview is configured, use a clean banner with:

```text
Crab
Rust-native local agent runtime
Goal-state agent loop · worker delegation · local-first desktop shell
```

Recommended visual direction: dark neutral background, high-contrast title, small
architecture keywords, and one screenshot strip from `docs/assets/screenshots/`.

## Label Themes

The canonical label list is in `.github/labels.yml`. The most important labels for public
project identity are:

- `area: agent-loop`
- `area: delegation`
- `area: tools`
- `area: desktop`
- `area: memory-skills`
- `area: security`

These labels make the project look organized while also nudging contributors toward the
ideas that make Crab different.
