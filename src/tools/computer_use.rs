use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::time::{Duration, Instant, sleep};

use crate::computer_use::{
    ComputerUseKey, ComputerUseNativeAction, ComputerUseScrollDirection, click_frontmost_app_ref,
    focus_frontmost_app_ref, frontmost_app_ref_details, frontmost_app_snapshot,
    inspect_computer_use, normalize_computer_use_key, normalize_computer_use_native_action,
    normalize_computer_use_scroll_direction, parse_ui_ref, perform_frontmost_app_ref_action,
    press_frontmost_app_key, scroll_frontmost_app_ref, set_frontmost_app_ref_text,
};
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct ComputerUseTool;

#[derive(Debug, Deserialize)]
struct ComputerUseArgs {
    #[serde(default = "default_action")]
    action: String,
    #[serde(default = "default_max_items")]
    max_items: usize,
    #[serde(default = "default_max_depth")]
    max_depth: usize,
    reference: Option<String>,
    #[serde(rename = "ref")]
    ref_alias: Option<String>,
    text: Option<String>,
    key: Option<String>,
    #[serde(alias = "nativeAction", alias = "ax_action", alias = "axAction")]
    native_action: Option<String>,
    direction: Option<String>,
    #[serde(alias = "scrollSteps", alias = "steps")]
    scroll_steps: Option<usize>,
    #[serde(alias = "waitUntil")]
    wait_until: Option<String>,
    #[serde(alias = "containsText")]
    contains_text: Option<String>,
    query: Option<String>,
    role: Option<String>,
    state: Option<String>,
    #[serde(alias = "maxResults")]
    max_results: Option<usize>,
    #[serde(alias = "expectedRole", alias = "expectRole")]
    expect_role: Option<String>,
    #[serde(alias = "expectedText", alias = "expectText")]
    expect_text: Option<String>,
    #[serde(alias = "expectedState", alias = "expectState")]
    expect_state: Option<String>,
    #[serde(alias = "expectedApp", alias = "expectApp")]
    expect_app: Option<String>,
    #[serde(alias = "expectedPid", alias = "expectPid")]
    expect_pid: Option<u32>,
    #[serde(alias = "caseSensitive")]
    case_sensitive: Option<bool>,
    #[serde(alias = "timeoutSeconds")]
    timeout_seconds: Option<u64>,
    #[serde(alias = "pollIntervalMs")]
    poll_interval_ms: Option<u64>,
    snapshot_id: Option<String>,
}

const MAX_SET_TEXT_CHARS: usize = 4_000;
const MAX_WAIT_TEXT_CHARS: usize = 1_000;
const MAX_FIND_QUERY_CHARS: usize = 1_000;
const MAX_FIND_ROLE_CHARS: usize = 120;
const MAX_EXPECT_TEXT_CHARS: usize = 1_000;
const MAX_EXPECT_ROLE_CHARS: usize = 120;
const MAX_EXPECT_APP_CHARS: usize = 240;
const DEFAULT_SCROLL_STEPS: usize = 1;
const MAX_SCROLL_STEPS: usize = 10;
const DEFAULT_FIND_MAX_RESULTS: usize = 12;
const MAX_FIND_RESULTS: usize = 50;
const DEFAULT_WAIT_TIMEOUT_SECONDS: u64 = 10;
const MAX_WAIT_TIMEOUT_SECONDS: u64 = 30;
const DEFAULT_WAIT_POLL_INTERVAL_MS: u64 = 250;
const MAX_SNAPSHOT_RECORD_AGE_SECONDS: u64 = 30;

fn default_action() -> String {
    "status".to_string()
}

fn default_max_items() -> usize {
    40
}

fn default_max_depth() -> usize {
    3
}

#[async_trait]
impl Tool for ComputerUseTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "computer_use",
            "Inspect and prepare native computer-use automation. On macOS, this checks Accessibility trust, can request the permission prompt, can return/search/inspect a shallow Accessibility UI tree for the frontmost app, can wait for the frontmost app or a specific UI ref to become ready, can focus, click, perform a whitelisted native Accessibility action, scroll, or set text on a UI ref after tool-policy approval, and can press a small whitelist of non-text keys after approval. Broad keyboard and app-control actions are intentionally not enabled yet.",
            object_schema(
                json!({
                    "action": {
                        "type": "string",
                        "enum": ["status", "request_permission", "snapshot", "inspect_ref", "find", "wait", "wait_app", "wait_ref", "focus", "click", "perform_action", "set_text", "scroll", "press_key"],
                        "description": "status checks support and permission; request_permission asks macOS to show the Accessibility prompt; snapshot reads the frontmost app Accessibility UI tree; inspect_ref reads the current details and available actions for a snapshot ref; find searches a fresh snapshot for candidate UI refs; wait polls snapshots until text appears, text disappears, or the UI settles; wait_app polls snapshots until the frontmost app name or pid matches expect_app/expect_pid; wait_ref polls one UI ref until it exists and optional role, text, state, or native Accessibility action expectations match; focus sets keyboard focus to a snapshot ref such as @u2 after approval; click activates a snapshot ref after approval; perform_action runs one whitelisted native Accessibility action on a ref after approval; set_text sets the Accessibility value for a ref after approval; scroll performs a small Accessibility scroll action on a snapshot ref after approval; press_key sends one whitelisted non-text key to the frontmost app after approval."
                    },
                    "max_items": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 50,
                        "description": "Maximum number of UI elements to include in snapshot output."
                    },
                    "max_depth": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 6,
                        "description": "Maximum Accessibility UI tree depth to traverse from each frontmost app window."
                    },
                    "reference": {
                        "type": "string",
                        "description": "UI ref from the latest computer_use snapshot, such as @u2. Required for inspect_ref, wait_ref, focus, click, perform_action, set_text, and scroll."
                    },
                    "ref": {
                        "type": "string",
                        "description": "Alias for reference."
                    },
                    "text": {
                        "type": "string",
                        "maxLength": 4000,
                        "description": "Text to set on the referenced Accessibility element. Required for set_text."
                    },
                    "key": {
                        "type": "string",
                        "enum": ["enter", "escape", "tab", "space", "backspace", "forward_delete", "arrow_up", "arrow_down", "arrow_left", "arrow_right", "page_up", "page_down", "home", "end"],
                        "description": "Whitelisted key to press in the frontmost app. Required for press_key. Aliases such as return, esc, left, right, up, down, delete, and page-down are also accepted."
                    },
                    "native_action": {
                        "type": "string",
                        "enum": ["press", "show_menu", "confirm", "cancel", "increment", "decrement"],
                        "description": "Whitelisted native Accessibility action for action=perform_action, an expected available action for action=wait_ref, or a reported action filter for action=find. AX-prefixed names such as AXPress and AXShowMenu are also accepted."
                    },
                    "direction": {
                        "type": "string",
                        "enum": ["up", "down", "left", "right"],
                        "description": "Direction for action=scroll. Aliases such as scroll_down are also accepted."
                    },
                    "scroll_steps": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 10,
                        "description": "Small number of repeated Accessibility scroll actions for action=scroll. Defaults to 1."
                    },
                    "wait_until": {
                        "type": "string",
                        "enum": ["text_present", "text_absent", "settled"],
                        "description": "Wait mode for action=wait. text_present and text_absent require contains_text; settled waits for two consecutive matching snapshots. Defaults to text_present when contains_text is provided, otherwise settled."
                    },
                    "contains_text": {
                        "type": "string",
                        "maxLength": 1000,
                        "description": "Substring to wait for in the rendered Accessibility snapshot when wait_until=text_present, or wait to disappear when wait_until=text_absent."
                    },
                    "query": {
                        "type": "string",
                        "maxLength": 1000,
                        "description": "Text to search for in frontmost Accessibility element lines when action=find. At least one of query, role, state, or native_action is required."
                    },
                    "role": {
                        "type": "string",
                        "maxLength": 120,
                        "description": "Optional role filter for action=find, such as button, text field, menu item, or window. At least one of query, role, state, or native_action is required."
                    },
                    "state": {
                        "type": "string",
                        "enum": ["focused", "selected", "enabled", "disabled"],
                        "description": "Optional state filter for action=find. enabled means no enabled=false flag is present in the snapshot line. At least one of query, role, state, or native_action is required."
                    },
                    "max_results": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 50,
                        "description": "Maximum matching Accessibility element lines to return for action=find. Defaults to 12."
                    },
                    "expect_role": {
                        "type": "string",
                        "maxLength": 120,
                        "description": "Optional expectation for wait_ref and optional pre-action guard for focus, click, perform_action, set_text, and scroll. When set, the current ref line must still have this role."
                    },
                    "expect_text": {
                        "type": "string",
                        "maxLength": 1000,
                        "description": "Optional expectation for wait_ref and optional pre-action guard for focus, click, perform_action, set_text, and scroll. When set, the current ref line must still contain this text."
                    },
                    "expect_state": {
                        "type": "string",
                        "enum": ["focused", "selected", "enabled", "disabled"],
                        "description": "Optional expectation for wait_ref and optional pre-action guard for focus, click, perform_action, set_text, and scroll. When set, the current ref line must still match this compact state."
                    },
                    "expect_app": {
                        "type": "string",
                        "maxLength": 240,
                        "description": "Expectation for action=wait_app and optional pre-action guard for focus, click, perform_action, set_text, scroll, and press_key. When set, the current frontmost app name must match."
                    },
                    "expect_pid": {
                        "type": "integer",
                        "minimum": 1,
                        "description": "Expectation for action=wait_app and optional pre-action guard for focus, click, perform_action, set_text, scroll, and press_key. When set, the current frontmost process id must match."
                    },
                    "case_sensitive": {
                        "type": "boolean",
                        "description": "Whether contains_text, find query, expect_text, or expect_app matching is case-sensitive. Defaults to false."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 30,
                        "description": "Maximum time to wait for action=wait, action=wait_app, or action=wait_ref. Defaults to 10 seconds."
                    },
                    "poll_interval_ms": {
                        "type": "integer",
                        "minimum": 100,
                        "maximum": 2000,
                        "description": "Polling interval for action=wait, action=wait_app, or action=wait_ref. Defaults to 250 ms."
                    },
                    "snapshot_id": {
                        "type": "string",
                        "description": "Optional id returned by the latest snapshot. Write actions validate this before acting; when omitted they use the latest saved snapshot for this session."
                    }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: ComputerUseArgs =
            serde_json::from_value(args).context("invalid computer_use arguments")?;
        match args.action.trim() {
            "" | "status" => Ok(render_status(false)),
            "request_permission" => Ok(render_status(true)),
            "snapshot" => {
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let snapshot = frontmost_app_snapshot(max_items, max_depth)?;
                let record = save_snapshot_record(ctx, max_items, max_depth, &snapshot)?;
                Ok(format!(
                    "{}{}\n\n{}",
                    render_snapshot_record_metadata(&record, "snapshot"),
                    render_status(false),
                    snapshot.trim()
                ))
            }
            "wait" => {
                let request = args.wait_request()?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let outcome = wait_for_frontmost_app(&request, max_items, max_depth).await?;
                let record = save_snapshot_record(ctx, max_items, max_depth, &outcome.snapshot)?;
                Ok(format!(
                    "{}wait_result: {}\nwait_until: {}\nattempts: {}\nelapsed_ms: {}\n{}\n\n{}",
                    render_snapshot_record_metadata(&record, "snapshot"),
                    outcome.result,
                    request.mode.label(),
                    outcome.attempts,
                    outcome.elapsed_ms,
                    render_status(false),
                    outcome.snapshot.trim()
                ))
            }
            "wait_app" => {
                let request = args.wait_app_request()?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let outcome = wait_for_frontmost_app_match(&request, max_items, max_depth).await?;
                let record = save_snapshot_record(ctx, max_items, max_depth, &outcome.snapshot)?;
                Ok(format!(
                    "{}wait_result: {}\nwait_until: app_matches\nattempts: {}\nelapsed_ms: {}\n{}\n\n{}",
                    render_snapshot_record_metadata(&record, "snapshot"),
                    outcome.result,
                    outcome.attempts,
                    outcome.elapsed_ms,
                    render_status(false),
                    outcome.snapshot.trim()
                ))
            }
            "wait_ref" => {
                let reference = args.reference()?;
                let request = args.wait_ref_request()?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let outcome =
                    wait_for_frontmost_app_ref(reference, &request, max_items, max_depth).await?;
                let record = save_snapshot_record(ctx, max_items, max_depth, &outcome.details)?;
                Ok(format!(
                    "{}wait_result: {}\nwait_until: ref_matches\nattempts: {}\nelapsed_ms: {}\n{}\n\n{}",
                    render_snapshot_record_metadata(&record, "snapshot"),
                    outcome.result,
                    outcome.attempts,
                    outcome.elapsed_ms,
                    render_status(false),
                    outcome.details.trim()
                ))
            }
            "find" => {
                let request = args.find_request()?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let snapshot = frontmost_app_snapshot(max_items, max_depth)?;
                let record = save_snapshot_record(ctx, max_items, max_depth, &snapshot)?;
                let outcome = find_snapshot_lines(&snapshot, &request, max_items, max_depth)?;
                Ok(format!(
                    "{}find_result_count: {}\nfind_truncated: {}\n{}\n\n{}",
                    render_snapshot_record_metadata(&record, "snapshot"),
                    outcome.matches.len(),
                    outcome.truncated,
                    render_status(false),
                    render_find_matches(&outcome)
                ))
            }
            "inspect_ref" => {
                let reference = args.reference()?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let details = frontmost_app_ref_details(reference, max_items, max_depth)?;
                let record = save_snapshot_record(ctx, max_items, max_depth, &details)?;
                Ok(format!(
                    "{}{}\n\n{}",
                    render_snapshot_record_metadata(&record, "snapshot"),
                    render_status(false),
                    details.trim()
                ))
            }
            "click" => {
                let reference = args.reference()?;
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let snapshot_record = resolve_snapshot_record(
                    ctx,
                    args.snapshot_id.as_deref(),
                    max_items,
                    max_depth,
                )?;
                let ref_guard_request = args.ref_guard_request()?;
                let app_guard_request = args.app_guard_request()?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let snapshot_origin_guard =
                    run_snapshot_origin_guard(&snapshot_record, max_items, max_depth)?;
                let app_guard = run_app_guard(max_items, max_depth, app_guard_request.as_ref())?;
                let ref_guard =
                    run_ref_guard(reference, max_items, max_depth, ref_guard_request.as_ref())?;
                let result = click_frontmost_app_ref(reference, max_items, max_depth)?;
                let post_record =
                    save_post_action_snapshot_record(ctx, max_items, max_depth, &result)?;
                Ok(render_write_result(
                    &snapshot_record.snapshot_id,
                    &post_record,
                    &snapshot_origin_guard,
                    ref_guard.as_ref(),
                    app_guard.as_ref(),
                    None,
                    &result,
                ))
            }
            "perform_action" => {
                let reference = args.reference()?;
                let request = args.native_action_request()?;
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let snapshot_record = resolve_snapshot_record(
                    ctx,
                    args.snapshot_id.as_deref(),
                    max_items,
                    max_depth,
                )?;
                let ref_guard_request = args.ref_guard_request()?;
                let app_guard_request = args.app_guard_request()?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let snapshot_origin_guard =
                    run_snapshot_origin_guard(&snapshot_record, max_items, max_depth)?;
                let app_guard = run_app_guard(max_items, max_depth, app_guard_request.as_ref())?;
                let ref_guard =
                    run_ref_guard(reference, max_items, max_depth, ref_guard_request.as_ref())?;
                let native_action_guard = run_native_action_guard(
                    reference,
                    max_items,
                    max_depth,
                    request.native_action,
                )?;
                let result = perform_frontmost_app_ref_action(
                    reference,
                    request.native_action.label,
                    max_items,
                    max_depth,
                )?;
                let post_record =
                    save_post_action_snapshot_record(ctx, max_items, max_depth, &result)?;
                Ok(render_write_result(
                    &snapshot_record.snapshot_id,
                    &post_record,
                    &snapshot_origin_guard,
                    ref_guard.as_ref(),
                    app_guard.as_ref(),
                    Some(&native_action_guard),
                    &result,
                ))
            }
            "focus" => {
                let reference = args.reference()?;
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let snapshot_record = resolve_snapshot_record(
                    ctx,
                    args.snapshot_id.as_deref(),
                    max_items,
                    max_depth,
                )?;
                let ref_guard_request = args.ref_guard_request()?;
                let app_guard_request = args.app_guard_request()?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let snapshot_origin_guard =
                    run_snapshot_origin_guard(&snapshot_record, max_items, max_depth)?;
                let app_guard = run_app_guard(max_items, max_depth, app_guard_request.as_ref())?;
                let ref_guard =
                    run_ref_guard(reference, max_items, max_depth, ref_guard_request.as_ref())?;
                let result = focus_frontmost_app_ref(reference, max_items, max_depth)?;
                let post_record =
                    save_post_action_snapshot_record(ctx, max_items, max_depth, &result)?;
                Ok(render_write_result(
                    &snapshot_record.snapshot_id,
                    &post_record,
                    &snapshot_origin_guard,
                    ref_guard.as_ref(),
                    app_guard.as_ref(),
                    None,
                    &result,
                ))
            }
            "set_text" => {
                let reference = args.reference()?;
                let text = args.text()?;
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let snapshot_record = resolve_snapshot_record(
                    ctx,
                    args.snapshot_id.as_deref(),
                    max_items,
                    max_depth,
                )?;
                let ref_guard_request = args.ref_guard_request()?;
                let app_guard_request = args.app_guard_request()?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let snapshot_origin_guard =
                    run_snapshot_origin_guard(&snapshot_record, max_items, max_depth)?;
                let app_guard = run_app_guard(max_items, max_depth, app_guard_request.as_ref())?;
                let ref_guard =
                    run_ref_guard(reference, max_items, max_depth, ref_guard_request.as_ref())?;
                let result = set_frontmost_app_ref_text(reference, text, max_items, max_depth)?;
                let post_record =
                    save_post_action_snapshot_record(ctx, max_items, max_depth, &result)?;
                Ok(render_write_result(
                    &snapshot_record.snapshot_id,
                    &post_record,
                    &snapshot_origin_guard,
                    ref_guard.as_ref(),
                    app_guard.as_ref(),
                    None,
                    &result,
                ))
            }
            "scroll" => {
                let reference = args.reference()?;
                let request = args.scroll_request()?;
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let snapshot_record = resolve_snapshot_record(
                    ctx,
                    args.snapshot_id.as_deref(),
                    max_items,
                    max_depth,
                )?;
                let ref_guard_request = args.ref_guard_request()?;
                let app_guard_request = args.app_guard_request()?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let snapshot_origin_guard =
                    run_snapshot_origin_guard(&snapshot_record, max_items, max_depth)?;
                let app_guard = run_app_guard(max_items, max_depth, app_guard_request.as_ref())?;
                let ref_guard =
                    run_ref_guard(reference, max_items, max_depth, ref_guard_request.as_ref())?;
                let result = scroll_frontmost_app_ref(
                    reference,
                    request.direction.label,
                    request.steps,
                    max_items,
                    max_depth,
                )?;
                let post_record =
                    save_post_action_snapshot_record(ctx, max_items, max_depth, &result)?;
                Ok(render_write_result(
                    &snapshot_record.snapshot_id,
                    &post_record,
                    &snapshot_origin_guard,
                    ref_guard.as_ref(),
                    app_guard.as_ref(),
                    None,
                    &result,
                ))
            }
            "press_key" => {
                let key = args.key()?;
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let snapshot_record = resolve_snapshot_record(
                    ctx,
                    args.snapshot_id.as_deref(),
                    max_items,
                    max_depth,
                )?;
                let app_guard_request = args.app_guard_request()?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let snapshot_origin_guard =
                    run_snapshot_origin_guard(&snapshot_record, max_items, max_depth)?;
                let app_guard = run_app_guard(max_items, max_depth, app_guard_request.as_ref())?;
                let result = press_frontmost_app_key(key.label, max_items, max_depth)?;
                let post_record =
                    save_post_action_snapshot_record(ctx, max_items, max_depth, &result)?;
                Ok(render_write_result(
                    &snapshot_record.snapshot_id,
                    &post_record,
                    &snapshot_origin_guard,
                    None,
                    app_guard.as_ref(),
                    None,
                    &result,
                ))
            }
            other => bail!(
                "unsupported computer_use action `{other}`; use status, request_permission, snapshot, inspect_ref, find, wait, wait_app, wait_ref, focus, click, perform_action, set_text, scroll, or press_key"
            ),
        }
    }
}

impl ComputerUseArgs {
    fn reference(&self) -> Result<&str> {
        match (self.reference.as_deref(), self.ref_alias.as_deref()) {
            (Some(reference), Some(ref_alias)) if reference != ref_alias => {
                bail!("computer_use received conflicting `reference` and `ref` values")
            }
            (Some(reference), _) if !reference.trim().is_empty() => Ok(reference.trim()),
            (_, Some(ref_alias)) if !ref_alias.trim().is_empty() => Ok(ref_alias.trim()),
            _ => bail!("computer_use action requires a non-empty `reference` or `ref`"),
        }
    }

    fn text(&self) -> Result<&str> {
        let Some(text) = self.text.as_deref() else {
            bail!("computer_use set_text requires a `text` value");
        };
        if text.chars().count() > MAX_SET_TEXT_CHARS {
            bail!(
                "computer_use set_text text is too long; maximum is {MAX_SET_TEXT_CHARS} characters"
            );
        }
        Ok(text)
    }

    fn key(&self) -> Result<ComputerUseKey> {
        let Some(key) = self.key.as_deref() else {
            bail!("computer_use press_key requires a `key` value");
        };
        normalize_computer_use_key(key)
    }

    fn native_action_request(&self) -> Result<ComputerUseNativeActionRequest> {
        let Some(native_action) = self.native_action.as_deref() else {
            bail!("computer_use perform_action requires a `native_action` value");
        };
        Ok(ComputerUseNativeActionRequest {
            native_action: normalize_computer_use_native_action(native_action)?,
        })
    }

    fn scroll_request(&self) -> Result<ComputerUseScrollRequest> {
        let Some(direction) = self.direction.as_deref() else {
            bail!("computer_use scroll requires a `direction` value");
        };
        Ok(ComputerUseScrollRequest {
            direction: normalize_computer_use_scroll_direction(direction)?,
            steps: self
                .scroll_steps
                .unwrap_or(DEFAULT_SCROLL_STEPS)
                .clamp(1, MAX_SCROLL_STEPS),
        })
    }

    fn wait_request(&self) -> Result<ComputerUseWaitRequest> {
        let contains_text = self
            .contains_text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let wait_until = self
            .wait_until
            .as_deref()
            .map(normalize_wait_mode)
            .unwrap_or_else(|| {
                if contains_text.is_some() {
                    "text_present".to_string()
                } else {
                    "settled".to_string()
                }
            });
        let mode = match wait_until.as_str() {
            "text_present" | "text_contains" | "contains_text" => {
                let Some(contains_text) = contains_text else {
                    bail!("computer_use wait_until=text_present requires `contains_text`");
                };
                if contains_text.chars().count() > MAX_WAIT_TEXT_CHARS {
                    bail!(
                        "computer_use wait contains_text is too long; maximum is {MAX_WAIT_TEXT_CHARS} characters"
                    );
                }
                ComputerUseWaitMode::TextPresent {
                    contains_text: contains_text.to_string(),
                    case_sensitive: self.case_sensitive.unwrap_or(false),
                }
            }
            "text_absent" | "text_missing" | "text_gone" | "gone" | "not_present" => {
                let Some(contains_text) = contains_text else {
                    bail!("computer_use wait_until=text_absent requires `contains_text`");
                };
                if contains_text.chars().count() > MAX_WAIT_TEXT_CHARS {
                    bail!(
                        "computer_use wait contains_text is too long; maximum is {MAX_WAIT_TEXT_CHARS} characters"
                    );
                }
                ComputerUseWaitMode::TextAbsent {
                    contains_text: contains_text.to_string(),
                    case_sensitive: self.case_sensitive.unwrap_or(false),
                }
            }
            "settled" | "stable" => ComputerUseWaitMode::Settled,
            "" => bail!("computer_use wait_until cannot be empty"),
            other => {
                bail!(
                    "unsupported computer_use wait_until `{other}`; use text_present, text_absent, or settled"
                )
            }
        };
        Ok(ComputerUseWaitRequest {
            mode,
            timeout_seconds: self
                .timeout_seconds
                .unwrap_or(DEFAULT_WAIT_TIMEOUT_SECONDS)
                .clamp(1, MAX_WAIT_TIMEOUT_SECONDS),
            poll_interval_ms: self
                .poll_interval_ms
                .unwrap_or(DEFAULT_WAIT_POLL_INTERVAL_MS)
                .clamp(100, 2_000),
        })
    }

    fn wait_ref_request(&self) -> Result<ComputerUseWaitRefRequest> {
        let guard = self.ref_guard_request()?;
        let native_action = self
            .native_action
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(normalize_computer_use_native_action)
            .transpose()?;
        Ok(ComputerUseWaitRefRequest {
            guard,
            native_action,
            timeout_seconds: self
                .timeout_seconds
                .unwrap_or(DEFAULT_WAIT_TIMEOUT_SECONDS)
                .clamp(1, MAX_WAIT_TIMEOUT_SECONDS),
            poll_interval_ms: self
                .poll_interval_ms
                .unwrap_or(DEFAULT_WAIT_POLL_INTERVAL_MS)
                .clamp(100, 2_000),
        })
    }

    fn wait_app_request(&self) -> Result<ComputerUseWaitAppRequest> {
        let Some(guard) = self.app_guard_request()? else {
            bail!("computer_use wait_app requires `expect_app` or `expect_pid`");
        };
        Ok(ComputerUseWaitAppRequest {
            guard,
            timeout_seconds: self
                .timeout_seconds
                .unwrap_or(DEFAULT_WAIT_TIMEOUT_SECONDS)
                .clamp(1, MAX_WAIT_TIMEOUT_SECONDS),
            poll_interval_ms: self
                .poll_interval_ms
                .unwrap_or(DEFAULT_WAIT_POLL_INTERVAL_MS)
                .clamp(100, 2_000),
        })
    }

    fn find_request(&self) -> Result<ComputerUseFindRequest> {
        let query = self
            .query
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        if let Some(query) = &query
            && query.chars().count() > MAX_FIND_QUERY_CHARS
        {
            bail!(
                "computer_use find query is too long; maximum is {MAX_FIND_QUERY_CHARS} characters"
            );
        }

        let role = self
            .role
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        if let Some(role) = &role
            && role.chars().count() > MAX_FIND_ROLE_CHARS
        {
            bail!(
                "computer_use find role is too long; maximum is {MAX_FIND_ROLE_CHARS} characters"
            );
        }

        let state = self
            .state
            .as_deref()
            .map(ComputerUseFindState::parse)
            .transpose()?;
        let native_action = self
            .native_action
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(normalize_computer_use_native_action)
            .transpose()?;
        if query.is_none() && role.is_none() && state.is_none() && native_action.is_none() {
            bail!(
                "computer_use find requires at least one of `query`, `role`, `state`, or `native_action`"
            );
        }

        Ok(ComputerUseFindRequest {
            query,
            role,
            state,
            native_action,
            case_sensitive: self.case_sensitive.unwrap_or(false),
            max_results: self
                .max_results
                .unwrap_or(DEFAULT_FIND_MAX_RESULTS)
                .clamp(1, MAX_FIND_RESULTS),
        })
    }

    fn ref_guard_request(&self) -> Result<Option<ComputerUseRefGuardRequest>> {
        let role = self
            .expect_role
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        if let Some(role) = &role
            && role.chars().count() > MAX_EXPECT_ROLE_CHARS
        {
            bail!(
                "computer_use expect_role is too long; maximum is {MAX_EXPECT_ROLE_CHARS} characters"
            );
        }

        let text = self
            .expect_text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        if let Some(text) = &text
            && text.chars().count() > MAX_EXPECT_TEXT_CHARS
        {
            bail!(
                "computer_use expect_text is too long; maximum is {MAX_EXPECT_TEXT_CHARS} characters"
            );
        }

        let state = self
            .expect_state
            .as_deref()
            .map(ComputerUseFindState::parse_expect)
            .transpose()?;
        if role.is_none() && text.is_none() && state.is_none() {
            return Ok(None);
        }

        Ok(Some(ComputerUseRefGuardRequest {
            role,
            text,
            state,
            case_sensitive: self.case_sensitive.unwrap_or(false),
        }))
    }

    fn app_guard_request(&self) -> Result<Option<ComputerUseAppGuardRequest>> {
        let app = self
            .expect_app
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        if let Some(app) = &app
            && app.chars().count() > MAX_EXPECT_APP_CHARS
        {
            bail!(
                "computer_use expect_app is too long; maximum is {MAX_EXPECT_APP_CHARS} characters"
            );
        }
        if app.is_none() && self.expect_pid.is_none() {
            return Ok(None);
        }
        Ok(Some(ComputerUseAppGuardRequest {
            app,
            pid: self.expect_pid,
            case_sensitive: self.case_sensitive.unwrap_or(false),
        }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComputerUseWaitRequest {
    mode: ComputerUseWaitMode,
    timeout_seconds: u64,
    poll_interval_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ComputerUseWaitMode {
    TextPresent {
        contains_text: String,
        case_sensitive: bool,
    },
    TextAbsent {
        contains_text: String,
        case_sensitive: bool,
    },
    Settled,
}

impl ComputerUseWaitMode {
    fn label(&self) -> &'static str {
        match self {
            ComputerUseWaitMode::TextPresent { .. } => "text_present",
            ComputerUseWaitMode::TextAbsent { .. } => "text_absent",
            ComputerUseWaitMode::Settled => "settled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComputerUseWaitOutcome {
    result: &'static str,
    snapshot: String,
    attempts: usize,
    elapsed_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComputerUseWaitAppRequest {
    guard: ComputerUseAppGuardRequest,
    timeout_seconds: u64,
    poll_interval_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComputerUseWaitRefRequest {
    guard: Option<ComputerUseRefGuardRequest>,
    native_action: Option<ComputerUseNativeAction>,
    timeout_seconds: u64,
    poll_interval_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComputerUseWaitRefOutcome {
    result: &'static str,
    details: String,
    attempts: usize,
    elapsed_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComputerUseNativeActionRequest {
    native_action: ComputerUseNativeAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComputerUseScrollRequest {
    direction: ComputerUseScrollDirection,
    steps: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComputerUseFindRequest {
    query: Option<String>,
    role: Option<String>,
    state: Option<ComputerUseFindState>,
    native_action: Option<ComputerUseNativeAction>,
    case_sensitive: bool,
    max_results: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComputerUseFindState {
    Focused,
    Selected,
    Enabled,
    Disabled,
}

impl ComputerUseFindState {
    fn parse(state: &str) -> Result<Self> {
        Self::parse_for(state, "find state")
    }

    fn parse_expect(state: &str) -> Result<Self> {
        Self::parse_for(state, "expect_state")
    }

    fn parse_for(state: &str, field: &str) -> Result<Self> {
        match state
            .trim()
            .to_ascii_lowercase()
            .replace([' ', '-'], "_")
            .as_str()
        {
            "focused" | "focus" => Ok(Self::Focused),
            "selected" | "select" => Ok(Self::Selected),
            "enabled" | "available" => Ok(Self::Enabled),
            "disabled" | "not_enabled" | "unavailable" => Ok(Self::Disabled),
            "" => bail!("computer_use {field} cannot be empty"),
            other => bail!(
                "unsupported computer_use {field} `{other}`; use focused, selected, enabled, or disabled"
            ),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Focused => "focused",
            Self::Selected => "selected",
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComputerUseFindOutcome {
    matches: Vec<String>,
    truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComputerUseRefGuardRequest {
    role: Option<String>,
    text: Option<String>,
    state: Option<ComputerUseFindState>,
    case_sensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComputerUseRefGuardOutcome {
    reference: String,
    line_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComputerUseAppGuardRequest {
    app: Option<String>,
    pid: Option<u32>,
    case_sensitive: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComputerUseAppGuardOutcome {
    app_line_sha256: String,
    pid: Option<u32>,
    snapshot_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComputerUseSnapshotOriginGuardOutcome {
    app_line_sha256: String,
    pid: Option<u32>,
    snapshot_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ComputerUseNativeActionGuardOutcome {
    reference: String,
    native_action: &'static str,
    details_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ComputerUseSnapshotRecord {
    snapshot_id: String,
    captured_at_unix: u64,
    max_items: usize,
    max_depth: usize,
    output_sha256: String,
    #[serde(default)]
    frontmost_app_line_sha256: Option<String>,
    #[serde(default)]
    pid: Option<u32>,
}

fn save_snapshot_record(
    ctx: &ToolContext,
    max_items: usize,
    max_depth: usize,
    output: &str,
) -> Result<ComputerUseSnapshotRecord> {
    let output_sha256 = sha256_hex(output.as_bytes());
    let frontmost_app_line_sha256 =
        snapshot_frontmost_app_line(output).map(|line| sha256_hex(line.as_bytes()));
    let pid = snapshot_pid(output);
    let captured_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let captured_at_unix = captured_at.as_secs();
    let snapshot_id = format!(
        "cu_{}",
        &sha256_hex(
            format!(
                "{}\0{}\0{}\0{}\0{}",
                ctx.current_session_id,
                max_items,
                max_depth,
                captured_at.as_nanos(),
                output_sha256
            )
            .as_bytes()
        )[..16]
    );
    let record = ComputerUseSnapshotRecord {
        snapshot_id,
        captured_at_unix,
        max_items,
        max_depth,
        output_sha256,
        frontmost_app_line_sha256,
        pid,
    };
    let path = snapshot_record_path(ctx);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&path, serde_json::to_vec_pretty(&record)?)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(record)
}

fn save_post_action_snapshot_record(
    ctx: &ToolContext,
    max_items: usize,
    max_depth: usize,
    result: &str,
) -> Result<ComputerUseSnapshotRecord> {
    let post_snapshot = post_action_snapshot_from_result(result)?;
    save_snapshot_record(ctx, max_items, max_depth, post_snapshot)
}

fn post_action_snapshot_from_result(result: &str) -> Result<&str> {
    const POST_SNAPSHOT_MARKERS: &[&str] = &[
        "post_click_snapshot:",
        "post_focus_snapshot:",
        "post_set_text_snapshot:",
        "post_key_snapshot:",
        "post_scroll_snapshot:",
        "post_action_snapshot:",
    ];
    for marker in POST_SNAPSHOT_MARKERS {
        if let Some((_, post_snapshot)) = result.split_once(marker) {
            let post_snapshot = post_snapshot.trim();
            if post_snapshot.is_empty() {
                bail!("computer_use write action did not return a post-action snapshot");
            }
            return Ok(post_snapshot);
        }
    }
    bail!("computer_use write action result did not include a post-action snapshot")
}

fn render_snapshot_record_metadata(record: &ComputerUseSnapshotRecord, prefix: &str) -> String {
    format!(
        "{prefix}_id: {}\n{prefix}_max_items: {}\n{prefix}_max_depth: {}\n{prefix}_sha256: {}\n{prefix}_app_line_sha256: {}\n{prefix}_pid: {}\n",
        record.snapshot_id,
        record.max_items,
        record.max_depth,
        record.output_sha256,
        record
            .frontmost_app_line_sha256
            .as_deref()
            .unwrap_or("unknown"),
        record
            .pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "unknown".to_string())
    )
}

fn resolve_snapshot_record(
    ctx: &ToolContext,
    requested_snapshot_id: Option<&str>,
    max_items: usize,
    max_depth: usize,
) -> Result<ComputerUseSnapshotRecord> {
    let record = load_snapshot_record(ctx)?.ok_or_else(|| {
        anyhow::anyhow!(
            "computer_use action requires a recent snapshot; call computer_use with action=snapshot first"
        )
    })?;
    if let Some(requested_snapshot_id) = requested_snapshot_id {
        let requested_snapshot_id = requested_snapshot_id.trim();
        if requested_snapshot_id.is_empty() {
            bail!("computer_use snapshot_id cannot be empty");
        }
        if requested_snapshot_id != record.snapshot_id {
            bail!(
                "computer_use snapshot_id `{requested_snapshot_id}` does not match latest snapshot `{}`; call snapshot again or use the latest id",
                record.snapshot_id
            );
        }
    }
    if record.max_items != max_items || record.max_depth != max_depth {
        bail!(
            "computer_use snapshot bounds changed for latest snapshot `{}`; latest max_items={}, max_depth={}, requested max_items={}, max_depth={}. call snapshot/find/inspect_ref/wait/wait_ref again with the requested bounds or reuse the latest bounds",
            record.snapshot_id,
            record.max_items,
            record.max_depth,
            max_items,
            max_depth
        );
    }
    let now_unix = current_unix_seconds();
    let snapshot_age_seconds = now_unix.saturating_sub(record.captured_at_unix);
    if snapshot_age_seconds > MAX_SNAPSHOT_RECORD_AGE_SECONDS {
        bail!(
            "computer_use snapshot `{}` is too old (age={}s, max={}s); call snapshot/find/inspect_ref/wait/wait_ref again before a write action",
            record.snapshot_id,
            snapshot_age_seconds,
            MAX_SNAPSHOT_RECORD_AGE_SECONDS
        );
    }
    Ok(record)
}

fn current_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn load_snapshot_record(ctx: &ToolContext) -> Result<Option<ComputerUseSnapshotRecord>> {
    let path = snapshot_record_path(ctx);
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let record = serde_json::from_slice(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(record))
}

fn snapshot_record_path(ctx: &ToolContext) -> PathBuf {
    ctx.data_dir
        .join("runtime")
        .join("computer-use")
        .join(format!(
            "latest-{}.json",
            &sha256_hex(ctx.current_session_id.as_bytes())[..16]
        ))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn normalize_wait_mode(wait_until: &str) -> String {
    wait_until
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_")
}

async fn wait_for_frontmost_app(
    request: &ComputerUseWaitRequest,
    max_items: usize,
    max_depth: usize,
) -> Result<ComputerUseWaitOutcome> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(request.timeout_seconds);
    let poll_interval = Duration::from_millis(request.poll_interval_ms);
    let mut attempts = 0usize;
    let mut previous_snapshot_sha256: Option<String> = None;

    loop {
        let snapshot = frontmost_app_snapshot(max_items, max_depth)?;
        attempts += 1;

        let matched = match &request.mode {
            ComputerUseWaitMode::TextPresent {
                contains_text,
                case_sensitive,
            } => snapshot_contains_text(&snapshot, contains_text, *case_sensitive),
            ComputerUseWaitMode::TextAbsent {
                contains_text,
                case_sensitive,
            } => !snapshot_contains_text(&snapshot, contains_text, *case_sensitive),
            ComputerUseWaitMode::Settled => {
                let current_sha256 = sha256_hex(snapshot.as_bytes());
                let settled = previous_snapshot_sha256
                    .as_deref()
                    .is_some_and(|previous| previous == current_sha256);
                previous_snapshot_sha256 = Some(current_sha256);
                settled
            }
        };
        if matched {
            let result = match &request.mode {
                ComputerUseWaitMode::TextPresent { .. } => "matched",
                ComputerUseWaitMode::TextAbsent { .. } => "matched",
                ComputerUseWaitMode::Settled => "settled",
            };
            return Ok(ComputerUseWaitOutcome {
                result,
                snapshot,
                attempts,
                elapsed_ms: started.elapsed().as_millis(),
            });
        }

        let now = Instant::now();
        if now >= deadline {
            return Ok(ComputerUseWaitOutcome {
                result: "timed_out",
                snapshot,
                attempts,
                elapsed_ms: started.elapsed().as_millis(),
            });
        }
        let remaining = deadline.saturating_duration_since(now);
        sleep(std::cmp::min(poll_interval, remaining)).await;
    }
}

async fn wait_for_frontmost_app_match(
    request: &ComputerUseWaitAppRequest,
    max_items: usize,
    max_depth: usize,
) -> Result<ComputerUseWaitOutcome> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(request.timeout_seconds);
    let poll_interval = Duration::from_millis(request.poll_interval_ms);
    let mut attempts = 0usize;

    loop {
        let snapshot = frontmost_app_snapshot(max_items, max_depth)?;
        attempts += 1;

        if snapshot_matches_app_guard(&snapshot, &request.guard) {
            return Ok(ComputerUseWaitOutcome {
                result: "matched",
                snapshot,
                attempts,
                elapsed_ms: started.elapsed().as_millis(),
            });
        }

        let now = Instant::now();
        if now >= deadline {
            return Ok(ComputerUseWaitOutcome {
                result: "timed_out",
                snapshot,
                attempts,
                elapsed_ms: started.elapsed().as_millis(),
            });
        }
        let remaining = deadline.saturating_duration_since(now);
        sleep(std::cmp::min(poll_interval, remaining)).await;
    }
}

async fn wait_for_frontmost_app_ref(
    reference: &str,
    request: &ComputerUseWaitRefRequest,
    max_items: usize,
    max_depth: usize,
) -> Result<ComputerUseWaitRefOutcome> {
    let started = Instant::now();
    let deadline = started + Duration::from_secs(request.timeout_seconds);
    let poll_interval = Duration::from_millis(request.poll_interval_ms);
    let mut attempts = 0usize;
    let mut latest_details: Option<String> = None;
    let mut latest_error_sha256: Option<String> = None;

    loop {
        attempts += 1;
        match frontmost_app_ref_details(reference, max_items, max_depth) {
            Ok(details) => {
                if details_match_wait_ref(reference, &details, request) {
                    return Ok(ComputerUseWaitRefOutcome {
                        result: "matched",
                        details,
                        attempts,
                        elapsed_ms: started.elapsed().as_millis(),
                    });
                }
                latest_details = Some(details);
            }
            Err(error) => {
                latest_error_sha256 = Some(sha256_hex(format!("{error:#}").as_bytes()));
            }
        }

        let now = Instant::now();
        if now >= deadline {
            return Ok(ComputerUseWaitRefOutcome {
                result: "timed_out",
                details: latest_details.unwrap_or_else(|| {
                    render_wait_ref_unavailable(reference, latest_error_sha256.as_deref())
                }),
                attempts,
                elapsed_ms: started.elapsed().as_millis(),
            });
        }
        let remaining = deadline.saturating_duration_since(now);
        sleep(std::cmp::min(poll_interval, remaining)).await;
    }
}

fn snapshot_contains_text(snapshot: &str, contains_text: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        snapshot.contains(contains_text)
    } else {
        snapshot
            .to_lowercase()
            .contains(&contains_text.to_lowercase())
    }
}

fn details_match_wait_ref(
    reference: &str,
    details: &str,
    request: &ComputerUseWaitRefRequest,
) -> bool {
    if let Some(guard) = &request.guard {
        let Some(line) = ref_line_from_details(details) else {
            return false;
        };
        if check_ref_guard_line(reference, &line, guard).is_err() {
            return false;
        }
    }

    if let Some(native_action) = request.native_action
        && !details_have_native_action(details, native_action)
    {
        return false;
    }

    true
}

fn ref_line_from_details(details: &str) -> Option<String> {
    details.lines().find_map(|line| {
        line.trim_start()
            .strip_prefix("ref_line: ")
            .map(|line| line.trim_end().to_string())
    })
}

fn details_have_native_action(details: &str, expected: ComputerUseNativeAction) -> bool {
    let Some(actions) = details.lines().find_map(|line| {
        line.trim_start()
            .strip_prefix("available_actions:")
            .map(str::trim)
    }) else {
        return false;
    };
    actions
        .split(',')
        .map(str::trim)
        .any(|action| action == expected.ax_action)
}

fn render_wait_ref_unavailable(reference: &str, latest_error_sha256: Option<&str>) -> String {
    let error_sha256 = latest_error_sha256.unwrap_or("none");
    format!(
        "ref: {}\nwait_ref_last_error: ref_details_unavailable\nwait_ref_last_error_sha256: {}",
        reference.trim(),
        error_sha256
    )
}

fn find_snapshot_lines(
    snapshot: &str,
    request: &ComputerUseFindRequest,
    max_items: usize,
    max_depth: usize,
) -> Result<ComputerUseFindOutcome> {
    let mut matches = Vec::new();
    let mut truncated = false;

    for line in snapshot.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("- @u") || !matches_find_request(trimmed, request) {
            continue;
        }
        if let Some(native_action) = request.native_action
            && !snapshot_line_ref_has_native_action(trimmed, native_action, max_items, max_depth)?
        {
            continue;
        }
        if matches.len() >= request.max_results {
            truncated = true;
            break;
        }
        matches.push(line.trim_end().to_string());
    }

    Ok(ComputerUseFindOutcome { matches, truncated })
}

fn matches_find_request(line: &str, request: &ComputerUseFindRequest) -> bool {
    if let Some(query) = &request.query
        && !contains_match(line, query, request.case_sensitive)
    {
        return false;
    }
    if let Some(role) = &request.role
        && !role_matches(line, role)
    {
        return false;
    }
    if let Some(state) = request.state
        && !state_matches(line, state)
    {
        return false;
    }
    true
}

fn snapshot_line_ref_has_native_action(
    line: &str,
    native_action: ComputerUseNativeAction,
    max_items: usize,
    max_depth: usize,
) -> Result<bool> {
    let Some(reference) = ui_ref_from_snapshot_line(line) else {
        return Ok(false);
    };
    parse_ui_ref(&reference)?;
    let details = match frontmost_app_ref_details(&reference, max_items, max_depth) {
        Ok(details) => details,
        Err(_) => return Ok(false),
    };
    Ok(details_have_native_action(&details, native_action))
}

fn ui_ref_from_snapshot_line(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("- @u")?;
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    Some(format!("@u{digits}"))
}

fn contains_match(value: &str, query: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        value.contains(query)
    } else {
        value.to_lowercase().contains(&query.to_lowercase())
    }
}

fn equals_match(value: &str, query: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        value == query
    } else {
        value.eq_ignore_ascii_case(query)
    }
}

fn role_matches(line: &str, expected_role: &str) -> bool {
    let Some(role) = quoted_field_value(line, "role") else {
        return false;
    };
    normalize_role(&role) == normalize_role(expected_role)
}

fn quoted_field_value(line: &str, field: &str) -> Option<String> {
    let prefix = format!("{field}='");
    let start = line.find(&prefix)? + prefix.len();
    let rest = &line[start..];
    let end = rest.find('\'')?;
    Some(rest[..end].to_string())
}

fn normalize_role(role: &str) -> String {
    role.trim()
        .to_lowercase()
        .replace(['_', '-'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn state_matches(line: &str, state: ComputerUseFindState) -> bool {
    match state {
        ComputerUseFindState::Focused => line.contains("focused=true"),
        ComputerUseFindState::Selected => line.contains("selected=true"),
        ComputerUseFindState::Disabled => line.contains("enabled=false"),
        ComputerUseFindState::Enabled => !line.contains("enabled=false"),
    }
}

fn render_find_matches(outcome: &ComputerUseFindOutcome) -> String {
    if outcome.matches.is_empty() {
        "matches:\n(no matches)".to_string()
    } else {
        format!("matches:\n{}", outcome.matches.join("\n"))
    }
}

fn run_app_guard(
    max_items: usize,
    max_depth: usize,
    request: Option<&ComputerUseAppGuardRequest>,
) -> Result<Option<ComputerUseAppGuardOutcome>> {
    let Some(request) = request else {
        return Ok(None);
    };
    let snapshot = frontmost_app_snapshot(max_items, max_depth)?;
    check_app_guard_snapshot(&snapshot, request).map(Some)
}

fn run_snapshot_origin_guard(
    record: &ComputerUseSnapshotRecord,
    max_items: usize,
    max_depth: usize,
) -> Result<ComputerUseSnapshotOriginGuardOutcome> {
    let snapshot = frontmost_app_snapshot(max_items, max_depth)?;
    check_snapshot_origin_guard(record, &snapshot)
}

fn check_snapshot_origin_guard(
    record: &ComputerUseSnapshotRecord,
    snapshot: &str,
) -> Result<ComputerUseSnapshotOriginGuardOutcome> {
    let expected_app_line_sha256 = record.frontmost_app_line_sha256.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "computer_use snapshot `{}` lacks frontmost-app origin metadata; call snapshot/find/inspect_ref/wait/wait_app/wait_ref again before a write action",
            record.snapshot_id
        )
    })?;
    let app_line = snapshot_frontmost_app_line(snapshot).ok_or_else(|| {
        anyhow::anyhow!(
            "computer_use snapshot origin guard could not read the current frontmost app (snapshot_sha256: {}). call snapshot again",
            sha256_hex(snapshot.as_bytes())
        )
    })?;
    let current_app_line_sha256 = sha256_hex(app_line.as_bytes());
    if current_app_line_sha256 != expected_app_line_sha256 {
        bail!(
            "computer_use snapshot origin guard failed for `{}`: frontmost app changed since observation (current_app_line_sha256: {}, expected_app_line_sha256: {}). call snapshot/find/inspect_ref/wait/wait_app/wait_ref again",
            record.snapshot_id,
            current_app_line_sha256,
            expected_app_line_sha256
        );
    }

    let current_pid = snapshot_pid(snapshot);
    if let Some(expected_pid) = record.pid
        && current_pid != Some(expected_pid)
    {
        bail!(
            "computer_use snapshot origin guard failed for `{}`: frontmost pid changed since observation (current_pid: {}, expected_pid: {}). call snapshot/find/inspect_ref/wait/wait_app/wait_ref again",
            record.snapshot_id,
            current_pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            expected_pid
        );
    }

    Ok(ComputerUseSnapshotOriginGuardOutcome {
        app_line_sha256: current_app_line_sha256,
        pid: current_pid,
        snapshot_sha256: sha256_hex(snapshot.as_bytes()),
    })
}

fn snapshot_matches_app_guard(snapshot: &str, request: &ComputerUseAppGuardRequest) -> bool {
    check_app_guard_snapshot(snapshot, request).is_ok()
}

fn check_app_guard_snapshot(
    snapshot: &str,
    request: &ComputerUseAppGuardRequest,
) -> Result<ComputerUseAppGuardOutcome> {
    let app_line = snapshot_frontmost_app_line(snapshot).ok_or_else(|| {
        anyhow::anyhow!(
            "computer_use app guard could not read the current frontmost app (snapshot_sha256: {}). call snapshot again",
            sha256_hex(snapshot.as_bytes())
        )
    })?;
    let app_name = app_line
        .trim_start()
        .strip_prefix("frontmost_app:")
        .map(str::trim)
        .unwrap_or("");
    let app_line_sha256 = sha256_hex(app_line.as_bytes());
    if let Some(expected_app) = &request.app
        && !equals_match(app_name, expected_app, request.case_sensitive)
    {
        bail!(
            "computer_use app guard failed: expected frontmost app did not match (app_line_sha256: {app_line_sha256}). call snapshot again"
        );
    }

    let current_pid = snapshot_pid(snapshot);
    if let Some(expected_pid) = request.pid
        && current_pid != Some(expected_pid)
    {
        bail!(
            "computer_use app guard failed: expected frontmost pid `{expected_pid}` did not match (app_line_sha256: {app_line_sha256}). call snapshot again"
        );
    }

    Ok(ComputerUseAppGuardOutcome {
        app_line_sha256,
        pid: current_pid,
        snapshot_sha256: sha256_hex(snapshot.as_bytes()),
    })
}

fn snapshot_frontmost_app_line(snapshot: &str) -> Option<&str> {
    snapshot
        .lines()
        .find(|line| line.trim_start().starts_with("frontmost_app:"))
}

fn snapshot_pid(snapshot: &str) -> Option<u32> {
    snapshot.lines().find_map(|line| {
        line.trim_start()
            .strip_prefix("pid:")
            .map(str::trim)
            .and_then(|pid| pid.parse::<u32>().ok())
    })
}

fn run_ref_guard(
    reference: &str,
    max_items: usize,
    max_depth: usize,
    request: Option<&ComputerUseRefGuardRequest>,
) -> Result<Option<ComputerUseRefGuardOutcome>> {
    let Some(request) = request else {
        return Ok(None);
    };
    let snapshot = frontmost_app_snapshot(max_items, max_depth)?;
    let line = snapshot_line_for_ref(&snapshot, reference)?;
    check_ref_guard_line(reference, &line, request)?;
    Ok(Some(ComputerUseRefGuardOutcome {
        reference: reference.to_string(),
        line_sha256: sha256_hex(line.as_bytes()),
    }))
}

fn run_native_action_guard(
    reference: &str,
    max_items: usize,
    max_depth: usize,
    native_action: ComputerUseNativeAction,
) -> Result<ComputerUseNativeActionGuardOutcome> {
    parse_ui_ref(reference)?;
    let details = frontmost_app_ref_details(reference, max_items, max_depth).map_err(|error| {
        anyhow::anyhow!(
            "computer_use native action guard could not inspect {reference} before perform_action (error_sha256: {}). call inspect_ref or wait_ref again",
            sha256_hex(format!("{error:#}").as_bytes())
        )
    })?;
    check_native_action_guard_details(reference, &details, native_action)
}

fn check_native_action_guard_details(
    reference: &str,
    details: &str,
    native_action: ComputerUseNativeAction,
) -> Result<ComputerUseNativeActionGuardOutcome> {
    let details_sha256 = sha256_hex(details.as_bytes());
    if !details_have_native_action(details, native_action) {
        bail!(
            "computer_use native action guard failed for {reference}: expected native action `{}` was not reported by the current ref (details_sha256: {details_sha256}). call inspect_ref or wait_ref again",
            native_action.ax_action
        );
    }
    Ok(ComputerUseNativeActionGuardOutcome {
        reference: reference.to_string(),
        native_action: native_action.ax_action,
        details_sha256,
    })
}

fn snapshot_line_for_ref(snapshot: &str, reference: &str) -> Result<String> {
    let target_index = parse_ui_ref(reference)?;
    let prefix = format!("- @u{target_index}");
    for line in snapshot.lines() {
        let trimmed = line.trim_start();
        if trimmed == prefix
            || trimmed
                .strip_prefix(&prefix)
                .is_some_and(|rest| rest.starts_with(' '))
        {
            return Ok(line.trim_end().to_string());
        }
    }
    bail!(
        "computer_use ref guard could not find {reference} in the current snapshot; call snapshot or find again"
    )
}

fn check_ref_guard_line(
    reference: &str,
    line: &str,
    request: &ComputerUseRefGuardRequest,
) -> Result<()> {
    let line_sha256 = sha256_hex(line.as_bytes());
    if let Some(role) = &request.role
        && !role_matches(line, role)
    {
        bail!(
            "computer_use ref guard failed for {reference}: expected role `{role}`; current ref line did not match (line_sha256: {line_sha256}). call snapshot or find again"
        );
    }
    if let Some(text) = &request.text
        && !contains_match(line, text, request.case_sensitive)
    {
        bail!(
            "computer_use ref guard failed for {reference}: expected text was not present (line_sha256: {line_sha256}). call snapshot or find again"
        );
    }
    if let Some(state) = request.state
        && !state_matches(line, state)
    {
        bail!(
            "computer_use ref guard failed for {reference}: expected state `{}`; current ref line did not match (line_sha256: {line_sha256}). call snapshot or find again",
            state.label()
        );
    }
    Ok(())
}

fn render_write_result(
    snapshot_id: &str,
    post_snapshot_record: &ComputerUseSnapshotRecord,
    snapshot_origin_guard: &ComputerUseSnapshotOriginGuardOutcome,
    ref_guard: Option<&ComputerUseRefGuardOutcome>,
    app_guard: Option<&ComputerUseAppGuardOutcome>,
    native_action_guard: Option<&ComputerUseNativeActionGuardOutcome>,
    result: &str,
) -> String {
    let mut output = format!(
        "using_snapshot_id: {snapshot_id}\n{}",
        render_snapshot_record_metadata(post_snapshot_record, "post_snapshot")
    );
    output.push_str(&format!(
        "snapshot_origin_guard: passed\nsnapshot_origin_guard_app_line_sha256: {}\nsnapshot_origin_guard_pid: {}\nsnapshot_origin_guard_snapshot_sha256: {}\n",
        snapshot_origin_guard.app_line_sha256,
        snapshot_origin_guard
            .pid
            .map(|pid| pid.to_string())
            .unwrap_or_else(|| "unknown".to_string()),
        snapshot_origin_guard.snapshot_sha256
    ));
    if let Some(ref_guard) = ref_guard {
        output.push_str(&format!(
            "ref_guard: passed\nref_guard_ref: {}\nref_guard_line_sha256: {}\n",
            ref_guard.reference, ref_guard.line_sha256
        ));
    }
    if let Some(app_guard) = app_guard {
        output.push_str(&format!(
            "app_guard: passed\napp_guard_app_line_sha256: {}\napp_guard_pid: {}\napp_guard_snapshot_sha256: {}\n",
            app_guard.app_line_sha256,
            app_guard
                .pid
                .map(|pid| pid.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            app_guard.snapshot_sha256
        ));
    }
    if let Some(native_action_guard) = native_action_guard {
        output.push_str(&format!(
            "native_action_guard: passed\nnative_action_guard_ref: {}\nnative_action_guard_action: {}\nnative_action_guard_details_sha256: {}\n",
            native_action_guard.reference,
            native_action_guard.native_action,
            native_action_guard.details_sha256
        ));
    }
    output.push_str(&render_status(false));
    output.push_str("\n\n");
    output.push_str(result.trim());
    output
}

fn render_status(prompt: bool) -> String {
    let status = inspect_computer_use(prompt);
    format!(
        "platform: {}\naccessibility_supported: {}\npermission_prompt_supported: {}\naccessibility_trusted: {}\nprompt_requested: {}\nguidance: {}",
        status.platform,
        status.accessibility_supported,
        status.permission_prompt_supported,
        status.accessibility_trusted,
        status.prompt_requested,
        status.guidance
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ComputerUseAppGuardRequest, ComputerUseFindRequest, ComputerUseFindState,
        ComputerUseRefGuardRequest, ComputerUseTool, ComputerUseWaitMode,
        ComputerUseWaitRefRequest, MAX_SET_TEXT_CHARS, MAX_SNAPSHOT_RECORD_AGE_SECONDS,
        check_app_guard_snapshot, check_native_action_guard_details, check_ref_guard_line,
        check_snapshot_origin_guard, details_have_native_action, details_match_wait_ref,
        find_snapshot_lines, load_snapshot_record, post_action_snapshot_from_result,
        ref_line_from_details, render_snapshot_record_metadata, render_wait_ref_unavailable,
        render_write_result, resolve_snapshot_record, save_post_action_snapshot_record,
        save_snapshot_record, snapshot_contains_text, snapshot_frontmost_app_line,
        snapshot_line_for_ref, snapshot_matches_app_guard, snapshot_pid, snapshot_record_path,
        ui_ref_from_snapshot_line,
    };
    use crate::computer_use::normalize_computer_use_native_action;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;

    fn ctx(root: &std::path::Path) -> ToolContext {
        ToolContext {
            workspace_root: root.to_path_buf(),
            data_dir: root.join(".data"),
            shell_enabled: false,
            skill_platform: "cli".to_string(),
            provider_id: "openai".to_string(),
            model: "test-model".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            api_key: None,
            api_mode: crate::llm::ApiMode::ChatCompletions,
            worker_model: None,
            max_iterations: 4,
            current_session_id: "computer-use-session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        }
    }

    #[tokio::test]
    async fn status_reports_permission_state() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let output = tool
            .execute(json!({ "action": "status" }), &ctx(tmp.path()))
            .await
            .expect("status");

        assert!(output.contains("accessibility_supported:"));
        assert!(output.contains("accessibility_trusted:"));
        assert!(output.contains("guidance:"));
    }

    #[tokio::test]
    async fn unsupported_action_is_rejected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(json!({ "action": "drag" }), &ctx(tmp.path()))
            .await
            .expect_err("unsupported action");

        assert!(format!("{error:#}").contains("unsupported computer_use action"));
    }

    #[tokio::test]
    async fn click_requires_reference() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(json!({ "action": "click" }), &ctx(tmp.path()))
            .await
            .expect_err("missing ref");

        assert!(format!("{error:#}").contains("requires a non-empty"));
    }

    #[tokio::test]
    async fn focus_requires_reference() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(json!({ "action": "focus" }), &ctx(tmp.path()))
            .await
            .expect_err("missing ref");

        assert!(format!("{error:#}").contains("requires a non-empty"));
    }

    #[tokio::test]
    async fn set_text_requires_text() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(
                json!({ "action": "set_text", "ref": "@u1" }),
                &ctx(tmp.path()),
            )
            .await
            .expect_err("missing text");

        assert!(format!("{error:#}").contains("requires a `text` value"));
    }

    #[tokio::test]
    async fn set_text_rejects_oversized_text() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let oversized = "x".repeat(MAX_SET_TEXT_CHARS + 1);
        let error = tool
            .execute(
                json!({ "action": "set_text", "ref": "@u1", "text": oversized }),
                &ctx(tmp.path()),
            )
            .await
            .expect_err("oversized text");

        assert!(format!("{error:#}").contains("too long"));
    }

    #[tokio::test]
    async fn press_key_requires_key() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(json!({ "action": "press_key" }), &ctx(tmp.path()))
            .await
            .expect_err("missing key");

        assert!(format!("{error:#}").contains("requires a `key` value"));
    }

    #[tokio::test]
    async fn press_key_rejects_unsupported_key() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(
                json!({ "action": "press_key", "key": "a" }),
                &ctx(tmp.path()),
            )
            .await
            .expect_err("unsupported key");

        assert!(format!("{error:#}").contains("unsupported computer_use key"));
    }

    #[tokio::test]
    async fn perform_action_requires_native_action() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(
                json!({ "action": "perform_action", "ref": "@u1" }),
                &ctx(tmp.path()),
            )
            .await
            .expect_err("missing native action");

        assert!(format!("{error:#}").contains("requires a `native_action` value"));
    }

    #[tokio::test]
    async fn perform_action_rejects_unsupported_native_action() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(
                json!({ "action": "perform_action", "ref": "@u1", "native_action": "raise" }),
                &ctx(tmp.path()),
            )
            .await
            .expect_err("unsupported native action");

        assert!(format!("{error:#}").contains("unsupported computer_use native_action"));
    }

    #[tokio::test]
    async fn scroll_requires_direction() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(
                json!({ "action": "scroll", "ref": "@u1" }),
                &ctx(tmp.path()),
            )
            .await
            .expect_err("missing direction");

        assert!(format!("{error:#}").contains("requires a `direction` value"));
    }

    #[tokio::test]
    async fn scroll_rejects_unsupported_direction() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(
                json!({ "action": "scroll", "ref": "@u1", "direction": "diagonal" }),
                &ctx(tmp.path()),
            )
            .await
            .expect_err("unsupported direction");

        assert!(format!("{error:#}").contains("unsupported computer_use scroll direction"));
    }

    #[tokio::test]
    async fn wait_text_present_requires_contains_text() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(
                json!({ "action": "wait", "wait_until": "text_present" }),
                &ctx(tmp.path()),
            )
            .await
            .expect_err("missing contains_text");

        assert!(format!("{error:#}").contains("requires `contains_text`"));
    }

    #[tokio::test]
    async fn wait_text_absent_requires_contains_text() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(
                json!({ "action": "wait", "wait_until": "text_absent" }),
                &ctx(tmp.path()),
            )
            .await
            .expect_err("missing contains_text");

        assert!(format!("{error:#}").contains("requires `contains_text`"));
    }

    #[tokio::test]
    async fn wait_rejects_unsupported_mode() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(
                json!({ "action": "wait", "wait_until": "visible" }),
                &ctx(tmp.path()),
            )
            .await
            .expect_err("unsupported wait mode");

        assert!(format!("{error:#}").contains("unsupported computer_use wait_until"));
    }

    #[tokio::test]
    async fn find_requires_filter() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(json!({ "action": "find" }), &ctx(tmp.path()))
            .await
            .expect_err("missing filter");

        assert!(format!("{error:#}").contains("requires at least one"));
    }

    #[tokio::test]
    async fn inspect_ref_requires_reference() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(json!({ "action": "inspect_ref" }), &ctx(tmp.path()))
            .await
            .expect_err("missing ref");

        assert!(format!("{error:#}").contains("requires a non-empty"));
    }

    #[tokio::test]
    async fn wait_ref_requires_reference() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(json!({ "action": "wait_ref" }), &ctx(tmp.path()))
            .await
            .expect_err("missing ref");

        assert!(format!("{error:#}").contains("requires a non-empty"));
    }

    #[tokio::test]
    async fn wait_app_requires_app_or_pid_expectation() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(json!({ "action": "wait_app" }), &ctx(tmp.path()))
            .await
            .expect_err("missing app expectation");

        assert!(format!("{error:#}").contains("requires `expect_app` or `expect_pid`"));
    }

    #[test]
    fn wait_request_defaults_to_settled_and_clamps_bounds() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "wait",
            "timeout_seconds": 500,
            "poll_interval_ms": 1
        }))
        .expect("args");
        let request = args.wait_request().expect("wait request");

        assert_eq!(request.mode, ComputerUseWaitMode::Settled);
        assert_eq!(request.timeout_seconds, super::MAX_WAIT_TIMEOUT_SECONDS);
        assert_eq!(request.poll_interval_ms, 100);
    }

    #[test]
    fn wait_app_request_parses_expectations_and_clamps_bounds() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "wait_app",
            "expect_app": "Finder",
            "expect_pid": 42,
            "case_sensitive": true,
            "timeout_seconds": 500,
            "poll_interval_ms": 1
        }))
        .expect("args");
        let request = args.wait_app_request().expect("wait_app request");

        assert_eq!(request.guard.app.as_deref(), Some("Finder"));
        assert_eq!(request.guard.pid, Some(42));
        assert!(request.guard.case_sensitive);
        assert_eq!(request.timeout_seconds, super::MAX_WAIT_TIMEOUT_SECONDS);
        assert_eq!(request.poll_interval_ms, 100);
    }

    #[test]
    fn wait_request_infers_text_present_from_contains_text() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "wait",
            "contains_text": "Ready",
            "case_sensitive": true
        }))
        .expect("args");
        let request = args.wait_request().expect("wait request");

        assert_eq!(
            request.mode,
            ComputerUseWaitMode::TextPresent {
                contains_text: "Ready".to_string(),
                case_sensitive: true,
            }
        );
    }

    #[test]
    fn wait_request_parses_text_absent_aliases() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "wait",
            "wait_until": "text-gone",
            "contains_text": "Loading",
            "case_sensitive": true
        }))
        .expect("args");
        let request = args.wait_request().expect("wait request");

        assert_eq!(request.mode.label(), "text_absent");
        assert_eq!(
            request.mode,
            ComputerUseWaitMode::TextAbsent {
                contains_text: "Loading".to_string(),
                case_sensitive: true,
            }
        );
    }

    #[test]
    fn wait_ref_request_parses_expectations_and_clamps_bounds() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "wait_ref",
            "ref": "@u2",
            "expect_role": " button ",
            "expect_text": " Continue ",
            "expect_state": "available",
            "native_action": "AXPress",
            "timeout_seconds": 500,
            "poll_interval_ms": 1
        }))
        .expect("args");
        let request = args.wait_ref_request().expect("wait_ref request");
        let guard = request.guard.expect("guard");

        assert_eq!(guard.role.as_deref(), Some("button"));
        assert_eq!(guard.text.as_deref(), Some("Continue"));
        assert_eq!(guard.state, Some(ComputerUseFindState::Enabled));
        assert_eq!(
            request.native_action.expect("native action").ax_action,
            "AXPress"
        );
        assert_eq!(request.timeout_seconds, super::MAX_WAIT_TIMEOUT_SECONDS);
        assert_eq!(request.poll_interval_ms, 100);
    }

    #[test]
    fn wait_ref_request_allows_ref_existence_only() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "wait_ref",
            "ref": "@u2"
        }))
        .expect("args");
        let request = args.wait_ref_request().expect("wait_ref request");

        assert_eq!(request.guard, None);
        assert_eq!(request.native_action, None);
    }

    #[test]
    fn scroll_request_normalizes_direction_and_clamps_steps() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "scroll",
            "ref": "@u2",
            "direction": "scroll-down",
            "scroll_steps": 500
        }))
        .expect("args");
        let request = args.scroll_request().expect("scroll request");

        assert_eq!(request.direction.label, "down");
        assert_eq!(request.direction.ax_action, "AXScrollDown");
        assert_eq!(request.steps, super::MAX_SCROLL_STEPS);
    }

    #[test]
    fn native_action_request_normalizes_allowed_actions() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "perform_action",
            "ref": "@u2",
            "native_action": "AXShowMenu"
        }))
        .expect("args");
        let request = args.native_action_request().expect("native action");

        assert_eq!(request.native_action.label, "show_menu");
        assert_eq!(request.native_action.ax_action, "AXShowMenu");
    }

    #[test]
    fn find_request_clamps_results_and_normalizes_state() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "find",
            "query": "Continue",
            "state": "not-enabled",
            "max_results": 500
        }))
        .expect("args");
        let request = args.find_request().expect("find request");

        assert_eq!(request.query.as_deref(), Some("Continue"));
        assert_eq!(request.state, Some(ComputerUseFindState::Disabled));
        assert_eq!(request.native_action, None);
        assert_eq!(request.max_results, super::MAX_FIND_RESULTS);
    }

    #[test]
    fn find_request_accepts_native_action_only_filter() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "find",
            "native_action": "AXPress"
        }))
        .expect("args");
        let request = args.find_request().expect("find request");

        assert_eq!(request.query, None);
        assert_eq!(request.role, None);
        assert_eq!(request.state, None);
        assert_eq!(
            request.native_action.expect("native action").ax_action,
            "AXPress"
        );
    }

    #[test]
    fn find_request_rejects_unsupported_state() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "find",
            "state": "hidden"
        }))
        .expect("args");
        let error = args.find_request().expect_err("unsupported state");

        assert!(format!("{error:#}").contains("unsupported computer_use find state"));
    }

    #[test]
    fn find_request_rejects_unsupported_native_action() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "find",
            "native_action": "raise"
        }))
        .expect("args");
        let error = args.find_request().expect_err("unsupported native action");

        assert!(format!("{error:#}").contains("unsupported computer_use native_action"));
    }

    #[test]
    fn ref_guard_request_trims_and_normalizes_expectations() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "click",
            "ref": "@u2",
            "expect_role": " button ",
            "expect_text": " Continue ",
            "expect_state": "not-enabled",
            "case_sensitive": true
        }))
        .expect("args");
        let request = args
            .ref_guard_request()
            .expect("ref guard request")
            .expect("guard exists");

        assert_eq!(request.role.as_deref(), Some("button"));
        assert_eq!(request.text.as_deref(), Some("Continue"));
        assert_eq!(request.state, Some(ComputerUseFindState::Disabled));
        assert!(request.case_sensitive);
    }

    #[test]
    fn ref_guard_request_returns_none_without_expectations() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "click",
            "ref": "@u2"
        }))
        .expect("args");

        assert_eq!(args.ref_guard_request().expect("request"), None);
    }

    #[test]
    fn app_guard_request_trims_expectations() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "click",
            "ref": "@u2",
            "expect_app": " Finder ",
            "expect_pid": 42,
            "case_sensitive": true
        }))
        .expect("args");
        let request = args
            .app_guard_request()
            .expect("app guard request")
            .expect("guard exists");

        assert_eq!(request.app.as_deref(), Some("Finder"));
        assert_eq!(request.pid, Some(42));
        assert!(request.case_sensitive);
    }

    #[test]
    fn app_guard_request_returns_none_without_expectations() {
        let args: super::ComputerUseArgs = serde_json::from_value(json!({
            "action": "press_key",
            "key": "enter"
        }))
        .expect("args");

        assert_eq!(args.app_guard_request().expect("request"), None);
    }

    #[test]
    fn snapshot_text_matching_can_ignore_case() {
        let snapshot = "frontmost_app: Notes\n- @u1 role='button' name='Continue'";

        assert!(snapshot_contains_text(snapshot, "continue", false));
        assert!(!snapshot_contains_text(snapshot, "continue", true));
    }

    #[test]
    fn find_snapshot_lines_matches_query_role_and_state() {
        let snapshot = r#"
frontmost_app: Notes
pid: 123
ui_tree:
- @u1 role='window' name='Notes' bounds=(0,0,900x700)
  - @u2 role='button' name='Continue' bounds=(20,20,80x28)
  - @u3 role='button' name='Continue' bounds=(20,58,80x28) enabled=false
  - @u4 role='text field' value='Continue draft' bounds=(20,96,240x28) focused=true
"#;
        let request = ComputerUseFindRequest {
            query: Some("continue".to_string()),
            role: Some("button".to_string()),
            state: Some(ComputerUseFindState::Disabled),
            native_action: None,
            case_sensitive: false,
            max_results: 12,
        };
        let outcome = find_snapshot_lines(snapshot, &request, 40, 3).expect("find lines");

        assert_eq!(outcome.matches.len(), 1);
        assert!(outcome.matches[0].contains("@u3"));
        assert!(!outcome.truncated);
    }

    #[test]
    fn find_snapshot_lines_respects_case_sensitive_query() {
        let snapshot = "frontmost_app: Notes\n- @u1 role='button' name='Continue'";
        let case_insensitive = ComputerUseFindRequest {
            query: Some("continue".to_string()),
            role: None,
            state: None,
            native_action: None,
            case_sensitive: false,
            max_results: 12,
        };
        let case_sensitive = ComputerUseFindRequest {
            case_sensitive: true,
            ..case_insensitive.clone()
        };

        assert_eq!(
            find_snapshot_lines(snapshot, &case_insensitive, 40, 3)
                .expect("find lines")
                .matches
                .len(),
            1
        );
        assert!(
            find_snapshot_lines(snapshot, &case_sensitive, 40, 3)
                .expect("find lines")
                .matches
                .is_empty()
        );
    }

    #[test]
    fn find_snapshot_lines_supports_role_aliases_and_enabled_state() {
        let snapshot = r#"
frontmost_app: App
ui_tree:
- @u1 role='text field' name='Search'
- @u2 role='text-field' name='Disabled search' enabled=false
"#;
        let request = ComputerUseFindRequest {
            query: None,
            role: Some("text_field".to_string()),
            state: Some(ComputerUseFindState::Enabled),
            native_action: None,
            case_sensitive: false,
            max_results: 12,
        };
        let outcome = find_snapshot_lines(snapshot, &request, 40, 3).expect("find lines");

        assert_eq!(outcome.matches.len(), 1);
        assert!(outcome.matches[0].contains("@u1"));
    }

    #[test]
    fn find_snapshot_lines_reports_truncation() {
        let snapshot = r#"
frontmost_app: App
ui_tree:
- @u1 role='button' name='A'
- @u2 role='button' name='B'
- @u3 role='button' name='C'
"#;
        let request = ComputerUseFindRequest {
            query: None,
            role: Some("button".to_string()),
            state: None,
            native_action: None,
            case_sensitive: false,
            max_results: 2,
        };
        let outcome = find_snapshot_lines(snapshot, &request, 40, 3).expect("find lines");

        assert_eq!(outcome.matches.len(), 2);
        assert!(outcome.truncated);
    }

    #[test]
    fn ui_ref_from_snapshot_line_extracts_exact_ref() {
        assert_eq!(
            ui_ref_from_snapshot_line("  - @u10 role='button' name='Ten'").as_deref(),
            Some("@u10")
        );
        assert_eq!(ui_ref_from_snapshot_line("- @u role='button'"), None);
        assert_eq!(ui_ref_from_snapshot_line("- @e1 role='button'"), None);
    }

    #[test]
    fn snapshot_line_for_ref_matches_exact_ref_index() {
        let snapshot = r#"
frontmost_app: App
ui_tree:
- @u1 role='button' name='One'
- @u10 role='button' name='Ten'
"#;

        assert!(
            snapshot_line_for_ref(snapshot, "@u1")
                .expect("line")
                .contains("name='One'")
        );
        assert!(
            snapshot_line_for_ref(snapshot, "@u10")
                .expect("line")
                .contains("name='Ten'")
        );
    }

    #[test]
    fn ref_guard_line_matches_role_text_and_state() {
        let line = "- @u3 role='button' name='Continue' enabled=false";
        let request = ComputerUseRefGuardRequest {
            role: Some("button".to_string()),
            text: Some("continue".to_string()),
            state: Some(ComputerUseFindState::Disabled),
            case_sensitive: false,
        };

        check_ref_guard_line("@u3", line, &request).expect("guard should pass");
    }

    #[test]
    fn ref_guard_line_rejects_mismatch_without_echoing_ui_line() {
        let line = "- @u3 role='button' name='Private Project Name'";
        let request = ComputerUseRefGuardRequest {
            role: Some("text field".to_string()),
            text: None,
            state: None,
            case_sensitive: false,
        };
        let error = check_ref_guard_line("@u3", line, &request).expect_err("guard mismatch");
        let error = format!("{error:#}");

        assert!(error.contains("ref guard failed"));
        assert!(error.contains("line_sha256:"));
        assert!(!error.contains("Private Project Name"));
    }

    #[test]
    fn app_guard_snapshot_matches_app_and_pid() {
        let snapshot = "frontmost_app: Finder\npid: 42\nui_tree:\n- @u1 role='window'";
        let request = ComputerUseAppGuardRequest {
            app: Some("finder".to_string()),
            pid: Some(42),
            case_sensitive: false,
        };
        let outcome = check_app_guard_snapshot(snapshot, &request).expect("app guard");

        assert_eq!(
            snapshot_frontmost_app_line(snapshot).expect("app line"),
            "frontmost_app: Finder"
        );
        assert_eq!(snapshot_pid(snapshot), Some(42));
        assert_eq!(
            outcome.app_line_sha256,
            super::sha256_hex("frontmost_app: Finder".as_bytes())
        );
        assert_eq!(outcome.pid, Some(42));
        assert_eq!(
            outcome.snapshot_sha256,
            super::sha256_hex(snapshot.as_bytes())
        );
    }

    #[test]
    fn wait_app_match_uses_app_guard_rules() {
        let snapshot = "frontmost_app: Finder\npid: 42\nui_tree:\n- @u1 role='window'";
        let matching = ComputerUseAppGuardRequest {
            app: Some("finder".to_string()),
            pid: Some(42),
            case_sensitive: false,
        };
        let mismatched = ComputerUseAppGuardRequest {
            app: Some("Finder".to_string()),
            pid: Some(43),
            case_sensitive: true,
        };

        assert!(snapshot_matches_app_guard(snapshot, &matching));
        assert!(!snapshot_matches_app_guard(snapshot, &mismatched));
    }

    #[test]
    fn app_guard_snapshot_rejects_mismatch_without_echoing_current_app() {
        let snapshot = "frontmost_app: Private Customer Console\npid: 42";
        let request = ComputerUseAppGuardRequest {
            app: Some("Finder".to_string()),
            pid: None,
            case_sensitive: false,
        };
        let error = check_app_guard_snapshot(snapshot, &request).expect_err("app mismatch");
        let error = format!("{error:#}");

        assert!(error.contains("app guard failed"));
        assert!(error.contains("app_line_sha256:"));
        assert!(!error.contains("Private Customer Console"));
    }

    #[test]
    fn wait_ref_details_match_guard_and_native_action() {
        let details = r#"
ref: @u3
ref_line: - @u3 role='button' name='Continue' bounds=(20,20,80x28)
available_actions: AXPress, AXShowMenu
"#;
        let request = ComputerUseWaitRefRequest {
            guard: Some(ComputerUseRefGuardRequest {
                role: Some("button".to_string()),
                text: Some("continue".to_string()),
                state: Some(ComputerUseFindState::Enabled),
                case_sensitive: false,
            }),
            native_action: Some(normalize_computer_use_native_action("press").expect("action")),
            timeout_seconds: 10,
            poll_interval_ms: 250,
        };

        assert_eq!(
            ref_line_from_details(details).expect("ref line"),
            "- @u3 role='button' name='Continue' bounds=(20,20,80x28)"
        );
        assert!(details_match_wait_ref("@u3", details, &request));
    }

    #[test]
    fn wait_ref_details_rejects_missing_native_action() {
        let details = r#"
ref: @u3
ref_line: - @u3 role='button' name='Continue' bounds=(20,20,80x28)
available_actions: AXShowMenu
"#;
        let action = normalize_computer_use_native_action("press").expect("action");
        let request = ComputerUseWaitRefRequest {
            guard: None,
            native_action: Some(action),
            timeout_seconds: 10,
            poll_interval_ms: 250,
        };

        assert!(!details_have_native_action(details, action));
        assert!(!details_match_wait_ref("@u3", details, &request));
    }

    #[test]
    fn native_action_guard_details_passes_and_hashes_details() {
        let details = r#"
ref: @u3
ref_line: - @u3 role='button' name='Private Continue' bounds=(20,20,80x28)
available_actions: AXPress, AXShowMenu
"#;
        let action = normalize_computer_use_native_action("press").expect("action");
        let guard =
            check_native_action_guard_details("@u3", details, action).expect("native guard");

        assert_eq!(guard.reference, "@u3");
        assert_eq!(guard.native_action, "AXPress");
        assert_eq!(guard.details_sha256, super::sha256_hex(details.as_bytes()));
    }

    #[test]
    fn native_action_guard_rejects_missing_action_without_echoing_details() {
        let details = r#"
ref: @u3
ref_line: - @u3 role='button' name='Private Project Name' bounds=(20,20,80x28)
available_actions: AXShowMenu
"#;
        let action = normalize_computer_use_native_action("press").expect("action");
        let error = check_native_action_guard_details("@u3", details, action)
            .expect_err("missing native action");
        let error = format!("{error:#}");

        assert!(error.contains("native action guard failed"));
        assert!(error.contains("details_sha256:"));
        assert!(!error.contains("Private Project Name"));
    }

    #[test]
    fn render_write_result_includes_post_snapshot_and_guard_evidence() {
        let details = r#"
ref: @u3
ref_line: - @u3 role='button' name='Continue' bounds=(20,20,80x28)
available_actions: AXPress
"#;
        let action = normalize_computer_use_native_action("press").expect("action");
        let guard =
            check_native_action_guard_details("@u3", details, action).expect("native guard");
        let app_snapshot = "frontmost_app: Finder\npid: 42";
        let app_guard = check_app_guard_snapshot(
            app_snapshot,
            &ComputerUseAppGuardRequest {
                app: Some("Finder".to_string()),
                pid: Some(42),
                case_sensitive: true,
            },
        )
        .expect("app guard");
        let tmp = tempfile::tempdir().expect("tempdir");
        let before_record =
            save_snapshot_record(&ctx(tmp.path()), 40, 3, app_snapshot).expect("save record");
        let post_snapshot = "frontmost_app: Finder\npid: 42\nui_tree:\n- @u1 role='window'";
        let post_record =
            save_snapshot_record(&ctx(tmp.path()), 40, 3, post_snapshot).expect("save post record");
        let origin_guard =
            check_snapshot_origin_guard(&before_record, app_snapshot).expect("origin guard");
        let rendered = render_write_result(
            "cu_test_before",
            &post_record,
            &origin_guard,
            None,
            Some(&app_guard),
            Some(&guard),
            "frontmost_app: TestApp",
        );

        assert!(rendered.contains("using_snapshot_id: cu_test_before"));
        assert!(rendered.contains(&format!("post_snapshot_id: {}", post_record.snapshot_id)));
        assert!(rendered.contains("post_snapshot_max_items: 40"));
        assert!(rendered.contains("post_snapshot_max_depth: 3"));
        assert!(rendered.contains(&format!(
            "post_snapshot_sha256: {}",
            post_record.output_sha256
        )));
        assert!(rendered.contains("post_snapshot_app_line_sha256:"));
        assert!(rendered.contains("post_snapshot_pid: 42"));
        assert!(rendered.contains("snapshot_origin_guard: passed"));
        assert!(rendered.contains("snapshot_origin_guard_app_line_sha256:"));
        assert!(rendered.contains("snapshot_origin_guard_pid: 42"));
        assert!(rendered.contains("snapshot_origin_guard_snapshot_sha256:"));
        assert!(rendered.contains("app_guard: passed"));
        assert!(rendered.contains("app_guard_app_line_sha256:"));
        assert!(rendered.contains("app_guard_pid: 42"));
        assert!(rendered.contains("app_guard_snapshot_sha256:"));
        assert!(rendered.contains("native_action_guard: passed"));
        assert!(rendered.contains("native_action_guard_ref: @u3"));
        assert!(rendered.contains("native_action_guard_action: AXPress"));
        assert!(rendered.contains("native_action_guard_details_sha256:"));
    }

    #[test]
    fn wait_ref_unavailable_output_does_not_echo_raw_error() {
        let raw_error = "Private Window Title could not be inspected";
        let rendered =
            render_wait_ref_unavailable("@u8", Some(&super::sha256_hex(raw_error.as_bytes())));

        assert!(rendered.contains("wait_ref_last_error_sha256:"));
        assert!(!rendered.contains("Private Window Title"));
    }

    #[test]
    fn snapshot_record_metadata_renders_non_sensitive_evidence() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ctx(tmp.path());
        let record = save_snapshot_record(
            &ctx,
            12,
            2,
            "frontmost_app: Private App\npid: 123\nui_tree:\n- @u1 role='window'",
        )
        .expect("save snapshot record");

        let rendered = render_snapshot_record_metadata(&record, "snapshot");

        assert!(rendered.contains(&format!("snapshot_id: {}", record.snapshot_id)));
        assert!(rendered.contains("snapshot_max_items: 12"));
        assert!(rendered.contains("snapshot_max_depth: 2"));
        assert!(rendered.contains(&format!("snapshot_sha256: {}", record.output_sha256)));
        assert!(rendered.contains("snapshot_app_line_sha256:"));
        assert!(rendered.contains("snapshot_pid: 123"));
        assert!(!rendered.contains("Private App"));
    }

    #[test]
    fn snapshot_record_metadata_uses_unknown_for_missing_origin() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ctx(tmp.path());
        let record = save_snapshot_record(&ctx, 12, 2, "ref: @u3\nwait_ref_last_error_sha256: abc")
            .expect("save snapshot record");

        let rendered = render_snapshot_record_metadata(&record, "snapshot");

        assert!(rendered.contains("snapshot_app_line_sha256: unknown"));
        assert!(rendered.contains("snapshot_pid: unknown"));
    }

    #[test]
    fn definition_exposes_snapshot_bounds() {
        let tool = ComputerUseTool;
        let definition = tool.definition();
        let schema = serde_json::to_string(&definition.function.parameters).expect("schema");

        assert!(schema.contains("\"max_items\""));
        assert!(schema.contains("\"max_depth\""));
        assert!(schema.contains("\"snapshot\""));
        assert!(schema.contains("\"inspect_ref\""));
        assert!(schema.contains("\"find\""));
        assert!(schema.contains("\"wait\""));
        assert!(schema.contains("\"wait_app\""));
        assert!(schema.contains("\"wait_ref\""));
        assert!(schema.contains("\"focus\""));
        assert!(schema.contains("\"click\""));
        assert!(schema.contains("\"perform_action\""));
        assert!(schema.contains("\"set_text\""));
        assert!(schema.contains("\"scroll\""));
        assert!(schema.contains("\"press_key\""));
        assert!(schema.contains("\"reference\""));
        assert!(schema.contains("\"text\""));
        assert!(schema.contains("\"key\""));
        assert!(schema.contains("\"native_action\""));
        assert!(schema.contains("\"direction\""));
        assert!(schema.contains("\"scroll_steps\""));
        assert!(schema.contains("\"wait_until\""));
        assert!(schema.contains("\"text_absent\""));
        assert!(schema.contains("\"contains_text\""));
        assert!(schema.contains("\"query\""));
        assert!(schema.contains("\"role\""));
        assert!(schema.contains("\"state\""));
        assert!(schema.contains("\"max_results\""));
        assert!(schema.contains("\"expect_role\""));
        assert!(schema.contains("\"expect_text\""));
        assert!(schema.contains("\"expect_state\""));
        assert!(schema.contains("\"expect_app\""));
        assert!(schema.contains("\"expect_pid\""));
        assert!(schema.contains("\"timeout_seconds\""));
        assert!(schema.contains("\"poll_interval_ms\""));
        assert!(schema.contains("\"snapshot_id\""));
    }

    #[test]
    fn snapshot_origin_guard_passes_for_matching_frontmost_app_and_pid() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ctx(tmp.path());
        let snapshot = "frontmost_app: Finder\npid: 42\nui_tree:\n- @u1 role='window'";
        let record = save_snapshot_record(&ctx, 40, 3, snapshot).expect("save snapshot record");

        let guard = check_snapshot_origin_guard(&record, snapshot).expect("snapshot origin guard");

        assert_eq!(
            guard.app_line_sha256,
            super::sha256_hex("frontmost_app: Finder".as_bytes())
        );
        assert_eq!(guard.pid, Some(42));
        assert_eq!(
            guard.snapshot_sha256,
            super::sha256_hex(snapshot.as_bytes())
        );
    }

    #[test]
    fn snapshot_origin_guard_rejects_changed_frontmost_app_without_echoing_names() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ctx(tmp.path());
        let record = save_snapshot_record(&ctx, 40, 3, "frontmost_app: SecretApp\npid: 42")
            .expect("save snapshot record");

        let error = check_snapshot_origin_guard(&record, "frontmost_app: OtherSecretApp\npid: 42")
            .expect_err("changed frontmost app should fail");
        let message = format!("{error:#}");

        assert!(message.contains("snapshot origin guard failed"));
        assert!(message.contains("current_app_line_sha256:"));
        assert!(message.contains("expected_app_line_sha256:"));
        assert!(!message.contains("SecretApp"));
        assert!(!message.contains("OtherSecretApp"));
    }

    #[test]
    fn snapshot_origin_guard_rejects_changed_pid() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ctx(tmp.path());
        let record = save_snapshot_record(&ctx, 40, 3, "frontmost_app: Finder\npid: 42")
            .expect("save snapshot record");

        let error = check_snapshot_origin_guard(&record, "frontmost_app: Finder\npid: 43")
            .expect_err("changed pid should fail");
        let message = format!("{error:#}");

        assert!(message.contains("frontmost pid changed"));
        assert!(message.contains("current_pid: 43"));
        assert!(message.contains("expected_pid: 42"));
    }

    #[test]
    fn post_action_snapshot_extraction_returns_only_snapshot_body() {
        let result = "clicked_ref: @u2\nfrontmost_app_before_click: Finder\n\npost_click_snapshot:\nfrontmost_app: Finder\npid: 42\nui_tree:\n- @u1 role='window'";

        let post_snapshot =
            post_action_snapshot_from_result(result).expect("post snapshot extraction");

        assert!(post_snapshot.starts_with("frontmost_app: Finder"));
        assert!(post_snapshot.contains("pid: 42"));
        assert!(!post_snapshot.contains("clicked_ref"));
        assert!(!post_snapshot.contains("frontmost_app_before_click"));
    }

    #[test]
    fn post_action_snapshot_record_hashes_only_post_snapshot() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ctx(tmp.path());
        let post_snapshot = "frontmost_app: Finder\npid: 42\nui_tree:\n- @u1 role='window'";
        let result = format!(
            "clicked_ref: @u2\nfrontmost_app_before_click: Finder\n\npost_click_snapshot:\n{post_snapshot}"
        );

        let record = save_post_action_snapshot_record(&ctx, 40, 3, &result)
            .expect("save post action snapshot record");
        let expected_app_line_sha256 = super::sha256_hex("frontmost_app: Finder".as_bytes());

        assert_eq!(
            record.output_sha256,
            super::sha256_hex(post_snapshot.as_bytes())
        );
        assert_eq!(
            record.frontmost_app_line_sha256.as_deref(),
            Some(expected_app_line_sha256.as_str())
        );
        assert_eq!(record.pid, Some(42));
        assert_ne!(record.output_sha256, super::sha256_hex(result.as_bytes()));
    }

    #[test]
    fn snapshot_record_roundtrip_avoids_raw_ui_persistence() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ctx(tmp.path());
        let record = save_snapshot_record(
            &ctx,
            40,
            3,
            "frontmost_app: SecretApp\nvalue='SECRET_TOKEN'",
        )
        .expect("save snapshot record");

        assert!(record.snapshot_id.starts_with("cu_"));
        assert!(record.frontmost_app_line_sha256.is_some());
        assert_eq!(record.pid, None);
        let loaded = load_snapshot_record(&ctx)
            .expect("load snapshot record")
            .expect("record exists");
        assert_eq!(loaded, record);

        let raw = std::fs::read_to_string(snapshot_record_path(&ctx)).expect("read record");
        assert!(!raw.contains("SecretApp"));
        assert!(!raw.contains("SECRET_TOKEN"));
    }

    #[test]
    fn post_action_snapshot_record_replaces_latest_without_raw_ui() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ctx(tmp.path());
        let before = save_snapshot_record(&ctx, 40, 3, "frontmost_app: Before")
            .expect("save before snapshot");
        let after = save_snapshot_record(
            &ctx,
            40,
            3,
            "frontmost_app: After\nvalue='POST_ACTION_SECRET'",
        )
        .expect("save post-action snapshot");

        assert_ne!(before.snapshot_id, after.snapshot_id);
        let loaded = load_snapshot_record(&ctx)
            .expect("load snapshot record")
            .expect("record exists");
        assert_eq!(loaded.snapshot_id, after.snapshot_id);

        let raw = std::fs::read_to_string(snapshot_record_path(&ctx)).expect("read record");
        assert!(!raw.contains("After"));
        assert!(!raw.contains("POST_ACTION_SECRET"));
    }

    #[test]
    fn snapshot_record_rejects_stale_snapshot_id() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ctx(tmp.path());
        save_snapshot_record(&ctx, 40, 3, "frontmost_app: Finder").expect("save snapshot record");

        let error = resolve_snapshot_record(&ctx, Some("cu_stale"), 40, 3)
            .expect_err("stale snapshot id should fail");
        assert!(format!("{error:#}").contains("does not match latest snapshot"));
    }

    #[test]
    fn snapshot_record_rejects_changed_bounds() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ctx(tmp.path());
        save_snapshot_record(&ctx, 40, 3, "frontmost_app: Finder").expect("save snapshot record");

        let error = resolve_snapshot_record(&ctx, None, 30, 3)
            .expect_err("changed snapshot bounds should fail");
        let message = format!("{error:#}");
        assert!(message.contains("snapshot bounds changed"));
        assert!(message.contains("latest max_items=40"));
        assert!(message.contains("requested max_items=30"));
    }

    #[test]
    fn snapshot_record_rejects_old_snapshot() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ctx(tmp.path());
        let mut record = save_snapshot_record(&ctx, 40, 3, "frontmost_app: Finder")
            .expect("save snapshot record");
        record.captured_at_unix = record
            .captured_at_unix
            .saturating_sub(MAX_SNAPSHOT_RECORD_AGE_SECONDS + 1);
        std::fs::write(
            snapshot_record_path(&ctx),
            serde_json::to_vec_pretty(&record).expect("serialize record"),
        )
        .expect("write old record");

        let error = resolve_snapshot_record(&ctx, None, 40, 3)
            .expect_err("old snapshot record should fail");
        let message = format!("{error:#}");
        assert!(message.contains("snapshot"));
        assert!(message.contains("too old"));
        assert!(message.contains("call snapshot/find/inspect_ref/wait/wait_ref again"));
    }

    #[test]
    fn action_without_snapshot_requires_snapshot_first() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ctx(tmp.path());

        let error = resolve_snapshot_record(&ctx, None, 40, 3)
            .expect_err("missing snapshot record should fail");
        assert!(format!("{error:#}").contains("action=snapshot first"));
    }
}
