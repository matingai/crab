use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::computer_use::{click_frontmost_app_ref, frontmost_app_snapshot, inspect_computer_use};
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
}

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
            "Inspect and prepare native computer-use automation. On macOS, this checks Accessibility trust, can request the permission prompt, can return a shallow Accessibility UI tree for the frontmost app, and can click a UI ref after tool-policy approval. Typing and other write actions are intentionally not enabled yet.",
            object_schema(
                json!({
                    "action": {
                        "type": "string",
                        "enum": ["status", "request_permission", "snapshot", "click"],
                        "description": "status checks support and permission; request_permission asks macOS to show the Accessibility prompt; snapshot reads the frontmost app Accessibility UI tree; click activates a snapshot ref such as @u2 after approval."
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
                    }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, _ctx: &ToolContext) -> Result<String> {
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
                let snapshot = frontmost_app_snapshot(
                    args.max_items.clamp(1, 50),
                    args.max_depth.clamp(1, 6),
                )?;
                Ok(format!("{}\n\n{}", render_status(false), snapshot.trim()))
            }
            "click" => {
                let reference = args.reference()?;
                let status = inspect_computer_use(false);
                if !status.ready() {
                    bail!("{}", status.guidance);
                }
                let result = click_frontmost_app_ref(
                    reference,
                    args.max_items.clamp(1, 50),
                    args.max_depth.clamp(1, 6),
                )?;
                Ok(format!("{}\n\n{}", render_status(false), result.trim()))
            }
            other => bail!(
                "unsupported computer_use action `{other}`; use status, request_permission, snapshot, or click"
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
            _ => bail!("computer_use click requires a non-empty `reference` or `ref`"),
        }
    }
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
    use super::ComputerUseTool;
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

    #[test]
    fn definition_exposes_snapshot_bounds() {
        let tool = ComputerUseTool;
        let definition = tool.definition();
        let schema = serde_json::to_string(&definition.function.parameters).expect("schema");

        assert!(schema.contains("\"max_items\""));
        assert!(schema.contains("\"max_depth\""));
        assert!(schema.contains("\"snapshot\""));
        assert!(schema.contains("\"click\""));
        assert!(schema.contains("\"reference\""));
    }
}
