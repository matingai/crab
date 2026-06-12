use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::io::Write;

use crate::tools::{
    Tool, ToolContext, ensure_clean_worktree_path, relative_display, resolve_workspace_path,
};
use crate::types::{ToolDefinition, object_schema};

pub struct WriteFileTool;

#[derive(Debug, Deserialize)]
struct WriteFileArgs {
    path: String,
    content: String,
    #[serde(default)]
    append: bool,
    #[serde(default)]
    allow_dirty: bool,
}

#[async_trait]
impl Tool for WriteFileTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "write_file",
            "Write or append UTF-8 text content to a file inside the workspace.",
            object_schema(
                json!({
                    "path": {
                        "type": "string",
                        "description": "Path relative to the workspace root."
                    },
                    "content": {
                        "type": "string",
                        "description": "Text content to write."
                    },
                    "append": {
                        "type": "boolean",
                        "description": "Append instead of overwriting when true."
                    },
                    "allow_dirty": {
                        "type": "boolean",
                        "description": "Allow modifying an existing file that has uncommitted Git worktree changes."
                    }
                }),
                &["path", "content"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: WriteFileArgs =
            serde_json::from_value(args).context("invalid write_file arguments")?;
        let path = resolve_workspace_path(&ctx.workspace_root, &args.path)?;
        ensure_clean_worktree_path(
            &ctx.workspace_root,
            &path,
            if args.append {
                "write_file append"
            } else {
                "write_file overwrite"
            },
            args.allow_dirty,
        )?;

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        if args.append {
            let mut file = fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .with_context(|| format!("failed to open {}", path.display()))?;
            file.write_all(args.content.as_bytes())
                .with_context(|| format!("failed to append {}", path.display()))?;
        } else {
            fs::write(&path, args.content.as_bytes())
                .with_context(|| format!("failed to write {}", path.display()))?;
        }

        Ok(format!(
            "path: {}\nmode: {}\nbytes_written: {}",
            relative_display(&ctx.workspace_root, &path),
            if args.append { "append" } else { "overwrite" },
            args.content.len()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::WriteFileTool;
    use crate::tools::{Tool, ToolContext};
    use serde_json::json;
    use std::path::Path;
    use std::process::Command;

    #[tokio::test]
    async fn writes_file_inside_workspace() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = WriteFileTool;
        tool.execute(
            json!({
                "path": "notes/demo.txt",
                "content": "hello"
            }),
            &ToolContext {
                workspace_root: tmp.path().to_path_buf(),
                data_dir: tmp.path().join(".data"),
                shell_enabled: false,
                skill_platform: "cli".to_string(),
                provider_id: "openai".to_string(),
                model: "test-model".to_string(),
                base_url: "https://example.invalid/v1".to_string(),
                api_key: None,
                max_iterations: 4,
                current_session_id: "test-session".to_string(),
                current_delegate_run_id: None,
                delegate_depth: 0,
            },
        )
        .await
        .expect("write should succeed");

        let saved = std::fs::read_to_string(tmp.path().join("notes/demo.txt")).expect("read");
        assert_eq!(saved, "hello");
    }

    #[tokio::test]
    async fn refuses_to_overwrite_dirty_git_file_without_override() {
        let tmp = tempfile::tempdir().expect("tempdir");
        init_git_repo(tmp.path());
        let path = tmp.path().join("demo.txt");
        std::fs::write(&path, "clean\n").expect("write");
        git(tmp.path(), &["add", "demo.txt"]);
        git(tmp.path(), &["commit", "-m", "init"]);
        std::fs::write(&path, "user change\n").expect("modify");

        let tool = WriteFileTool;
        let ctx = test_context(tmp.path());
        let error = tool
            .execute(
                json!({
                    "path": "demo.txt",
                    "content": "agent overwrite\n"
                }),
                &ctx,
            )
            .await
            .expect_err("dirty file should be blocked")
            .to_string();
        assert!(error.contains("write_file overwrite refused"));
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "user change\n"
        );

        tool.execute(
            json!({
                "path": "demo.txt",
                "content": "agent overwrite\n",
                "allow_dirty": true
            }),
            &ctx,
        )
        .await
        .expect("override should allow write");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read"),
            "agent overwrite\n"
        );
    }

    fn test_context(root: &Path) -> ToolContext {
        ToolContext {
            workspace_root: root.to_path_buf(),
            data_dir: root.join(".data"),
            shell_enabled: false,
            skill_platform: "cli".to_string(),
            provider_id: "openai".to_string(),
            model: "test-model".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            api_key: None,
            max_iterations: 4,
            current_session_id: "test-session".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        }
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
