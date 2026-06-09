use crate::skills::SkillMatch;
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillLifecycleAction {
    Create,
    Update,
}

impl SkillLifecycleAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillLifecycleSuggestion {
    pub action: SkillLifecycleAction,
    pub category: String,
    pub name: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub task_kinds: Vec<String>,
    pub requires_tools: Vec<String>,
    pub requires_shell: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct SkillAdviceInput {
    pub user_input: String,
    pub matched_skills: Vec<SkillMatch>,
    pub tool_names_used: Vec<String>,
    pub skill_manage_used: bool,
    pub shell_enabled: bool,
}

pub fn suggest_skill_lifecycle(input: &SkillAdviceInput) -> Option<SkillLifecycleSuggestion> {
    if input.skill_manage_used {
        return None;
    }

    let normalized_tools = normalize_tags(&input.tool_names_used);
    let task_kinds = detect_task_kinds(&input.user_input);
    let name = suggest_skill_name(&input.user_input);
    let category = suggest_category(&task_kinds);
    let description = build_description(&input.user_input, &task_kinds);
    let keywords = collect_keywords(&input.user_input, &task_kinds, &normalized_tools, &name);

    if input.tool_names_used.len() >= 4 {
        if let Some(existing) = input.matched_skills.first() {
            return Some(SkillLifecycleSuggestion {
                action: SkillLifecycleAction::Update,
                category: existing.document.summary.category.clone(),
                name: existing.document.summary.name.clone(),
                description: existing.document.summary.description.clone(),
                keywords,
                task_kinds: if existing.document.summary.activation.task_kinds.is_empty() {
                    task_kinds
                } else {
                    existing.document.summary.activation.task_kinds.clone()
                },
                requires_tools: merge_lists(
                    &existing.document.summary.activation.requires_tools,
                    &normalized_tools,
                ),
                requires_shell: existing.document.summary.activation.requires_shell
                    || input.shell_enabled,
                reason: format!(
                    "This turn used {} tool calls and overlapped with an existing skill. Consider updating `{}/{}` with the refined workflow.",
                    input.tool_names_used.len(),
                    existing.document.summary.category,
                    existing.document.summary.name
                ),
            });
        }
    }

    if input.tool_names_used.len() >= 5 {
        return Some(SkillLifecycleSuggestion {
            action: SkillLifecycleAction::Create,
            category,
            name,
            description,
            keywords,
            task_kinds,
            requires_tools: normalized_tools,
            requires_shell: input.shell_enabled,
            reason: format!(
                "This turn used {} tool calls without saving a skill. The workflow looks reusable.",
                input.tool_names_used.len()
            ),
        });
    }

    None
}

fn suggest_category(task_kinds: &[String]) -> String {
    if task_kinds.iter().any(|kind| kind == "operations") {
        return "ops".to_string();
    }
    if task_kinds.iter().any(|kind| kind == "documentation") {
        return "docs".to_string();
    }
    if task_kinds.iter().any(|kind| kind == "planning") {
        return "planning".to_string();
    }
    "coding".to_string()
}

fn build_description(user_input: &str, task_kinds: &[String]) -> String {
    let prefix = if task_kinds.iter().any(|kind| kind == "debugging") {
        "Reusable debugging workflow"
    } else if task_kinds.iter().any(|kind| kind == "documentation") {
        "Reusable documentation workflow"
    } else if task_kinds.iter().any(|kind| kind == "analysis") {
        "Reusable analysis workflow"
    } else {
        "Reusable coding workflow"
    };
    format!("{prefix} for: {}", truncate(user_input, 100))
}

fn suggest_skill_name(user_input: &str) -> String {
    let stop_words = [
        "the", "a", "an", "this", "that", "with", "from", "into", "for", "and", "or", "but",
        "please", "help", "me", "you", "about", "need", "want", "should",
    ];
    let stop_words = stop_words.into_iter().collect::<HashSet<_>>();

    let parts = user_input
        .split(|ch: char| !ch.is_alphanumeric())
        .map(normalize_tag)
        .filter(|part| part.len() >= 3)
        .filter(|part| !stop_words.contains(part.as_str()))
        .take(5)
        .collect::<Vec<_>>();

    if parts.is_empty() {
        "general-workflow".to_string()
    } else {
        parts.join("-")
    }
}

fn collect_keywords(
    user_input: &str,
    task_kinds: &[String],
    tools: &[String],
    name: &str,
) -> Vec<String> {
    let mut values = Vec::new();
    values.extend(task_kinds.iter().cloned());
    values.extend(tools.iter().take(4).cloned());
    values.extend(
        user_input
            .split(|ch: char| !ch.is_alphanumeric())
            .map(normalize_tag)
            .filter(|part| part.len() >= 4)
            .take(6),
    );
    values.push(name.to_string());
    dedupe(values)
}

fn detect_task_kinds(query: &str) -> Vec<String> {
    let lowered = query.to_lowercase();
    let mut kinds = Vec::new();

    if contains_any(
        &lowered,
        &["fix", "bug", "debug", "failing", "error", "broken", "issue"],
    ) {
        kinds.push("debugging".to_string());
    }
    if contains_any(
        &lowered,
        &[
            "implement",
            "write",
            "create",
            "build",
            "add",
            "refactor",
            "port",
        ],
    ) {
        kinds.push("coding".to_string());
    }
    if contains_any(
        &lowered,
        &[
            "review",
            "explain",
            "summarize",
            "analyze",
            "inspect",
            "understand",
            "learn",
        ],
    ) {
        kinds.push("analysis".to_string());
    }
    if contains_any(&lowered, &["plan", "design", "architecture", "approach"]) {
        kinds.push("planning".to_string());
    }
    if contains_any(&lowered, &["doc", "readme", "document"]) {
        kinds.push("documentation".to_string());
    }
    if contains_any(
        &lowered,
        &[
            "deploy",
            "release",
            "install",
            "docker",
            "infra",
            "kubernetes",
            "environment",
        ],
    ) {
        kinds.push("operations".to_string());
    }

    if kinds.is_empty() {
        kinds.push("general".to_string());
    }

    dedupe(kinds)
}

fn merge_lists(left: &[String], right: &[String]) -> Vec<String> {
    let mut values = Vec::new();
    values.extend(left.iter().cloned());
    values.extend(right.iter().cloned());
    dedupe(values)
}

fn normalize_tags(values: &[String]) -> Vec<String> {
    dedupe(values.iter().map(|value| normalize_tag(value)).collect())
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .into_iter()
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn normalize_tag(value: &str) -> String {
    value.trim().to_lowercase()
}

fn truncate(value: &str, max_len: usize) -> String {
    if value.chars().count() <= max_len {
        return value.trim().to_string();
    }
    let mut clipped = value.chars().take(max_len).collect::<String>();
    clipped.push_str("...");
    clipped
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::{SkillAdviceInput, SkillLifecycleAction, suggest_skill_lifecycle};
    use crate::skills::{SkillActivation, SkillDocument, SkillMatch, SkillSummary};
    use std::path::PathBuf;

    #[test]
    fn suggests_creating_skill_for_tool_heavy_new_workflow() {
        let suggestion = suggest_skill_lifecycle(&SkillAdviceInput {
            user_input: "implement auth middleware and patch route handlers".to_string(),
            matched_skills: Vec::new(),
            tool_names_used: vec![
                "read_file".to_string(),
                "search_files".to_string(),
                "write_file".to_string(),
                "patch_file".to_string(),
                "list_files".to_string(),
            ],
            skill_manage_used: false,
            shell_enabled: false,
        })
        .expect("suggestion");

        assert_eq!(suggestion.action, SkillLifecycleAction::Create);
        assert_eq!(suggestion.category, "coding");
        assert!(suggestion.name.contains("implement"));
        assert!(
            suggestion
                .requires_tools
                .contains(&"patch_file".to_string())
        );
    }

    #[test]
    fn suggests_updating_matching_skill_when_workflow_expands() {
        let matched = SkillMatch {
            document: SkillDocument {
                summary: SkillSummary {
                    category: "coding".to_string(),
                    name: "rust-review".to_string(),
                    description: "Review Rust code carefully.".to_string(),
                    keywords: vec!["rust".to_string()],
                    activation: SkillActivation {
                        task_kinds: vec!["analysis".to_string()],
                        requires_tools: vec!["read_file".to_string()],
                        requires_shell: false,
                    },
                    path: PathBuf::from("/tmp/SKILL.md"),
                    updated_at_unix: None,
                },
                content: "# Rust Review".to_string(),
            },
            score: 12,
        };

        let suggestion = suggest_skill_lifecycle(&SkillAdviceInput {
            user_input: "review rust module and explain ownership issues".to_string(),
            matched_skills: vec![matched],
            tool_names_used: vec![
                "read_file".to_string(),
                "search_files".to_string(),
                "patch_file".to_string(),
                "write_file".to_string(),
            ],
            skill_manage_used: false,
            shell_enabled: false,
        })
        .expect("suggestion");

        assert_eq!(suggestion.action, SkillLifecycleAction::Update);
        assert_eq!(suggestion.name, "rust-review");
        assert!(
            suggestion
                .requires_tools
                .contains(&"patch_file".to_string())
        );
    }
}
