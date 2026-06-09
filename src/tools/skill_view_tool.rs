use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::skills::SkillStore;
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct SkillViewTool;

#[derive(Debug, Deserialize)]
struct SkillViewArgs {
    name: String,
    category: Option<String>,
    file_path: Option<String>,
}

#[async_trait]
impl Tool for SkillViewTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "skill_view",
            "View a local skill file.",
            object_schema(
                json!({
                    "name": { "type": "string", "description": "Skill name." },
                    "category": { "type": "string", "description": "Optional skill category." },
                    "file_path": {
                        "type": "string",
                        "description": "Optional relative path to a linked file inside the skill directory."
                    }
                }),
                &["name"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: SkillViewArgs =
            serde_json::from_value(args).context("invalid skill_view arguments")?;
        let store = SkillStore::new_with_platform(&ctx.data_dir, Some(&ctx.skill_platform))?;
        let skill = store.view_with_file(
            &args.name,
            args.category.as_deref(),
            args.file_path.as_deref(),
        )?;
        let linked_files = if skill.linked_files.is_empty() {
            "none".to_string()
        } else {
            skill
                .linked_files
                .iter()
                .map(|(group, files)| {
                    format!(
                        "{}: {}",
                        group,
                        files
                            .iter()
                            .map(|file| file.path.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        Ok(format!(
            "skill: {}/{}\nfile: {}\nsource: {}\nreadiness: {}\nlinked_files:\n{}\n\n{}",
            skill.summary.category,
            skill.summary.name,
            skill.file_path,
            skill.summary.path.display(),
            skill.readiness.readiness_status,
            linked_files,
            skill.content
        ))
    }
}
