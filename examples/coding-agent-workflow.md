# Example: Coding Agent Workflow

This workflow is meant to demonstrate Crab as a local coding agent runtime, not merely a
chat UI.

## What This Shows

- Goal-state tracking across a multi-step coding task.
- Workspace-aware context assembly.
- File, search, Git, test, and patch workflows.
- Context compression and retry recovery for longer sessions.
- Evidence-oriented final answers.

## Setup

Use a small disposable repository or a branch created only for the demo.

```bash
export OPENAI_API_KEY="your-api-key"
export OPENAI_BASE_URL="https://api.openai.com/v1"
export HERMES_RS_MODEL="gpt-4.1-mini"
```

For Cockpit/NewAPI-style gateways, use the exported OpenAI-compatible base URL and key.

## Demo Prompt

```bash
cargo run -- chat --prompt "Inspect this repository, find one small reliability or documentation improvement, implement it, run the relevant checks, and summarize the evidence."
```

## What To Narrate

Crab should be framed as a runtime:

- the main model keeps the objective and tradeoffs in view;
- tools are governed registry entries, not unrestricted magic;
- local state captures sessions, goal state, memories, skills, traces, and todos;
- delegated worker runs can handle bounded subtasks while the main agent stays focused on
  the goal.

## Good Follow-Up Prompts

```text
Continue from the current goal state and turn the fix into a concise PR description.
```

```text
Review your own change for risks, missing tests, and release-note impact.
```

```text
Create a reusable skill for this workflow if it would help future sessions.
```

## Expected Evidence

A strong demo ends with:

- files changed;
- checks run;
- any remaining risk;
- next useful action;
- a clear distinction between verified facts and model judgment.

## Safety Notes

Do not run this in a private repository during a public demo unless the screen is
controlled and logs are sanitized. Use `--enable-shell` only when you are comfortable with
the command boundary.
