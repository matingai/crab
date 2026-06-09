# Crab FAQ

## Do I need an API key to try Crab?

Not for the first smoke test. Use `debug-context` to inspect the prompt, runtime profile,
skills, memory snapshot, goal-state digest, and tool definitions that Crab would send to a
model:

```bash
cargo run -- debug-context --prompt "Explain how Crab tracks goals and delegates work."
```

You need a model provider only when you want Crab to execute a real model-backed response,
for example `cargo run -- chat --prompt "..."` or `debug-context --execute`.

## Can I use Cockpit, NewAPI, a local gateway, or a local model?

Yes, if the gateway exposes an OpenAI-compatible API. Configure the endpoint and model with
environment variables:

```bash
export OPENAI_API_KEY="your-gateway-key"
export OPENAI_BASE_URL="https://your-gateway.example.com/v1"
export HERMES_RS_MODEL="your-routed-model"
```

Local gateways can use a loopback URL such as `http://127.0.0.1:11434/v1` when they provide
compatible Chat Completions or Responses-style behavior.

## What is the main idea behind Crab?

Crab treats the agent loop as the product. The main model behaves like a goal-solving
controller: it tracks objectives, blockers, evidence, risks, confidence, and next actions.
Tools and worker runs produce bounded observations, and the main loop folds those
observations back into local state.

## How is Crab different from a normal chat wrapper?

A normal chat wrapper usually forwards user text to a model and renders the answer. Crab
keeps runtime state around the conversation: sessions, goal state, memory, skills, todos,
approval requests, tool observations, delegated worker runs, and bridge events. The desktop
shell can show the timeline instead of hiding execution behind a single final message.

## What does worker delegation mean?

Worker delegation means the main loop can hand a bounded subtask to another run or
auxiliary model, such as "review the docs for privacy claims" or "inspect this file area
and return evidence." The main model still owns orchestration and final synthesis; workers
produce focused findings.

## Is Crab production-ready?

No. Crab is an active 0.1.x prototype. The project is suitable for experimentation,
architecture review, and local workflow trials. Public APIs, event formats, desktop
behavior, and local data layout can still change.

## Is Crab safe to run on private workspaces?

Treat it as an experimental local automation tool. The terminal tool is disabled by
default, and sensitive actions can be approval-gated, but the project is not formally
audited. Use trusted workspaces, review model outputs, avoid committing `.hermes-agent-rs/`
or `.env`, and keep shell disabled unless you intentionally need it.

## Where does Crab store local state?

The current compatibility data directory is:

```text
<workspace>/.hermes-agent-rs
```

It can contain sessions, archives, memory, runtime data, provider configuration, and model
outputs. The directory is ignored by Git and may be renamed in a future breaking release.

## Why does the binary use `crab` but some variables still say `HERMES_RS_*`?

The project has been renamed to Crab, but some environment variables and compatibility
paths still keep the older `HERMES_RS_*` prefix to avoid unnecessary breakage during the
0.1.x line. A future breaking release can migrate these names more cleanly.

## Which desktop shell should I use?

The current practical shell is Next.js plus Electron. Tauri scaffolding is present for
native integration work, but Electron is the more complete path today.

## How should I give feedback?

Use the `Agent loop feedback` issue template for design feedback around goal state,
delegation, tool evidence, approval boundaries, and local state. Use bug reports for
reproducible failures and feature requests for new capabilities.

## What is the best first demo?

Start with the no-key context preview:

```bash
cargo run -- debug-context --prompt "Explain Crab's agent loop and worker delegation design."
```

Then run the desktop renderer:

```bash
cd desktop-shell
npm install
npm run dev
```

Open `http://localhost:1420` and inspect the first-run Agent Loop demo state.
