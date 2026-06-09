# Installing Crab

Crab is early, so source installation is the most reliable path today. Prebuilt binaries
are not published yet.

## Requirements

- Rust 1.85 or newer.
- Git.
- A model provider only for model-backed runs. The no-key smoke test works without one.
- Node.js and npm only if you want to run the desktop shell.

## Install From GitHub

Install the CLI directly from the public repository:

```bash
cargo install --git https://github.com/matingai/crab.git --locked
```

The first source build can take several minutes because Crab includes browser, PDF, Office,
and local runtime dependencies.

Verify the binary:

```bash
crab --help
crab debug-context --prompt "Explain how Crab tracks goals and delegates work."
```

The second command does not call a model. It prints the context Crab would send to a model.

## Install From A Local Checkout

For development or local patching:

```bash
git clone https://github.com/matingai/crab.git
cd crab
cargo install --path . --locked
```

Like the GitHub install path, the first local release build can take several minutes.

You can also run without installing:

```bash
cargo run -- debug-context --prompt "Explain the runtime architecture."
cargo run -- chat
```

## Configure A Model Provider

Crab accepts OpenAI-compatible endpoints:

```bash
export OPENAI_API_KEY="your-api-key"
export OPENAI_BASE_URL="https://api.openai.com/v1"
export HERMES_RS_MODEL="gpt-4.1-mini"
```

For Cockpit, NewAPI, or a local gateway, point `OPENAI_BASE_URL` at the gateway's
OpenAI-compatible `/v1` endpoint and set `HERMES_RS_MODEL` to the routed model name.

## Run The Desktop Shell

The desktop shell is not distributed as a packaged app yet. Run it from source:

```bash
cd desktop-shell
npm install
npm run electron:dev
```

For renderer-only preview:

```bash
cd desktop-shell
npm run dev
```

Open `http://localhost:1420`.

## Local State

Crab currently stores local runtime state in:

```text
<workspace>/.hermes-agent-rs
```

This directory is ignored by Git. It can contain sessions, memory, archives, provider
configuration, and model outputs. Do not commit it.

## Troubleshooting

- If `cargo install` fails, check `rustc --version` and upgrade to Rust 1.85 or newer.
- If model calls fail, confirm `OPENAI_API_KEY`, `OPENAI_BASE_URL`, and `HERMES_RS_MODEL`.
- If the desktop shell fails to start, run `npm install` inside `desktop-shell/` and check
  that Node.js is available.
- Keep the terminal tool disabled unless you intentionally need it. Use `--enable-shell`
  only in trusted workspaces.
