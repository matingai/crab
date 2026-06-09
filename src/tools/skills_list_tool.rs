use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::skills::SkillStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct SkillsListTool;

#[derive(Debug, Deserialize)]
struct SkillsListArgs {
    category: Option<String>,
}

#[async_trait]
impl Tool for SkillsListTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "skills_list",
            "List available local skills.",
            object_schema(
                json!({
                    "category": { "type": "string", "description": "Optional category filter." }
                }),
                &[],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: SkillsListArgs =
            serde_json::from_value(args).context("invalid skills_list arguments")?;
        let store = SkillStore::new_with_platform(&ctx.data_dir, Some(&ctx.skill_platform))?;
        let skills = store
            .list()?
            .into_iter()
            .filter(|skill| {
                args.category
                    .as_deref()
                    .map(|value| value == skill.category)
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();

        if skills.is_empty() {
            return Ok("no skills available".to_string());
        }

        Ok(skills
            .into_iter()
            .map(|skill| format!("{}/{}: {}", skill.category, skill.name, skill.description))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}
