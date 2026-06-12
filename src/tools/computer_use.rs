use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::computer_use::{
    click_frontmost_app_ref, frontmost_app_snapshot, inspect_computer_use,
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
    snapshot_id: Option<String>,
}

const MAX_SET_TEXT_CHARS: usize = 4_000;

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
            "Inspect and prepare native computer-use automation. On macOS, this checks Accessibility trust, can request the permission prompt, can return a shallow Accessibility UI tree for the frontmost app, can click a UI ref after tool-policy approval, and can set text on a UI ref after approval. Broad keyboard and app-control actions are intentionally not enabled yet.",
            object_schema(
                json!({
                    "action": {
                        "type": "string",
                        "enum": ["status", "request_permission", "snapshot", "click", "set_text"],
                        "description": "status checks support and permission; request_permission asks macOS to show the Accessibility prompt; snapshot reads the frontmost app Accessibility UI tree; click activates a snapshot ref such as @u2 after approval; set_text sets the Accessibility value for a ref after approval."
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
                        "description": "UI ref from the latest computer_use snapshot, such as @u2. Required for click."
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
            "click" => {
                let reference = args.reference()?;
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let snapshot_record = resolve_snapshot_record(ctx, args.snapshot_id.as_deref())?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let result = click_frontmost_app_ref(reference, max_items, max_depth)?;
                Ok(format!(
                    "using_snapshot_id: {}\n{}\n\n{}",
                    snapshot_record.snapshot_id,
                    render_status(false),
                    result.trim()
                ))
            }
            "set_text" => {
                let reference = args.reference()?;
                let text = args.text()?;
                let max_items = args.max_items.clamp(1, 50);
                let max_depth = args.max_depth.clamp(1, 6);
                let snapshot_record = resolve_snapshot_record(ctx, args.snapshot_id.as_deref())?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let result = set_frontmost_app_ref_text(reference, text, max_items, max_depth)?;
                Ok(format!(
                    "using_snapshot_id: {}\n{}\n\n{}",
                    snapshot_record.snapshot_id,
                    render_status(false),
                    result.trim()
                ))
            }
            other => bail!(
                "unsupported computer_use action `{other}`; use status, request_permission, snapshot, click, or set_text"
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
        ComputerUseTool, MAX_SET_TEXT_CHARS, load_snapshot_record, resolve_snapshot_record,
        save_snapshot_record, snapshot_record_path,
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

    #[test]
    fn definition_exposes_snapshot_bounds() {
        let tool = ComputerUseTool;
        let definition = tool.definition();
        let schema = serde_json::to_string(&definition.function.parameters).expect("schema");

        assert!(schema.contains("\"max_items\""));
        assert!(schema.contains("\"max_depth\""));
        assert!(schema.contains("\"snapshot\""));
        assert!(schema.contains("\"click\""));
        assert!(schema.contains("\"set_text\""));
        assert!(schema.contains("\"reference\""));
        assert!(schema.contains("\"text\""));
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
