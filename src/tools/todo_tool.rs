use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::todo::{TodoItem, TodoStore, summarize_todos};
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct TodoTool;

#[derive(Debug, Deserialize)]
struct TodoArgs {
    todos: Option<Vec<TodoInput>>,
    #[serde(default)]
    merge: bool,
}

#[derive(Debug, Deserialize)]
struct TodoInput {
    id: Option<String>,
    content: Option<String>,
    status: Option<String>,
}

#[async_trait]
impl Tool for TodoTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "todo",
            "Manage a task list for the current session. Use it to plan multi-step work, track progress, and keep one item in_progress at a time.",
            object_schema(
                json!({
                    "todos": {
                        "type": "array",
                        "description": "Items to write. Omit to read the current task list.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": {
                                    "type": "string",
                                    "description": "Stable task identifier."
                                },
                                "content": {
                                    "type": "string",
                                    "description": "Task description."
                                },
                                "status": {
                                    "type": "string",
                                    "enum": ["pending", "in_progress", "blocked", "completed", "cancelled"],
                                    "description": "Task status."
                                }
                            },
                            "additionalProperties": false
                        }
                    },
                    "merge": {
                        "type": "boolean",
                        "description": "When true, update existing items by id and append new ones. When false, replace the full list."
                    }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: TodoArgs = serde_json::from_value(args).context("invalid todo arguments")?;
        let store = TodoStore::new(&ctx.data_dir)?;
        let items = match args.todos {
            Some(inputs) => {
                let items = if args.merge {
                    merge_items(store.load(&ctx.current_session_id)?, inputs)
                } else {
                    replace_items(inputs)
                };
                if items.is_empty() {
                    store.clear(&ctx.current_session_id)?;
                } else {
                    store.save(&ctx.current_session_id, &items)?;
                }
                items
            }
            None => store.load(&ctx.current_session_id)?,
        };

        let summary = summarize_todos(&items);
        serde_json::to_string_pretty(&json!({
            "todos": items,
            "summary": summary,
        }))
        .context("failed to serialize todo response")
    }
}

fn replace_items(inputs: Vec<TodoInput>) -> Vec<TodoItem> {
    inputs
        .into_iter()
        .map(|item| {
            TodoItem::new(
                item.id.unwrap_or_default(),
                item.content.unwrap_or_default(),
                item.status.unwrap_or_else(|| "pending".to_string()),
            )
        })
        .collect()
}

fn merge_items(mut existing: Vec<TodoItem>, inputs: Vec<TodoInput>) -> Vec<TodoItem> {
    for input in inputs {
        let id = input.id.unwrap_or_default().trim().to_string();
        if id.is_empty() {
            continue;
        }

        if let Some(item) = existing.iter_mut().find(|item| item.id == id) {
            if let Some(content) = input.content {
                if !content.trim().is_empty() {
                    item.content = content;
                }
            }
            if let Some(status) = input.status {
                item.status = status;
            }
            item.normalize();
            continue;
        }

        existing.push(TodoItem::new(
            id,
            input.content.unwrap_or_default(),
            input.status.unwrap_or_else(|| "pending".to_string()),
        ));
    }

    existing
}

#[cfg(test)]
mod tests {
    use super::TodoTool;
    use crate::tools::{Tool, ToolContext};
    use serde_json::{Value, json};

    fn list_ids(output: &str) -> Vec<String> {
        serde_json::from_str::<Value>(output)
            .expect("json")
            .get("todos")
            .and_then(Value::as_array)
            .expect("todos")
            .iter()
            .filter_map(|item| item.get("id").and_then(Value::as_str))
            .map(ToString::to_string)
            .collect()
    }

    fn read_summary_count(output: &str, field: &str) -> u64 {
        serde_json::from_str::<Value>(output)
            .expect("json")
            .get("summary")
            .and_then(|summary| summary.get(field))
            .and_then(Value::as_u64)
            .expect("summary field")
    }

    fn test_context(root: &std::path::Path) -> ToolContext {
        ToolContext {
            workspace_root: root.to_path_buf(),
            data_dir: root.join(".data"),
            shell_enabled: false,
            skill_platform: "desktop".to_string(),
            provider_id: "openai".to_string(),
            model: "test-model".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            api_key: None,
            max_iterations: 4,
            current_session_id: "session-1".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        }
    }

    #[tokio::test]
    async fn todo_replaces_and_reads_items() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = TodoTool;
        let ctx = test_context(tmp.path());

        let written = tool
            .execute(
                json!({
                    "todos": [
                        { "id": "1", "content": "Investigate bug", "status": "in_progress" },
                        { "id": "2", "content": "Write regression test", "status": "pending" }
                    ]
                }),
                &ctx,
            )
            .await
            .expect("write");
        assert_eq!(list_ids(&written), vec!["1".to_string(), "2".to_string()]);
        assert_eq!(read_summary_count(&written, "in_progress"), 1);

        let read = tool.execute(json!({}), &ctx).await.expect("read");
        assert_eq!(list_ids(&read), vec!["1".to_string(), "2".to_string()]);
    }

    #[tokio::test]
    async fn todo_merge_updates_existing_items() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = TodoTool;
        let ctx = test_context(tmp.path());

        tool.execute(
            json!({
                "todos": [
                    { "id": "1", "content": "Investigate bug", "status": "pending" }
                ]
            }),
            &ctx,
        )
        .await
        .expect("seed");

        let output = tool
            .execute(
                json!({
                    "merge": true,
                    "todos": [
                        { "id": "1", "status": "completed" },
                        { "id": "2", "content": "Document fix", "status": "pending" }
                    ]
                }),
                &ctx,
            )
            .await
            .expect("merge");

        assert_eq!(list_ids(&output), vec!["1".to_string(), "2".to_string()]);
        assert_eq!(read_summary_count(&output, "completed"), 1);
    }
}
