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
therefore treats tool results as observations that can be summarized and routed
into the right state:

- Conversation history for user-visible continuity.
- Goal state for active reasoning.
- Solve trace for decision history.
- Memory for reusable facts.
- Archive records for later search.
- UI events for live progress.

The point is to preserve meaning, not just bytes.

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

- Model request and streaming output.
- Tool call start and completion.
- Approval requests.
- Delegated run lifecycle.
- Context pressure warnings.
- Runtime status.
- Final completion.

This turns the agent from a black box into something closer to an execution timeline.

## The Design Bet

The project bets that useful local agents will be less like chatbots and more like small
operating systems for goals. The main loop holds the goal, tools provide grounded actions,
workers expand capacity, memory preserves learning, and the desktop shell makes the
process visible.

That is the heart of Crab.
