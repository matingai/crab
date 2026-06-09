use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::experience_store::{ExperienceRecord, ExperienceState};

const MAX_CONTEXT_PATTERNS: usize = 2;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetaPatternState {
    #[serde(default)]
    pub patterns: Vec<MetaPatternRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetaPatternRecord {
    pub id: String,
    #[serde(default)]
    pub kind: String,
    pub problem_cluster: String,
    #[serde(default)]
    pub source_experience_ids: Vec<String>,
    #[serde(default)]
    pub signal_patterns: Vec<String>,
    #[serde(default)]
    pub recommended_strategies: Vec<String>,
    #[serde(default)]
    pub failure_patterns: Vec<String>,
    #[serde(default)]
    pub representative_outcomes: Vec<String>,
    #[serde(default)]
    pub sample_count: usize,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    #[serde(default)]
    pub model_summary: Option<String>,
    #[serde(default)]
    pub match_hints: Vec<String>,
    #[serde(default)]
    pub strategy_template: Option<MetaPatternStrategyTemplate>,
    #[serde(default)]
    pub created_at_unix: u64,
    #[serde(default)]
    pub updated_at_unix: u64,
}

impl MetaPatternRecord {
    pub fn normalize(&mut self) {
        self.id = normalized_id(&self.id, "pattern");
        self.kind = normalized_text(&self.kind, "solve_pattern");
        self.problem_cluster = normalized_text(&self.problem_cluster, "(unspecified cluster)");
        dedupe_strings(&mut self.source_experience_ids, 8, 64);
        dedupe_strings(&mut self.signal_patterns, 5, 160);
        dedupe_strings(&mut self.recommended_strategies, 5, 180);
        dedupe_strings(&mut self.failure_patterns, 4, 180);
        dedupe_strings(&mut self.representative_outcomes, 3, 180);
        self.model_summary = self
            .model_summary
            .take()
            .map(|item| inline_clip(item.trim(), 220))
            .filter(|item| !item.is_empty());
        dedupe_strings(&mut self.match_hints, 4, 120);
        if let Some(template) = &mut self.strategy_template {
            template.normalize();
        }
        self.confidence = self.confidence.clamp(0.0, 1.0);
        if self.created_at_unix == 0 {
            self.created_at_unix = unix_now();
        }
        if self.updated_at_unix == 0 {
            self.updated_at_unix = self.created_at_unix;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MetaPatternStrategyTemplate {
    #[serde(default)]
    pub applicable_when: Vec<String>,
    #[serde(default)]
    pub preferred_actions: Vec<String>,
    #[serde(default)]
    pub avoid: Vec<String>,
    #[serde(default)]
    pub escalate_when: Vec<String>,
}

impl MetaPatternStrategyTemplate {
    pub fn normalize(&mut self) {
        dedupe_strings(&mut self.applicable_when, 4, 120);
        dedupe_strings(&mut self.preferred_actions, 5, 140);
        dedupe_strings(&mut self.avoid, 4, 140);
        dedupe_strings(&mut self.escalate_when, 4, 140);
    }
}

#[derive(Debug, Clone)]
pub struct MetaPatternStore {
    root: PathBuf,
}

impl MetaPatternStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("meta_patterns")).with_context(|| {
            format!(
                "failed to create meta_patterns dir under {}",
                root.display()
            )
        })?;
        Ok(Self { root })
    }

    pub fn load(&self, session_id: &str) -> Result<MetaPatternState> {
        let path = self.state_path(session_id);
        if !path.exists() {
            return Ok(MetaPatternState::default());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read meta pattern state {}", path.display()))?;
        let mut state: MetaPatternState = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse meta pattern state {}", path.display()))?;
        normalize_state(&mut state);
        Ok(state)
    }

    pub fn save(&self, session_id: &str, state: &MetaPatternState) -> Result<PathBuf> {
        let path = self.state_path(session_id);
        let tmp = path.with_extension("json.tmp");
        let mut normalized = state.clone();
        normalize_state(&mut normalized);
        let raw = serde_json::to_string_pretty(&normalized)
            .context("failed to serialize meta pattern state")?;
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

    pub fn rebuild_from_experience_state(
        &self,
        session_id: &str,
        experience_state: &ExperienceState,
    ) -> Result<()> {
        let previous = self.load(session_id).ok();
        let grouped = group_experiences(experience_state, previous.as_ref());
        let state = MetaPatternState { patterns: grouped };
        self.save(session_id, &state)?;
        Ok(())
    }

    pub fn update_pattern(
        &self,
        session_id: &str,
        pattern_id: &str,
        model_summary: Option<String>,
        match_hints: Vec<String>,
        strategy_template: Option<MetaPatternStrategyTemplate>,
        confidence: Option<f32>,
    ) -> Result<()> {
        let mut state = self.load(session_id)?;
        if let Some(pattern) = state.patterns.iter_mut().find(|item| item.id == pattern_id) {
            if let Some(model_summary) = model_summary {
                pattern.model_summary = Some(model_summary);
            }
            if !match_hints.is_empty() {
                pattern.match_hints = match_hints;
            }
            if let Some(strategy_template) = strategy_template {
                pattern.strategy_template = Some(strategy_template);
            }
            if let Some(confidence) = confidence {
                pattern.confidence = confidence;
            }
            pattern.updated_at_unix = unix_now();
        }
        self.save(session_id, &state)?;
        Ok(())
    }

    pub fn build_context_block(&self, session_id: &str, query: &str) -> Result<Option<String>> {
        let state = self.load(session_id)?;
        let ranked = rank_patterns(&state.patterns, query);
        if ranked.is_empty() {
            return Ok(None);
        }

        let mut lines = vec![
            "<meta-pattern-context>".to_string(),
            "[System note: The following is aggregated meta-pattern memory distilled from multiple prior solve episodes. Treat these patterns and templates as optional heuristics, not fixed procedures. Use them when they fit the current evidence, and ignore them when they do not.]".to_string(),
            String::new(),
        ];
        for pattern in ranked.into_iter().take(MAX_CONTEXT_PATTERNS) {
            lines.push(format!(
                "- cluster={} | kind={} | samples={} | confidence={:.2}",
                pattern.problem_cluster, pattern.kind, pattern.sample_count, pattern.confidence
            ));
            if let Some(summary) = &pattern.model_summary {
                lines.push(format!("  summary: {}", inline_clip(summary, 140)));
            }
            if !pattern.signal_patterns.is_empty() {
                lines.push(format!(
                    "  recurring_signals: {}",
                    pattern
                        .signal_patterns
                        .iter()
                        .take(3)
                        .map(|item| inline_clip(item, 90))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            if !pattern.recommended_strategies.is_empty() {
                lines.push(format!(
                    "  strategies: {}",
                    pattern
                        .recommended_strategies
                        .iter()
                        .take(3)
                        .map(|item| inline_clip(item, 100))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            if !pattern.failure_patterns.is_empty() {
                lines.push(format!(
                    "  avoid: {}",
                    pattern
                        .failure_patterns
                        .iter()
                        .take(2)
                        .map(|item| inline_clip(item, 100))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            if !pattern.representative_outcomes.is_empty() {
                lines.push(format!(
                    "  outcomes: {}",
                    pattern
                        .representative_outcomes
                        .iter()
                        .take(2)
                        .map(|item| inline_clip(item, 100))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            if !pattern.match_hints.is_empty() {
                lines.push(format!(
                    "  match_hints: {}",
                    pattern
                        .match_hints
                        .iter()
                        .take(3)
                        .map(|item| inline_clip(item, 90))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            if let Some(template) = &pattern.strategy_template {
                if !template.applicable_when.is_empty() {
                    lines.push(format!(
                        "  applicable_when: {}",
                        template
                            .applicable_when
                            .iter()
                            .take(3)
                            .map(|item| inline_clip(item, 90))
                            .collect::<Vec<_>>()
                            .join("; ")
                    ));
                }
                if !template.preferred_actions.is_empty() {
                    lines.push(format!(
                        "  preferred_actions: {}",
                        template
                            .preferred_actions
                            .iter()
                            .take(3)
                            .map(|item| inline_clip(item, 100))
                            .collect::<Vec<_>>()
                            .join("; ")
                    ));
                }
                if !template.avoid.is_empty() {
                    lines.push(format!(
                        "  avoid_template: {}",
                        template
                            .avoid
                            .iter()
                            .take(2)
                            .map(|item| inline_clip(item, 100))
                            .collect::<Vec<_>>()
                            .join("; ")
                    ));
                }
                if !template.escalate_when.is_empty() {
                    lines.push(format!(
                        "  escalate_when: {}",
                        template
                            .escalate_when
                            .iter()
                            .take(2)
                            .map(|item| inline_clip(item, 100))
                            .collect::<Vec<_>>()
                            .join("; ")
                    ));
                }
            }
        }
        lines.push("</meta-pattern-context>".to_string());
        Ok(Some(lines.join("\n")))
    }

    fn state_path(&self, session_id: &str) -> PathBuf {
        self.root
            .join("meta_patterns")
            .join(format!("{session_id}.json"))
    }
}

fn group_experiences(
    state: &ExperienceState,
    previous: Option<&MetaPatternState>,
) -> Vec<MetaPatternRecord> {
    let mut groups: BTreeMap<String, Vec<&ExperienceRecord>> = BTreeMap::new();
    for record in &state.records {
        groups.entry(group_key(record)).or_default().push(record);
    }

    let mut patterns = groups
        .into_iter()
        .map(|(key, items)| build_pattern_record(&key, &items, previous))
        .collect::<Vec<_>>();
    patterns.sort_by(|left, right| {
        right
            .sample_count
            .cmp(&left.sample_count)
            .then_with(|| right.updated_at_unix.cmp(&left.updated_at_unix))
    });
    patterns
}

fn build_pattern_record(
    key: &str,
    items: &[&ExperienceRecord],
    previous: Option<&MetaPatternState>,
) -> MetaPatternRecord {
    let first = items[0];
    let previous_pattern = previous.and_then(|state| {
        state
            .patterns
            .iter()
            .find(|pattern| pattern.id == format!("pattern:{key}"))
    });
    let mut record = MetaPatternRecord {
        id: format!("pattern:{key}"),
        kind: first.kind.clone(),
        problem_cluster: first.problem_frame.clone(),
        source_experience_ids: items.iter().map(|item| item.id.clone()).collect(),
        signal_patterns: collect_common_strings(items, |item| &item.signals, 5),
        recommended_strategies: collect_common_strings(items, |item| &item.successful_strategy, 5),
        failure_patterns: collect_common_strings(items, |item| &item.failure_patterns, 4),
        representative_outcomes: collect_common_strings(
            items,
            |item| std::slice::from_ref(&item.outcome),
            3,
        ),
        sample_count: items.len(),
        confidence: derive_pattern_confidence(items),
        model_summary: previous_pattern.and_then(|pattern| pattern.model_summary.clone()),
        match_hints: previous_pattern
            .map(|pattern| pattern.match_hints.clone())
            .unwrap_or_default(),
        strategy_template: previous_pattern.and_then(|pattern| pattern.strategy_template.clone()),
        created_at_unix: items
            .iter()
            .map(|item| item.created_at_unix)
            .min()
            .unwrap_or_else(unix_now),
        updated_at_unix: items
            .iter()
            .map(|item| item.updated_at_unix)
            .max()
            .unwrap_or_else(unix_now),
    };
    record.normalize();
    record
}

fn collect_common_strings(
    items: &[&ExperienceRecord],
    values: impl Fn(&ExperienceRecord) -> &[String],
    limit: usize,
) -> Vec<String> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for item in items {
        let mut seen = HashSet::new();
        for value in values(item) {
            let normalized = canonicalize_text(value);
            if normalized.is_empty() || !seen.insert(normalized.clone()) {
                continue;
            }
            *counts.entry(normalized).or_insert(0) += 1;
        }
    }

    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    ranked
        .into_iter()
        .take(limit)
        .map(|(value, _)| value)
        .collect()
}

fn derive_pattern_confidence(items: &[&ExperienceRecord]) -> f32 {
    let avg = items.iter().map(|item| item.confidence).sum::<f32>() / items.len() as f32;
    let sample_bonus = ((items.len().saturating_sub(1)) as f32 * 0.05).min(0.18);
    (avg + sample_bonus).clamp(0.0, 0.95)
}

fn group_key(record: &ExperienceRecord) -> String {
    format!(
        "{}:{}",
        record.kind,
        canonicalize_text(&record.problem_frame)
            .split_whitespace()
            .take(8)
            .collect::<Vec<_>>()
            .join("-")
    )
}

fn rank_patterns<'a>(patterns: &'a [MetaPatternRecord], query: &str) -> Vec<&'a MetaPatternRecord> {
    let query_tokens = tokenize(query);
    let mut ranked = patterns
        .iter()
        .map(|pattern| (score_pattern(pattern, &query_tokens), pattern))
        .filter(|(score, _)| *score > 0)
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.sample_count.cmp(&left.1.sample_count))
            .then_with(|| right.1.updated_at_unix.cmp(&left.1.updated_at_unix))
    });
    ranked.into_iter().map(|(_, pattern)| pattern).collect()
}

fn score_pattern(pattern: &MetaPatternRecord, query_tokens: &HashSet<String>) -> usize {
    let mut haystack = vec![pattern.problem_cluster.as_str()];
    if let Some(summary) = &pattern.model_summary {
        haystack.push(summary.as_str());
    }
    haystack.extend(pattern.signal_patterns.iter().map(String::as_str));
    haystack.extend(pattern.recommended_strategies.iter().map(String::as_str));
    haystack.extend(pattern.failure_patterns.iter().map(String::as_str));
    haystack.extend(pattern.representative_outcomes.iter().map(String::as_str));
    haystack.extend(pattern.match_hints.iter().map(String::as_str));
    if let Some(template) = &pattern.strategy_template {
        haystack.extend(template.applicable_when.iter().map(String::as_str));
        haystack.extend(template.preferred_actions.iter().map(String::as_str));
        haystack.extend(template.avoid.iter().map(String::as_str));
        haystack.extend(template.escalate_when.iter().map(String::as_str));
    }
    haystack
        .into_iter()
        .map(tokenize)
        .map(|tokens| tokens.intersection(query_tokens).count())
        .sum::<usize>()
        + pattern.sample_count
}

fn normalize_state(state: &mut MetaPatternState) {
    for pattern in &mut state.patterns {
        pattern.normalize();
    }
    dedupe_patterns(&mut state.patterns);
}

fn dedupe_patterns(items: &mut Vec<MetaPatternRecord>) {
    let mut deduped: Vec<MetaPatternRecord> = Vec::with_capacity(items.len());
    for item in items.drain(..) {
        if let Some(existing) = deduped.iter_mut().find(|pattern| pattern.id == item.id) {
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

fn canonicalize_text(value: &str) -> String {
    value
        .split(|ch: char| !ch.is_alphanumeric())
        .filter_map(|token| {
            let token = token.trim().to_ascii_lowercase();
            (!token.is_empty() && token.len() > 1).then_some(token)
        })
        .collect::<Vec<_>>()
        .join(" ")
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
    use super::{MetaPatternStore, MetaPatternStrategyTemplate};
    use crate::experience_store::{ExperienceRecord, ExperienceState};

    #[test]
    fn rebuilds_and_renders_meta_patterns() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = MetaPatternStore::new(tmp.path()).expect("store");
        let state = ExperienceState {
            records: vec![
                ExperienceRecord {
                    id: "exp:1".to_string(),
                    source_episode_id: "turn-1".to_string(),
                    kind: "debug_pattern".to_string(),
                    problem_frame: "Fix build failures".to_string(),
                    signals: vec!["trait bound mismatches dominate".to_string()],
                    successful_strategy: vec![
                        "group errors before editing leaf implementations".to_string(),
                        "check shared trait definition first".to_string(),
                    ],
                    failure_patterns: vec![
                        "patching leaf impls first may mask the root cause".to_string(),
                    ],
                    outcome: "root cause isolated in shared trait".to_string(),
                    confidence: 0.8,
                    created_at_unix: 1,
                    updated_at_unix: 1,
                },
                ExperienceRecord {
                    id: "exp:2".to_string(),
                    source_episode_id: "turn-2".to_string(),
                    kind: "debug_pattern".to_string(),
                    problem_frame: "Fix build failures".to_string(),
                    signals: vec!["trait bound mismatches dominate".to_string()],
                    successful_strategy: vec!["check shared trait definition first".to_string()],
                    failure_patterns: vec![
                        "patching leaf impls first may mask the root cause".to_string(),
                    ],
                    outcome: "shared trait change confirmed as root cause".to_string(),
                    confidence: 0.82,
                    created_at_unix: 2,
                    updated_at_unix: 2,
                },
            ],
        };
        store
            .rebuild_from_experience_state("session-a", &state)
            .expect("rebuild");
        let block = store
            .build_context_block("session-a", "trait build failure")
            .expect("context")
            .expect("block");
        assert!(block.contains("Fix build failures"));
        assert!(block.contains("samples=2"));
        assert!(block.contains("check shared trait definition first"));
        assert!(block.contains("patching leaf impls first may mask the root cause"));
    }

    #[test]
    fn stores_and_renders_strategy_template() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = MetaPatternStore::new(tmp.path()).expect("store");
        let state = ExperienceState {
            records: vec![ExperienceRecord {
                id: "exp:1".to_string(),
                source_episode_id: "turn-1".to_string(),
                kind: "debug_pattern".to_string(),
                problem_frame: "Fix build failures".to_string(),
                signals: vec!["trait bound mismatches dominate".to_string()],
                successful_strategy: vec!["check shared trait definition first".to_string()],
                failure_patterns: vec!["patch leaf impls first".to_string()],
                outcome: "shared trait root cause confirmed".to_string(),
                confidence: 0.8,
                created_at_unix: 1,
                updated_at_unix: 1,
            }],
        };
        store
            .rebuild_from_experience_state("session-a", &state)
            .expect("rebuild");
        store
            .update_pattern(
                "session-a",
                "pattern:debug_pattern:fix-build-failures",
                Some(
                    "Use the shared-interface-first strategy for cascading build errors"
                        .to_string(),
                ),
                vec!["many trait bound errors".to_string()],
                Some(MetaPatternStrategyTemplate {
                    applicable_when: vec!["errors cluster around shared types".to_string()],
                    preferred_actions: vec!["inspect shared trait definitions first".to_string()],
                    avoid: vec!["patching leaf implementations first".to_string()],
                    escalate_when: vec!["evidence conflicts across multiple modules".to_string()],
                }),
                Some(0.91),
            )
            .expect("update");
        let block = store
            .build_context_block("session-a", "trait errors in shared types")
            .expect("context")
            .expect("block");
        assert!(block.contains("Use the shared-interface-first strategy"));
        assert!(block.contains("applicable_when"));
        assert!(block.contains("inspect shared trait definitions first"));
        assert!(block.contains("evidence conflicts across multiple modules"));
    }
}
