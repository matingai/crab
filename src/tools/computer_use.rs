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
    ComputerUseKey, click_frontmost_app_ref, focus_frontmost_app_ref, frontmost_app_snapshot,
    inspect_computer_use, normalize_computer_use_key, parse_ui_ref, press_frontmost_app_key,
    set_frontmost_app_ref_text,
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
const DEFAULT_FIND_MAX_RESULTS: usize = 12;
const MAX_FIND_RESULTS: usize = 50;
const DEFAULT_WAIT_TIMEOUT_SECONDS: u64 = 10;
const MAX_WAIT_TIMEOUT_SECONDS: u64 = 30;
const DEFAULT_WAIT_POLL_INTERVAL_MS: u64 = 250;

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
            "Inspect and prepare native computer-use automation. On macOS, this checks Accessibility trust, can request the permission prompt, can return or search a shallow Accessibility UI tree for the frontmost app, can focus or click a UI ref after tool-policy approval, can set text on a UI ref after approval, and can press a small whitelist of non-text keys after approval. Broad keyboard and app-control actions are intentionally not enabled yet.",
            object_schema(
                json!({
                    "action": {
                        "type": "string",
                        "enum": ["status", "request_permission", "snapshot", "find", "wait", "focus", "click", "set_text", "press_key"],
                        "description": "status checks support and permission; request_permission asks macOS to show the Accessibility prompt; snapshot reads the frontmost app Accessibility UI tree; find searches a fresh snapshot for candidate UI refs; wait polls snapshots until text appears or the UI settles; focus sets keyboard focus to a snapshot ref such as @u2 after approval; click activates a snapshot ref after approval; set_text sets the Accessibility value for a ref after approval; press_key sends one whitelisted non-text key to the frontmost app after approval."
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
                        "description": "UI ref from the latest computer_use snapshot, such as @u2. Required for focus, click, and set_text."
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
                    "wait_until": {
                        "type": "string",
                        "enum": ["text_present", "settled"],
                        "description": "Wait mode for action=wait. text_present requires contains_text; settled waits for two consecutive matching snapshots. Defaults to text_present when contains_text is provided, otherwise settled."
                    },
                    "contains_text": {
                        "type": "string",
                        "maxLength": 1000,
                        "description": "Substring to wait for in the rendered Accessibility snapshot when wait_until=text_present."
                    },
                    "query": {
                        "type": "string",
                        "maxLength": 1000,
                        "description": "Text to search for in frontmost Accessibility element lines when action=find."
                    },
                    "role": {
                        "type": "string",
                        "maxLength": 120,
                        "description": "Optional role filter for action=find, such as button, text field, menu item, or window."
                    },
                    "state": {
                        "type": "string",
                        "enum": ["focused", "selected", "enabled", "disabled"],
                        "description": "Optional state filter for action=find. enabled means no enabled=false flag is present in the snapshot line."
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
                        "description": "Optional pre-action guard for focus, click, and set_text. When set, the current ref line must still have this role before the write action runs."
                    },
                    "expect_text": {
                        "type": "string",
                        "maxLength": 1000,
                        "description": "Optional pre-action guard for focus, click, and set_text. When set, the current ref line must still contain this text before the write action runs."
                    },
                    "expect_state": {
                        "type": "string",
                        "enum": ["focused", "selected", "enabled", "disabled"],
                        "description": "Optional pre-action guard for focus, click, and set_text. When set, the current ref line must still match this compact state before the write action runs."
                    },
                    "case_sensitive": {
                        "type": "boolean",
                        "description": "Whether contains_text, find query, or expect_text matching is case-sensitive. Defaults to false."
                    },
                    "timeout_seconds": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 30,
                        "description": "Maximum time to wait for action=wait. Defaults to 10 seconds."
                    },
                    "poll_interval_ms": {
                        "type": "integer",
                        "minimum": 100,
                        "maximum": 2000,
                        "description": "Polling interval for action=wait. Defaults to 250 ms."
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
                    "snapshot_id: {}\n{}\n\n{}",
                    record.snapshot_id,
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
                    "snapshot_id: {}\nwait_result: {}\nwait_until: {}\nattempts: {}\nelapsed_ms: {}\n{}\n\n{}",
                    record.snapshot_id,
                    outcome.result,
                    request.mode.label(),
                    outcome.attempts,
                    outcome.elapsed_ms,
                    render_status(false),
                    outcome.snapshot.trim()
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
                let outcome = find_snapshot_lines(&snapshot, &request);
                Ok(format!(
                    "snapshot_id: {}\nfind_result_count: {}\nfind_truncated: {}\n{}\n\n{}",
                    record.snapshot_id,
                    outcome.matches.len(),
                    outcome.truncated,
                    render_status(false),
                    render_find_matches(&outcome)
                ))
            }
            "click" => {
                let reference = args.reference()?;
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let snapshot_record = resolve_snapshot_record(ctx, args.snapshot_id.as_deref())?;
                let ref_guard_request = args.ref_guard_request()?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let ref_guard =
                    run_ref_guard(reference, max_items, max_depth, ref_guard_request.as_ref())?;
                let result = click_frontmost_app_ref(reference, max_items, max_depth)?;
                Ok(render_write_result(
                    &snapshot_record.snapshot_id,
                    ref_guard.as_ref(),
                    &result,
                ))
            }
            "focus" => {
                let reference = args.reference()?;
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let snapshot_record = resolve_snapshot_record(ctx, args.snapshot_id.as_deref())?;
                let ref_guard_request = args.ref_guard_request()?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let ref_guard =
                    run_ref_guard(reference, max_items, max_depth, ref_guard_request.as_ref())?;
                let result = focus_frontmost_app_ref(reference, max_items, max_depth)?;
                Ok(render_write_result(
                    &snapshot_record.snapshot_id,
                    ref_guard.as_ref(),
                    &result,
                ))
            }
            "set_text" => {
                let reference = args.reference()?;
                let text = args.text()?;
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let snapshot_record = resolve_snapshot_record(ctx, args.snapshot_id.as_deref())?;
                let ref_guard_request = args.ref_guard_request()?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let ref_guard =
                    run_ref_guard(reference, max_items, max_depth, ref_guard_request.as_ref())?;
                let result = set_frontmost_app_ref_text(reference, text, max_items, max_depth)?;
                Ok(render_write_result(
                    &snapshot_record.snapshot_id,
                    ref_guard.as_ref(),
                    &result,
                ))
            }
            "press_key" => {
                let key = args.key()?;
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let snapshot_record = resolve_snapshot_record(ctx, args.snapshot_id.as_deref())?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let result = press_frontmost_app_key(key.label, max_items, max_depth)?;
                Ok(format!(
                    "using_snapshot_id: {}\n{}\n\n{}",
                    snapshot_record.snapshot_id,
                    render_status(false),
                    result.trim()
                ))
            }
            other => bail!(
                "unsupported computer_use action `{other}`; use status, request_permission, snapshot, find, wait, focus, click, set_text, or press_key"
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
            "settled" | "stable" => ComputerUseWaitMode::Settled,
            "" => bail!("computer_use wait_until cannot be empty"),
            other => {
                bail!("unsupported computer_use wait_until `{other}`; use text_present or settled")
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
        if query.is_none() && role.is_none() && state.is_none() {
            bail!("computer_use find requires at least one of `query`, `role`, or `state`");
        }

        Ok(ComputerUseFindRequest {
            query,
            role,
            state,
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
    Settled,
}

impl ComputerUseWaitMode {
    fn label(&self) -> &'static str {
        match self {
            ComputerUseWaitMode::TextPresent { .. } => "text_present",
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
struct ComputerUseFindRequest {
    query: Option<String>,
    role: Option<String>,
    state: Option<ComputerUseFindState>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct ComputerUseSnapshotRecord {
    snapshot_id: String,
    captured_at_unix: u64,
    max_items: usize,
    max_depth: usize,
    output_sha256: String,
}

fn save_snapshot_record(
    ctx: &ToolContext,
    max_items: usize,
    max_depth: usize,
    output: &str,
) -> Result<ComputerUseSnapshotRecord> {
    let output_sha256 = sha256_hex(output.as_bytes());
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

fn resolve_snapshot_record(
    ctx: &ToolContext,
    requested_snapshot_id: Option<&str>,
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
    Ok(record)
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

fn snapshot_contains_text(snapshot: &str, contains_text: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        snapshot.contains(contains_text)
    } else {
        snapshot
            .to_lowercase()
            .contains(&contains_text.to_lowercase())
    }
}

fn find_snapshot_lines(snapshot: &str, request: &ComputerUseFindRequest) -> ComputerUseFindOutcome {
    let mut matches = Vec::new();
    let mut truncated = false;

    for line in snapshot.lines() {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("- @u") || !matches_find_request(trimmed, request) {
            continue;
        }
        if matches.len() >= request.max_results {
            truncated = true;
            break;
        }
        matches.push(line.trim_end().to_string());
    }

    ComputerUseFindOutcome { matches, truncated }
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

fn contains_match(value: &str, query: &str, case_sensitive: bool) -> bool {
    if case_sensitive {
        value.contains(query)
    } else {
        value.to_lowercase().contains(&query.to_lowercase())
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
    ref_guard: Option<&ComputerUseRefGuardOutcome>,
    result: &str,
) -> String {
    let mut output = format!("using_snapshot_id: {snapshot_id}\n");
    if let Some(ref_guard) = ref_guard {
        output.push_str(&format!(
            "ref_guard: passed\nref_guard_ref: {}\nref_guard_line_sha256: {}\n",
            ref_guard.reference, ref_guard.line_sha256
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
        ComputerUseFindRequest, ComputerUseFindState, ComputerUseRefGuardRequest, ComputerUseTool,
        ComputerUseWaitMode, MAX_SET_TEXT_CHARS, check_ref_guard_line, find_snapshot_lines,
        load_snapshot_record, resolve_snapshot_record, save_snapshot_record,
        snapshot_contains_text, snapshot_line_for_ref, snapshot_record_path,
    };
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
    async fn wait_rejects_unsupported_mode() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = ComputerUseTool;
        let error = tool
            .execute(
                json!({ "action": "wait", "wait_until": "gone" }),
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
        assert_eq!(request.max_results, super::MAX_FIND_RESULTS);
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
            case_sensitive: false,
            max_results: 12,
        };
        let outcome = find_snapshot_lines(snapshot, &request);

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
            case_sensitive: false,
            max_results: 12,
        };
        let case_sensitive = ComputerUseFindRequest {
            case_sensitive: true,
            ..case_insensitive.clone()
        };

        assert_eq!(
            find_snapshot_lines(snapshot, &case_insensitive)
                .matches
                .len(),
            1
        );
        assert!(
            find_snapshot_lines(snapshot, &case_sensitive)
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
            case_sensitive: false,
            max_results: 12,
        };
        let outcome = find_snapshot_lines(snapshot, &request);

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
            case_sensitive: false,
            max_results: 2,
        };
        let outcome = find_snapshot_lines(snapshot, &request);

        assert_eq!(outcome.matches.len(), 2);
        assert!(outcome.truncated);
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
    fn definition_exposes_snapshot_bounds() {
        let tool = ComputerUseTool;
        let definition = tool.definition();
        let schema = serde_json::to_string(&definition.function.parameters).expect("schema");

        assert!(schema.contains("\"max_items\""));
        assert!(schema.contains("\"max_depth\""));
        assert!(schema.contains("\"snapshot\""));
        assert!(schema.contains("\"find\""));
        assert!(schema.contains("\"wait\""));
        assert!(schema.contains("\"focus\""));
        assert!(schema.contains("\"click\""));
        assert!(schema.contains("\"set_text\""));
        assert!(schema.contains("\"press_key\""));
        assert!(schema.contains("\"reference\""));
        assert!(schema.contains("\"text\""));
        assert!(schema.contains("\"key\""));
        assert!(schema.contains("\"wait_until\""));
        assert!(schema.contains("\"contains_text\""));
        assert!(schema.contains("\"query\""));
        assert!(schema.contains("\"role\""));
        assert!(schema.contains("\"state\""));
        assert!(schema.contains("\"max_results\""));
        assert!(schema.contains("\"expect_role\""));
        assert!(schema.contains("\"expect_text\""));
        assert!(schema.contains("\"expect_state\""));
        assert!(schema.contains("\"timeout_seconds\""));
        assert!(schema.contains("\"poll_interval_ms\""));
        assert!(schema.contains("\"snapshot_id\""));
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
        let loaded = load_snapshot_record(&ctx)
            .expect("load snapshot record")
            .expect("record exists");
        assert_eq!(loaded, record);

        let raw = std::fs::read_to_string(snapshot_record_path(&ctx)).expect("read record");
        assert!(!raw.contains("SecretApp"));
        assert!(!raw.contains("SECRET_TOKEN"));
    }

    #[test]
    fn snapshot_record_rejects_stale_snapshot_id() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ctx(tmp.path());
        save_snapshot_record(&ctx, 40, 3, "frontmost_app: Finder").expect("save snapshot record");

        let error = resolve_snapshot_record(&ctx, Some("cu_stale"))
            .expect_err("stale snapshot id should fail");
        assert!(format!("{error:#}").contains("does not match latest snapshot"));
    }

    #[test]
    fn action_without_snapshot_requires_snapshot_first() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let ctx = ctx(tmp.path());

        let error =
            resolve_snapshot_record(&ctx, None).expect_err("missing snapshot record should fail");
        assert!(format!("{error:#}").contains("action=snapshot first"));
    }
}
