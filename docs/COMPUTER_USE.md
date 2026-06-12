# Computer Use

Crab's computer-use layer is the native desktop counterpart to the browser tools. Browser
tools operate inside a captured web session. Computer-use tools prepare Crab to inspect and
eventually act on the user's real desktop with operating-system permissions.

The current implementation is deliberately conservative:

- macOS Accessibility trust detection through native ApplicationServices APIs.
- A permission-prompt path for first-time setup.
- A shallow frontmost-app/window snapshot when Accessibility is granted.
- No mouse, keyboard, file, or app-control write actions yet.

That shape is intentional. System-level automation should be introduced behind explicit
permission, observable tool calls, and approval policy instead of appearing as a hidden
side effect of ordinary chat.

## Tool Surface

The built-in `computer_use` tool supports three actions:

| Action | Behavior |
| --- | --- |
| `status` | Reports platform support, Accessibility trust, prompt support, and setup guidance. |
| `request_permission` | Calls the macOS Accessibility prompt API and reports the resulting state. |
| `snapshot` | Reads a compact outline of the frontmost application and its windows. |

Example tool arguments:

```json
{
  "action": "status"
}
```

```json
{
  "action": "snapshot",
  "max_items": 10
}
```

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

The first milestone is read-only. It lets the agent know whether native automation is
possible and gives it a small, inspectable desktop snapshot. Future write actions should
stay gated by:

- explicit tool names and arguments;
- local `tool_policy` approval rules;
- redacted event and archive records;
- visible desktop timeline events;
- per-platform permission checks before execution.

This keeps computer use aligned with Crab's agent-loop thesis: the model can reason about
the desktop, but the runtime remains the authority for permissions, evidence, and action
boundaries.
