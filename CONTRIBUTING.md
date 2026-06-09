# Contributing to Crab

Thanks for taking the time to improve Crab. The project is still in the 0.1.x phase, so
good contributions are usually small, well-scoped, and backed by clear evidence.

## Project Direction

Crab is a Rust-native local agent runtime with a desktop shell. Its most important design
idea is the agent loop: a goal-state controller that tracks progress, delegates bounded
work to workers or auxiliary models, and folds tool evidence back into persistent local
state.

Contributions are especially useful when they improve:

- agent loop reliability, goal-state reconciliation, or recovery behavior;
- safe and inspectable tool execution;
- worker delegation and evidence handoff;
- local memory, skills, and workspace context assembly;
- desktop event streaming and UI ergonomics;
- documentation, screenshots, examples, and open-source hygiene.

## Development Setup

```bash
cargo check
cargo test
```

For the desktop shell:

```bash
cd desktop-shell
npm install
npm run build
```

The Electron development shell can be started with:

```bash
cd desktop-shell
npm run electron:dev
```

## Before Opening A Pull Request

- Keep the change focused. Split unrelated work into separate PRs.
- Run the relevant checks for the area you touched.
- Update README or docs when behavior, commands, configuration, or architecture changes.
- Do not commit local runtime state, `.env` files, generated reports, generated decks,
  private screenshots, or provider credentials.
- Include screenshots for visible desktop-shell changes when possible.
- Explain the tradeoff if the change adds a new dependency or long-lived data format.

## Pull Request Style

A good PR description usually includes:

- what changed;
- why it matters;
- how it was tested;
- screenshots or traces for UI/runtime behavior;
- known limitations or follow-up work.

## Privacy And Security

Crab is designed to run in local workspaces and can interact with files, browsers, Office
documents, and optional shell commands. Treat test data and logs carefully. If you find a
real vulnerability or accidental secret exposure, follow [SECURITY.md](SECURITY.md)
instead of opening a public issue.
