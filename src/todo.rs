use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_ACTIVE_TODO_CONTEXT_ITEMS: usize = 8;
const MAX_TODO_CONTEXT_CONTENT_CHARS: usize = 120;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: String,
}

impl TodoItem {
    pub fn new(
        id: impl Into<String>,
        content: impl Into<String>,
        status: impl Into<String>,
    ) -> Self {
        let mut item = Self {
            id: id.into(),
            content: content.into(),
            status: status.into(),
        };
        item.normalize();
        item
    }

    pub fn normalize(&mut self) {
        self.id = self.id.trim().to_string();
        if self.id.is_empty() {
            self.id = "?".to_string();
        }

        self.content = self.content.trim().to_string();
        if self.content.is_empty() {
            self.content = "(no description)".to_string();
        }

        self.status = normalize_status(&self.status);
    }

    pub fn is_active(&self) -> bool {
        matches!(self.status.as_str(), "pending" | "in_progress" | "blocked")
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TodoSummary {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub blocked: usize,
    pub completed: usize,
    pub cancelled: usize,
}

#[derive(Debug, Clone)]
pub struct TodoStore {
    root: PathBuf,
}

impl TodoStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("todos"))
            .with_context(|| format!("failed to create todo dir under {}", root.display()))?;
        Ok(Self { root })
    }

    pub fn load(&self, session_id: &str) -> Result<Vec<TodoItem>> {
        let path = self.todo_path(session_id);
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read todo state {}", path.display()))?;
        let mut items: Vec<TodoItem> = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse todo state {}", path.display()))?;
        for item in &mut items {
            item.normalize();
        }
        Ok(items)
    }

    pub fn save(&self, session_id: &str, items: &[TodoItem]) -> Result<PathBuf> {
        let path = self.todo_path(session_id);
        let tmp = path.with_extension("json.tmp");
        let raw = serde_json::to_string_pretty(items).context("failed to serialize todo state")?;
        fs::write(&tmp, raw).with_context(|| format!("failed to write {}", tmp.display()))?;
        fs::rename(&tmp, &path)
            .with_context(|| format!("failed to move {} to {}", tmp.display(), path.display()))?;
        Ok(path)
    }

    pub fn clear(&self, session_id: &str) -> Result<()> {
        let path = self.todo_path(session_id);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
        Ok(())
    }

    pub fn build_context_block(&self, session_id: &str) -> Result<Option<String>> {
        Ok(format_active_todo_block(&self.load(session_id)?))
    }

    fn todo_path(&self, session_id: &str) -> PathBuf {
        self.root.join("todos").join(format!("{session_id}.json"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

pub fn normalize_status(status: &str) -> String {
    match status.trim().to_ascii_lowercase().as_str() {
        "pending" => "pending".to_string(),
        "in_progress" => "in_progress".to_string(),
        "blocked" => "blocked".to_string(),
        "completed" => "completed".to_string(),
        "cancelled" => "cancelled".to_string(),
        _ => "pending".to_string(),
    }
}

pub fn summarize_todos(items: &[TodoItem]) -> TodoSummary {
    TodoSummary {
        total: items.len(),
        pending: items.iter().filter(|item| item.status == "pending").count(),
        in_progress: items
            .iter()
            .filter(|item| item.status == "in_progress")
            .count(),
        blocked: items.iter().filter(|item| item.status == "blocked").count(),
        completed: items
            .iter()
            .filter(|item| item.status == "completed")
            .count(),
        cancelled: items
            .iter()
            .filter(|item| item.status == "cancelled")
            .count(),
    }
}

pub fn format_active_todo_block(items: &[TodoItem]) -> Option<String> {
    let active = items
        .iter()
        .filter(|item| item.is_active())
        .take(MAX_ACTIVE_TODO_CONTEXT_ITEMS)
        .collect::<Vec<_>>();
    if active.is_empty() {
        return None;
    }

    let mut lines = vec!["# Active Todo List".to_string()];
    for item in active {
        let marker = match item.status.as_str() {
            "completed" => "[x]",
            "in_progress" => "[>]",
            "blocked" => "[!]",
            "cancelled" => "[~]",
            _ => "[ ]",
        };
        lines.push(format!(
            "- {} {}. {} ({})",
            marker,
            item.id,
            inline_clip(&item.content, MAX_TODO_CONTEXT_CONTENT_CHARS),
            item.status
        ));
    }
    Some(lines.join("\n"))
}

fn inline_clip(value: &str, max_chars: usize) -> String {
    let normalized = value.replace('\n', " ").trim().to_string();
    if normalized.chars().count() <= max_chars {
        normalized
    } else {
        normalized.chars().take(max_chars).collect::<String>() + "..."
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_ACTIVE_TODO_CONTEXT_ITEMS, TodoItem, TodoStore, format_active_todo_block,
        summarize_todos,
    };

    #[test]
    fn saves_and_loads_todo_state() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = TodoStore::new(tmp.path()).expect("store");
        let items = vec![
            TodoItem::new("1", "Write tests", "in_progress"),
            TodoItem::new("2", "Review output", "pending"),
        ];
        store.save("demo", &items).expect("save");

        let loaded = store.load("demo").expect("load");
        assert_eq!(loaded, items);
    }

    #[test]
    fn active_context_block_excludes_completed_items() {
        let items = vec![
            TodoItem::new("1", "Ship feature", "completed"),
            TodoItem::new("2", "Write docs", "pending"),
        ];
        let block = format_active_todo_block(&items).expect("block");
        assert!(block.contains("Write docs"));
        assert!(!block.contains("Ship feature"));
    }

    #[test]
    fn summary_counts_statuses() {
        let items = vec![
            TodoItem::new("1", "A", "pending"),
            TodoItem::new("2", "B", "in_progress"),
            TodoItem::new("3", "C", "completed"),
            TodoItem::new("4", "D", "cancelled"),
        ];
        let summary = summarize_todos(&items);
        assert_eq!(summary.total, 4);
        assert_eq!(summary.pending, 1);
        assert_eq!(summary.in_progress, 1);
        assert_eq!(summary.blocked, 0);
        assert_eq!(summary.completed, 1);
        assert_eq!(summary.cancelled, 1);
    }

    #[test]
    fn blocked_items_are_active() {
        let item = TodoItem::new("1", "Needs user input", "blocked");
        assert!(item.is_active());
        assert_eq!(item.status, "blocked");
    }

    #[test]
    fn active_context_block_caps_item_count_and_clips_content() {
        let items = (0..12)
            .map(|index| {
                TodoItem::new(
                    index.to_string(),
                    format!("{}{}", "x".repeat(150), index),
                    "pending",
                )
            })
            .collect::<Vec<_>>();

        let block = format_active_todo_block(&items).expect("block");
        assert_eq!(block.lines().count(), 1 + MAX_ACTIVE_TODO_CONTEXT_ITEMS);
        assert!(block.contains("..."));
        assert!(!block.contains("\n- [ ] 11."));
    }
}
