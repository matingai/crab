# Crab Promotion Playbook

This playbook turns Crab's launch materials into a repeatable promotion plan. It is
written for an early open-source project: ambitious enough to attract attention, but honest
about the current prototype stage.

## Launch Goal

The first public push should not try to convince everyone that Crab is the final answer to
AI agents. It should make one idea memorable:

> Crab treats the agent loop as a runtime. The main model tracks the goal, tools produce
> evidence, and worker models handle bounded subtasks.

Useful first-wave outcomes:

- developers understand the project thesis in under one minute;
- Rust and agent-runtime builders know where to inspect the architecture;
- early users can run a no-key demo and a model-backed workflow;
- feedback arrives as issues, discussions, or concrete architecture questions;
- repository visitors see screenshots, tests, license, docs, and contribution paths.

## Audience Map

| Audience | Hook | Proof to show | Best CTA |
| --- | --- | --- | --- |
| Rust developers | Rust-native agent runtime instead of script glue | `src/agent.rs`, tool registry, test suite | Read the architecture and try `debug-context` |
| Agent builders | Goal-state loop and worker delegation | `docs/AGENT_LOOP.md`, desktop timeline | Challenge the loop design |
| Local-first users | Workspace-local sessions, memory, approvals, tools | README safety notes, release process | Try it in a disposable workspace |
| Coding-agent users | Long-running task state and retry recovery | examples, session timeline, tests | Run a repo-audit prompt |
| Research/document users | Browser, PDF, Office, Slidev workflows | examples and screenshots | Try a document workflow |
| Open-source maintainers | Clear docs, templates, hygiene, MIT license | `.github/`, `SECURITY.md`, `OPEN_SOURCE_REVIEW.md` | File issues for rough edges |

## Message Ladder

Use this order when writing posts, talks, or release notes.

1. **Problem**: many agent prototypes are chat UIs with tools attached.
2. **Thesis**: Crab makes the agent loop the product surface.
3. **Mechanism**: the main model owns goal state; tools and workers produce evidence.
4. **Runtime**: Rust core, local state, governed tool registry, bridge events, desktop shell.
5. **Proof**: screenshots, `cargo test --locked`, `debug-context`, examples, docs.
6. **Ask**: feedback on agent loop design, delegation boundaries, and local tool safety.

## Asset Checklist

Keep these assets fresh before a public push:

- README banner and badges;
- four desktop screenshots in `docs/assets/screenshots/`;
- one 30-60 second screen recording or GIF;
- `docs/AGENT_LOOP.md` and `docs/ARCHITECTURE.md`;
- one no-key CLI demo command;
- one model-backed demo prompt;
- FAQ entries for setup, model gateways, safety, and project status;
- release notes with limitations;
- GitHub topics from `docs/BADGES_AND_TOPICS.md`;
- a pinned issue asking for feedback on the agent-loop design.

## Two-Week Launch Sequence

### Days -7 To -5: Foundation

- Run `cargo test --locked` and `cd desktop-shell && npm run build`.
- Refresh screenshots with `node scripts/capture-screenshots.mjs` if the desktop UI changed.
- Confirm README links, license, security policy, issue templates, and contribution docs.
- Create a draft GitHub release with one paragraph on the agent-loop thesis.
- Prepare a short video using `docs/DEMO_SCRIPT.md`.

### Days -4 To -2: Soft Launch

- Share with a small technical circle first.
- Ask for feedback on clarity, install friction, and whether the agent-loop idea lands.
- Fix obvious README or setup confusion immediately.
- Keep a short changelog entry for every polish commit.

### Day 0: Public Launch

- Publish the GitHub release.
- Post one English thread and one Chinese post.
- Submit to technical communities with a concrete artifact, not a vague announcement.
- Stay available for comments during the first few hours.
- Turn repeated questions into README or docs updates the same day.

### Days 1 To 7: Follow-Through

- Reply to issues quickly, especially setup and safety questions.
- Add a FAQ section if the same objection appears twice.
- Publish a technical deep dive on the agent loop.
- Cut a patch release if early users find install or demo blockers.
- Summarize lessons in a maintainer note instead of letting the launch disappear.

## Channel Playbooks

### GitHub Release

Use a release title like:

```text
Crab v0.1.0: Rust-native local agent runtime with goal-state loops
```

Release body shape:

- what Crab is;
- why the agent loop matters;
- what is already implemented;
- how to try it;
- known limitations;
- where feedback is wanted.

### X / Twitter

Use a tight thread. The first post should carry the whole idea:

```text
I am open-sourcing Crab, a Rust-native local agent runtime.

The design bet: the main model should track goals and evidence, while tools and worker
models handle bounded execution.

Repo: https://github.com/matingai/crab
```

Follow with:

- screenshot of the desktop timeline;
- diagram or README architecture section;
- short explanation of goal state;
- worker delegation example;
- no-key demo command;
- request for feedback.

### Hacker News

Best title:

```text
Show HN: Crab, a Rust-native local agent runtime with goal-state loops
```

Submission notes:

- link directly to the GitHub repository;
- make sure people can try something without signing up;
- be present in the thread;
- do not ask for upvotes;
- explain tradeoffs plainly when challenged.

### Reddit

Use community-specific angles:

- `r/rust`: Rust runtime, test coverage, tool boundaries.
- `r/LocalLLaMA`: OpenAI-compatible endpoints, local gateways, local-first state.
- `r/opensource`: MIT license, contribution paths, roadmap.
- `r/ChatGPTCoding`: coding-agent workflows, repo audits, task delegation.

Post style:

- lead with what was built;
- include one screenshot;
- list current limitations;
- ask one focused question at the end.

### Chinese Technical Communities

Suggested title:

```text
不只是聊天壳：我用 Rust 做了一个本地 Agent Runtime
```

Strong structure:

1. 为什么普通 chat loop 不够。
2. Crab 的目标状态 agent loop。
3. 主模型为什么应该做调度者。
4. 子模型和工具如何处理边界清晰的任务。
5. 本地状态、审批、桌面时间线和示例。
6. 当前限制和希望大家反馈的问题。

Good places to adapt this article:

- V2EX;
- 掘金;
- 知乎;
- 开源中国;
- B 站动态 or short demo video.

### Product Hunt

Do this later, after the desktop demo is smoother and packaged binaries are easier to try.

Prepare:

- logo or square icon;
- 30-60 second video;
- tagline under one sentence;
- three to five screenshots;
- maker comment explaining the problem and current prototype status;
- FAQ for setup, model providers, local data, and security.

## Copy Bank

### Short Taglines

- Rust-native local agent runtime.
- Agent loop as a runtime, not a chat wrapper.
- Goal-state control for local AI agents.
- Main model orchestrates; tools and workers produce evidence.
- Local-first workspace agent with an inspectable timeline.

### English Short Post

```text
Crab is an experimental Rust-native local agent runtime.

The core idea is a goal-state agent loop: the main model tracks objectives, blockers,
evidence, risks, and next actions, while tools and worker models handle bounded subtasks.

It includes local session state, governed tools, memory/skills, context compression,
browser/PDF/Office workflows, and a desktop timeline.

https://github.com/matingai/crab
```

### Chinese Short Post

```text
Crab（螃蟹）是一个实验性的 Rust 原生本地 Agent Runtime。

核心思路是目标状态驱动的 Agent Loop：主模型负责追踪目标、阻塞点、证据、风险和下一步，
工具与子模型负责边界清晰的执行任务。

目前包含本地 session、受控工具注册表、memory/skills、上下文压缩、浏览器/PDF/Office
工作流，以及可观察的桌面时间线。

https://github.com/matingai/crab
```

### Demo Prompt

```text
Inspect README.md and docs/AGENT_LOOP.md. Summarize Crab's strongest positioning angles,
risky claims to avoid, and one demo workflow.
```

### Feedback Request

```text
I am especially looking for feedback on the agent-loop design: what should stay in the
main model's goal state, what should be delegated to workers, and where local tool
approval boundaries should be stricter.
```

## Objection Handling

| Objection | Useful answer |
| --- | --- |
| Is this production-ready? | No. It is an active 0.1.x prototype with tests, docs, and clear limitations. |
| Why Rust? | The runtime needs durable state, tool boundaries, event streaming, and embeddable APIs. Rust is a good fit for that core. |
| Why not just use an existing coding assistant? | Crab is exploring the runtime loop itself: goal state, evidence, delegation, approvals, and local state. |
| Can it run local models? | It targets OpenAI-compatible endpoints, so local gateways can work when they expose compatible APIs. |
| Is it safe? | It is local-first and approval-aware, but it is not formally audited. Use trusted workspaces and keep shell disabled unless needed. |
| What is the most interesting part? | The main model is treated as a goal-solving controller instead of the worker for every subtask. |

## Metrics That Matter

Track more than stars:

- GitHub unique visitors and clones;
- README click-through to docs and examples;
- issues opened by real users;
- discussions or comments about architecture;
- failed setup reports;
- accepted external PRs;
- repeat questions that indicate unclear docs;
- demo completion from README command to first model-backed run.

## Post-Launch Maintenance

- Keep a `good first issue` list small and real.
- Convert good feedback into roadmap items within 48 hours.
- Prefer patch releases over silent fixes when launch users hit setup bugs.
- Keep screenshots current with the actual UI.
- Avoid overstating security, stability, or benchmark claims.
- Publish a short technical note whenever the agent loop or delegation model improves.

## Practical Next Steps

1. Record the 60-second demo from `docs/DEMO_SCRIPT.md`.
2. Add GitHub topics from `docs/BADGES_AND_TOPICS.md`.
3. Publish a `v0.1.0` GitHub release.
4. Post the English and Chinese short launch posts.
5. Collect the first ten pieces of feedback into issues or roadmap notes.
