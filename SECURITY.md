# Security Policy

Crab is an experimental local agent runtime. It can read local workspaces, call tools,
interact with browser and document workflows, and optionally run shell commands when the
user enables that capability. Please treat security reports seriously.

## Supported Versions

| Version | Support |
| --- | --- |
| `0.1.x` | Active development and best-effort security fixes |
| `< 0.1.0` | Not supported |

## Reporting A Vulnerability

Use GitHub's private vulnerability reporting or Security Advisories when available. If the
issue is not sensitive, open a normal GitHub issue with a minimal reproduction.

Please include:

- affected commit, version, or branch;
- operating system and relevant runtime details;
- reproduction steps;
- expected and actual behavior;
- whether the issue can expose local files, credentials, browser state, model keys, or
  command execution;
- any temporary mitigation you have found.

Do not post real API keys, private logs, personal data, browser cookies, private model
responses, or third-party confidential material in public issues.

## Scope

Reports are especially valuable when they involve:

- unintended file access outside the selected workspace;
- unsafe shell, browser, PDF, Office, or Git tool behavior;
- prompt/context injection paths that bypass user approval boundaries;
- persistence of secrets in sessions, memory, traces, logs, or screenshots;
- model-provider credential leaks;
- desktop bridge or local runtime command injection.

## Current Safety Posture

- The terminal tool is disabled by default and must be enabled explicitly.
- The terminal tool and `execute_code` share destructive shell-risk checks; obvious
  dangerous command fragments pause for approval before execution.
- Local `tool_policy` config protects common sensitive paths by default, including
  `.env*`, `.ssh/*`, `.aws/*`, `.gnupg/*`, private key files, and common credential
  config files. The preflight recursively inspects path-like tool arguments, including
  nested arrays/objects and camelCase aliases. Local config can extend those protections,
  require approval for selected tools, opt out of the defaults, or disable tools/paths
  entirely before execution.
- Local `network_policy` blocks direct web-fetch tools from accessing loopback, private,
  link-local, and metadata-style hosts by default. Trusted workspaces can explicitly
  allow private network fetches or selected hosts in local config.
- Tool outputs, live previews, timeline details, archive records, and stored assistant
  tool-call arguments redact common credential patterns before becoming durable context.
- In Git workspaces, file mutation tools refuse to overwrite, patch, delete, or move
  existing paths with uncommitted changes unless the tool call explicitly opts into
  `allow_dirty`.
- Local runtime state is expected to live in ignored data directories.
- Model and provider credentials should be supplied through environment variables or
  ignored local configuration.
- The project is pre-stable, so APIs and safety boundaries may change as the runtime
  matures.
