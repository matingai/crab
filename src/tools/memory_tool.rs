use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::memory::MemoryStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct MemoryTool;

#[derive(Debug, Deserialize)]
struct MemoryArgs {
    action: String,
    target: Option<String>,
    id: Option<String>,
    old_text: Option<String>,
    content: Option<String>,
    query: Option<String>,
    limit: Option<usize>,
}

#[async_trait]
impl Tool for MemoryTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "memory",
            "Store, update, remove, or recall durable memory across sessions. Use target=memory for stable project/environment facts and target=user for user preferences or profile facts. Do not save temporary task progress.",
            object_schema(
                json!({
                    "action": {
                        "type": "string",
                        "enum": ["add", "replace", "remove", "search", "list"],
                        "description": "Memory action."
                    },
                    "target": {
                        "type": "string",
                        "enum": ["memory", "user"],
                        "description": "Memory bucket. Use memory for project/environment facts, user for user preferences/profile facts."
                    },
                    "id": {
                        "type": "string",
                        "description": "Exact memory id to replace or remove."
                    },
                    "old_text": {
                        "type": "string",
                        "description": "Unique substring of an entry to replace or remove when id is unavailable."
                    },
                    "content": {
                        "type": "string",
                        "description": "Memory content to save when action=add or action=replace."
                    },
                    "query": {
                        "type": "string",
                        "description": "Search query when action=search."
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 50,
                        "description": "Maximum number of entries to return."
                    }
                }),
                &["action"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: MemoryArgs = serde_json::from_value(args).context("invalid memory arguments")?;
        let store = MemoryStore::new(&ctx.data_dir)?;
        let limit = args.limit.unwrap_or(10);
        let target = args.target.as_deref();

        match args.action.as_str() {
            "add" => {
                let Some(content) = args.content else {
                    bail!("memory action=add requires `content`");
                };
                let entry = store.add_to(target.unwrap_or("memory"), content)?;
                Ok(format!(
                    "saved memory {} [{}]\n{}",
                    entry.id, entry.target, entry.content
                ))
            }
            "replace" => {
                let Some(content) = args.content else {
                    bail!("memory action=replace requires `content`");
                };
                let entry = store.replace(
                    target,
                    args.id.as_deref(),
                    args.old_text.as_deref(),
                    content,
                )?;
                Ok(format!(
                    "replaced memory {} [{}]\n{}",
                    entry.id, entry.target, entry.content
                ))
            }
            "remove" => {
                let entry = store.remove(target, args.id.as_deref(), args.old_text.as_deref())?;
                Ok(format!(
                    "removed memory {} [{}]\n{}",
                    entry.id, entry.target, entry.content
                ))
            }
            "search" => {
                let Some(query) = args.query else {
                    bail!("memory action=search requires `query`");
                };
                let items = store.search_target(&query, target, limit)?;
                if items.is_empty() {
                    return Ok(format!("no memory results for query `{query}`"));
                }
                Ok(items
                    .into_iter()
                    .map(|item| format!("- [{}] [{}] {}", item.id, item.target, item.content))
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            "list" => {
                let items = store.list_target(target)?;
                if items.is_empty() {
                    return Ok("no memory entries saved".to_string());
                }
                Ok(items
                    .into_iter()
                    .take(limit)
                    .map(|item| format!("- [{}] [{}] {}", item.id, item.target, item.content))
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            other => bail!("unsupported memory action `{other}`"),
        }
    }
}
