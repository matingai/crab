use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};

use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};
use crate::wiki_store::WikiStore;

pub struct WikiViewTool;

#[derive(Debug, Deserialize)]
struct WikiViewArgs {
    action: String,
    section: Option<String>,
    name: Option<String>,
}

#[async_trait]
impl Tool for WikiViewTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "wiki_view",
            "List or read generated local wiki pages for sessions, topics, and user memory.",
            object_schema(
                json!({
                    "action": {
                        "type": "string",
                        "enum": ["list", "read"],
                        "description": "Whether to list available pages or read a page."
                    },
                    "section": {
                        "type": "string",
                        "enum": ["sessions", "topics", "user"],
                        "description": "Wiki section to inspect."
                    },
                    "name": {
                        "type": "string",
                        "description": "Page name without .md when action=read."
                    }
                }),
                &["action", "section"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: WikiViewArgs =
            serde_json::from_value(args).context("invalid wiki_view arguments")?;
        let root = WikiStore::new(&ctx.data_dir)?.root().to_path_buf();
        let section = args.section.as_deref().unwrap_or("user");
        let section_root = resolve_section_root(&root, section)?;

        match args.action.as_str() {
            "list" => list_pages(&section_root, section),
            "read" => {
                let Some(name) = args.name.as_deref() else {
                    bail!("wiki_view action=read requires `name`");
                };
                read_page(&section_root, section, name)
            }
            other => bail!("unsupported wiki_view action `{other}`"),
        }
    }
}

fn resolve_section_root(root: &Path, section: &str) -> Result<PathBuf> {
    let section_root = match section {
        "sessions" | "topics" | "user" => root.join(section),
        other => bail!("unsupported wiki section `{other}`"),
    };
    Ok(section_root)
}

fn list_pages(section_root: &Path, section: &str) -> Result<String> {
    let mut pages = Vec::new();
    for entry in fs::read_dir(section_root)
        .with_context(|| format!("failed to read wiki section {}", section_root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("md") {
            continue;
        }
        if let Some(name) = path.file_stem().and_then(|value| value.to_str()) {
            pages.push(name.to_string());
        }
    }
    pages.sort();
    if pages.is_empty() {
        return Ok(format!("wiki section `{section}` has no pages"));
    }
    Ok(format!(
        "wiki section: {}\npages:\n{}",
        section,
        pages
            .into_iter()
            .map(|name| format!("- {}", name))
            .collect::<Vec<_>>()
            .join("\n")
    ))
}

fn read_page(section_root: &Path, section: &str, name: &str) -> Result<String> {
    validate_page_name(name)?;
    let path = section_root.join(format!("{name}.md"));
    if !path.is_file() {
        bail!("wiki page `{}` not found in section `{}`", name, section);
    }
    let content =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    Ok(format!(
        "wiki section: {}\npage: {}\npath: {}\n\n{}",
        section,
        name,
        path.display(),
        content
    ))
}

fn validate_page_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        bail!("wiki page name cannot be empty");
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        bail!("wiki page name contains invalid path segments");
    }
    Ok(())
}
