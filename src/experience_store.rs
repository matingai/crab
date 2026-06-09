use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::solve_trace::SolveEpisode;

const MAX_CONTEXT_EXPERIENCES: usize = 2;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExperienceState {
    #[serde(default)]
    pub records: Vec<ExperienceRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceRecord {
    pub id: String,
    pub source_episode_id: String,
    #[serde(default)]
    pub kind: String,
    pub problem_frame: String,
    #[serde(default)]
    pub signals: Vec<String>,
    #[serde(default)]
    pub successful_strategy: Vec<String>,
    #[serde(default)]
    pub failure_patterns: Vec<String>,
    pub outcome: String,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    #[serde(default)]
    pub created_at_unix: u64,
    #[serde(default)]
    pub updated_at_unix: u64,
}

impl ExperienceRecord {
    pub fn normalize(&mut self) {
        self.id = normalized_id(&self.id, "experience");
        self.source_episode_id = normalized_id(&self.source_episode_id, "episode");
        self.kind = normalized_text(&self.kind, "solve_pattern");
        self.problem_frame = normalized_text(&self.problem_frame, "(unspecified problem)");
        dedupe_strings(&mut self.signals, 5, 160);
        dedupe_strings(&mut self.successful_strategy, 5, 180);
        dedupe_strings(&mut self.failure_patterns, 4, 180);
        self.outcome = normalized_text(&self.outcome, "(unspecified outcome)");
        self.confidence = self.confidence.clamp(0.0, 1.0);
        if self.created_at_unix == 0 {
            self.created_at_unix = unix_now();
        }
        if self.updated_at_unix == 0 {
            self.updated_at_unix = self.created_at_unix;
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExperienceStore {
    root: PathBuf,
}

impl ExperienceStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("experience"))
            .with_context(|| format!("failed to create experience dir under {}", root.display()))?;
        Ok(Self { root })
    }

    pub fn load(&self, session_id: &str) -> Result<ExperienceState> {
        let path = self.state_path(session_id);
        if !path.exists() {
            return Ok(ExperienceState::default());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read experience state {}", path.display()))?;
        let mut state: ExperienceState = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse experience state {}", path.display()))?;
        normalize_state(&mut state);
        Ok(state)
    }

    pub fn save(&self, session_id: &str, state: &ExperienceState) -> Result<PathBuf> {
        let path = self.state_path(session_id);
        let tmp = path.with_extension("json.tmp");
        let mut normalized = state.clone();
        normalize_state(&mut normalized);
        let raw = serde_json::to_string_pretty(&normalized)
            .context("failed to serialize experience state")?;
        fs::write(&tmp, raw).with_context(|| format!("failed to write {}", tmp.display()))?;
        fs::rename(&tmp, &path)
            .with_context(|| format!("failed to move {} to {}", tmp.display(), path.display()))?;
        Ok(path)
    }

    pub fn clear(&self, session_id: &str) -> Result<()> {
        let path = self.state_path(session_id);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
        Ok(())
    }

    pub fn upsert_record(&self, session_id: &str, record: ExperienceRecord) -> Result<()> {
        let mut state = self.load(session_id)?;
        if let Some(existing) = state
            .records
            .iter_mut()
            .find(|item| item.id == record.id || item.source_episode_id == record.source_episode_id)
        {
            *existing = record;
        } else {
            state.records.push(record);
        }
        self.save(session_id, &state)?;
        Ok(())
    }

    pub fn build_context_block(
        &self,
        session_id: &str,
        query: &str,
        exclude_source_episode_id: Option<&str>,
    ) -> Result<Option<String>> {
        let state = self.load(session_id)?;
        let ranked = rank_records(&state.records, query, exclude_source_episode_id);
        if ranked.is_empty() {
            return Ok(None);
        }

        let mut lines = vec![
            "<experience-context>".to_string(),
            "[System note: The following is distilled reusable experience from prior solve episodes. Treat it as optional heuristic guidance. Use it when it matches the current evidence, and ignore it when it does not.]".to_string(),
            String::new(),
        ];
        for record in ranked.into_iter().take(MAX_CONTEXT_EXPERIENCES) {
            lines.push(format!(
                "- problem={} | kind={} | confidence={:.2}",
                record.problem_frame, record.kind, record.confidence
            ));
            if !record.signals.is_empty() {
                lines.push(format!(
                    "  signals: {}",
                    record
                        .signals
                        .iter()
                        .take(3)
                        .map(|item| inline_clip(item, 90))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            if !record.successful_strategy.is_empty() {
                lines.push(format!(
                    "  strategy: {}",
                    record
                        .successful_strategy
                        .iter()
                        .take(3)
                        .map(|item| inline_clip(item, 100))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            if !record.failure_patterns.is_empty() {
                lines.push(format!(
                    "  failure_patterns: {}",
                    record
                        .failure_patterns
                        .iter()
                        .take(2)
                        .map(|item| inline_clip(item, 100))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            lines.push(format!("  outcome: {}", inline_clip(&record.outcome, 120)));
        }
        lines.push("</experience-context>".to_string());
        Ok(Some(lines.join("\n")))
    }

    fn state_path(&self, session_id: &str) -> PathBuf {
        self.root
            .join("experience")
            .join(format!("{session_id}.json"))
    }
}

pub fn derive_experience_from_episode(episode: &SolveEpisode) -> Option<ExperienceRecord> {
    let outcome = episode.outcome.as_ref()?;

    let signals = episode
        .supplements
        .iter()
        .take(3)
        .cloned()
        .chain(
            episode
                .steps
                .iter()
                .filter(|step| !step.observation.is_empty())
                .take(2)
                .map(|step| step.observation.clone()),
        )
        .collect::<Vec<_>>();

    let mut successful_strategy = episode
        .steps
        .iter()
        .filter(|step| matches!(step.status.as_str(), "completed" | "in_progress"))
        .take(3)
        .map(|step| step.action.clone())
        .collect::<Vec<_>>();
    successful_strategy.extend(
        episode
            .decisions
            .iter()
            .take(2)
            .map(|decision| decision.chosen.clone()),
    );

    let mut failure_patterns = episode
        .supplements
        .iter()
        .filter(|item| {
            let lowered = item.to_ascii_lowercase();
            lowered.starts_with("risk:") || lowered.contains("mask") || lowered.contains("block")
        })
        .take(3)
        .cloned()
        .collect::<Vec<_>>();
    failure_patterns.extend(
        episode
            .steps
            .iter()
            .filter(|step| step.status == "blocked")
            .take(2)
            .map(|step| {
                if step.observation.is_empty() {
                    step.action.clone()
                } else {
                    step.observation.clone()
                }
            }),
    );

    let mut record = ExperienceRecord {
        id: format!("exp:{}", episode.id),
        source_episode_id: episode.id.clone(),
        kind: infer_experience_kind(episode),
        problem_frame: episode
            .focus_goal_title
            .clone()
            .unwrap_or_else(|| episode.goal.clone()),
        signals,
        successful_strategy,
        failure_patterns,
        outcome: outcome.summary.clone(),
        confidence: derive_confidence(episode),
        created_at_unix: episode.created_at_unix,
        updated_at_unix: unix_now(),
    };
    record.normalize();
    Some(record)
}

fn infer_experience_kind(episode: &SolveEpisode) -> String {
    let lowered = format!(
        "{} {}",
        episode.goal.to_ascii_lowercase(),
        episode.user_input.to_ascii_lowercase()
    );
    if lowered.contains("debug") || lowered.contains("error") || lowered.contains("fix") {
        "debug_pattern".to_string()
    } else if lowered.contains("investigate") || lowered.contains("analyze") {
        "investigation_pattern".to_string()
    } else {
        "solve_pattern".to_string()
    }
}

fn derive_confidence(episode: &SolveEpisode) -> f32 {
    match episode.status.as_str() {
        "completed" => 0.8,
        "blocked" => 0.62,
        "failed" => 0.55,
        _ => 0.5,
    }
}

fn rank_records<'a>(
    records: &'a [ExperienceRecord],
    query: &str,
    exclude_source_episode_id: Option<&str>,
) -> Vec<&'a ExperienceRecord> {
    let query_tokens = tokenize(query);
    let mut ranked = records
        .iter()
        .filter(|record| exclude_source_episode_id.is_none_or(|id| record.source_episode_id != id))
        .map(|record| (score_record(record, &query_tokens), record))
        .filter(|(score, _)| *score > 0)
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.updated_at_unix.cmp(&left.1.updated_at_unix))
    });
    ranked.into_iter().map(|(_, record)| record).collect()
}

fn score_record(record: &ExperienceRecord, query_tokens: &HashSet<String>) -> usize {
    let mut haystack = vec![record.problem_frame.as_str(), record.outcome.as_str()];
    haystack.extend(record.signals.iter().map(String::as_str));
    haystack.extend(record.successful_strategy.iter().map(String::as_str));
    haystack.extend(record.failure_patterns.iter().map(String::as_str));
    haystack
        .into_iter()
        .map(tokenize)
        .map(|tokens| tokens.intersection(query_tokens).count())
        .sum::<usize>()
        + 1
}

fn normalize_state(state: &mut ExperienceState) {
    for record in &mut state.records {
        record.normalize();
    }
    dedupe_records(&mut state.records);
}

fn dedupe_records(items: &mut Vec<ExperienceRecord>) {
    let mut deduped: Vec<ExperienceRecord> = Vec::with_capacity(items.len());
    for item in items.drain(..) {
        if let Some(existing) = deduped.iter_mut().find(|record| {
            record.id == item.id || record.source_episode_id == item.source_episode_id
        }) {
            *existing = item;
        } else {
            deduped.push(item);
        }
    }
    deduped.sort_by(|left, right| left.created_at_unix.cmp(&right.created_at_unix));
    *items = deduped;
}

fn dedupe_strings(items: &mut Vec<String>, max_items: usize, max_chars: usize) {
    let mut seen = HashSet::new();
    let mut deduped = Vec::with_capacity(items.len());
    for item in items.drain(..) {
        let normalized = inline_clip(item.trim(), max_chars);
        if normalized.is_empty() || !seen.insert(normalized.clone()) {
            continue;
        }
        deduped.push(normalized);
        if deduped.len() >= max_items {
            break;
        }
    }
    *items = deduped;
}

fn tokenize(value: &str) -> HashSet<String> {
    value
        .split(|ch: char| !ch.is_alphanumeric())
        .filter_map(|token| {
            let token = token.trim().to_ascii_lowercase();
            (!token.is_empty() && token.len() > 1).then_some(token)
        })
        .collect()
}

fn normalized_text(value: &str, fallback: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn normalized_id(value: &str, fallback_prefix: &str) -> String {
    let value = value.trim();
    if value.is_empty() {
        format!("{fallback_prefix}-{}", unix_now())
    } else {
        value.to_string()
    }
}

fn default_confidence() -> f32 {
    0.5
}

fn inline_clip(value: &str, max_chars: usize) -> String {
    let normalized = value.replace('\n', " ").trim().to_string();
    if normalized.chars().count() <= max_chars {
        normalized
    } else {
        normalized.chars().take(max_chars).collect::<String>() + "..."
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{ExperienceStore, derive_experience_from_episode};
    use crate::solve_trace::{SolveDecision, SolveEpisode, SolveOutcome, SolveStep};

    #[test]
    fn derives_and_renders_experience_context() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = ExperienceStore::new(tmp.path()).expect("store");
        let episode = SolveEpisode {
            id: "turn-1".to_string(),
            turn_id: "turn-1".to_string(),
            goal: "Fix build failures".to_string(),
            user_input: "Fix the Rust build".to_string(),
            focus_goal_id: Some("goal-build".to_string()),
            focus_goal_title: Some("Fix build failures".to_string()),
            status: "completed".to_string(),
            supplements: vec![
                "Errors cluster in the types layer".to_string(),
                "Risk: patching leaf impls first may mask the root cause".to_string(),
            ],
            steps: vec![SolveStep {
                id: "step-1".to_string(),
                kind: "tool".to_string(),
                trigger: "build failure".to_string(),
                action: "Group errors before editing leaf implementations".to_string(),
                observation: "Trait bound mismatches dominate the error set".to_string(),
                evidence_refs: vec![],
                status: "completed".to_string(),
                created_at_unix: 1,
            }],
            decisions: vec![SolveDecision {
                id: "decision-1".to_string(),
                question: "What should we inspect first?".to_string(),
                chosen: "Check the shared trait definition before leaf implementations".to_string(),
                rationale: vec!["The errors look like a cascade".to_string()],
                created_at_unix: 1,
            }],
            outcome: Some(SolveOutcome {
                status: "completed".to_string(),
                summary: "Root cause isolated to a shared trait signature change".to_string(),
                next_focus: Some("Update implementor signatures".to_string()),
                created_at_unix: 1,
            }),
            created_at_unix: 1,
            updated_at_unix: 1,
        };

        let record = derive_experience_from_episode(&episode).expect("record");
        store.upsert_record("session-a", record).expect("save");
        let block = store
            .build_context_block("session-a", "trait build failure", None)
            .expect("context")
            .expect("block");
        assert!(block.contains("Fix build failures"));
        assert!(block.contains("shared trait definition"));
        assert!(block.contains("mask the root cause"));
        assert!(block.contains("Root cause isolated"));
    }
}
