# Crab Examples

These examples are designed for demos, issue reproduction, blog posts, and onboarding.
They are intentionally written as scenario playbooks rather than polished benchmark claims.

## Example Index

- [Coding Agent Workflow](coding-agent-workflow.md): show the goal-state loop, file/Git
  tools, recovery, and a reviewable implementation path.
- [Research Browser PDF Workflow](research-browser-pdf-workflow.md): show browser/PDF
  oriented research with evidence capture and synthesis.
- [Document Office Workflow](document-office-workflow.md): show Office/document workflows
  and local artifact handling.

## Model Setup

Crab uses OpenAI-compatible providers. You can point it at OpenAI directly or at a local
gateway/proxy such as Cockpit/NewAPI as long as it exposes an OpenAI-compatible endpoint.

```bash
export OPENAI_API_KEY="your-api-key"
export OPENAI_BASE_URL="https://api.openai.com/v1"
export HERMES_RS_MODEL="gpt-4.1-mini"
```

For a gateway exported from Cockpit, keep the same shape and replace `OPENAI_BASE_URL` and
`OPENAI_API_KEY` with the values from that gateway:

```bash
export OPENAI_API_KEY="cockpit-exported-key"
export OPENAI_BASE_URL="https://your-cockpit-gateway.example.com/v1"
export HERMES_RS_MODEL="your-routed-model"
```

Do not commit real keys or local provider configuration.

## No-Key Smoke Demo

The `debug-context` command is useful for showing Crab's workspace-aware context assembly
without making a model request:

```bash
cargo run -- debug-context --prompt "Explain how the Crab agent loop tracks goals and delegates work."
```

This is a good first command for screenshots, quick talks, and CI-safe smoke checks.

## Recommended Demo Order

1. Run `cargo test --locked` to show the runtime is currently green.
2. Run `cargo run -- profile` to show local capability detection.
3. Run `cargo run -- debug-context --prompt "..."` to show prompt/context assembly without
   spending model tokens.
4. Start the desktop renderer:

   ```bash
   cd desktop-shell
   npm run dev
   ```

5. Open `http://localhost:1420` and show the timeline-oriented interface.
6. Run one live model-backed workflow from the examples below.

For public demos, use demo repositories and sanitized documents only.
