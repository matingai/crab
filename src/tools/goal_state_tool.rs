use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::goal_state::{
    CognitionItem, EvidenceItem, GoalItem, GoalState, GoalStateStore, HotDataItem,
};
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct GoalStateTool;

#[derive(Debug, Deserialize)]
struct GoalStateArgs {
    action: String,
    mission: Option<String>,
    phase: Option<String>,
    current_focus_goal_id: Option<Option<String>>,
    reflection: Option<String>,
    goal_id: Option<String>,
    goals: Option<Vec<GoalInput>>,
    cognition: Option<Vec<CognitionInput>>,
    hot_data: Option<Vec<HotDataInput>>,
}

#[derive(Debug, Deserialize)]
struct GoalInput {
    id: Option<String>,
    title: Option<String>,
    level: Option<String>,
    status: Option<String>,
    confidence: Option<f32>,
    parent_id: Option<String>,
    summary: Option<String>,
    evidence: Option<String>,
    evidence_items: Option<Vec<EvidenceInput>>,
}

#[derive(Debug, Deserialize)]
struct CognitionInput {
    id: Option<String>,
    kind: Option<String>,
    content: Option<String>,
    confidence: Option<f32>,
    evidence: Option<String>,
    evidence_items: Option<Vec<EvidenceInput>>,
}

#[derive(Debug, Deserialize)]
struct HotDataInput {
    id: Option<String>,
    content: Option<String>,
    confidence: Option<f32>,
    source: Option<String>,
    goal_id: Option<String>,
    expires_at_unix: Option<u64>,
    evidence_items: Option<Vec<EvidenceInput>>,
}

#[derive(Debug, Deserialize)]
struct EvidenceInput {
    source_type: Option<String>,
    source: Option<String>,
    summary: Option<String>,
    relation: Option<String>,
    observed_at_unix: Option<u64>,
}

#[async_trait]
impl Tool for GoalStateTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "goal_state",
            "Maintain the current goal hierarchy, mission, phase, reflection, working cognition, and hot recall data for this session. Use it to track long_term/current/subgoal targets, the active focus goal, status transitions, confidence, evidence, assumptions, risks, unknowns, and current execution phase.",
            object_schema(
                json!({
                    "action": {
                        "type": "string",
                        "enum": ["read", "replace", "merge", "clear", "focus"],
                        "description": "Goal-state action."
                    },
                    "mission": {
                        "type": "string",
                        "description": "Short current mission statement for replace/merge."
                    },
                    "phase": {
                        "type": "string",
                        "enum": ["understand", "investigate", "act", "verify", "finalize"],
                        "description": "Current execution phase for replace/merge."
                    },
                    "current_focus_goal_id": {
                        "type": "string",
                        "description": "Focus goal id for replace/merge."
                    },
                    "reflection": {
                        "type": "string",
                        "description": "Short self-assessment of the current strategy, blocker, or recent adjustment for replace/merge."
                    },
                    "goal_id": {
                        "type": "string",
                        "description": "Goal id for action=focus."
                    },
                    "goals": {
                        "type": "array",
                        "description": "Goal hierarchy items. Use level=long_term/current/subgoal and status=pending/in_progress/blocked/succeeded/failed/transferred/cancelled.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "title": { "type": "string" },
                                "level": { "type": "string", "enum": ["long_term", "current", "subgoal"] },
                                "status": { "type": "string", "enum": ["pending", "in_progress", "blocked", "succeeded", "failed", "transferred", "cancelled"] },
                                "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
                                "parent_id": { "type": "string" },
                                "summary": { "type": "string" },
                                "evidence": { "type": "string" },
                                "evidence_items": { "$ref": "#/$defs/evidenceItems" }
                            },
                            "additionalProperties": false
                        }
                    },
                    "cognition": {
                        "type": "array",
                        "description": "Working cognition items such as facts, assumptions, unknowns, risks, and decisions.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "kind": { "type": "string", "enum": ["fact", "assumption", "unknown", "risk", "decision"] },
                                "content": { "type": "string" },
                                "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
                                "evidence": { "type": "string" },
                                "evidence_items": { "$ref": "#/$defs/evidenceItems" }
                            },
                            "additionalProperties": false
                        }
                    },
                    "hot_data": {
                        "type": "array",
                        "description": "Short-lived, high-utility recall items that should stay easy to retrieve while working on the current focus goal.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "id": { "type": "string" },
                                "content": { "type": "string" },
                                "confidence": { "type": "number", "minimum": 0, "maximum": 1 },
                                "source": { "type": "string" },
                                "goal_id": { "type": "string" },
                                "expires_at_unix": { "type": "integer", "minimum": 1 },
                                "evidence_items": { "$ref": "#/$defs/evidenceItems" }
                            },
                            "additionalProperties": false
                        }
                    },
                    "$defs": {
                        "evidenceItems": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "source_type": { "type": "string", "enum": ["user_input", "tool_output", "file_verification", "model_inference", "assistant_response", "system_state", "session_history", "legacy"] },
                                    "source": { "type": "string" },
                                    "summary": { "type": "string" },
                                    "relation": { "type": "string", "enum": ["supports", "conflicts", "context"] },
                                    "observed_at_unix": { "type": "integer", "minimum": 1 }
                                },
                                "required": ["summary"],
                                "additionalProperties": false
                            }
                        }
                    }
                }),
                &["action"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: GoalStateArgs =
            serde_json::from_value(args).context("invalid goal_state arguments")?;
        let store = GoalStateStore::new(&ctx.data_dir)?;

        let state = match args.action.as_str() {
            "read" => store.load(&ctx.current_session_id)?,
            "clear" => {
                store.clear(&ctx.current_session_id)?;
                GoalState {
                    mission: String::new(),
                    phase: "understand".to_string(),
                    current_focus_goal_id: None,
                    reflection: String::new(),
                    goals: Vec::new(),
                    cognition: Vec::new(),
                    hot_data: Vec::new(),
                    updated_at_unix: 0,
                }
            }
            "replace" => store.replace(
                &ctx.current_session_id,
                GoalState {
                    mission: args.mission.unwrap_or_default(),
                    phase: args.phase.unwrap_or_else(|| "understand".to_string()),
                    current_focus_goal_id: args.current_focus_goal_id.flatten(),
                    reflection: args.reflection.unwrap_or_default(),
                    goals: to_goal_items(args.goals.unwrap_or_default()),
                    cognition: to_cognition_items(args.cognition.unwrap_or_default()),
                    hot_data: to_hot_data_items(args.hot_data.unwrap_or_default()),
                    updated_at_unix: 0,
                },
            )?,
            "merge" => store.merge(
                &ctx.current_session_id,
                args.mission,
                args.phase,
                args.current_focus_goal_id,
                args.reflection,
                to_goal_items(args.goals.unwrap_or_default()),
                to_cognition_items(args.cognition.unwrap_or_default()),
                to_hot_data_items(args.hot_data.unwrap_or_default()),
            )?,
            "focus" => store.set_focus(&ctx.current_session_id, args.goal_id.as_deref())?,
            other => bail!("unsupported goal_state action `{other}`"),
        };

        serde_json::to_string_pretty(&state).context("failed to serialize goal_state response")
    }
}

fn to_goal_items(inputs: Vec<GoalInput>) -> Vec<GoalItem> {
    inputs
        .into_iter()
        .map(|item| GoalItem {
            id: item.id.unwrap_or_default(),
            title: item.title.unwrap_or_default(),
            level: item.level.unwrap_or_else(|| "current".to_string()),
            status: item.status.unwrap_or_else(|| "pending".to_string()),
            confidence: item.confidence.unwrap_or(0.5),
            parent_id: item.parent_id,
            summary: item.summary.unwrap_or_default(),
            evidence: item.evidence.unwrap_or_default(),
            evidence_items: to_evidence_items(item.evidence_items.unwrap_or_default()),
            updated_at_unix: 0,
        })
        .collect()
}

fn to_cognition_items(inputs: Vec<CognitionInput>) -> Vec<CognitionItem> {
    inputs
        .into_iter()
        .map(|item| CognitionItem {
            id: item.id.unwrap_or_default(),
            kind: item.kind.unwrap_or_else(|| "fact".to_string()),
            content: item.content.unwrap_or_default(),
            confidence: item.confidence.unwrap_or(0.5),
            evidence: item.evidence.unwrap_or_default(),
            evidence_items: to_evidence_items(item.evidence_items.unwrap_or_default()),
            updated_at_unix: 0,
        })
        .collect()
}

fn to_hot_data_items(inputs: Vec<HotDataInput>) -> Vec<HotDataItem> {
    inputs
        .into_iter()
        .map(|item| HotDataItem {
            id: item.id.unwrap_or_default(),
            content: item.content.unwrap_or_default(),
            confidence: item.confidence.unwrap_or(0.5),
            source: item.source.unwrap_or_default(),
            goal_id: item.goal_id,
            expires_at_unix: item.expires_at_unix,
            evidence_items: to_evidence_items(item.evidence_items.unwrap_or_default()),
            updated_at_unix: 0,
        })
        .collect()
}

fn to_evidence_items(inputs: Vec<EvidenceInput>) -> Vec<EvidenceItem> {
    inputs
        .into_iter()
        .map(|item| EvidenceItem {
            source_type: item
                .source_type
                .unwrap_or_else(|| "system_state".to_string()),
            source: item.source.unwrap_or_default(),
            summary: item.summary.unwrap_or_default(),
            relation: item.relation.unwrap_or_else(|| "supports".to_string()),
            observed_at_unix: item.observed_at_unix.unwrap_or(0),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::GoalStateTool;
    use crate::tools::{Tool, ToolContext};
    use serde_json::{Value, json};

    fn test_context(root: &std::path::Path) -> ToolContext {
        ToolContext {
            workspace_root: root.to_path_buf(),
            data_dir: root.join(".data"),
            shell_enabled: false,
            skill_platform: "desktop".to_string(),
            provider_id: "openai".to_string(),
            model: "test-model".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            api_key: None,
            api_mode: crate::llm::ApiMode::ChatCompletions,
            worker_model: None,
            max_iterations: 4,
            current_session_id: "session-1".to_string(),
            current_delegate_run_id: None,
            delegate_depth: 0,
        }
    }

    #[tokio::test]
    async fn goal_state_replace_and_focus() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = GoalStateTool;
        let ctx = test_context(tmp.path());

        let replaced = tool
            .execute(
                json!({
                    "action": "replace",
                    "current_focus_goal_id": "g2",
                    "goals": [
                        { "id": "g1", "title": "Long term", "level": "long_term", "status": "pending", "confidence": 0.7 },
                        { "id": "g2", "title": "Current", "level": "current", "status": "in_progress", "confidence": 0.9, "parent_id": "g1" }
                    ],
                    "cognition": [
                        { "id": "c1", "kind": "fact", "content": "Need a first version", "confidence": 0.95 }
                    ],
                    "hot_data": [
                        { "id": "h1", "content": "todo covers execution tasks", "confidence": 0.8, "goal_id": "g2" }
                    ]
                }),
                &ctx,
            )
            .await
            .expect("replace");
        let replaced_json: Value = serde_json::from_str(&replaced).expect("json");
        assert_eq!(
            replaced_json
                .get("current_focus_goal_id")
                .and_then(Value::as_str),
            Some("g2")
        );

        let focused = tool
            .execute(
                json!({
                    "action": "focus",
                    "goal_id": "g1"
                }),
                &ctx,
            )
            .await
            .expect("focus");
        let focused_json: Value = serde_json::from_str(&focused).expect("json");
        assert_eq!(
            focused_json
                .get("current_focus_goal_id")
                .and_then(Value::as_str),
            Some("g1")
        );
    }

    #[tokio::test]
    async fn goal_state_merge_updates_items() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let tool = GoalStateTool;
        let ctx = test_context(tmp.path());

        tool.execute(
            json!({
                "action": "merge",
                "goals": [
                    { "id": "g1", "title": "Current", "level": "current", "status": "pending", "confidence": 0.4 }
                ]
            }),
            &ctx,
        )
        .await
        .expect("seed");

        let merged = tool
            .execute(
                json!({
                    "action": "merge",
                    "mission": "Unblock the current task",
                    "phase": "investigate",
                    "reflection": "Need clarification before resuming implementation.",
                    "goals": [
                        { "id": "g1", "title": "Current", "level": "current", "status": "blocked", "confidence": 0.7, "summary": "Need clarification" }
                    ],
                    "cognition": [
                        { "id": "c1", "kind": "unknown", "content": "Need user clarification", "confidence": 0.2 }
                    ]
                }),
                &ctx,
            )
            .await
            .expect("merge");
        let merged_json: Value = serde_json::from_str(&merged).expect("json");
        assert_eq!(
            merged_json["mission"].as_str(),
            Some("Unblock the current task")
        );
        assert_eq!(merged_json["phase"].as_str(), Some("investigate"));
        assert_eq!(
            merged_json["reflection"].as_str(),
            Some("Need clarification before resuming implementation.")
        );
        assert_eq!(merged_json["goals"][0]["status"].as_str(), Some("blocked"));
        assert_eq!(merged_json["cognition"].as_array().map(Vec::len), Some(1));
    }
}
