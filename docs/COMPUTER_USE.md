# Computer Use

Crab's computer-use layer is the native desktop counterpart to the browser tools. Browser
tools operate inside a captured web session. Computer-use tools prepare Crab to inspect and
eventually act on the user's real desktop with operating-system permissions.

The current implementation is deliberately conservative:

- macOS Accessibility trust detection through native ApplicationServices APIs.
- A permission-prompt path for first-time setup.
- A shallow frontmost-app Accessibility UI tree when permission is granted, with stable
  element references such as `@u1`.
- Approval-gated click support for a current `@u` ref.
- Approval-gated text setting for a current `@u` ref when the target Accessibility element
  supports a writable value.
- No global keyboard typing, file, or broad app-control write actions yet.

That shape is intentional. System-level automation should be introduced behind explicit
permission, observable tool calls, and approval policy instead of appearing as a hidden
side effect of ordinary chat.

## Tool Surface

The built-in `computer_use` tool supports five actions:

| Action | Behavior |
| --- | --- |
| `status` | Reports platform support, Accessibility trust, prompt support, and setup guidance. |
| `request_permission` | Calls the macOS Accessibility prompt API and reports the resulting state. |
| `snapshot` | Reads a compact Accessibility UI tree for the frontmost application and its windows. |
| `click` | Activates a snapshot ref such as `@u2`, then returns a post-click snapshot. |
| `set_text` | Sets the Accessibility value for a snapshot ref, then returns a post-action snapshot. |

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

Snapshot output includes the frontmost app name, process id, and a bounded UI tree. Each
visible element line uses a stable reference for that snapshot and includes the best
available role, name, value, and bounds:

```text
frontmost_app: Finder
pid: 123
ui_tree:
- @u1 role='window' name='Documents' bounds=(80,80,900x640)
  - @u2 role='button' name='Back' bounds=(94,96,28x28)
```

The refs are observation handles only in the current milestone. They are designed so
approval-gated actions can target a concrete element without guessing coordinates.

Action refs are deliberately ephemeral: take a fresh `snapshot`, choose a visible `@u`
reference from that output, then call `click` or `set_text` immediately. If the app
changes before the action, the ref may resolve to a different UI element in the new tree.

```json
{
  "action": "click",
  "ref": "@u2",
  "max_items": 40,
  "max_depth": 3
}
```

```json
{
  "action": "set_text",
  "ref": "@u5",
  "text": "hello",
  "max_items": 40,
  "max_depth": 3
}
```

`click` and `set_text` are write actions. Crab's default tool policy requires approval
before they run, even if the user has not configured a custom `tool_policy`. Read-only
actions stay available without approval. `set_text` does not send global keystrokes; it
attempts to set the target Accessibility element's value directly, so it is mainly for
text fields and similar controls.

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
- local `tool_policy` approval rules;
- redacted event and archive records;
- visible desktop timeline events;
- per-platform permission checks before execution.

This keeps computer use aligned with Crab's agent-loop thesis: the model can reason about
the desktop, but the runtime remains the authority for permissions, evidence, and action
boundaries.
