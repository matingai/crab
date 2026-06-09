# Project Overview

Crab is a Rust-native local agent runtime for coding, research, document work,
and desktop-assisted automation. It is inspired by Hermes Agent, but its center of gravity
is different from a direct port: the project focuses on a durable agent loop, local state,
tool governance, and desktop integration.

The project is still early, but the shape is already clear. It wants to be a serious
foundation for local agents that can work across many turns without losing the plot.

## The Problem

Many agent demos are impressive for one turn and fragile across ten turns. They can call a
tool, but they do not always know what the tool result means. They can edit files, but they
do not reliably preserve the user's goal. They can stream text, but the UI cannot inspect
the underlying execution. They can run commands, but everything collapses into a transcript
that grows until the model forgets why it started.

Crab is built around the belief that long-running agent work needs more than a
prompt. It needs a runtime.

## The Project Thesis

Crab treats the main agent as a goal-solving controller:

- It keeps a compact representation of the active goal.
- It tracks blockers, evidence, confidence, risks, and next actions.
- It calls tools through a schema-driven registry.
- It summarizes and classifies observations instead of dumping raw logs into history.
- It delegates bounded work to workers or auxiliary models when the main loop should stay
  focused on judgment and integration.
- It persists local state so a session can continue after a pause, restart, or context
  compression.
- It emits structured events so a desktop shell can show what the agent is actually doing.

This makes the project feel less like a command-line chatbot and more like a local
operating layer for agentic work.

## What Exists Today

The current implementation includes:

- A reusable Rust agent core with CLI entry points.
- OpenAI-compatible model clients for Responses API and Chat Completions style endpoints.
- A tool registry covering files, Git, browser actions, PDF, Office, memory, skills, MCP,
  cron, delegation, and optional terminal execution.
- Session persistence and local runtime data under the current compatibility directory
  `.hermes-agent-rs/`.
- Goal state, todos, solve traces, memory, archive records, and delegated run records.
- Context compression and recovery behavior for long sessions.
- A bridge API that can be used by a desktop UI.
- A Next.js + Electron desktop shell, with Tauri scaffolding kept available.
- Bundled skills for Office and Slidev-oriented workflows.

The system is not polished enough to claim stability, but it is already beyond a toy
agent loop. The interesting work is in how the parts compose.

## What Makes It Worth Opening

Crab is valuable as an open-source project because it is exploring several hard
problems in the open:

- How should a local agent remember goals without overloading the model context?
- How should tool results become durable evidence instead of transcript noise?
- How should desktop UIs represent agent progress, approval, and delegation?
- How should skills become reusable operational knowledge rather than just markdown files?
- How should a main agent decide what to solve itself and what to delegate?
- How can a Rust runtime make agent execution more inspectable, typed, and reliable?

The answers are still evolving, but the project has a strong architecture for asking those
questions seriously.

## Intended Audience

This repository is for:

- Developers interested in local coding agents.
- Builders experimenting with desktop agent shells.
- Rust developers who want a typed agent runtime instead of a Python-only stack.
- Researchers and tool builders interested in memory, skills, delegation, and long-running
  agent workflows.
- Product engineers exploring how agent execution can become visible and controllable in a
  real UI.

## Non-goals For Now

The project is not trying to be a hosted multi-tenant agent platform. It is not trying to
hide all complexity behind a minimal chat box. It is also not claiming production-grade
security isolation yet.

The priority is to make the local agent loop understandable, extensible, and strong enough
to support real work.
