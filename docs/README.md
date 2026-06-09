# Crab Documentation

This directory explains the engineering ideas behind Crab beyond the quick
start material in the root README.

Crab (中文名：螃蟹) is not just a chat wrapper. It is an attempt to build a Rust-native local
agent runtime where the main agent behaves like a goal-solving control model: it tracks
the objective, maintains compact working state, delegates bounded subtasks, integrates
evidence, and exposes the whole execution as a stream of structured events.

## Reading Map

- [Project Overview](PROJECT_OVERVIEW.md): the product and engineering thesis.
- [Architecture](ARCHITECTURE.md): how the runtime, bridge, tools, desktop shell, and local
  state fit together.
- [Agent Loop](AGENT_LOOP.md): the core reasoning loop, goal tracking, tool protocol, and
  worker delegation model.
- [Demo Script](DEMO_SCRIPT.md): a short walkthrough for recordings and live demos.
- [Launch Kit](LAUNCH_KIT.md): positioning, channel plan, public posts, article outlines,
  and launch-day checklist.
- [Promotion Playbook](PROMOTION_PLAYBOOK.md): launch sequence, channel-specific playbooks,
  reusable copy, metrics, and objection handling.
- [Future Vision](FUTURE_VISION.md): where the project can go if the current ideas are
  pushed further.
- [Open-source Privacy Review](OPEN_SOURCE_REVIEW.md): privacy and repository-hygiene notes
  for public release.
- [Badges, Topics, And Repository Packaging](BADGES_AND_TOPICS.md): badges, recommended
  GitHub topics, label themes, and social-preview guidance.
- [Release Process](RELEASE_PROCESS.md): pre-release checklist, versioning notes, and
  release-note shape.
- [Maintainer Guide](MAINTAINER_GUIDE.md): triage priorities, labels, positioning, and
  public hygiene.

## Core Thesis

Most agent prototypes start with a prompt and add tools around it. Crab starts
from the opposite direction: it treats the agent loop as the product.

The model is only one participant in the system. Around it there is a durable workspace
state, a governed tool registry, a memory and skill layer, an approval path, a desktop
event stream, and a delegation surface for worker runs. The goal is to make long-running
agent work inspectable, restartable, and progressively more competent inside a local
workspace.

## Documentation Tone

These documents are intentionally a little ambitious. They describe both what already
exists and the design direction that makes the project worth opening up. Implementation
details will evolve, but the central bet should stay stable: local agents become much more
useful when they can preserve goals, reason over evidence, delegate work, and expose their
execution as a real system rather than a single black-box answer.
