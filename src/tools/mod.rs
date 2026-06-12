mod archive_query_tool;
mod browser_back;
mod browser_click;
mod browser_eval;
mod browser_find;
mod browser_forward;
mod browser_get_images;
mod browser_hover;
mod browser_navigate;
mod browser_press;
mod browser_screenshot;
mod browser_scroll;
mod browser_select_option;
mod browser_snapshot;
mod browser_type;
mod browser_upload_file;
mod browser_wait;
mod context_module_tool;
mod cron_manage_tool;
mod delegate_task;
mod delegate_to_worker;
mod delete_file;
mod execute_code;
mod git_branch_tool;
mod git_diff_tool;
mod git_log_tool;
mod git_status_tool;
mod goal_state_tool;
mod list_files;
mod mcp_call_tool;
mod mcp_dynamic_tool;
mod mcp_list_tools_tool;
mod memory_digest_tool;
mod memory_query_tool;
mod memory_tool;
mod move_file;
mod office_apply_ops;
mod office_create;
mod office_extract_ir;
mod office_inspect;
mod office_preview;
mod patch_file;
mod pdf_extract_ir;
mod pdf_inspect;
mod pdf_preview;
mod plugin_tool;
mod read_delegate_context;
mod read_file;
mod search_files;
mod session_search_tool;
mod skill_manage_tool;
mod skill_view_tool;
mod skills_list_tool;
mod slidev_create;
mod slidev_preview;
mod terminal;
mod todo_tool;
mod web_extract;
mod wiki_view_tool;
mod write_file;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use tokio::sync::mpsc::UnboundedSender;

use crate::mcp::list_cached_inspections;
use crate::plugins::{PluginHookRegistry, load_plugin_catalog};
use crate::tool_policy::{ToolPolicyPreflight, evaluate_tool_policy};
use crate::types::ToolDefinition;

pub use archive_query_tool::ArchiveQueryTool;
pub use browser_back::BrowserBackTool;
pub use browser_click::BrowserClickTool;
pub use browser_eval::BrowserEvalTool;
pub use browser_find::BrowserFindTool;
pub use browser_forward::BrowserForwardTool;
pub use browser_get_images::BrowserGetImagesTool;
pub use browser_hover::BrowserHoverTool;
pub use browser_navigate::BrowserNavigateTool;
pub use browser_press::BrowserPressTool;
pub use browser_screenshot::BrowserScreenshotTool;
pub use browser_scroll::BrowserScrollTool;
pub use browser_select_option::BrowserSelectOptionTool;
pub use browser_snapshot::BrowserSnapshotTool;
pub use browser_type::BrowserTypeTool;
pub use browser_upload_file::BrowserUploadFileTool;
pub use browser_wait::BrowserWaitTool;
pub use context_module_tool::ContextModuleTool;
pub use cron_manage_tool::CronManageTool;
pub use delegate_task::DelegateTaskTool;
pub use delegate_to_worker::DelegateToWorkerTool;
pub use delete_file::DeleteFileTool;
pub use execute_code::ExecuteCodeTool;
pub use git_branch_tool::GitBranchTool;
pub use git_diff_tool::GitDiffTool;
pub use git_log_tool::GitLogTool;
pub use git_status_tool::GitStatusTool;
pub use goal_state_tool::GoalStateTool;
pub use list_files::ListFilesTool;
pub use mcp_call_tool::McpCallTool;
pub use mcp_dynamic_tool::McpDynamicTool;
pub use mcp_list_tools_tool::McpListToolsTool;
pub use memory_digest_tool::MemoryDigestTool;
pub use memory_query_tool::MemoryQueryTool;
pub use memory_tool::MemoryTool;
pub use move_file::MoveFileTool;
pub use office_apply_ops::OfficeApplyOpsTool;
pub use office_create::OfficeCreateTool;
pub use office_extract_ir::OfficeExtractIrTool;
pub use office_inspect::OfficeInspectTool;
pub use office_preview::OfficePreviewTool;
pub use patch_file::PatchFileTool;
pub use pdf_extract_ir::PdfExtractIrTool;
pub use pdf_inspect::PdfInspectTool;
pub use pdf_preview::PdfPreviewTool;
pub use plugin_tool::PluginTool;
pub use read_delegate_context::ReadDelegateContextTool;
pub use read_file::ReadFileTool;
pub use search_files::SearchFilesTool;
pub use session_search_tool::SessionSearchTool;
pub use skill_manage_tool::SkillManageTool;
pub use skill_view_tool::SkillViewTool;
pub use skills_list_tool::SkillsListTool;
pub use slidev_create::SlidevCreateTool;
pub use slidev_preview::SlidevPreviewTool;
pub use terminal::TerminalTool;
pub use todo_tool::TodoTool;
pub use web_extract::WebExtractTool;
pub use wiki_view_tool::WikiViewTool;
pub use write_file::WriteFileTool;

#[derive(Debug, Clone)]
pub enum ToolRuntimeEvent {
    Stdout { tool_call_id: String, chunk: String },
    Stderr { tool_call_id: String, chunk: String },
}

tokio::task_local! {
    static ACTIVE_TOOL_CALL_ID: String;
}

fn tool_event_senders() -> &'static Mutex<BTreeMap<String, UnboundedSender<ToolRuntimeEvent>>> {
    static TOOL_EVENT_SENDERS: OnceLock<
        Mutex<BTreeMap<String, UnboundedSender<ToolRuntimeEvent>>>,
    > = OnceLock::new();
    TOOL_EVENT_SENDERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub(crate) fn register_tool_event_sender(
    session_id: &str,
    sender: UnboundedSender<ToolRuntimeEvent>,
) {
    tool_event_senders()
        .lock()
        .expect("tool event senders")
        .insert(session_id.to_string(), sender);
}

pub(crate) fn clear_tool_event_sender(session_id: &str) {
    tool_event_senders()
        .lock()
        .expect("tool event senders")
        .remove(session_id);
}

pub(crate) fn emit_tool_runtime_event(session_id: &str, event: ToolRuntimeEvent) {
    let sender = tool_event_senders()
        .lock()
        .expect("tool event senders")
        .get(session_id)
        .cloned();
    if let Some(sender) = sender {
        let _ = sender.send(event);
    }
}

pub(crate) async fn with_tool_runtime_scope<F>(tool_call_id: String, future: F) -> F::Output
where
    F: Future,
{
    ACTIVE_TOOL_CALL_ID.scope(tool_call_id, future).await
}

pub(crate) fn emit_tool_stdout(session_id: &str, chunk: String) {
    let Ok(tool_call_id) = ACTIVE_TOOL_CALL_ID.try_with(|value| value.clone()) else {
        return;
    };
    emit_tool_runtime_event(
        session_id,
        ToolRuntimeEvent::Stdout {
            tool_call_id,
            chunk,
        },
    );
}

pub(crate) fn emit_tool_stderr(session_id: &str, chunk: String) {
    let Ok(tool_call_id) = ACTIVE_TOOL_CALL_ID.try_with(|value| value.clone()) else {
        return;
    };
    emit_tool_runtime_event(
        session_id,
        ToolRuntimeEvent::Stderr {
            tool_call_id,
            chunk,
        },
    );
}

#[derive(Debug, Clone)]
pub struct ToolContext {
    pub workspace_root: PathBuf,
    pub data_dir: PathBuf,
    pub shell_enabled: bool,
    pub skill_platform: String,
    pub provider_id: String,
    pub model: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub max_iterations: usize,
    pub current_session_id: String,
    pub current_delegate_run_id: Option<String>,
    pub delegate_depth: usize,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String>;
}

pub struct ToolRegistry {
    tools: BTreeMap<String, Box<dyn Tool>>,
    hooks: PluginHookRegistry,
}

const PRIMARY_MODEL_DETAILED_TOOLS: &[&str] = &[
    "browser_find",
    "browser_navigate",
    "browser_snapshot",
    "context_module",
    "execute_code",
    "git_diff",
    "git_status",
    "list_files",
    "patch_file",
    "read_file",
    "search_files",
    "web_extract",
    "write_file",
];

fn insert_tool<T>(
    tools: &mut BTreeMap<String, Box<dyn Tool>>,
    allowset: Option<&BTreeSet<String>>,
    tool: T,
) where
    T: Tool + 'static,
{
    let definition = tool.definition();
    let tool_name = definition.function.name.clone();
    if allowset.is_some_and(|items| !items.contains(&tool_name)) {
        return;
    }
    tools.insert(tool_name, Box::new(tool));
}

impl ToolRegistry {
    pub fn hermes_default(data_dir: &Path) -> Self {
        Self::hermes_default_with_allowlist(data_dir, None)
    }

    pub fn hermes_default_with_allowlist(data_dir: &Path, allowlist: Option<&[String]>) -> Self {
        let mut tools: BTreeMap<String, Box<dyn Tool>> = BTreeMap::new();
        let mut hooks = PluginHookRegistry::default();
        let allowset = allowlist.map(|items| {
            items
                .iter()
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect::<BTreeSet<_>>()
        });
        let archive_query = ArchiveQueryTool;
        let cron_manage = CronManageTool;
        let context_module = ContextModuleTool;
        let delegate_to_worker = DelegateToWorkerTool;
        let browser_back = BrowserBackTool;
        let browser_click = BrowserClickTool;
        let browser_eval = BrowserEvalTool;
        let browser_find = BrowserFindTool;
        let browser_forward = BrowserForwardTool;
        let browser_get_images = BrowserGetImagesTool;
        let browser_hover = BrowserHoverTool;
        let browser_navigate = BrowserNavigateTool;
        let browser_press = BrowserPressTool;
        let browser_select_option = BrowserSelectOptionTool;
        let browser_scroll = BrowserScrollTool;
        let browser_snapshot = BrowserSnapshotTool;
        let browser_screenshot = BrowserScreenshotTool;
        let browser_type = BrowserTypeTool;
        let browser_upload_file = BrowserUploadFileTool;
        let browser_wait = BrowserWaitTool;
        let delete_file = DeleteFileTool;
        let delegate_task = DelegateTaskTool;
        let execute_code = ExecuteCodeTool;
        let git_branch = GitBranchTool;
        let git_diff = GitDiffTool;
        let git_log = GitLogTool;
        let git_status = GitStatusTool;
        let goal_state = GoalStateTool;
        let read_file = ReadFileTool;
        let list_files = ListFilesTool;
        let search_files = SearchFilesTool;
        let session_search = SessionSearchTool;
        let terminal = TerminalTool;
        let todo = TodoTool;
        let mcp_list_tools = McpListToolsTool;
        let mcp_call = McpCallTool;
        let move_file = MoveFileTool;
        let write_file = WriteFileTool;
        let memory_query = MemoryQueryTool;
        let memory_digest = MemoryDigestTool;
        let memory = MemoryTool;
        let office_apply_ops = OfficeApplyOpsTool;
        let office_create = OfficeCreateTool;
        let office_extract_ir = OfficeExtractIrTool;
        let office_inspect = OfficeInspectTool;
        let office_preview = OfficePreviewTool;
        let patch_file = PatchFileTool;
        let pdf_extract_ir = PdfExtractIrTool;
        let pdf_inspect = PdfInspectTool;
        let pdf_preview = PdfPreviewTool;
        let read_delegate_context = ReadDelegateContextTool;
        let skills_list = SkillsListTool;
        let skill_view = SkillViewTool;
        let skill_manage = SkillManageTool;
        let slidev_create = SlidevCreateTool;
        let slidev_preview = SlidevPreviewTool;
        let web_extract = WebExtractTool;
        let wiki_view = WikiViewTool;

        insert_tool(&mut tools, allowset.as_ref(), archive_query);
        insert_tool(&mut tools, allowset.as_ref(), cron_manage);
        insert_tool(&mut tools, allowset.as_ref(), context_module);
        insert_tool(&mut tools, allowset.as_ref(), delegate_to_worker);
        insert_tool(&mut tools, allowset.as_ref(), browser_navigate);
        insert_tool(&mut tools, allowset.as_ref(), browser_forward);
        insert_tool(&mut tools, allowset.as_ref(), browser_back);
        insert_tool(&mut tools, allowset.as_ref(), browser_click);
        insert_tool(&mut tools, allowset.as_ref(), browser_hover);
        insert_tool(&mut tools, allowset.as_ref(), browser_type);
        insert_tool(&mut tools, allowset.as_ref(), browser_select_option);
        insert_tool(&mut tools, allowset.as_ref(), browser_upload_file);
        insert_tool(&mut tools, allowset.as_ref(), browser_scroll);
        insert_tool(&mut tools, allowset.as_ref(), browser_press);
        insert_tool(&mut tools, allowset.as_ref(), browser_wait);
        insert_tool(&mut tools, allowset.as_ref(), browser_eval);
        insert_tool(&mut tools, allowset.as_ref(), browser_find);
        insert_tool(&mut tools, allowset.as_ref(), browser_snapshot);
        insert_tool(&mut tools, allowset.as_ref(), browser_screenshot);
        insert_tool(&mut tools, allowset.as_ref(), browser_get_images);
        insert_tool(&mut tools, allowset.as_ref(), delete_file);
        insert_tool(&mut tools, allowset.as_ref(), delegate_task);
        insert_tool(&mut tools, allowset.as_ref(), execute_code);
        insert_tool(&mut tools, allowset.as_ref(), git_branch);
        insert_tool(&mut tools, allowset.as_ref(), git_diff);
        insert_tool(&mut tools, allowset.as_ref(), git_log);
        insert_tool(&mut tools, allowset.as_ref(), git_status);
        insert_tool(&mut tools, allowset.as_ref(), goal_state);
        insert_tool(&mut tools, allowset.as_ref(), list_files);
        insert_tool(&mut tools, allowset.as_ref(), mcp_list_tools);
        insert_tool(&mut tools, allowset.as_ref(), mcp_call);
        insert_tool(&mut tools, allowset.as_ref(), move_file);
        insert_tool(&mut tools, allowset.as_ref(), read_file);
        insert_tool(&mut tools, allowset.as_ref(), read_delegate_context);
        insert_tool(&mut tools, allowset.as_ref(), search_files);
        insert_tool(&mut tools, allowset.as_ref(), session_search);
        insert_tool(&mut tools, allowset.as_ref(), terminal);
        insert_tool(&mut tools, allowset.as_ref(), todo);
        insert_tool(&mut tools, allowset.as_ref(), write_file);
        insert_tool(&mut tools, allowset.as_ref(), memory_query);
        insert_tool(&mut tools, allowset.as_ref(), memory_digest);
        insert_tool(&mut tools, allowset.as_ref(), memory);
        insert_tool(&mut tools, allowset.as_ref(), office_apply_ops);
        insert_tool(&mut tools, allowset.as_ref(), office_create);
        insert_tool(&mut tools, allowset.as_ref(), office_extract_ir);
        insert_tool(&mut tools, allowset.as_ref(), office_inspect);
        insert_tool(&mut tools, allowset.as_ref(), office_preview);
        insert_tool(&mut tools, allowset.as_ref(), patch_file);
        insert_tool(&mut tools, allowset.as_ref(), pdf_extract_ir);
        insert_tool(&mut tools, allowset.as_ref(), pdf_inspect);
        insert_tool(&mut tools, allowset.as_ref(), pdf_preview);
        insert_tool(&mut tools, allowset.as_ref(), skills_list);
        insert_tool(&mut tools, allowset.as_ref(), skill_view);
        insert_tool(&mut tools, allowset.as_ref(), skill_manage);
        insert_tool(&mut tools, allowset.as_ref(), slidev_create);
        insert_tool(&mut tools, allowset.as_ref(), slidev_preview);
        insert_tool(&mut tools, allowset.as_ref(), web_extract);
        insert_tool(&mut tools, allowset.as_ref(), wiki_view);

        if let Ok(inspections) = list_cached_inspections(data_dir) {
            for inspection in inspections {
                for tool in McpDynamicTool::from_cached_inspection(&inspection) {
                    insert_tool(&mut tools, allowset.as_ref(), tool);
                }
            }
        }

        if let Ok(plugins) = load_plugin_catalog(data_dir) {
            hooks = plugins.hooks;
            for tool in plugins.tools {
                if tools.contains_key(&tool.tool_name) {
                    continue;
                }
                let plugin_tool = PluginTool::new(tool);
                insert_tool(&mut tools, allowset.as_ref(), plugin_tool);
            }
        }

        Self { tools, hooks }
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|tool| tool.definition()).collect()
    }

    pub fn primary_model_definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|tool| primary_model_tool_definition(tool.definition()))
            .collect()
    }

    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    pub async fn run_hooks(&self, event: &str, payload: &Value) -> Vec<String> {
        self.hooks.run(event, payload).await
    }

    pub async fn call(&self, name: &str, raw_arguments: &str, ctx: &ToolContext) -> String {
        let Some(tool) = self.tools.get(name) else {
            return format!("tool_error: unknown tool `{name}`");
        };

        let arguments = match parse_tool_args(raw_arguments) {
            Ok(value) => value,
            Err(error) => {
                return format!("tool_error: invalid JSON arguments for `{name}`: {error}");
            }
        };

        match evaluate_tool_policy(&ctx.data_dir, &ctx.current_session_id, name, raw_arguments) {
            Ok(ToolPolicyPreflight::Allow) => {}
            Ok(ToolPolicyPreflight::Deny(reason)) => {
                return format!("tool_error: {reason}");
            }
            Ok(ToolPolicyPreflight::ApprovalRequired(approval)) => {
                return format!(
                    "approval_required\napproval_id: {}\nsession_id: {}\nreason: {}\ncommand: {}",
                    approval.id, approval.session_id, approval.reason, approval.command
                );
            }
            Err(error) => {
                return format!(
                    "tool_error: failed to evaluate tool policy for `{name}`: {error:#}"
                );
            }
        }

        let pre_payload = serde_json::json!({
            "event": "pre_tool_call",
            "tool_name": name,
            "raw_arguments": raw_arguments,
            "session_id": ctx.current_session_id,
            "delegate_run_id": ctx.current_delegate_run_id,
            "provider": ctx.provider_id,
        });
        let _ = self.run_hooks("pre_tool_call", &pre_payload).await;

        let result = match tool.execute(arguments, ctx).await {
            Ok(output) => output,
            Err(error) => format!("tool_error: {error:#}"),
        };

        let post_payload = serde_json::json!({
            "event": "post_tool_call",
            "tool_name": name,
            "raw_arguments": raw_arguments,
            "result": result,
            "session_id": ctx.current_session_id,
            "delegate_run_id": ctx.current_delegate_run_id,
            "provider": ctx.provider_id,
        });
        let _ = self.run_hooks("post_tool_call", &post_payload).await;
        post_payload
            .get("result")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string()
    }
}

fn primary_model_tool_definition(mut definition: ToolDefinition) -> ToolDefinition {
    let tool_name = definition.function.name.clone();
    if primary_model_tool_should_stay_detailed(&tool_name) {
        return definition;
    }

    definition.function.description =
        primary_model_tool_description(&tool_name, &definition.function.description);
    definition.function.parameters = strip_schema_annotations(definition.function.parameters);
    definition
}

fn primary_model_tool_should_stay_detailed(tool_name: &str) -> bool {
    PRIMARY_MODEL_DETAILED_TOOLS.contains(&tool_name)
}

fn primary_model_tool_description(tool_name: &str, fallback: &str) -> String {
    if tool_name.starts_with("office_") {
        return "Office document capability. Use when docx/xlsx/pptx editing or structured office inspection becomes relevant.".to_string();
    }
    if tool_name.starts_with("pdf_") {
        return "PDF inspection and extraction capability. Use when PDF content or previews are directly relevant.".to_string();
    }
    if tool_name.starts_with("slidev_") {
        return "Slide deck capability. Use for slide generation or preview workflows when presentation work is relevant.".to_string();
    }
    if tool_name.starts_with("browser_") {
        return "Browser interaction capability. Use for deeper live-page interaction when the basic browse flow is not enough.".to_string();
    }
    if tool_name.starts_with("mcp_") {
        return "External integration capability. Use when configured MCP resources are specifically relevant.".to_string();
    }
    match tool_name {
        "archive_query" | "session_search" | "memory" | "memory_digest" | "memory_query"
        | "wiki_view" => {
            "Memory and history recall capability. Use when prior sessions, stored notes, or durable facts matter.".to_string()
        }
        "goal_state" | "todo" => {
            "Planning and working-memory capability. Use when you need to track, inspect, or update task state explicitly.".to_string()
        }
        "skills_list" | "skill_view" | "skill_manage" => {
            "Skill workflow capability. Use to discover, inspect, or save reusable local workflows on demand.".to_string()
        }
        "delegate_task" | "delegate_to_worker" | "read_delegate_context" => {
            "Delegation capability. Use when work should be split out or delegated context must be inspected.".to_string()
        }
        "cron_manage" => {
            "Scheduled automation capability. Use when cron jobs or delayed recurring work are relevant.".to_string()
        }
        "git_branch" | "git_log" => {
            "Git inspection capability. Use when branch or history context matters beyond the current diff.".to_string()
        }
        "plugin_tool" => {
            "Workspace-specific plugin capability. Use only when a configured plugin workflow is explicitly relevant.".to_string()
        }
        _ => summarize_tool_description(fallback, 140),
    }
}

fn summarize_tool_description(description: &str, max_chars: usize) -> String {
    let first_line = description
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("Additional capability available on demand.");
    if first_line.chars().count() <= max_chars {
        first_line.to_string()
    } else {
        let mut clipped = first_line
            .chars()
            .take(max_chars.saturating_sub(3))
            .collect::<String>();
        clipped.push_str("...");
        clipped
    }
}

fn strip_schema_annotations(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let filtered = map
                .into_iter()
                .filter(|(key, _)| {
                    !matches!(
                        key.as_str(),
                        "default" | "description" | "examples" | "title"
                    )
                })
                .map(|(key, value)| (key, strip_schema_annotations(value)))
                .collect();
            Value::Object(filtered)
        }
        Value::Array(items) => {
            Value::Array(items.into_iter().map(strip_schema_annotations).collect())
        }
        other => other,
    }
}

fn parse_tool_args(raw_arguments: &str) -> Result<Value> {
    if raw_arguments.trim().is_empty() {
        return Ok(Value::Object(Default::default()));
    }
    serde_json::from_str(raw_arguments).context("failed to parse JSON tool arguments")
}

pub fn classify_shell_risk(command: &str) -> Option<&'static str> {
    let lowered = command.trim().to_ascii_lowercase();
    let patterns = [
        ("rm -rf", "destructive file deletion"),
        ("rm -fr", "destructive file deletion"),
        ("git reset --hard", "destructive git reset"),
        ("git clean -fd", "destructive git clean"),
        ("git clean -xfd", "destructive git clean"),
        ("sudo ", "privileged command"),
        ("mkfs", "disk formatting command"),
        ("dd ", "raw disk write command"),
        ("chmod -r", "recursive permission change"),
        ("chown -r", "recursive ownership change"),
    ];

    for (pattern, reason) in patterns {
        if lowered.contains(pattern) {
            return Some(reason);
        }
    }

    let uses_downloader = lowered.contains("curl ") || lowered.contains("wget ");
    if uses_downloader && (lowered.contains("| sh") || lowered.contains("| bash")) {
        return Some("piped remote shell command");
    }

    None
}

pub fn relative_display(root: &Path, path: &Path) -> String {
    let canonical_root = root.canonicalize().ok();
    let canonical_path = path.canonicalize().ok();

    match canonical_path
        .as_deref()
        .or(Some(path))
        .and_then(|candidate| canonical_root.as_deref().map(|root| (candidate, root)))
    {
        Some((candidate, root)) => match candidate.strip_prefix(root) {
            Ok(relative) if !relative.as_os_str().is_empty() => relative.display().to_string(),
            _ => path.display().to_string(),
        },
        None => match path.strip_prefix(root) {
            Ok(relative) if !relative.as_os_str().is_empty() => relative.display().to_string(),
            _ => path.display().to_string(),
        },
    }
}

pub fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

pub fn resolve_existing_path(root: &Path, requested: &str) -> Result<PathBuf> {
    let candidate = if Path::new(requested).is_absolute() {
        PathBuf::from(requested)
    } else {
        root.join(requested)
    };
    let canonical = candidate
        .canonicalize()
        .with_context(|| format!("path does not exist: {}", candidate.display()))?;
    ensure_within_root(root, &canonical)?;
    Ok(canonical)
}

pub fn resolve_workspace_path(root: &Path, requested: &str) -> Result<PathBuf> {
    let root = root
        .canonicalize()
        .with_context(|| format!("failed to resolve workspace root: {}", root.display()))?;
    let candidate = if Path::new(requested).is_absolute() {
        PathBuf::from(requested)
    } else {
        root.join(requested)
    };
    let normalized_candidate = normalize_path(&candidate);
    if normalized_candidate.starts_with(&root) {
        return Ok(normalized_candidate);
    }
    bail!(
        "path escapes workspace root: {} is outside {}",
        normalized_candidate.display(),
        root.display()
    )
}

pub fn ensure_within_root(root: &Path, candidate: &Path) -> Result<()> {
    let root = root
        .canonicalize()
        .with_context(|| format!("failed to resolve workspace root: {}", root.display()))?;
    let normalized_candidate = match candidate.canonicalize() {
        Ok(path) => path,
        Err(_) => normalize_path(candidate),
    };
    if normalized_candidate.starts_with(&root) {
        return Ok(());
    }
    bail!(
        "path escapes workspace root: {} is outside {}",
        candidate.display(),
        root.display()
    )
}

pub fn ensure_clean_worktree_path(
    workspace_root: &Path,
    path: &Path,
    operation: &str,
    allow_dirty: bool,
) -> Result<()> {
    if allow_dirty {
        return Ok(());
    }

    let Some(repo_root) = git_repo_root(workspace_root)? else {
        return Ok(());
    };

    let status = Command::new("git")
        .arg("-C")
        .arg(&repo_root)
        .args(["status", "--porcelain=v1", "--"])
        .arg(path)
        .output()
        .with_context(|| format!("failed to inspect Git status for {}", path.display()))?;
    if !status.status.success() {
        let stderr = String::from_utf8_lossy(&status.stderr);
        bail!(
            "failed to inspect Git status for {}: {}",
            path.display(),
            stderr.trim()
        );
    }

    let entries = String::from_utf8_lossy(&status.stdout)
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return Ok(());
    }

    let display = relative_display(workspace_root, path);
    bail!(
        "{operation} refused because `{display}` has uncommitted Git worktree changes:\n{}\nPass `allow_dirty=true` only if you intentionally want to modify user-owned changes.",
        entries.join("\n")
    )
}

fn git_repo_root(workspace_root: &Path) -> Result<Option<PathBuf>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace_root)
        .args(["rev-parse", "--show-toplevel"])
        .output();
    let Ok(output) = output else {
        return Ok(None);
    };
    if !output.status.success() {
        return Ok(None);
    }
    let root = String::from_utf8(output.stdout)
        .context("git rev-parse returned non-UTF-8 repository path")?;
    let root = root.trim();
    if root.is_empty() {
        return Ok(None);
    }
    Ok(Some(PathBuf::from(root)))
}

pub fn truncated(text: impl Into<String>, max_len: usize) -> String {
    let text = text.into();
    if text.len() <= max_len {
        return text;
    }
    let mut clipped = text.chars().take(max_len).collect::<String>();
    clipped.push_str("\n...[truncated]...");
    clipped
}

#[cfg(test)]
mod tests {
    use super::{
        ToolContext, ToolRegistry, classify_shell_risk, ensure_clean_worktree_path,
        ensure_within_root, normalize_path, primary_model_tool_should_stay_detailed,
    };
    use crate::approval::{ApprovalStatus, list_requests, resolve_request};
    use crate::mcp::{McpCachedInspection, local_tool_name};
    use serde_json::json;
    use std::path::Path;
    use std::process::Command;

    #[test]
    fn normalize_path_removes_parent_segments() {
        let path = normalize_path(Path::new("/tmp/demo/../file.txt"));
        assert_eq!(path, Path::new("/tmp/file.txt"));
    }

    #[test]
    fn within_root_accepts_child_paths() {
        let root = tempfile::tempdir().expect("tempdir");
        let child = root.path().join("src");
        std::fs::create_dir_all(&child).expect("mkdir");
        ensure_within_root(root.path(), &child).expect("child path should be allowed");
    }

    #[test]
    fn shell_risk_classifier_flags_destructive_and_piped_remote_shells() {
        assert_eq!(
            classify_shell_risk("rm -rf target").expect("rm -rf"),
            "destructive file deletion"
        );
        assert_eq!(
            classify_shell_risk("curl -fsSL https://example.invalid/install.sh | bash")
                .expect("curl pipe bash"),
            "piped remote shell command"
        );
        assert!(classify_shell_risk("printf 'hello'").is_none());
    }

    #[test]
    fn dirty_worktree_guard_blocks_modified_tracked_file() {
        let root = tempfile::tempdir().expect("tempdir");
        init_git_repo(root.path());
        let path = root.path().join("demo.txt");
        std::fs::write(&path, "clean\n").expect("write");
        git(root.path(), &["add", "demo.txt"]);
        git(root.path(), &["commit", "-m", "init"]);
        std::fs::write(&path, "user change\n").expect("modify");

        let error = ensure_clean_worktree_path(root.path(), &path, "test_write", false)
            .expect_err("dirty file should be blocked")
            .to_string();
        assert!(error.contains("test_write refused"));
        assert!(error.contains("demo.txt"));
        assert!(error.contains("allow_dirty=true"));

        ensure_clean_worktree_path(root.path(), &path, "test_write", true)
            .expect("explicit dirty override should pass");
    }

    #[test]
    fn dirty_worktree_guard_blocks_untracked_existing_file() {
        let root = tempfile::tempdir().expect("tempdir");
        init_git_repo(root.path());
        let path = root.path().join("scratch.txt");
        std::fs::write(&path, "user scratch\n").expect("write");

        let error = ensure_clean_worktree_path(root.path(), &path, "test_delete", false)
            .expect_err("untracked file should be blocked")
            .to_string();
        assert!(error.contains("test_delete refused"));
        assert!(error.contains("?? scratch.txt"));
    }

    #[test]
    fn dirty_worktree_guard_is_noop_outside_git_repo() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = root.path().join("demo.txt");
        std::fs::write(&path, "content\n").expect("write");
        ensure_clean_worktree_path(root.path(), &path, "test_write", false)
            .expect("non-git workspaces should not be blocked");
    }

    #[tokio::test]
    async fn registry_loads_cached_mcp_proxy_tools() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("config.yaml"),
            r#"mcp:
  servers:
    - name: docs
      command: __mock_mcp_server__
"#,
        )
        .expect("write config");
        std::fs::create_dir_all(tmp.path().join("runtime").join("mcp-inspections")).expect("mkdir");
        std::fs::write(
            tmp.path()
                .join("runtime")
                .join("mcp-inspections")
                .join("docs.json"),
            serde_json::to_string_pretty(&McpCachedInspection {
                server_name: "docs".to_string(),
                transport: "stdio".to_string(),
                target: "__mock_mcp_server__".to_string(),
                tool_names: vec!["search_docs".to_string()],
                tools: vec![crate::mcp::McpToolDescriptor {
                    name: "search_docs".to_string(),
                    description: "Search docs".to_string(),
                    input_schema: json!({
                        "type": "object",
                        "properties": { "query": { "type": "string" } },
                        "required": ["query"]
                    }),
                }],
                updated_at_unix: 1,
            })
            .expect("serialize"),
        )
        .expect("write cache");

        let registry = ToolRegistry::hermes_default(tmp.path());
        let tool_name = local_tool_name("docs", "search_docs");
        assert!(registry.tool_names().contains(&tool_name));

        let output = registry
            .call(
                &tool_name,
                &json!({ "query": "patterns" }).to_string(),
                &ToolContext {
                    workspace_root: tmp.path().to_path_buf(),
                    data_dir: tmp.path().to_path_buf(),
                    shell_enabled: false,
                    skill_platform: "cli".to_string(),
                    provider_id: "openai".to_string(),
                    model: "test-model".to_string(),
                    base_url: "https://example.invalid/v1".to_string(),
                    api_key: None,
                    max_iterations: 4,
                    current_session_id: "session".to_string(),
                    current_delegate_run_id: None,
                    delegate_depth: 0,
                },
            )
            .await;

        assert!(output.contains("proxy:"));
        assert!(output.contains("patterns"));
    }

    #[tokio::test]
    async fn registry_applies_tool_policy_before_execution() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("config.yaml"),
            r#"tool_policy:
  require_approval:
    - read_file
"#,
        )
        .expect("write config");
        std::fs::write(tmp.path().join("secret-token.txt"), "classified").expect("write secret");
        let registry = ToolRegistry::hermes_default(tmp.path());
        let ctx = ToolContext {
            workspace_root: tmp.path().to_path_buf(),
            data_dir: tmp.path().to_path_buf(),
            shell_enabled: false,
            skill_platform: "cli".to_string(),
            provider_id: "openai".to_string(),
            model: "test-model".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            api_key: None,
            max_iterations: 4,
            current_session_id: "session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        };
        let args = json!({ "path": "secret-token.txt" }).to_string();

        let first = registry.call("read_file", &args, &ctx).await;
        assert!(first.contains("approval_required"));
        assert!(first.contains("tool policy requires approval"));
        assert!(!first.contains("classified"));
        assert!(!first.contains("secret-token.txt"));
        let requests = list_requests(tmp.path()).expect("requests");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].status, ApprovalStatus::Pending);
        assert!(!requests[0].command.contains("secret-token.txt"));

        resolve_request(tmp.path(), &requests[0].id, true).expect("approve");
        let second = registry.call("read_file", &args, &ctx).await;
        assert!(second.contains("classified"));
    }

    #[tokio::test]
    async fn registry_blocks_disabled_tool_policy() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("config.yaml"),
            r#"tool_policy:
  disabled:
    - read_file
"#,
        )
        .expect("write config");
        std::fs::write(tmp.path().join("demo.txt"), "hello").expect("write file");
        let registry = ToolRegistry::hermes_default(tmp.path());
        let output = registry
            .call(
                "read_file",
                &json!({ "path": "demo.txt" }).to_string(),
                &ToolContext {
                    workspace_root: tmp.path().to_path_buf(),
                    data_dir: tmp.path().to_path_buf(),
                    shell_enabled: false,
                    skill_platform: "cli".to_string(),
                    provider_id: "openai".to_string(),
                    model: "test-model".to_string(),
                    base_url: "https://example.invalid/v1".to_string(),
                    api_key: None,
                    max_iterations: 4,
                    current_session_id: "session".to_string(),
                    current_delegate_run_id: None,
                    delegate_depth: 0,
                },
            )
            .await;

        assert_eq!(
            output,
            "tool_error: tool `read_file` is disabled by tool_policy"
        );
        assert!(list_requests(tmp.path()).expect("requests").is_empty());
    }

    #[tokio::test]
    async fn registry_applies_protected_path_policy_before_execution() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("config.yaml"),
            r#"tool_policy:
  protected_paths:
    - .env*
"#,
        )
        .expect("write config");
        let registry = ToolRegistry::hermes_default(tmp.path());
        let ctx = ToolContext {
            workspace_root: tmp.path().to_path_buf(),
            data_dir: tmp.path().to_path_buf(),
            shell_enabled: false,
            skill_platform: "cli".to_string(),
            provider_id: "openai".to_string(),
            model: "test-model".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            api_key: None,
            max_iterations: 4,
            current_session_id: "session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        };
        let args = json!({
            "path": ".env.local",
            "content": "OPENAI_API_KEY=secret"
        })
        .to_string();

        let first = registry.call("write_file", &args, &ctx).await;
        assert!(first.contains("approval_required"));
        assert!(first.contains("path pattern `.env*`"));
        assert!(!tmp.path().join(".env.local").exists());
        assert!(!first.contains("OPENAI_API_KEY"));
        let requests = list_requests(tmp.path()).expect("requests");
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].command.contains(".env.local"));

        resolve_request(tmp.path(), &requests[0].id, true).expect("approve");
        let second = registry.call("write_file", &args, &ctx).await;
        assert!(second.contains("bytes_written"));
        assert_eq!(
            std::fs::read_to_string(tmp.path().join(".env.local")).expect("read env"),
            "OPENAI_API_KEY=secret"
        );
    }

    #[test]
    fn primary_model_definitions_condense_cold_tools_only() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let registry = ToolRegistry::hermes_default(tmp.path());
        let definitions = registry.primary_model_definitions();

        let read_file = definitions
            .iter()
            .find(|definition| definition.function.name == "read_file")
            .expect("read_file definition");
        let office_inspect = definitions
            .iter()
            .find(|definition| definition.function.name == "office_inspect")
            .expect("office_inspect definition");

        assert!(primary_model_tool_should_stay_detailed("read_file"));
        assert!(
            serde_json::to_string(&read_file.function.parameters)
                .expect("serialize")
                .contains("\"description\"")
        );
        assert!(
            !serde_json::to_string(&office_inspect.function.parameters)
                .expect("serialize")
                .contains("\"description\"")
        );
        assert!(
            office_inspect
                .function
                .description
                .contains("Office document capability")
        );
    }

    fn init_git_repo(root: &Path) {
        git(root, &["init"]);
        git(
            root,
            &["config", "user.email", "crab-tests@example.invalid"],
        );
        git(root, &["config", "user.name", "Crab Tests"]);
    }

    fn git(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
            args,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
