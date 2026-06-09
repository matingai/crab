use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::skills::{SkillActivation, SkillStore};
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct SkillManageTool;

#[derive(Debug, Deserialize)]
struct SkillManageArgs {
    action: String,
    name: String,
    category: Option<String>,
    description: Option<String>,
    keywords: Option<Vec<String>>,
    task_kinds: Option<Vec<String>>,
    requires_tools: Option<Vec<String>>,
    requires_shell: Option<bool>,
    content: Option<String>,
    file_path: Option<String>,
    file_content: Option<String>,
    old_string: Option<String>,
    new_string: Option<String>,
    replace_all: Option<bool>,
}

#[async_trait]
impl Tool for SkillManageTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "skill_manage",
            "Create, update, patch, delete, or manage supporting files for a local skill.",
            object_schema(
                json!({
                    "action": {
                        "type": "string",
                        "enum": ["create", "overwrite", "append", "patch", "delete", "write_file", "remove_file"]
                    },
                    "name": { "type": "string", "description": "Skill name." },
                    "category": { "type": "string", "description": "Skill category. Defaults to general." },
                    "description": { "type": "string", "description": "Skill summary used for indexing." },
                    "keywords": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional skill keywords used for matching."
                    },
                    "task_kinds": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional task categories like coding, analysis, debugging, planning, documentation, operations."
                    },
                    "requires_tools": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Optional tool names that should exist before this skill is considered relevant."
                    },
                    "requires_shell": {
                        "type": "boolean",
                        "description": "Whether the skill should only activate when shell access is enabled."
                    },
                    "content": {
                        "type": "string",
                        "description": "Skill markdown content for create/overwrite or appended text for append."
                    },
                    "file_path": {
                        "type": "string",
                        "description": "Supporting file path under references/, templates/, scripts/, or assets/."
                    },
                    "file_content": {
                        "type": "string",
                        "description": "File content for write_file."
                    },
                    "old_string": {
                        "type": "string",
                        "description": "Text to find when action=patch."
                    },
                    "new_string": {
                        "type": "string",
                        "description": "Replacement text when action=patch. Use an empty string to delete matched text."
                    },
                    "replace_all": {
                        "type": "boolean",
                        "description": "Whether patch should replace every occurrence instead of exactly one."
                    }
                }),
                &["action", "name"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: SkillManageArgs =
            serde_json::from_value(args).context("invalid skill_manage arguments")?;
        let category = args.category.as_deref().unwrap_or("general");
        let store = SkillStore::new_with_platform(&ctx.data_dir, Some(&ctx.skill_platform))?;

        let path = match args.action.as_str() {
            "create" | "overwrite" => {
                let content = args
                    .content
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| anyhow!("skill_manage `content` is required"))?;
                let description = args
                    .description
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("No description provided.");
                let keywords = args.keywords.clone().unwrap_or_default();
                let activation = SkillActivation {
                    task_kinds: args.task_kinds.unwrap_or_default(),
                    requires_tools: args.requires_tools.unwrap_or_default(),
                    requires_shell: args.requires_shell.unwrap_or(false),
                };
                store.save_with_metadata(
                    category,
                    &args.name,
                    description,
                    &keywords,
                    &activation,
                    content,
                )?
            }
            "append" => {
                let content = args
                    .content
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| anyhow!("skill_manage `content` is required"))?;
                store.append(category, &args.name, content)?
            }
            "patch" => store.patch(
                &args.name,
                args.category.as_deref(),
                args.old_string
                    .as_deref()
                    .ok_or_else(|| anyhow!("skill_manage `old_string` is required"))?,
                args.new_string
                    .as_deref()
                    .ok_or_else(|| anyhow!("skill_manage `new_string` is required"))?,
                args.file_path.as_deref(),
                args.replace_all.unwrap_or(false),
            )?,
            "delete" => store.delete(&args.name, args.category.as_deref())?,
            "write_file" => store.write_supporting_file(
                &args.name,
                args.category.as_deref(),
                args.file_path
                    .as_deref()
                    .ok_or_else(|| anyhow!("skill_manage `file_path` is required"))?,
                args.file_content
                    .as_deref()
                    .ok_or_else(|| anyhow!("skill_manage `file_content` is required"))?,
            )?,
            "remove_file" => store.remove_supporting_file(
                &args.name,
                args.category.as_deref(),
                args.file_path
                    .as_deref()
                    .ok_or_else(|| anyhow!("skill_manage `file_path` is required"))?,
            )?,
            other => bail!("unsupported skill_manage action `{other}`"),
        };

        Ok(format!(
            "skill updated: {}/{}\npath: {}",
            args.category.as_deref().unwrap_or(category),
            args.name,
            path.display()
        ))
    }
}
