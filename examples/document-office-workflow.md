# Example: Document Office Workflow

This workflow is meant to show Crab's document and Office-oriented tooling. It is useful
for demos aimed at people who care about research reports, decks, and local knowledge work.

## What This Shows

- Local document inspection.
- Office/PDF workflow integration.
- Structured artifact planning.
- Agent loop progress tracking for non-code tasks.
- Reviewable local outputs rather than opaque chat-only answers.

## Setup

Use generated or public demo documents. Avoid private reports, customer decks, personal
files, and unreleased business material in public recordings.

```bash
export OPENAI_API_KEY="your-api-key"
export OPENAI_BASE_URL="https://api.openai.com/v1"
export HERMES_RS_MODEL="gpt-4.1-mini"
```

## Demo Prompt

```bash
cargo run -- chat --prompt "Create a short project brief for Crab based on README.md and docs/AGENT_LOOP.md. Include an executive summary, architecture highlights, demo ideas, and risks to avoid when promoting the project."
```

If Office generation/rendering is configured:

```bash
cargo run -- chat --prompt "Create a concise demo brief document for Crab from README.md and docs/AGENT_LOOP.md, then inspect the generated file and summarize what changed."
```

## What To Narrate

The point is not that Crab can write prose. The point is that the same runtime can move
between code, research, local documents, and desktop workflows while keeping a durable goal
state.

Good narration:

- Crab treats documents as local artifacts;
- tool results are evidence, not just hidden implementation details;
- generated outputs should be inspected or previewed before being treated as final;
- the same agent loop can support coding, research, and office workflows.

## Public Demo Checklist

- Use generated demo documents.
- Keep output paths inside a disposable workspace.
- Inspect the artifact before showing it.
- Remove generated reports/decks before committing.
