use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::prompts::scan_context_content;

const HINT_FILENAMES: &[&str] = &[
    "AGENTS.md",
    "agents.md",
    "CLAUDE.md",
    "claude.md",
    ".cursorrules",
];

#[derive(Debug, Clone)]
pub struct SubdirectoryHintTracker {
    workspace_root: PathBuf,
    loaded_dirs: HashSet<PathBuf>,
}

impl SubdirectoryHintTracker {
    pub fn new(workspace_root: impl Into<PathBuf>) -> Self {
        let workspace_root = workspace_root.into();
        let workspace_root = workspace_root.canonicalize().unwrap_or(workspace_root);
        let mut loaded_dirs = HashSet::new();
        loaded_dirs.insert(workspace_root.clone());
        Self {
            workspace_root,
            loaded_dirs,
        }
    }

    pub fn check_tool_call(&mut self, tool_name: &str, args: &Value) -> Option<String> {
        let mut dirs = Vec::new();

        if let Some(path) = args.get("path").and_then(Value::as_str) {
            self.collect_path_and_ancestors(path, &mut dirs);
        }
        if let Some(path) = args.get("workdir").and_then(Value::as_str) {
            self.collect_path_and_ancestors(path, &mut dirs);
        }
        if tool_name == "terminal" {
            if let Some(command) = args.get("command").and_then(Value::as_str) {
                for token in command.split_whitespace() {
                    let token = token.trim_matches('"').trim_matches('\'');
                    if token.contains('/') || token.contains('.') {
                        self.collect_path_and_ancestors(token, &mut dirs);
                    }
                }
            }
        }

        let mut hints = Vec::new();
        for directory in dirs {
            if let Some(hint) = self.load_hints_for_directory(&directory) {
                hints.push(hint);
            }
        }
        if hints.is_empty() {
            None
        } else {
            Some(hints.join("\n\n"))
        }
    }

    fn collect_path_and_ancestors(&self, raw: &str, out: &mut Vec<PathBuf>) {
        let candidate = if Path::new(raw).is_absolute() {
            PathBuf::from(raw)
        } else {
            self.workspace_root.join(raw)
        };
        let mut path = match candidate.canonicalize() {
            Ok(path) => path,
            Err(_) => candidate,
        };
        if path.is_file() || path.extension().is_some() {
            if let Some(parent) = path.parent() {
                path = parent.to_path_buf();
            }
        }

        for _ in 0..5 {
            if !path.starts_with(&self.workspace_root) {
                break;
            }
            if !out.contains(&path) {
                out.push(path.clone());
            }
            if path == self.workspace_root {
                break;
            }
            let Some(parent) = path.parent() else {
                break;
            };
            path = parent.to_path_buf();
        }
    }

    fn load_hints_for_directory(&mut self, directory: &Path) -> Option<String> {
        if self.loaded_dirs.contains(directory) {
            return None;
        }
        self.loaded_dirs.insert(directory.to_path_buf());

        for filename in HINT_FILENAMES {
            let path = directory.join(filename);
            if !path.is_file() {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path) {
                let content = content.trim();
                if content.is_empty() {
                    continue;
                }
                let display = path
                    .strip_prefix(&self.workspace_root)
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|_| path.display().to_string());
                let scanned = scan_context_content(content, filename);
                return Some(format!(
                    "[Subdirectory context discovered: {display}]\n{}",
                    scanned
                ));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::SubdirectoryHintTracker;
    use serde_json::json;

    #[test]
    fn discovers_subdirectory_agents_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        std::fs::write(src.join("AGENTS.md"), "Use module-level tests").expect("write");
        std::fs::write(src.join("main.rs"), "fn main() {}").expect("write");

        let mut tracker = SubdirectoryHintTracker::new(tmp.path());
        let hint = tracker
            .check_tool_call("read_file", &json!({ "path": "src/main.rs" }))
            .expect("hint");
        assert!(hint.contains("Subdirectory context discovered"));
        assert!(hint.contains("Use module-level tests"));
    }
}
