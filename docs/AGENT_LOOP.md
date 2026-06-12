# Agent Loop

The agent loop is the most important part of Crab. It is the reason the project
is more than a chat UI with tools.

The main agent is designed as a goal-solving control model. It tracks the active objective,
maintains working state, chooses the next action, delegates bounded work when useful, and
integrates the results back into the goal.

## Loop Responsibilities

At a high level, each turn can involve:

1. Loading the current session and workspace state.
2. Recalling relevant memory and skills.
3. Rendering project context, goal state, todos, runtime profile, and recent history.
4. Calling the model with a compact but meaningful prompt.
5. Executing tool calls through the registry.
6. Summarizing and classifying tool observations.
7. Updating goal state, solve trace, todos, memory, archive records, and delegated runs.
8. Handling approval pauses, retries, and context compression when necessary.
9. Emitting structured events throughout the process.
10. Producing a final response that reflects what changed and what remains.

## Project Instructions

Crab treats project instructions as part of the control loop, not as passive README text.
At session start it loads root-level context such as `AGENTS.md`, `CLAUDE.md`,
`.hermes.md`, `.cursorrules`, or `.cursor/rules/*.mdc`.

When a tool touches a nested path, Crab lazily checks that path's ancestor directories for
additional instruction files. Newly discovered files are returned to the model as a
root-to-leaf instruction stack, with later entries marked as more specific. This mirrors
the way coding agents need to honor broad repository conventions while still adapting to
module-level rules.

Instruction content is scanned for obvious prompt-injection patterns and invisible control
characters before it is loaded. Suspicious files are reported as blocked context instead
of being silently trusted.

## Goal State As Working Memory

The goal state is the loop's durable working memory. It is not a transcript summary. It is
a structured representation of what the agent believes it is trying to accomplish.

It can track:

- The current focus goal.
- Subgoals and their status.
- Blockers and uncertainty.
- Evidence that supports or conflicts with a belief.
- Risks and assumptions.
- Hot data from recent tool outputs.
- Confidence levels and next actions.

This lets the agent ask better questions: What is still unresolved? Which fact was
actually verified? Is this worker result relevant to the current goal? Should the next
action be implementation, verification, research, or user clarification?

## Main Agent As Orchestrator

The primary model should spend its attention on judgment:

- Frame the task.
- Decide which context matters.
- Choose the next action.
- Decide what can be delegated.
- Interpret tool results.
- Maintain approval boundaries.
- Integrate evidence.
- Explain the result to the user.

This is different from asking the main model to do every low-level activity itself. The
main loop is strongest when it keeps the goal coherent and uses tools or workers for
bounded execution.

Approval boundaries apply across equivalent execution surfaces. A destructive shell
fragment should pause whether it arrives through the terminal tool or through a local
script runner such as `execute_code`. Terminal commands also carry an explicit bounded
timeout, so short trusted commands can fail fast instead of occupying the loop for the
default command window.

The registry also supports local `tool_policy` preflight rules. Common sensitive paths
such as `.env*`, `.ssh/*`, `.aws/*`, private keys, and credential config files are
protected by default before a tool implementation runs. The preflight recursively inspects
path-like arguments in nested objects and arrays, so richer tools and plugin calls inherit
the same guardrail. A workspace can extend those rules, require approval for entire tool
families such as `browser_*`, opt out of the defaults, or disable a tool/path before it
reaches its implementation.

Approval requests are also treated as durable context. Display fields such as the command
and reason are redacted before persistence, while the runtime uses a stable command hash
to match approved requests back to the original execution intent. That keeps approval
resume deterministic without making sensitive command text part of the user-visible record.

Direct web-fetch tools also pass through `network_policy`. By default, loopback, private,
link-local, and metadata-style hosts are denied before the runtime issues an HTTP request.
Trusted workspaces can opt into selected local hosts or private network fetches through
local config.

## Delegation Model

Delegation is not a gimmick. It is a way to protect the main loop's attention.

Good delegated tasks are:

- Bounded.
- Verifiable.
- Clear about expected output.
- Independent enough to run without constant coordination.
- Useful to the main goal when read back.

Examples:

- Explore where a feature is implemented.
- Verify whether a test failure is related to a recent change.
- Draft a document section from known context.
- Inspect a browser workflow and report observed states.
- Compare two implementation strategies.

The main agent remains accountable for integration. Worker output should become evidence,
todo updates, solve trace entries, or goal-state changes, not just another long message in
the transcript.

## Tool Results As Evidence

Raw tool output can be too large, too noisy, or too local to be useful forever. Crab
therefore redacts common credential patterns and treats tool results as observations that
can be summarized and routed into the right state:

- Conversation history for user-visible continuity.
- Goal state for active reasoning.
- Solve trace for decision history.
- Memory for reusable facts.
- Archive records for later search.
- UI events for live progress.

Tool completion is classified before it enters those surfaces. `tool_error:` responses,
approval denials, timeouts, cancellations, and non-zero shell exit codes are recorded as
`error` rather than ordinary `done` observations. Parallel batches can still complete as a
batch while reporting `completed_with_errors`, which lets the main loop continue with
clear evidence about which tool calls need repair.

Completion events also include elapsed duration for each tool call and each parallel
batch. Failed tool calls carry a stable `error_kind` such as `invalid_json_arguments`,
`invalid_arguments`, `tool_policy_denied`, `approval_denied`, `process_exit`, or
`timeout`, so a UI and the next model turn can distinguish malformed arguments from
environment, policy, and execution failures. That timing and error metadata is shown in
the desktop timeline, giving users a concrete feel for which parts of the loop are doing
work, waiting on tools, repairable by the model, or worth optimizing.

The point is to preserve meaning, not just bytes.

## Learning Context

Crab can feed distilled experience back into later turns. After a solve episode ends, the
loop derives compact experience records and rebuilds aggregated meta-patterns. When a new
request matches those records, the prompt can receive small `<experience-context>` and
`<meta-pattern-context>` blocks with signals, recommended strategies, failure patterns,
and model-refined strategy templates.

This learning context is enabled by default for CLI and desktop runs when matching records
exist. It can be disabled with `HERMES_RS_DISABLE_LEARNING_CONTEXT=1`, or controlled more
granularly with `HERMES_RS_DISABLE_EXPERIENCE_CONTEXT=1` and
`HERMES_RS_DISABLE_META_PATTERN_CONTEXT=1`. The older
`HERMES_RS_ENABLE_EXPERIENCE_CONTEXT` and `HERMES_RS_ENABLE_META_PATTERN_CONTEXT` flags
still force those blocks on when a workspace wants explicit opt-in behavior.

The intent is Codex-like reuse without turning memory into a bag of stale instructions:
learning blocks are clipped, optional, redacted in event previews, and framed as
heuristics that should be ignored when the current evidence does not fit.

## Context Pressure And Recovery

Long-running agent sessions eventually hit context pressure. The loop includes recovery
paths for this:

- Estimate prompt size before model calls.
- Compress old history when needed.
- Preserve critical context and recent turns.
- Detect provider context-limit errors.
- Retry with adjusted prompt shape.
- Reduce output budget when the provider reports insufficient output space.

This is part of treating the agent as a runtime. Context is a managed resource.

## Event Stream

The loop emits events because agent work should be inspectable while it happens. A desktop
UI can show:

- Turn start and resume boundaries, including turn id and redacted user-input preview.
- Model request and streaming output.
- Main and background model request mode, continuation/budget metadata, completion status,
  duration, and provider token usage when available.
- Model recovery attempts for output-budget reduction, transient retry backoff, and
  context-overflow compression, without exposing full provider errors.
- Prompt context preparation budgets, retained blocks, trimming labels, and preparation
  duration without exposing raw prompt content.
- Context-source update events for optional memory, goal-state, todo, solve-trace, and
  plugin blocks, with kept/clipped/skipped status, compact redacted previews, and per-block
  character counts.
- Context compaction events with before/after message counts, estimated token counts,
  summary usage, and pruned-tool-output counts.
- Goal-state update events for user-seeded goals, tool evidence, tool-result reconcile,
  and turn-end reconcile, with summary metrics instead of raw working memory.
- Todo-state update events for goal-state sync, explicit todo-tool writes, and delegated
  worker step updates, with counts and redacted active-item previews.
- Solve-trace update events for episode starts, tool/delegation steps, decisions, and
  turn outcomes, with compact redacted previews and trace counters.
- Learning-state update events for experience distillation, meta-pattern rebuilds, and
  model-assisted pattern summaries, with counts and redacted previews instead of full
  memory records.
- Turn interruption events for user-requested stops, including the turn id, runtime phase,
  reason, and redacted display message.
- Tool call start and completion.
- Approval request and resolution boundaries, including whether a paused tool was
  approved or denied before execution resumes.
- Session checkpoint events after persistence, including turn id, history/timeline counts,
  pending approval counts, response-continuation availability, and a compact path preview.
- Delegated run lifecycle updates, including worker run ids, child session ids, attempt
  numbers, status, objective previews, and compact result previews.
- Context pressure warnings.
- Runtime status.
- Turn-level completion boundaries with status, duration, and tool-call counts.
- Final completion.

This turns the agent from a black box into something closer to an execution timeline.

## The Design Bet

The project bets that useful local agents will be less like chatbots and more like small
operating systems for goals. The main loop holds the goal, tools provide grounded actions,
workers expand capacity, memory preserves learning, and the desktop shell makes the
process visible.

That is the heart of Crab.
