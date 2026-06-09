use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DelegateWorkerTask {
    #[serde(default)]
    pub objective: String,
    #[serde(default)]
    pub focus_goal_id: Option<String>,
    #[serde(default)]
    pub background_summary: String,
    #[serde(default)]
    pub relevant_state: Value,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub scope: Vec<String>,
    #[serde(default = "default_context_access")]
    pub context_access: String,
    #[serde(default)]
    pub output_schema: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegateRunRecord {
    pub id: String,
    pub parent_session_id: String,
    pub parent_delegate_run_id: Option<String>,
    pub root_delegate_run_id: String,
    pub session_id: String,
    pub prompt: String,
    pub prompt_preview: String,
    pub status: String,
    pub result_preview: String,
    pub max_iterations: usize,
    pub attempt: usize,
    #[serde(default)]
    pub worker_task: Option<DelegateWorkerTask>,
    pub created_at_unix: u64,
    pub updated_at_unix: u64,
}

pub fn save_record(data_dir: &Path, record: &DelegateRunRecord) -> Result<()> {
    let root = records_root(data_dir);
    fs::create_dir_all(&root)
        .with_context(|| format!("failed to create delegate run dir {}", root.display()))?;
    let path = record_path(data_dir, &record.id);
    let raw = serde_json::to_string_pretty(record).context("failed to serialize delegate run")?;
    fs::write(&path, raw).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

pub fn list_records(
    data_dir: &Path,
    parent_session_id: Option<&str>,
) -> Result<Vec<DelegateRunRecord>> {
    let root = records_root(data_dir);
    fs::create_dir_all(&root)
        .with_context(|| format!("failed to create delegate run dir {}", root.display()))?;

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
        let item = serde_json::from_str::<DelegateRunRecord>(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        if parent_session_id.is_some_and(|value| item.parent_session_id != value) {
            continue;
        }
        items.push(item);
    }

    items.sort_by(|a, b| {
        b.updated_at_unix
            .cmp(&a.updated_at_unix)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(items)
}

pub fn load_record(data_dir: &Path, id: &str) -> Result<Option<DelegateRunRecord>> {
    let path = record_path(data_dir, id);
    if !path.is_file() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let item = serde_json::from_str::<DelegateRunRecord>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(item))
}

pub fn new_record(
    parent_session_id: &str,
    parent_delegate_run_id: Option<&str>,
    session_id: &str,
    prompt: &str,
    max_iterations: usize,
    attempt: usize,
    root_delegate_run_id: Option<&str>,
) -> DelegateRunRecord {
    let now = unix_now();
    DelegateRunRecord {
        id: session_id.to_string(),
        parent_session_id: parent_session_id.to_string(),
        parent_delegate_run_id: parent_delegate_run_id.map(ToString::to_string),
        root_delegate_run_id: root_delegate_run_id
            .map(ToString::to_string)
            .unwrap_or_else(|| session_id.to_string()),
        session_id: session_id.to_string(),
        prompt: prompt.to_string(),
        prompt_preview: truncate(prompt, 180),
        status: "running".to_string(),
        result_preview: String::new(),
        max_iterations,
        attempt,
        worker_task: None,
        created_at_unix: now,
        updated_at_unix: now,
    }
}

pub fn finalize_record(record: &mut DelegateRunRecord, status: &str, result_preview: &str) {
    record.status = status.to_string();
    record.result_preview = truncate(result_preview, 240);
    record.updated_at_unix = unix_now();
}

fn records_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("delegate-runs")
}

fn record_path(data_dir: &Path, id: &str) -> PathBuf {
    records_root(data_dir).join(format!("{id}.json"))
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect::<String>() + "..."
}

fn default_context_access() -> String {
    "expanded".to_string()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{finalize_record, list_records, load_record, new_record, save_record};

    #[test]
    fn saves_and_lists_delegate_runs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mut first = new_record("parent-a", None, "session-1", "inspect files", 3, 1, None);
        finalize_record(&mut first, "completed", "done");
        save_record(tmp.path(), &first).expect("save first");

        let second = new_record(
            "parent-b",
            Some("delegate-root"),
            "session-2",
            "grep references",
            2,
            2,
            Some("delegate-root"),
        );
        save_record(tmp.path(), &second).expect("save second");

        let filtered = list_records(tmp.path(), Some("parent-a")).expect("list");
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].session_id, "session-1");
        assert_eq!(filtered[0].attempt, 1);

        let all = list_records(tmp.path(), None).expect("list all");
        assert_eq!(all.len(), 2);
        let loaded = load_record(tmp.path(), "session-2")
            .expect("load")
            .expect("record");
        assert_eq!(
            loaded.parent_delegate_run_id.as_deref(),
            Some("delegate-root")
        );
    }
}
