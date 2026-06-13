# Computer Use

Crab's computer-use layer is the native desktop counterpart to the browser tools. Browser
tools operate inside a captured web session. Computer-use tools prepare Crab to inspect and
eventually act on the user's real desktop with operating-system permissions.

The current implementation is deliberately conservative:

- macOS Accessibility trust detection through native ApplicationServices APIs.
- A permission-prompt path for first-time setup.
- A shallow frontmost-app Accessibility UI tree when permission is granted, with stable
  element references such as `@u1`.
- Read-only inspection for a current ref, including its current line and reported native
  Accessibility actions.
- Read-only waiting for a current ref to exist and match role, text, state, or native
  Accessibility action expectations before any write is attempted.
- Read-only searching across a fresh Accessibility snapshot to locate candidate refs by
  text, role, compact state, and reported native Accessibility action.
- Read-only waiting for text to appear, disappear, or for the frontmost Accessibility
  tree to settle.
- Optional pre-action ref guards that check a current ref's role, text, or compact state
  before an approval-gated write action runs.
- Approval-gated focus support for a current `@u` ref.
- Approval-gated click support for a current `@u` ref.
- Approval-gated native Accessibility action execution for a small allowlist such as
  press, show menu, confirm, cancel, increment, and decrement.
- Approval-gated text setting for a current `@u` ref when the target Accessibility element
  supports a writable value.
- Approval-gated small-step scrolling for a current `@u` ref through native
  Accessibility scroll actions.
- Approval-gated key pressing for a small non-text whitelist such as Enter, Escape, Tab,
  arrows, and paging keys.
- No arbitrary global keyboard typing, file, or broad app-control write actions yet.

That shape is intentional. System-level automation should be introduced behind explicit
permission, observable tool calls, and approval policy instead of appearing as a hidden
side effect of ordinary chat.

## Tool Surface

The built-in `computer_use` tool supports fourteen actions:

| Action | Behavior |
| --- | --- |
| `status` | Reports platform support, Accessibility trust, prompt support, and setup guidance. |
| `request_permission` | Calls the macOS Accessibility prompt API and reports the resulting state. |
| `snapshot` | Reads a compact Accessibility UI tree for the frontmost application and its windows. |
| `inspect_ref` | Reads current details and reported native Accessibility actions for a snapshot ref. |
| `find` | Searches a fresh snapshot for candidate UI refs by query, role, or state, and returns matching element lines. |
| `wait` | Polls snapshots until target text appears, disappears, or the UI tree settles, then returns the latest snapshot. |
| `wait_app` | Polls snapshots until the frontmost app name or pid matches `expect_app` or `expect_pid`. |
| `wait_ref` | Polls one UI ref until it exists and optional role, text, state, or native action expectations match. |
| `focus` | Sets keyboard focus to a snapshot ref such as `@u2`, then returns a post-focus snapshot. |
| `click` | Activates a snapshot ref such as `@u2`, then returns a post-click snapshot. |
| `perform_action` | Runs one whitelisted native Accessibility action on a snapshot ref, then returns a post-action snapshot. |
| `set_text` | Sets the Accessibility value for a snapshot ref, then returns a post-action snapshot. |
| `scroll` | Performs a small Accessibility scroll action on a snapshot ref, then returns a post-action snapshot. |
| `press_key` | Presses one whitelisted non-text key in the frontmost app, then returns a post-action snapshot. |

Example tool arguments:

```json
{
  "action": "status"
}
```

```json
{
  "action": "snapshot",
  "max_items": 40,
  "max_depth": 3
}
```

Snapshot output includes non-sensitive record metadata, the frontmost app name, process
id, and a bounded UI tree. `snapshot`, `find`, `wait`, `wait_app`, `wait_ref`, and `inspect_ref`
return the same `snapshot_*` metadata header so the agent can carry explicit evidence
between observation and action. Each visible element line uses a stable reference for that
snapshot and includes the best available role, name, value, bounds, and compact state
flags:

```text
snapshot_id: cu_7d3c0a5d21a9e472
snapshot_max_items: 40
snapshot_max_depth: 3
snapshot_sha256: 5b2f...
snapshot_app_line_sha256: 9d8e...
snapshot_pid: 123
frontmost_app: Finder
pid: 123
ui_tree:
- @u1 role='window' name='Documents' bounds=(80,80,900x640)
  - @u2 role='button' name='Back' bounds=(94,96,28x28) focused=true
  - @u3 role='button' name='Continue' bounds=(740,680,120x32) enabled=false
```

The refs are observation handles only in the current milestone. They are designed so
approval-gated actions can target a concrete element without guessing coordinates.
Snapshot state flags are intentionally sparse: `focused=true` and `selected=true` are
shown only when present, and `enabled=false` marks unavailable controls without adding
noise to every enabled element.

`inspect_ref` is a read-only preflight for a single observed ref. It re-reads the current
frontmost app, returns the target element line plus `available_actions`, and saves a fresh
`snapshot_id` that can be used by the next action:

```json
{
  "action": "inspect_ref",
  "ref": "@u8",
  "max_items": 40,
  "max_depth": 3
}
```

This helps the agent choose between `perform_action`, `click`, `scroll`, `set_text`, or
a key-driven flow based on the UI element's reported native actions instead of guessing
from text alone.

`wait_ref` is the read-only readiness check for one observed ref. It is useful when the
agent has already found a likely control, but needs to wait for it to become enabled or
for a native action such as `AXPress` to appear before requesting approval for the write
step:

```json
{
  "action": "wait_ref",
  "ref": "@u8",
  "expect_role": "button",
  "expect_text": "Continue",
  "expect_state": "enabled",
  "native_action": "AXPress",
  "timeout_seconds": 10,
  "poll_interval_ms": 250,
  "max_items": 40,
  "max_depth": 3
}
```

When `wait_ref` matches or times out, it returns the latest details for that ref and a
fresh `snapshot_id`. If the ref never becomes inspectable, it returns a compact
unavailable marker with a hash of the last internal error rather than echoing raw UI
text from the failure path.

`find` is the lightweight targeting step for native UI work. It takes a fresh snapshot,
saves a new `snapshot_id`, and returns only matching element lines. Use it when the agent
knows what it wants but should avoid dumping the entire UI tree again:

```json
{
  "action": "find",
  "query": "Continue",
  "role": "button",
  "state": "enabled",
  "native_action": "AXPress",
  "max_results": 12,
  "max_items": 40,
  "max_depth": 3
}
```

At least one of `query`, `role`, `state`, or `native_action` is required. `state` accepts
`focused`, `selected`, `enabled`, and `disabled`; `enabled` means the snapshot line does
not include `enabled=false`. When `native_action` is supplied, Crab takes an extra
read-only details check for candidate refs and only returns elements that currently
report the requested action in `available_actions`. The returned `snapshot_id` can be
used immediately by approval-gated `focus`, `click`, `set_text`, or `press_key` calls, or
by read-only `wait_ref` when the agent wants to confirm a specific ref is ready before
asking for approval.

`wait` is the read-only observation loop for native UI work. It returns a fresh
`snapshot_id` and the latest snapshot whether the condition matched or timed out, so the
next action can be based on current evidence:

```json
{
  "action": "wait",
  "wait_until": "text_present",
  "contains_text": "Ready",
  "timeout_seconds": 10,
  "poll_interval_ms": 250,
  "max_items": 40,
  "max_depth": 3
}
```

```json
{
  "action": "wait",
  "wait_until": "text_absent",
  "contains_text": "Loading",
  "timeout_seconds": 10,
  "poll_interval_ms": 250,
  "max_items": 40,
  "max_depth": 3
}
```

```json
{
  "action": "wait",
  "wait_until": "settled",
  "timeout_seconds": 5,
  "max_items": 40,
  "max_depth": 3
}
```

`wait_app` is the read-only frontmost-application loop. It is useful when the user or OS
focus may move away from the target app and the agent wants to wait for the expected app
or pid before asking for approval on a write action:

```json
{
  "action": "wait_app",
  "expect_app": "Finder",
  "timeout_seconds": 10,
  "poll_interval_ms": 250,
  "max_items": 40,
  "max_depth": 3
}
```

Action refs are deliberately ephemeral: take a fresh `snapshot`, choose a visible `@u`
reference from that output, then call `click` or `set_text` immediately. If the app
changes before the action, the ref may resolve to a different UI element in the new tree.
Write actions validate the latest `snapshot_id`, its traversal bounds, and its age before
acting. If the id is omitted, Crab uses the latest snapshot record for the current session;
if a stale id is supplied, the action fails and the agent must observe the desktop again.
Because `@u` refs are assigned inside the bounded tree, write actions also require the
requested `max_items` and `max_depth` to match the snapshot that produced the ref. Snapshot
records older than 30 seconds are rejected for writes, so an agent cannot apply a stale UI
observation to a changed desktop. Crab also records the observed frontmost-app origin as a
hash plus pid and re-checks that origin before every write action; if focus moved to
another app or process, the write is rejected before any Accessibility mutation runs. When
a write action succeeds, Crab saves the returned post-action observation as the new latest
snapshot record and returns `post_snapshot_id`, so the next step can continue from fresh UI
evidence instead of the pre-action id. The saved post-action record is built from the
extracted `post_*_snapshot` body, not from the surrounding action log lines. Write output
also includes the post snapshot bounds, output hash, frontmost-app-line hash, and pid when
available, so the agent can continue with explicit evidence instead of inferring hidden
state.

For safer targeting, write actions can also include optional ref guards. `expect_role`,
`expect_text`, and `expect_state` make Crab take one more read-only snapshot before the
write action and verify that the chosen ref still looks like the observed control. If the
guard fails, the write action is not attempted and the agent should run `snapshot` or
`find` again.

Write actions can also include `expect_app` and `expect_pid` frontmost-app guards. These
make Crab verify the active application before the write action runs. This is useful when
the user may have changed focus between observation and action, and it is especially
important for `press_key`, which acts on the frontmost app rather than a specific ref.
Guard output contains hashed evidence for the current app line instead of echoing the raw
frontmost app text.

```json
{
  "action": "click",
  "ref": "@u2",
  "snapshot_id": "cu_7d3c0a5d21a9e472",
  "expect_role": "button",
  "expect_text": "Continue",
  "expect_state": "enabled",
  "expect_app": "Finder",
  "expect_pid": 123,
  "max_items": 40,
  "max_depth": 3
}
```

```json
{
  "action": "focus",
  "ref": "@u5",
  "snapshot_id": "cu_7d3c0a5d21a9e472",
  "max_items": 40,
  "max_depth": 3
}
```

```json
{
  "action": "perform_action",
  "ref": "@u8",
  "snapshot_id": "cu_7d3c0a5d21a9e472",
  "native_action": "AXPress",
  "expect_role": "button",
  "expect_text": "Continue",
  "max_items": 40,
  "max_depth": 3
}
```

```json
{
  "action": "set_text",
  "ref": "@u5",
  "snapshot_id": "cu_7d3c0a5d21a9e472",
  "text": "hello",
  "max_items": 40,
  "max_depth": 3
}
```

```json
{
  "action": "scroll",
  "ref": "@u8",
  "snapshot_id": "cu_7d3c0a5d21a9e472",
  "direction": "down",
  "scroll_steps": 2,
  "expect_role": "scroll area",
  "max_items": 40,
  "max_depth": 3
}
```

```json
{
  "action": "press_key",
  "key": "enter",
  "snapshot_id": "cu_7d3c0a5d21a9e472",
  "expect_app": "Finder",
  "max_items": 40,
  "max_depth": 3
}
```

`focus`, `click`, `perform_action`, `set_text`, `scroll`, and `press_key` are write
actions. Crab's default tool policy requires approval before they run, even if the user
has not configured a custom `tool_policy`. `status`, `snapshot`, `inspect_ref`, `find`,
`wait`, `wait_app`, and `wait_ref` stay available without approval. `set_text` does not
send global keystrokes; it attempts to set the target Accessibility element's value directly, so it
is mainly for text fields and similar controls.

`perform_action` accepts only a small native Accessibility action allowlist: `press`,
`show_menu`, `confirm`, `cancel`, `increment`, and `decrement`. AX-prefixed names such as
`AXPress` and `AXShowMenu` are accepted and normalized. Before the action runs, Crab
re-reads the ref details and verifies that the current element still reports the chosen
native action in `available_actions`. The write result includes a
`native_action_guard_details_sha256` evidence hash, not the raw ref details. Use
`inspect_ref` or `wait_ref` first when possible so the chosen native action is backed by
current UI evidence.

`scroll` intentionally acts on a specific observed ref and accepts only `up`, `down`,
`left`, or `right`, with `scroll_steps` clamped to `1..=10`. It is for moving within
lists, scroll areas, tables, and panels after observation, not for arbitrary global mouse
wheel injection.

`press_key` intentionally accepts only a small whitelist: `enter`, `escape`, `tab`,
`space`, `backspace`, `forward_delete`, `arrow_up`, `arrow_down`, `arrow_left`,
`arrow_right`, `page_up`, `page_down`, `home`, and `end`. It is for focused UI navigation
and confirmation flows, not arbitrary text entry.

The session snapshot record is intentionally small. It stores the `snapshot_id`, capture
time, bounds used for the read, a SHA-256 hash of the rendered UI observation, a hash of
the frontmost-app line, and the frontmost pid when available. It does not persist the raw
Accessibility tree, app name, element names, field values, or window text.

## macOS Permission Flow

On macOS, Accessibility permission belongs to the process that launches Crab. During local
development that is usually Terminal, iTerm, or the Electron/Tauri desktop app. Packaged
desktop builds should appear as Crab.

To enable it:

1. Run `computer_use` with `action=request_permission`.
2. Open System Settings.
3. Go to Privacy & Security > Accessibility.
4. Enable Crab or the launching terminal.
5. Restart the app or shell if macOS does not refresh the trust state immediately.

`crab doctor` also reports this optional capability. A missing permission is a warning,
not a core runtime failure.

## Safety Model

The first milestone is mostly read-only, with tiny write paths limited to approval-gated
actions on observed refs. It lets the agent know whether native automation is possible and
gives it a bounded, inspectable desktop UI tree. Future write actions should stay gated by:

- explicit tool names and arguments;
- read-only ref inspection before choosing an available native action;
- read-only find steps before choosing an observed ref;
- read-only ref readiness waits before requesting a write action;
- pre-action ref guards for role, text, and state when the target is important;
- pre-action frontmost-app guards for app name or pid;
- a small native action allowlist instead of arbitrary AX action execution;
- pre-action native action guards for `perform_action`;
- read-only waits after actions before choosing the next ref;
- snapshot-bound refs instead of coordinate guessing;
- small, ref-bound scrolling instead of global wheel injection;
- focused UI targets before key-driven navigation;
- a narrow key whitelist instead of arbitrary keyboard injection;
- local `tool_policy` approval rules;
- redacted event and archive records;
- visible desktop timeline events;
- per-platform permission checks before execution.

This keeps computer use aligned with Crab's agent-loop thesis: the model can reason about
the desktop, but the runtime remains the authority for permissions, evidence, and action
boundaries.
