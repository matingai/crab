use anyhow::{Context, Result};
use serde::Deserialize;
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
        self.disabled.is_empty() && self.require_approval.is_empty()
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

    fn normalized(self) -> Self {
        Self {
            disabled: normalize_patterns(self.disabled),
            require_approval: normalize_patterns(self.require_approval),
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

    if !config.requires_approval(tool_name) {
        return Ok(ToolPolicyPreflight::Allow);
    }

    let command = tool_policy_approval_command(tool_name, raw_arguments);
    if consume_approved_request(data_dir, session_id, &command)?.is_some() {
        return Ok(ToolPolicyPreflight::Allow);
    }

    let reason = format!("tool policy requires approval for `{tool_name}`");
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

fn normalize_patterns(items: Vec<String>) -> Vec<String> {
    items
        .into_iter()
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
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
        ToolPolicyPreflight, evaluate_tool_policy, load_tool_policy_config, matches_tool_pattern,
        tool_policy_approval_command,
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
"#,
        )
        .expect("write config");

        let config = load_tool_policy_config(tmp.path()).expect("policy");
        assert_eq!(config.disabled, vec!["browser_eval"]);
        assert_eq!(config.require_approval, vec!["browser_*", "terminal"]);
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
