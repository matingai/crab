use anyhow::{Context, Result};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::approval::{ApprovalRequest, consume_approved_request, request_approval};

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ToolPolicyConfig {
    #[serde(default, alias = "disabled_tools", alias = "disabledTools")]
    pub disabled: Vec<String>,
    #[serde(
        default,
        alias = "approval_required",
        alias = "approvalRequired",
        alias = "requireApproval"
    )]
    pub require_approval: Vec<String>,
    #[serde(default, alias = "protectedPaths", alias = "approval_paths")]
    pub protected_paths: Vec<String>,
    #[serde(
        default,
        alias = "disabledPaths",
        alias = "blocked_paths",
        alias = "blockedPaths"
    )]
    pub disabled_paths: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum ToolPolicyPreflight {
    Allow,
    Deny(String),
    ApprovalRequired(ApprovalRequest),
}

#[derive(Debug, Deserialize, Default)]
struct RootConfig {
    #[serde(default, alias = "toolPolicy")]
    tool_policy: ToolPolicyConfig,
}

impl ToolPolicyConfig {
    pub fn is_empty(&self) -> bool {
        self.disabled.is_empty()
            && self.require_approval.is_empty()
            && self.protected_paths.is_empty()
            && self.disabled_paths.is_empty()
    }

    pub fn disables(&self, tool_name: &str) -> bool {
        self.disabled
            .iter()
            .any(|pattern| matches_tool_pattern(pattern, tool_name))
    }

    pub fn requires_approval(&self, tool_name: &str) -> bool {
        self.require_approval
            .iter()
            .any(|pattern| matches_tool_pattern(pattern, tool_name))
    }

    pub fn disabled_path_match(&self, paths: &[String]) -> Option<String> {
        first_path_pattern_match(&self.disabled_paths, paths)
    }

    pub fn protected_path_match(&self, paths: &[String]) -> Option<String> {
        first_path_pattern_match(&self.protected_paths, paths)
    }

    fn normalized(self) -> Self {
        Self {
            disabled: normalize_patterns(self.disabled),
            require_approval: normalize_patterns(self.require_approval),
            protected_paths: normalize_patterns(self.protected_paths),
            disabled_paths: normalize_patterns(self.disabled_paths),
        }
    }
}

pub fn load_tool_policy_config(data_dir: &Path) -> Result<ToolPolicyConfig> {
    let Some(config_path) = existing_config_path(data_dir) else {
        return Ok(ToolPolicyConfig::default());
    };
    let raw = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let root = serde_yaml::from_str::<RootConfig>(&raw)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;
    Ok(root.tool_policy.normalized())
}

pub fn evaluate_tool_policy(
    data_dir: &Path,
    session_id: &str,
    tool_name: &str,
    raw_arguments: &str,
) -> Result<ToolPolicyPreflight> {
    let config = load_tool_policy_config(data_dir)?;
    if config.disables(tool_name) {
        return Ok(ToolPolicyPreflight::Deny(format!(
            "tool `{tool_name}` is disabled by tool_policy"
        )));
    }

    let referenced_paths = extract_policy_paths(raw_arguments);
    if let Some(pattern) = config.disabled_path_match(&referenced_paths) {
        return Ok(ToolPolicyPreflight::Deny(format!(
            "tool `{tool_name}` is blocked by tool_policy for path pattern `{pattern}`"
        )));
    }

    let path_approval = config.protected_path_match(&referenced_paths);
    if !config.requires_approval(tool_name) && path_approval.is_none() {
        return Ok(ToolPolicyPreflight::Allow);
    }

    let command = tool_policy_approval_command(tool_name, raw_arguments);
    if consume_approved_request(data_dir, session_id, &command)?.is_some() {
        return Ok(ToolPolicyPreflight::Allow);
    }

    let reason = match path_approval {
        Some(pattern) => {
            format!("tool policy requires approval for `{tool_name}` path pattern `{pattern}`")
        }
        None => format!("tool policy requires approval for `{tool_name}`"),
    };
    let approval = request_approval(data_dir, session_id, &command, &reason)?;
    Ok(ToolPolicyPreflight::ApprovalRequired(approval))
}

pub fn tool_policy_approval_command(tool_name: &str, raw_arguments: &str) -> String {
    format!(
        "tool_policy tool={} args_hash={}",
        tool_name,
        sha256_hex(&format!("{tool_name}\0{raw_arguments}"))
    )
}

pub fn matches_tool_pattern(pattern: &str, tool_name: &str) -> bool {
    let pattern = pattern.trim();
    if pattern == "*" {
        return true;
    }
    match pattern.strip_suffix('*') {
        Some(prefix) => tool_name.starts_with(prefix),
        None => pattern == tool_name,
    }
}

pub fn matches_path_pattern(pattern: &str, path: &str) -> bool {
    let pattern = normalize_policy_path(pattern);
    let path = normalize_policy_path(path);
    if pattern == "*" {
        return true;
    }
    if !pattern.contains('*') {
        return path == pattern || path.starts_with(&format!("{pattern}/"));
    }
    wildcard_match(&pattern, &path)
}

pub fn extract_policy_paths(raw_arguments: &str) -> Vec<String> {
    let Ok(value) = serde_json::from_str::<Value>(raw_arguments) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    collect_policy_paths(&value, &mut paths);
    paths
        .into_iter()
        .map(|path| normalize_policy_path(&path))
        .filter(|path| !path.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn normalize_patterns(items: Vec<String>) -> Vec<String> {
    items
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn first_path_pattern_match(patterns: &[String], paths: &[String]) -> Option<String> {
    patterns.iter().find_map(|pattern| {
        paths
            .iter()
            .any(|path| matches_path_pattern(pattern, path))
            .then(|| pattern.clone())
    })
}

fn collect_policy_paths(value: &Value, paths: &mut Vec<String>) {
    let Some(map) = value.as_object() else {
        return;
    };
    for key in [
        "path",
        "source_path",
        "destination_path",
        "workdir",
        "file_path",
        "output_path",
    ] {
        if let Some(path) = map.get(key).and_then(Value::as_str) {
            paths.push(path.to_string());
        }
    }
}

fn normalize_policy_path(path: &str) -> String {
    let path = path.trim().replace('\\', "/");
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return pattern == value;
    }

    let mut cursor = 0;
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let Some(found) = value[cursor..].find(part) else {
            return false;
        };
        if index == 0 && found != 0 {
            return false;
        }
        cursor += found + part.len();
    }

    if !pattern.ends_with('*') {
        let last = parts.iter().rev().find(|part| !part.is_empty());
        if let Some(last) = last {
            return value.ends_with(last);
        }
    }
    true
}

fn existing_config_path(data_dir: &Path) -> Option<PathBuf> {
    ["config.yaml", "config.yml"]
        .iter()
        .map(|name| data_dir.join(name))
        .find(|path| path.is_file())
}

fn sha256_hex(input: &str) -> String {
    let digest = Sha256::digest(input.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{
        ToolPolicyPreflight, evaluate_tool_policy, extract_policy_paths, load_tool_policy_config,
        matches_path_pattern, matches_tool_pattern, tool_policy_approval_command,
    };
    use crate::approval::{ApprovalStatus, list_requests, resolve_request};
    use std::fs;

    #[test]
    fn load_missing_policy_returns_default() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let config = load_tool_policy_config(tmp.path()).expect("policy");

        assert!(config.is_empty());
    }

    #[test]
    fn loads_and_normalizes_policy_patterns() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("config.yaml"),
            r#"
model: gpt-4.1-mini
tool_policy:
  disabled:
    - " browser_eval "
    - ""
  require_approval:
    - terminal
    - browser_*
    - terminal
  protected_paths:
    - .env*
    - .github/workflows/*
  disabled_paths:
    - secrets/*
"#,
        )
        .expect("write config");

        let config = load_tool_policy_config(tmp.path()).expect("policy");
        assert_eq!(config.disabled, vec!["browser_eval"]);
        assert_eq!(config.require_approval, vec!["browser_*", "terminal"]);
        assert_eq!(config.protected_paths, vec![".env*", ".github/workflows/*"]);
        assert_eq!(config.disabled_paths, vec!["secrets/*"]);
    }

    #[test]
    fn policy_patterns_match_exact_prefix_and_global_wildcards() {
        assert!(matches_tool_pattern("read_file", "read_file"));
        assert!(!matches_tool_pattern("read_file", "write_file"));
        assert!(matches_tool_pattern("browser_*", "browser_navigate"));
        assert!(!matches_tool_pattern("browser_*", "read_file"));
        assert!(matches_tool_pattern("*", "mcp__docs__search"));
    }

    #[test]
    fn path_patterns_match_exact_directory_and_wildcards() {
        assert!(matches_path_pattern(".env", ".env"));
        assert!(matches_path_pattern("config", "config/local.yaml"));
        assert!(matches_path_pattern(".env*", ".env.local"));
        assert!(matches_path_pattern(
            ".github/workflows/*",
            ".github/workflows/release.yml"
        ));
        assert!(matches_path_pattern(
            "secrets/*/prod.json",
            "secrets/app/prod.json"
        ));
        assert!(!matches_path_pattern(
            ".github/workflows/*",
            ".github/actions/build.yml"
        ));
    }

    #[test]
    fn extracts_and_normalizes_common_policy_paths() {
        let paths = extract_policy_paths(
            r#"{
                "path": "./.env.local",
                "source_path": "src/../secrets/input.txt",
                "destination_path": "dist\\bundle.js",
                "content": "not a path"
            }"#,
        );

        assert_eq!(
            paths,
            vec![".env.local", "dist/bundle.js", "secrets/input.txt"]
        );
    }

    #[test]
    fn approval_command_uses_stable_hash_without_raw_arguments() {
        let command = tool_policy_approval_command("read_file", r#"{"path":"secret.txt"}"#);

        assert!(command.starts_with("tool_policy tool=read_file args_hash="));
        assert!(!command.contains("secret.txt"));
        assert_eq!(
            command,
            tool_policy_approval_command("read_file", r#"{"path":"secret.txt"}"#)
        );
    }

    #[test]
    fn policy_requires_approval_without_leaking_raw_arguments() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("config.yaml"),
            r#"
tool_policy:
  require_approval:
    - read_file
"#,
        )
        .expect("write config");

        let decision = evaluate_tool_policy(
            tmp.path(),
            "session-1",
            "read_file",
            r#"{"path":"secret-token.txt"}"#,
        )
        .expect("policy");
        let ToolPolicyPreflight::ApprovalRequired(approval) = decision else {
            panic!("expected approval");
        };
        assert_eq!(approval.status, ApprovalStatus::Pending);
        assert!(approval.command.contains("args_hash="));
        assert!(!approval.command.contains("secret-token"));

        let requests = list_requests(tmp.path()).expect("requests");
        assert_eq!(requests.len(), 1);
        assert!(!requests[0].command.contains("secret-token"));
    }

    #[test]
    fn protected_path_requires_approval_without_leaking_raw_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("config.yaml"),
            r#"
tool_policy:
  protected_paths:
    - .env*
"#,
        )
        .expect("write config");

        let decision = evaluate_tool_policy(
            tmp.path(),
            "session-1",
            "write_file",
            r#"{"path":".env.local","content":"OPENAI_API_KEY=secret"}"#,
        )
        .expect("policy");
        let ToolPolicyPreflight::ApprovalRequired(approval) = decision else {
            panic!("expected approval");
        };

        assert!(approval.reason.contains("path pattern `.env*`"));
        assert!(!approval.command.contains(".env.local"));
        assert!(!approval.command.contains("OPENAI_API_KEY"));
    }

    #[test]
    fn disabled_path_blocks_before_approval() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("config.yaml"),
            r#"
tool_policy:
  disabled_paths:
    - secrets/*
  require_approval:
    - write_file
"#,
        )
        .expect("write config");

        let decision = evaluate_tool_policy(
            tmp.path(),
            "session-1",
            "write_file",
            r#"{"path":"secrets/prod.env","content":"secret"}"#,
        )
        .expect("policy");

        let ToolPolicyPreflight::Deny(reason) = decision else {
            panic!("expected deny");
        };
        assert!(reason.contains("path pattern `secrets/*`"));
        assert!(list_requests(tmp.path()).expect("requests").is_empty());
    }

    #[test]
    fn approved_policy_call_is_consumed_once() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("config.yaml"),
            r#"
tool_policy:
  require_approval:
    - read_file
"#,
        )
        .expect("write config");
        let first =
            evaluate_tool_policy(tmp.path(), "session-1", "read_file", r#"{"path":"a.txt"}"#)
                .expect("first");
        let ToolPolicyPreflight::ApprovalRequired(approval) = first else {
            panic!("expected approval");
        };
        resolve_request(tmp.path(), &approval.id, true).expect("approve");

        let second =
            evaluate_tool_policy(tmp.path(), "session-1", "read_file", r#"{"path":"a.txt"}"#)
                .expect("second");
        assert!(matches!(second, ToolPolicyPreflight::Allow));

        let third =
            evaluate_tool_policy(tmp.path(), "session-1", "read_file", r#"{"path":"a.txt"}"#)
                .expect("third");
        assert!(matches!(third, ToolPolicyPreflight::ApprovalRequired(_)));
    }

    #[test]
    fn disabled_policy_takes_precedence_over_approval() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("config.yaml"),
            r#"
tool_policy:
  disabled:
    - read_file
  require_approval:
    - read_file
"#,
        )
        .expect("write config");

        let decision =
            evaluate_tool_policy(tmp.path(), "session-1", "read_file", r#"{"path":"a.txt"}"#)
                .expect("policy");

        assert!(matches!(decision, ToolPolicyPreflight::Deny(_)));
    }
}
