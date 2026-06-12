use serde::Serialize;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::config::AppConfig;
use crate::runtime::{RuntimeStatus, RuntimeStatusDetail};
use crate::tool_policy::load_tool_policy_config;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DoctorLevel {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheck {
    pub area: String,
    pub name: String,
    pub level: DoctorLevel,
    pub message: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub status: DoctorLevel,
    pub version: String,
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub fn render_text(&self) -> String {
        let failures = self
            .checks
            .iter()
            .filter(|check| check.level == DoctorLevel::Fail)
            .count();
        let warnings = self
            .checks
            .iter()
            .filter(|check| check.level == DoctorLevel::Warn)
            .count();

        let mut output = String::new();
        output.push_str("Crab Doctor\n");
        output.push_str(&format!(
            "Status: {} ({} failures, {} warnings, {} checks)\n\n",
            level_label(self.status),
            failures,
            warnings,
            self.checks.len()
        ));

        for check in &self.checks {
            output.push_str(&format!(
                "[{}] {}: {}\n",
                level_label(check.level),
                check.name,
                check.message
            ));
            if let Some(detail) = check.detail.as_ref() {
                output.push_str(&format!("     {}\n", detail));
            }
        }

        if warnings > 0 || failures > 0 {
            output.push_str("\nSuggested next steps:\n");
            for step in suggested_steps(&self.checks) {
                output.push_str(&format!("- {step}\n"));
            }
        }

        output
    }
}

pub fn build_doctor_report(config: &AppConfig, runtime_status: RuntimeStatus) -> DoctorReport {
    let mut checks = Vec::new();

    push_check(
        &mut checks,
        "core",
        "Version",
        DoctorLevel::Pass,
        format!("crab {}", env!("CARGO_PKG_VERSION")),
        None,
    );

    let workspace_exists = config.workspace_root.is_dir();
    push_check(
        &mut checks,
        "workspace",
        "Workspace",
        if workspace_exists {
            DoctorLevel::Pass
        } else {
            DoctorLevel::Fail
        },
        if workspace_exists {
            "workspace directory is available".to_string()
        } else {
            "workspace directory does not exist".to_string()
        },
        Some(config.workspace_root.display().to_string()),
    );

    push_check(
        &mut checks,
        "workspace",
        "Local state",
        if config.data_dir.exists() {
            DoctorLevel::Pass
        } else {
            DoctorLevel::Warn
        },
        if config.data_dir.exists() {
            "data directory exists".to_string()
        } else {
            "data directory will be created on the first stateful run".to_string()
        },
        Some(config.data_dir.display().to_string()),
    );

    push_check(
        &mut checks,
        "model",
        "Model endpoint",
        DoctorLevel::Pass,
        format!(
            "{} / {} via {}",
            config.provider_label,
            config.model,
            config.api_mode.as_str()
        ),
        Some(config.base_url.clone()),
    );

    push_check(
        &mut checks,
        "model",
        "Model key",
        if config.api_key.is_some() {
            DoctorLevel::Pass
        } else {
            DoctorLevel::Warn
        },
        if config.api_key.is_some() {
            "API key is configured and redacted from this report".to_string()
        } else {
            "no API key found; no-key commands still work".to_string()
        },
        if config.api_key.is_some() {
            Some(
                "Use environment variables or ignored local config; never commit keys.".to_string(),
            )
        } else {
            Some(
                "Set OPENAI_API_KEY for model-backed chat or keep using debug-context.".to_string(),
            )
        },
    );

    push_check(
        &mut checks,
        "safety",
        "Terminal tool",
        if config.enable_shell_tool {
            DoctorLevel::Warn
        } else {
            DoctorLevel::Pass
        },
        if config.enable_shell_tool {
            "terminal execution is enabled for this run".to_string()
        } else {
            "terminal execution is disabled by default".to_string()
        },
        if config.enable_shell_tool {
            Some("Use only in trusted workspaces.".to_string())
        } else {
            Some("Pass --enable-shell or set HERMES_RS_ENABLE_SHELL=1 when needed.".to_string())
        },
    );

    push_tool_policy_check(&mut checks, &config.data_dir);
    push_runtime_check(&mut checks, runtime_status);
    push_profile_checks(&mut checks, config);
    push_toolchain_checks(&mut checks);
    push_hygiene_checks(&mut checks, &config.workspace_root);
    push_release_checks(&mut checks, &config.workspace_root);

    let status = if checks.iter().any(|check| check.level == DoctorLevel::Fail) {
        DoctorLevel::Fail
    } else if checks.iter().any(|check| check.level == DoctorLevel::Warn) {
        DoctorLevel::Warn
    } else {
        DoctorLevel::Pass
    };

    DoctorReport {
        status,
        version: env!("CARGO_PKG_VERSION").to_string(),
        checks,
    }
}

fn push_tool_policy_check(checks: &mut Vec<DoctorCheck>, data_dir: &Path) {
    match load_tool_policy_config(data_dir) {
        Ok(policy) if policy.is_empty() => push_check(
            checks,
            "safety",
            "Tool policy",
            DoctorLevel::Pass,
            "default tool policy is active".to_string(),
            Some(
                "Set tool_policy in the local config to require approval or disable tools."
                    .to_string(),
            ),
        ),
        Ok(policy) => push_check(
            checks,
            "safety",
            "Tool policy",
            DoctorLevel::Pass,
            "custom tool policy is configured".to_string(),
            Some(format!(
                "require_approval=[{}], disabled=[{}], protected_paths=[{}], disabled_paths=[{}]",
                policy.require_approval.join(", "),
                policy.disabled.join(", "),
                policy.protected_paths.join(", "),
                policy.disabled_paths.join(", ")
            )),
        ),
        Err(error) => push_check(
            checks,
            "safety",
            "Tool policy",
            DoctorLevel::Fail,
            "tool policy config could not be loaded".to_string(),
            Some(format!("{error:#}")),
        ),
    }
}

fn push_runtime_check(checks: &mut Vec<DoctorCheck>, runtime_status: RuntimeStatus) {
    let detail = match runtime_status.detail {
        RuntimeStatusDetail::Local { available } => {
            format!("backend=local, available={available}")
        }
    };
    push_check(
        checks,
        "runtime",
        "Runtime",
        if runtime_status.ready {
            DoctorLevel::Pass
        } else {
            DoctorLevel::Fail
        },
        if runtime_status.ready {
            format!("{} is ready", runtime_status.display_name)
        } else {
            "runtime is not ready".to_string()
        },
        runtime_status.last_error.or(Some(detail)),
    );
}

fn push_profile_checks(checks: &mut Vec<DoctorCheck>, config: &AppConfig) {
    push_check(
        checks,
        "runtime",
        "Browser backend",
        DoctorLevel::Pass,
        format!("{:?}", config.runtime_profile.browser_backend).to_lowercase(),
        Some("Set HERMES_RS_BROWSER_BACKEND to override when testing browser tools.".to_string()),
    );

    push_check(
        checks,
        "runtime",
        "Office tools",
        if config.runtime_profile.office.enabled {
            DoctorLevel::Pass
        } else {
            DoctorLevel::Warn
        },
        if config.runtime_profile.office.enabled {
            "office/PDF-adjacent runtime features are enabled".to_string()
        } else {
            "office runtime features are disabled".to_string()
        },
        Some("Set HERMES_RS_OFFICE_ENABLED=false to disable office integrations.".to_string()),
    );
}

fn push_toolchain_checks(checks: &mut Vec<DoctorCheck>) {
    let required_for_source = [
        ("rustc", "Rust compiler for source builds"),
        ("cargo", "Cargo for source builds and tests"),
        ("git", "Git for repository installs and project inspection"),
    ];
    for (binary, purpose) in required_for_source {
        push_command_check(checks, "toolchain", binary, purpose, DoctorLevel::Warn);
    }

    let optional_tools = [
        ("node", "desktop shell development"),
        ("npm", "desktop shell dependency and build commands"),
        ("swift", "PDF inspection and extraction on Apple platforms"),
    ];
    for (binary, purpose) in optional_tools {
        push_command_check(checks, "optional", binary, purpose, DoctorLevel::Warn);
    }
}

fn push_hygiene_checks(checks: &mut Vec<DoctorCheck>, workspace_root: &Path) {
    let gitignore_path = workspace_root.join(".gitignore");
    let gitignore = fs::read_to_string(&gitignore_path).unwrap_or_default();
    let protects_env =
        gitignore_has_token(&gitignore, ".env") || gitignore_has_token(&gitignore, ".env.*");
    let protects_state = gitignore_has_token(&gitignore, ".hermes-agent-rs");

    push_check(
        checks,
        "hygiene",
        "Secret ignore rules",
        if protects_env && protects_state {
            DoctorLevel::Pass
        } else {
            DoctorLevel::Warn
        },
        if protects_env && protects_state {
            "local env files and runtime state are ignored".to_string()
        } else {
            "review .gitignore before publishing from this checkout".to_string()
        },
        Some(gitignore_path.display().to_string()),
    );

    let env_path = workspace_root.join(".env");
    push_check(
        checks,
        "hygiene",
        "Workspace .env",
        if env_path.exists() {
            DoctorLevel::Warn
        } else {
            DoctorLevel::Pass
        },
        if env_path.exists() {
            "a local .env file exists; keep it untracked and private".to_string()
        } else {
            "no workspace .env file found".to_string()
        },
        Some(env_path.display().to_string()),
    );
}

fn push_release_checks(checks: &mut Vec<DoctorCheck>, workspace_root: &Path) {
    let install_sh = workspace_root.join("scripts/install.sh");
    let install_ps1 = workspace_root.join("scripts/install.ps1");
    let release_workflow = workspace_root.join(".github/workflows/release.yml");

    push_check(
        checks,
        "release",
        "Install scripts",
        if install_sh.is_file() && install_ps1.is_file() {
            DoctorLevel::Pass
        } else {
            DoctorLevel::Warn
        },
        if install_sh.is_file() && install_ps1.is_file() {
            "macOS/Linux and Windows installers are present".to_string()
        } else {
            "one-command installers are incomplete".to_string()
        },
        Some("scripts/install.sh, scripts/install.ps1".to_string()),
    );

    push_check(
        checks,
        "release",
        "Release workflow",
        if release_workflow.is_file() {
            DoctorLevel::Pass
        } else {
            DoctorLevel::Warn
        },
        if release_workflow.is_file() {
            "GitHub release packaging workflow is present".to_string()
        } else {
            "release packaging workflow is missing".to_string()
        },
        Some(release_workflow.display().to_string()),
    );
}

fn push_command_check(
    checks: &mut Vec<DoctorCheck>,
    area: &str,
    binary: &str,
    purpose: &str,
    missing_level: DoctorLevel,
) {
    match command_version(binary) {
        Some(version) => push_check(
            checks,
            area,
            binary,
            DoctorLevel::Pass,
            version,
            Some(purpose.to_string()),
        ),
        None => push_check(
            checks,
            area,
            binary,
            missing_level,
            format!("{binary} was not found on PATH"),
            Some(format!("Needed for {purpose}.")),
        ),
    }
}

fn command_version(binary: &str) -> Option<String> {
    let output = Command::new(binary).arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }

    let combined = if output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stderr).to_string()
    } else {
        String::from_utf8_lossy(&output.stdout).to_string()
    };
    combined
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(|line| truncate(line.trim(), 120))
}

fn push_check(
    checks: &mut Vec<DoctorCheck>,
    area: impl Into<String>,
    name: impl Into<String>,
    level: DoctorLevel,
    message: impl Into<String>,
    detail: Option<String>,
) {
    checks.push(DoctorCheck {
        area: area.into(),
        name: name.into(),
        level,
        message: message.into(),
        detail,
    });
}

fn suggested_steps(checks: &[DoctorCheck]) -> Vec<String> {
    let mut steps = Vec::new();
    if checks
        .iter()
        .any(|check| check.name == "Model key" && check.level == DoctorLevel::Warn)
    {
        steps.push(
            "Set OPENAI_API_KEY, OPENAI_BASE_URL, and HERMES_RS_MODEL before model-backed runs."
                .to_string(),
        );
    }
    if checks
        .iter()
        .any(|check| check.name == "Terminal tool" && check.level == DoctorLevel::Warn)
    {
        steps.push(
            "Disable shell access unless this is a trusted automation workspace.".to_string(),
        );
    }
    if checks.iter().any(|check| {
        matches!(check.area.as_str(), "toolchain" | "optional") && check.level == DoctorLevel::Warn
    }) {
        steps.push(
            "Install missing optional toolchain pieces only for the workflows you plan to use."
                .to_string(),
        );
    }
    if checks
        .iter()
        .any(|check| check.area == "hygiene" && check.level == DoctorLevel::Warn)
    {
        steps.push(
            "Review ignored local files before publishing demos or release artifacts.".to_string(),
        );
    }
    if steps.is_empty() {
        steps.push("Run `crab debug-context --prompt \"Explain Crab's agent loop.\"` for a no-key smoke test.".to_string());
    }
    steps
}

fn gitignore_has_token(content: &str, token: &str) -> bool {
    let normalized_token = normalize_gitignore_token(token);
    content.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && normalize_gitignore_token(trimmed) == normalized_token
    })
}

fn normalize_gitignore_token(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('/')
        .trim_end_matches('/')
        .to_string()
}

fn truncate(value: &str, max_len: usize) -> String {
    let char_count = value.chars().count();
    if char_count <= max_len {
        value.to_string()
    } else {
        let keep = max_len.saturating_sub(3);
        format!("{}...", value.chars().take(keep).collect::<String>())
    }
}

fn level_label(level: DoctorLevel) -> &'static str {
    match level {
        DoctorLevel::Pass => "ok",
        DoctorLevel::Warn => "warn",
        DoctorLevel::Fail => "fail",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitignore_tokens_ignore_leading_slashes() {
        let content = "/.hermes-agent-rs\n.env.*\n";
        assert!(gitignore_has_token(content, ".hermes-agent-rs"));
        assert!(gitignore_has_token(content, ".env.*"));
        assert!(!gitignore_has_token(content, "target"));
    }

    #[test]
    fn report_status_prefers_failures_over_warnings() {
        let report = DoctorReport {
            status: DoctorLevel::Fail,
            version: "0.1.0".to_string(),
            checks: vec![
                DoctorCheck {
                    area: "a".to_string(),
                    name: "A".to_string(),
                    level: DoctorLevel::Warn,
                    message: "warn".to_string(),
                    detail: None,
                },
                DoctorCheck {
                    area: "b".to_string(),
                    name: "B".to_string(),
                    level: DoctorLevel::Fail,
                    message: "fail".to_string(),
                    detail: None,
                },
            ],
        };
        let rendered = report.render_text();
        assert!(rendered.contains("Status: fail"));
        assert!(rendered.contains("1 failures"));
        assert!(rendered.contains("1 warnings"));
    }

    #[test]
    fn tool_policy_check_reports_custom_policy() {
        let tmp = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            tmp.path().join("config.yaml"),
            r#"tool_policy:
  require_approval:
    - terminal
  disabled:
    - browser_eval
  protected_paths:
    - .env*
"#,
        )
        .expect("write config");

        let mut checks = Vec::new();
        push_tool_policy_check(&mut checks, tmp.path());

        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].level, DoctorLevel::Pass);
        assert_eq!(checks[0].name, "Tool policy");
        assert!(checks[0].message.contains("custom tool policy"));
        assert!(
            checks[0]
                .detail
                .as_deref()
                .unwrap_or_default()
                .contains("terminal")
        );
        assert!(
            checks[0]
                .detail
                .as_deref()
                .unwrap_or_default()
                .contains(".env*")
        );
    }
}
