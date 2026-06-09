# Installing Crab

Crab can be installed from source or from GitHub Release CLI archives. Source installation
is still the most reliable path during early 0.1.x development, but release archives avoid
the long first compile.

## Requirements

- Rust 1.85 or newer.
- Git.
- A model provider only for model-backed runs. The no-key smoke test works without one.
- Node.js and npm only if you want to run the desktop shell.
- Swift on `PATH` only if you want to use the current PDF inspection and extraction tools.

## Install From A GitHub Release

Tagged releases can publish these CLI archives:

| Platform | Asset |
| --- | --- |
| macOS Apple Silicon | `crab-vX.Y.Z-aarch64-apple-darwin.tar.gz` |
| macOS Intel | `crab-vX.Y.Z-x86_64-apple-darwin.tar.gz` |
| Linux x64 | `crab-vX.Y.Z-x86_64-unknown-linux-gnu.tar.gz` |
| Windows x64 | `crab-vX.Y.Z-x86_64-pc-windows-msvc.zip` |

### One-command install

For macOS or Linux:

```bash
curl -fsSL https://raw.githubusercontent.com/matingai/crab/main/scripts/install.sh | bash
```

For Windows PowerShell:

```powershell
irm https://raw.githubusercontent.com/matingai/crab/main/scripts/install.ps1 | iex
```

The installer tracks the newest GitHub release, including pre-releases in the 0.1.x line.
Set `CRAB_VERSION` to install a specific release tag, or `CRAB_INSTALL_DIR` to choose the
binary directory.

### Manual install

For macOS or Linux, download the matching asset from
`https://github.com/matingai/crab/releases`, then install the binary:

```bash
VERSION=v0.1.2
TARGET=aarch64-apple-darwin
curl -LO "https://github.com/matingai/crab/releases/download/${VERSION}/crab-${VERSION}-${TARGET}.tar.gz"
tar -xzf "crab-${VERSION}-${TARGET}.tar.gz"
sudo install -m 0755 "crab-${VERSION}-${TARGET}/crab" /usr/local/bin/crab
crab --help
crab --version
```

For Windows, download the `.zip`, expand it, and add the extracted directory to `PATH` or
move `crab.exe` into a directory already on `PATH`.

Each release archive also includes a `.sha256` checksum file.

## Install From GitHub Source

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

## Build A Local Release Archive

To generate an installable archive for the current machine:

```bash
scripts/package-release.sh
```

The script writes `dist/crab-v<version>-<target>.tar.gz` or `.zip` plus a `.sha256`
checksum. Set `CRAB_VERSION` or `CRAB_TARGET` to override the default version or target.

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
- If PDF inspection or extraction fails, confirm `swift --version` works in the same terminal.
- If the desktop shell fails to start, run `npm install` inside `desktop-shell/` and check
  that Node.js is available.
- Keep the terminal tool disabled unless you intentionally need it. Use `--enable-shell`
  only in trusted workspaces.
