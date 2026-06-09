use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalStatus {
    Pending,
    Approved,
    Denied,
    Consumed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub session_id: String,
    pub command: String,
    pub reason: String,
    pub tool_name: Option<String>,
    pub execution_mode: Option<String>,
    pub batch_id: Option<String>,
    pub batch_index: Option<usize>,
    pub batch_total: Option<usize>,
    pub status: ApprovalStatus,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingApproval {
    pub approval_id: String,
    pub session_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub execution_mode: String,
    pub batch_id: Option<String>,
    pub batch_index: Option<usize>,
    pub batch_total: Option<usize>,
    pub raw_arguments: String,
    pub command: String,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

pub fn list_requests(data_dir: &Path) -> Result<Vec<ApprovalRequest>> {
    let root = approvals_root(data_dir);
    fs::create_dir_all(&root)
        .with_context(|| format!("failed to create approvals dir {}", root.display()))?;
    let mut items = Vec::new();
    for entry in
        fs::read_dir(&root).with_context(|| format!("failed to read {}", root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let item = serde_json::from_str::<ApprovalRequest>(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        items.push(item);
    }
    items.sort_by(|a, b| {
        b.updated_at_unix
            .cmp(&a.updated_at_unix)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(items)
}

pub fn request_approval(
    data_dir: &Path,
    session_id: &str,
    command: &str,
    reason: &str,
) -> Result<ApprovalRequest> {
    if let Some(existing) = list_requests(data_dir)?.into_iter().find(|item| {
        item.session_id == session_id
            && item.command == command
            && item.status == ApprovalStatus::Pending
    }) {
        return Ok(existing);
    }

    let now = unix_now();
    let item = ApprovalRequest {
        id: Uuid::new_v4().to_string(),
        session_id: session_id.to_string(),
        command: command.to_string(),
        reason: reason.to_string(),
        tool_name: None,
        execution_mode: None,
        batch_id: None,
        batch_index: None,
        batch_total: None,
        status: ApprovalStatus::Pending,
        created_at_unix: now,
        updated_at_unix: now,
    };
    save_request(data_dir, &item)?;
    Ok(item)
}

pub fn resolve_request(data_dir: &Path, id: &str, approved: bool) -> Result<ApprovalRequest> {
    let mut item = load_request(data_dir, id)?;
    item.status = if approved {
        ApprovalStatus::Approved
    } else {
        ApprovalStatus::Denied
    };
    item.updated_at_unix = unix_now();
    save_request(data_dir, &item)?;
    Ok(item)
}

pub fn get_request(data_dir: &Path, id: &str) -> Result<ApprovalRequest> {
    load_request(data_dir, id)
}

pub fn consume_approved_request(
    data_dir: &Path,
    session_id: &str,
    command: &str,
) -> Result<Option<ApprovalRequest>> {
    let Some(mut item) = list_requests(data_dir)?.into_iter().find(|item| {
        item.session_id == session_id
            && item.command == command
            && item.status == ApprovalStatus::Approved
    }) else {
        return Ok(None);
    };

    item.status = ApprovalStatus::Consumed;
    item.updated_at_unix = unix_now();
    save_request(data_dir, &item)?;
    Ok(Some(item))
}

pub fn save_pending_approval(
    data_dir: &Path,
    approval_id: &str,
    session_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    execution_mode: &str,
    batch_id: Option<&str>,
    batch_index: Option<usize>,
    batch_total: Option<usize>,
    raw_arguments: &str,
    command: &str,
) -> Result<PendingApproval> {
    let now = unix_now();
    let item = PendingApproval {
        approval_id: approval_id.to_string(),
        session_id: session_id.to_string(),
        tool_call_id: tool_call_id.to_string(),
        tool_name: tool_name.to_string(),
        execution_mode: execution_mode.to_string(),
        batch_id: batch_id.map(str::to_string),
        batch_index,
        batch_total,
        raw_arguments: raw_arguments.to_string(),
        command: command.to_string(),
        created_at_unix: now,
        updated_at_unix: now,
    };
    save_pending(data_dir, &item)?;
    if let Ok(mut request) = load_request(data_dir, approval_id) {
        request.tool_name = Some(tool_name.to_string());
        request.execution_mode = Some(execution_mode.to_string());
        request.batch_id = batch_id.map(str::to_string);
        request.batch_index = batch_index;
        request.batch_total = batch_total;
        request.updated_at_unix = now;
        save_request(data_dir, &request)?;
    }
    Ok(item)
}

pub fn load_pending_approval(
    data_dir: &Path,
    approval_id: &str,
) -> Result<Option<PendingApproval>> {
    let path = pending_approval_path(data_dir, approval_id);
    if !path.is_file() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let item = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(item))
}

pub fn remove_pending_approval(data_dir: &Path, approval_id: &str) -> Result<()> {
    let path = pending_approval_path(data_dir, approval_id);
    if !path.exists() {
        return Ok(());
    }
    fs::remove_file(&path).with_context(|| format!("failed to remove {}", path.display()))?;
    Ok(())
}

fn load_request(data_dir: &Path, id: &str) -> Result<ApprovalRequest> {
    let path = approval_path(data_dir, id);
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

fn save_request(data_dir: &Path, item: &ApprovalRequest) -> Result<()> {
    let root = approvals_root(data_dir);
    fs::create_dir_all(&root)
        .with_context(|| format!("failed to create approvals dir {}", root.display()))?;
    let path = approval_path(data_dir, &item.id);
    let raw = serde_json::to_string_pretty(item).context("failed to serialize approval request")?;
    fs::write(&path, raw).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn approvals_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("approvals")
}

fn approval_path(data_dir: &Path, id: &str) -> PathBuf {
    approvals_root(data_dir).join(format!("{id}.json"))
}

fn pending_approvals_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("pending-approvals")
}

fn pending_approval_path(data_dir: &Path, id: &str) -> PathBuf {
    pending_approvals_root(data_dir).join(format!("{id}.json"))
}

fn save_pending(data_dir: &Path, item: &PendingApproval) -> Result<()> {
    let root = pending_approvals_root(data_dir);
    fs::create_dir_all(&root)
        .with_context(|| format!("failed to create pending approvals dir {}", root.display()))?;
    let path = pending_approval_path(data_dir, &item.approval_id);
    let raw = serde_json::to_string_pretty(item).context("failed to serialize pending approval")?;
    fs::write(&path, raw).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{
        ApprovalStatus, consume_approved_request, list_requests, load_pending_approval,
        remove_pending_approval, request_approval, resolve_request, save_pending_approval,
    };

    #[test]
    fn approval_lifecycle_works() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let request = request_approval(tmp.path(), "demo", "rm -rf build", "destructive command")
            .expect("request");
        assert_eq!(request.status, ApprovalStatus::Pending);

        let approved = resolve_request(tmp.path(), &request.id, true).expect("approve");
        assert_eq!(approved.status, ApprovalStatus::Approved);

        let consumed = consume_approved_request(tmp.path(), "demo", "rm -rf build")
            .expect("consume")
            .expect("approval");
        assert_eq!(consumed.status, ApprovalStatus::Consumed);

        let items = list_requests(tmp.path()).expect("list");
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn pending_approval_roundtrip_works() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let pending = save_pending_approval(
            tmp.path(),
            "approval-1",
            "demo",
            "tool-1",
            "terminal",
            "parallel",
            Some("parallel-1-call-terminal"),
            Some(2),
            Some(3),
            "{\"command\":\"rm -rf build\"}",
            "rm -rf build",
        )
        .expect("save pending");
        let loaded = load_pending_approval(tmp.path(), "approval-1")
            .expect("load pending")
            .expect("item");
        assert_eq!(loaded.tool_call_id, pending.tool_call_id);
        assert_eq!(loaded.execution_mode, "parallel");
        assert_eq!(loaded.batch_id.as_deref(), Some("parallel-1-call-terminal"));
        assert_eq!(loaded.batch_index, Some(2));
        assert_eq!(loaded.batch_total, Some(3));
        remove_pending_approval(tmp.path(), "approval-1").expect("remove pending");
        assert!(
            load_pending_approval(tmp.path(), "approval-1")
                .expect("load none")
                .is_none()
        );
    }
}
