# Agent Loop Eval

Crab includes a lightweight CLI eval harness for checking whether the agent loop behaves like a
goal-driven runtime rather than a plain chat wrapper.

The harness is intentionally small and local-first. It verifies the behaviors that matter most for
the current architecture:

- simple turns route to the small model;
- small-model simple turns send zero tool schemas;
- tool-grounded tasks stay on the main model;
- tool calls become session evidence;
- model request events expose model, API mode, tool count, duration, and token usage when the
  provider returns usage fields.

## Run The Core Suite

```bash
export OPENAI_API_KEY="..."

python3 scripts/agent-loop-eval.py \
  --base-url http://localhost:50930/v1 \
  --api-mode responses \
  --main-model gpt-5.5 \
  --small-model gpt-5.4-mini
```

The default `core` suite runs stable tasks:

- `direct_simple`: confirms a short direct answer uses the small model and no tools.
- `workspace_tool`: confirms an explicit workspace inspection stays on the main model and calls
  `list_files`.
- `code_navigation`: confirms code-search style work stays on the main model and uses repository
  tools.

Reports are written under:

```text
target/agent-loop-eval/<timestamp>/
```

The important files are:

- `summary.md`: human-readable pass/fail table.
- `summary.json`: machine-readable metrics.
- `<case>/stdout.json`: raw CLI result for the case.
- `<case>/stderr.log`: CLI stderr for debugging.
- `<case>/data/`: isolated Crab data directory with sessions and context-debug snapshots.

## Extended Suite

Browser and desktop-control tasks depend on local runtime state, so they are not part of the
default suite.

```bash
python3 scripts/agent-loop-eval.py \
  --suite extended \
  --base-url http://localhost:50930/v1 \
  --api-mode responses \
  --main-model gpt-5.5 \
  --small-model gpt-5.4-mini
```

Extended cases include:

- `computer_use_status`: asks the model to call the read-only `computer_use` status action.
- `delegated_worker`: asks the main model to call `delegate_to_worker` and verifies that the
  child worker session uses the configured small model.
- `browser_local_page`: asks the model to navigate to `https://example.com` and inspect the page.

These cases are useful for smoke testing, but a failure can indicate missing local permissions,
network conditions, or browser runtime setup rather than a loop regression.

## Preview Without Spending Tokens

Use `--preview-only` to prepare the prompt and routing decision without executing model calls:

```bash
python3 scripts/agent-loop-eval.py --preview-only
```

This is useful when tuning prompt budgets, tool allowlists, or smart-routing thresholds.

## What To Watch

`summary.md` includes:

- routed model;
- effective tool count;
- actual tool calls recorded in the session;
- projected prompt tokens;
- provider token usage when available;
- pass/fail notes.

For the desired "main model thinks, small model executes" profile, the key signal is:

```text
direct_simple -> routed_model=gpt-5.4-mini, effective_tool_definition_count=0
workspace_tool/code_navigation -> routed_model=gpt-5.5, tool_call_count>=1
delegated_worker -> parent routed_model=gpt-5.5, child worker model=gpt-5.4-mini
```

That means the control loop is conserving the main model for judgment-heavy work while allowing
cheap, bounded turns to complete without carrying the full tool schema payload.
