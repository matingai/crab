# Crab Quickstart

This guide is the shortest path from a fresh checkout or release install to a useful
Crab demo. It avoids model calls until the final optional step.

## 1. Install Or Build

Install the latest macOS/Linux CLI release:

```bash
curl -fsSL https://raw.githubusercontent.com/matingai/crab/main/scripts/install.sh | bash
```

Or run from source:

```bash
git clone https://github.com/matingai/crab.git
cd crab
cargo run -- doctor
```

## 2. Run The Local Doctor

```bash
crab doctor
```

The doctor checks the local workspace, runtime state, model endpoint, shell safety,
release scripts, `.gitignore` hygiene, and optional developer tools. It does not call a
model and it never prints API key values.

For automation or issue reports:

```bash
crab doctor --json
```

## 3. Inspect The Agent Loop Without A Key

```bash
crab debug-context --prompt "Explain how Crab tracks goals and delegates work."
```

This command prints the context Crab would send to a model: system prompt, workspace
instructions, goal-state digest, memory snapshot, runtime profile, and tool definitions.
It is the safest smoke test for talks, screenshots, and first-time users.

## 4. Configure A Model Provider

Crab speaks to OpenAI-compatible endpoints:

```bash
export OPENAI_API_KEY="your-api-key"
export OPENAI_BASE_URL="https://api.openai.com/v1"
export HERMES_RS_MODEL="gpt-4.1-mini"
```

For Cockpit, NewAPI, or another gateway, keep the same variables and point
`OPENAI_BASE_URL` at the gateway's `/v1` endpoint.

Check again:

```bash
crab doctor
```

The model-key warning should disappear once the key is available in the environment or
ignored local config.

## 5. Run A First Prompt

```bash
crab chat --prompt "Read README.md and summarize Crab's agent-loop design in five bullets."
```

To enable shell execution for trusted coding workspaces:

```bash
crab --enable-shell chat --prompt "Inspect the repository and propose one safe improvement."
```

Keep shell access disabled for untrusted directories.

## 6. Open The Desktop Preview

From a source checkout:

```bash
cd desktop-shell
npm install
npm run dev
```

Open `http://localhost:1420` and use the first-run demo state to show the timeline,
runtime settings, skills, and agent-loop story before connecting a real workspace.

## 7. Demo Path

For a clean public demo:

1. Run `crab doctor`.
2. Run `crab debug-context --prompt "Explain Crab's agent loop and worker delegation."`.
3. Open the desktop preview.
4. Run one workflow from [examples](../examples/README.md) with demo credentials.
5. Link viewers to [Agent Loop](AGENT_LOOP.md), [Architecture](ARCHITECTURE.md), and
   [Future Vision](FUTURE_VISION.md).
