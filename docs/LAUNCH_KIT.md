# Crab Launch Kit

This document is a practical promotion kit for Crab. It keeps the public message sharp,
repeatable, and honest.

For the operational launch sequence, channel-specific tactics, social copy bank, metrics,
and objection handling, use [Promotion Playbook](PROMOTION_PLAYBOOK.md).

## One-Line Positioning

Crab is a Rust-native local agent runtime built around a goal-state agent loop, worker
delegation, governed tools, and a local-first desktop shell.

Chinese:

Crab（螃蟹）是一个 Rust 原生本地 Agent Runtime，核心是目标状态驱动的 Agent Loop、Worker
委派、受控工具注册表和本地优先桌面壳。

## What To Emphasize

- **Agent loop as the product**: Crab is not just a chat wrapper with tools bolted on.
- **Goal-state reasoning**: the main agent tracks objective, blockers, evidence, risk,
  next actions, and confidence.
- **Worker delegation**: bounded subtasks can be delegated to workers or auxiliary models
  while the main model stays responsible for orchestration.
- **Local-first runtime**: sessions, tools, memory, skills, traces, and desktop events live
  around the user's workspace.
- **Rust-native core**: the runtime, tool loop, model client, sessions, and bridge are
  implemented in Rust.
- **Inspectable execution**: the desktop shell can show the timeline of model output,
  tools, approvals, delegation, and completion.

## Claims To Avoid

Avoid these until the project has stronger evidence:

- production-ready;
- enterprise-grade;
- formally audited;
- benchmark-leading;
- secure by default for arbitrary untrusted workspaces;
- fully stable API;
- a replacement for every coding assistant.

Better wording:

- active 0.1.x prototype;
- experimental local agent runtime;
- designed for inspectable long-running agent work;
- useful for coding, research, browser, PDF, and Office-style workflows.

## Demo Checklist

Before recording or posting:

```bash
cargo fmt --check
cargo test --locked
cd desktop-shell
npm run build
```

For a no-key smoke demo:

```bash
cargo run -- debug-context --prompt "Explain Crab's agent loop and worker delegation design."
```

For the desktop renderer:

```bash
cd desktop-shell
npm run dev
```

Open `http://localhost:1420`.

## 60-Second Demo Script

1. Open the GitHub README and point at the banner, badges, architecture diagram, and
   screenshots.
2. Say: "Crab is not a chat wrapper. It is a Rust-native local agent runtime."
3. Show the agent loop diagram or docs and explain goal-state tracking.
4. Show the desktop timeline UI at `localhost:1420`.
5. Run a no-key context preview:

   ```bash
   cargo run -- debug-context --prompt "Explain the runtime architecture."
   ```

6. If model credentials are available, run one workflow from `examples/`.
7. End with the strongest callout: "The main model orchestrates; tools and workers produce
   evidence; the runtime keeps state local and inspectable."

## Longer Video Outline

Use this for a 5-8 minute video:

1. Problem: chat wrappers do not handle long-running work well.
2. Crab thesis: agent loop as a runtime, not a UI trick.
3. Architecture: CLI, desktop shell, bridge, Rust core, tools, local state, models.
4. Goal state: objective, blockers, evidence, confidence, next actions.
5. Delegation: workers handle bounded subtasks.
6. Demo: context preview, desktop shell, one model-backed workflow.
7. Safety: local state, approval boundaries, shell disabled by default.
8. Roadmap: better worker schemas, timeline UI, plugins/MCP, packaged desktop.

## English Launch Post

```text
I am open-sourcing Crab, an experimental Rust-native local agent runtime.

Unlike a simple chat wrapper, Crab is built around a goal-state agent loop: the main model
tracks objectives, blockers, evidence, risks, and next actions, while bounded subtasks can
be delegated to worker runs or auxiliary models.

It includes a governed local tool registry, memory/skills, context compression, request
recovery, browser/PDF/Office workflows, a desktop event stream, and a Next.js + Electron
desktop shell.

Repo: https://github.com/matingai/crab
```

## Chinese Launch Post

```text
开源了一个实验性项目 Crab（螃蟹）：Rust 原生本地 Agent Runtime。

它不是普通 chat wrapper，核心是一个目标状态驱动的 agent loop：主模型负责追踪目标、阻塞点、
证据、风险和下一步，边界清晰的研究/验证/实现任务可以委派给 worker 或辅助模型。

目前包含本地工具注册表、memory/skills、上下文压缩、请求恢复、浏览器/PDF/Office 工作流、
桌面事件流，以及 Next.js + Electron 桌面壳。

GitHub：https://github.com/matingai/crab
```

## Hacker News Title Ideas

- Show HN: Crab, a Rust-native local agent runtime with goal-state loops
- Show HN: I built an experimental local agent runtime in Rust
- Show HN: Crab - local-first agent loop, worker delegation, and desktop timeline

## Reddit / X Thread Outline

1. "Most agent prototypes start as chat + tools. Crab starts with the runtime loop."
2. Explain goal state.
3. Explain worker delegation.
4. Explain local-first state and governed tools.
5. Show screenshots.
6. Link docs: `docs/AGENT_LOOP.md`, `docs/ARCHITECTURE.md`, `examples/`.
7. Ask for feedback on agent-loop design, local tool boundaries, and worker handoff.

## Chinese Article Outline

Title:

```text
不只是聊天壳：我用 Rust 做了一个本地 Agent Runtime
```

Structure:

1. 为什么 chat loop 不够。
2. Crab 的核心设计：目标状态驱动的 agent loop。
3. 主模型为什么应该做调度者。
4. Worker/子模型适合处理什么。
5. 本地工具、memory、skills 和桌面事件流。
6. 当前项目状态和限制。
7. 未来路线。

## Launch Channels

Recommended order:

1. GitHub release and README polish.
2. Chinese technical article: V2EX, 掘金, 知乎, 开源中国.
3. Short demo video or GIF.
4. X/Twitter thread.
5. Reddit: `r/rust`, `r/LocalLLaMA`, `r/opensource`, `r/ChatGPTCoding`.
6. Hacker News Show HN.
7. Product Hunt only after the desktop demo is smoother.

## Assets To Keep Updated

- README screenshots.
- `docs/assets/crab-banner.svg`.
- `examples/`.
- `ROADMAP.md`.
- `CHANGELOG.md`.
- `docs/AGENT_LOOP.md`.

## Release-Day Checklist

- `cargo test --locked` is green.
- `desktop-shell npm run build` is green.
- README screenshots match the current UI.
- Star History is visible near the license section.
- GitHub topics are set from `docs/BADGES_AND_TOPICS.md`.
- One release tag exists.
- Demo post links to the repo, docs, examples, and screenshots.
