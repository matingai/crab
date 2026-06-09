# Crab Demo Script

Use this when recording a short video, doing a live stream, or preparing a conference-style
walkthrough.

## Before Recording

```bash
cargo test --locked
cd desktop-shell
npm run build
```

Use a clean demo workspace and demo credentials only. Do not show private workspaces,
provider keys, cookies, or local documents.

## Scene 1: The Thesis

Show the README.

Say:

```text
Crab is a Rust-native local agent runtime. The key idea is that the agent loop is the
product: the main model tracks goals and evidence, tools produce observations, and worker
runs can handle bounded subtasks.
```

Point to:

- badges;
- architecture diagram;
- screenshots;
- agent loop section.

## Scene 2: No-Key Context Preview

Run:

```bash
cargo run -- debug-context --prompt "Explain how Crab tracks goals and delegates work."
```

Narrate:

- project context is assembled before model execution;
- skills, memory, todos, runtime profile, and goal state can become prompt inputs;
- this command is safe for demos because it does not require a model request unless
  `--execute` is passed.

## Scene 3: Desktop Shell

Run:

```bash
cd desktop-shell
npm run dev
```

Open `http://localhost:1420`.

Show:

- sidebar navigation;
- workspace area;
- timeline panel;
- model selector;
- settings surface.

Say:

```text
The desktop shell is not the runtime. It is one surface over the Rust core and bridge
event stream.
```

## Scene 4: Model-Backed Workflow

When model credentials are available:

```bash
cargo run -- chat --prompt "Inspect README.md and docs/AGENT_LOOP.md. Summarize Crab's strongest positioning angles and list claims we should avoid."
```

If using Cockpit/NewAPI:

```bash
export OPENAI_API_KEY="cockpit-exported-key"
export OPENAI_BASE_URL="https://your-cockpit-gateway.example.com/v1"
export HERMES_RS_MODEL="your-routed-model"
```

Narrate:

- the model is OpenAI-compatible, but the runtime is provider-agnostic at the boundary;
- local tools and state remain part of Crab's runtime loop;
- final answers should cite evidence and avoid unsupported claims.

## Scene 5: Close

End with:

```text
Crab is early, but it is exploring a serious agent-runtime direction: goal-state control,
worker delegation, governed local tools, and inspectable desktop execution.
```

Show:

- `docs/AGENT_LOOP.md`;
- `examples/`;
- `ROADMAP.md`;
- GitHub star button.
