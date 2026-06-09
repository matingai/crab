use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_CONTEXT_EPISODES: usize = 2;
const MAX_CONTEXT_STEPS: usize = 3;
const MAX_CONTEXT_DECISIONS: usize = 2;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SolveTraceState {
    #[serde(default)]
    pub episodes: Vec<SolveEpisode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolveEpisode {
    pub id: String,
    pub turn_id: String,
    pub goal: String,
    pub user_input: String,
    #[serde(default)]
    pub focus_goal_id: Option<String>,
    #[serde(default)]
    pub focus_goal_title: Option<String>,
    #[serde(default = "default_episode_status")]
    pub status: String,
    #[serde(default)]
    pub supplements: Vec<String>,
    #[serde(default)]
    pub steps: Vec<SolveStep>,
    #[serde(default)]
    pub decisions: Vec<SolveDecision>,
    #[serde(default)]
    pub outcome: Option<SolveOutcome>,
    #[serde(default)]
    pub created_at_unix: u64,
    #[serde(default)]
    pub updated_at_unix: u64,
}

impl SolveEpisode {
    pub fn normalize(&mut self) {
        self.id = normalized_id(&self.id, "episode");
        self.turn_id = normalized_id(&self.turn_id, "turn");
        self.goal = normalized_text(&self.goal, "(unspecified goal)");
        self.user_input = normalized_text(&self.user_input, "(unspecified request)");
        self.focus_goal_id = normalize_optional(self.focus_goal_id.take());
        self.focus_goal_title = normalize_optional(self.focus_goal_title.take());
        self.status = normalize_episode_status(&self.status);
        dedupe_strings(&mut self.supplements, 8, 220);
        for step in &mut self.steps {
            step.normalize();
        }
        dedupe_steps(&mut self.steps);
        for decision in &mut self.decisions {
            decision.normalize();
        }
        dedupe_decisions(&mut self.decisions);
        if let Some(outcome) = &mut self.outcome {
            outcome.normalize();
        }
        if self.created_at_unix == 0 {
            self.created_at_unix = unix_now();
        }
        if self.updated_at_unix == 0 {
            self.updated_at_unix = self.created_at_unix;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolveStep {
    pub id: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub trigger: String,
    pub action: String,
    #[serde(default)]
    pub observation: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default = "default_step_status")]
    pub status: String,
    #[serde(default)]
    pub created_at_unix: u64,
}

impl SolveStep {
    pub fn normalize(&mut self) {
        self.id = normalized_id(&self.id, "step");
        self.kind = normalized_text(&self.kind, "action");
        self.trigger = self.trigger.trim().to_string();
        self.action = normalized_text(&self.action, "(unspecified action)");
        self.observation = self.observation.trim().to_string();
        dedupe_strings(&mut self.evidence_refs, 4, 96);
        self.status = normalize_step_status(&self.status);
        if self.created_at_unix == 0 {
            self.created_at_unix = unix_now();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolveDecision {
    pub id: String,
    pub question: String,
    pub chosen: String,
    #[serde(default)]
    pub rationale: Vec<String>,
    #[serde(default)]
    pub created_at_unix: u64,
}

impl SolveDecision {
    pub fn normalize(&mut self) {
        self.id = normalized_id(&self.id, "decision");
        self.question = normalized_text(&self.question, "(unspecified question)");
        self.chosen = normalized_text(&self.chosen, "(unspecified choice)");
        dedupe_strings(&mut self.rationale, 5, 180);
        if self.created_at_unix == 0 {
            self.created_at_unix = unix_now();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolveOutcome {
    #[serde(default = "default_episode_status")]
    pub status: String,
    pub summary: String,
    #[serde(default)]
    pub next_focus: Option<String>,
    #[serde(default)]
    pub created_at_unix: u64,
}

impl SolveOutcome {
    pub fn normalize(&mut self) {
        self.status = normalize_episode_status(&self.status);
        self.summary = normalized_text(&self.summary, "(unspecified outcome)");
        self.next_focus = normalize_optional(self.next_focus.take());
        if self.created_at_unix == 0 {
            self.created_at_unix = unix_now();
        }
    }
}

#[derive(Debug, Clone)]
pub struct SolveTraceStore {
    root: PathBuf,
}

impl SolveTraceStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("solve_trace")).with_context(|| {
            format!("failed to create solve_trace dir under {}", root.display())
        })?;
        Ok(Self { root })
    }

    pub fn load(&self, session_id: &str) -> Result<SolveTraceState> {
        let path = self.state_path(session_id);
        if !path.exists() {
            return Ok(SolveTraceState::default());
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read solve trace {}", path.display()))?;
        let mut state: SolveTraceState = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse solve trace {}", path.display()))?;
        normalize_state(&mut state);
        Ok(state)
    }

    pub fn save(&self, session_id: &str, state: &SolveTraceState) -> Result<PathBuf> {
        let path = self.state_path(session_id);
        let tmp = path.with_extension("json.tmp");
        let mut normalized = state.clone();
        normalize_state(&mut normalized);
        let raw =
            serde_json::to_string_pretty(&normalized).context("failed to serialize solve trace")?;
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

    #[allow(clippy::too_many_arguments)]
    pub fn start_episode(
        &self,
        session_id: &str,
        episode_id: &str,
        turn_id: &str,
        goal: &str,
        user_input: &str,
        focus_goal_id: Option<&str>,
        focus_goal_title: Option<&str>,
    ) -> Result<SolveEpisode> {
        let mut state = self.load(session_id)?;
        let now = unix_now();
        if let Some(existing) = state.episodes.iter_mut().find(|item| item.id == episode_id) {
            existing.turn_id = turn_id.to_string();
            existing.goal = goal.to_string();
            existing.user_input = user_input.to_string();
            existing.focus_goal_id = focus_goal_id.map(ToString::to_string);
            existing.focus_goal_title = focus_goal_title.map(ToString::to_string);
            existing.updated_at_unix = now;
            existing.normalize();
            let episode = existing.clone();
            self.save(session_id, &state)?;
            return Ok(episode);
        }

        let mut episode = SolveEpisode {
            id: episode_id.to_string(),
            turn_id: turn_id.to_string(),
            goal: goal.to_string(),
            user_input: user_input.to_string(),
            focus_goal_id: focus_goal_id.map(ToString::to_string),
            focus_goal_title: focus_goal_title.map(ToString::to_string),
            status: default_episode_status(),
            supplements: Vec::new(),
            steps: Vec::new(),
            decisions: Vec::new(),
            outcome: None,
            created_at_unix: now,
            updated_at_unix: now,
        };
        episode.normalize();
        state.episodes.push(episode.clone());
        self.save(session_id, &state)?;
        Ok(episode)
    }

    pub fn append_supplements(
        &self,
        session_id: &str,
        episode_id: &str,
        supplements: &[String],
    ) -> Result<()> {
        if supplements.is_empty() {
            return Ok(());
        }
        self.update_episode(session_id, episode_id, |episode| {
            episode.supplements.extend(supplements.iter().cloned());
        })
    }

    pub fn append_step(&self, session_id: &str, episode_id: &str, step: SolveStep) -> Result<()> {
        self.update_episode(session_id, episode_id, |episode| {
            if let Some(existing) = episode.steps.iter_mut().find(|item| item.id == step.id) {
                *existing = step.clone();
            } else {
                episode.steps.push(step.clone());
            }
        })
    }

    pub fn append_decision(
        &self,
        session_id: &str,
        episode_id: &str,
        decision: SolveDecision,
    ) -> Result<()> {
        self.update_episode(session_id, episode_id, |episode| {
            if let Some(existing) = episode
                .decisions
                .iter_mut()
                .find(|item| item.id == decision.id)
            {
                *existing = decision.clone();
            } else {
                episode.decisions.push(decision.clone());
            }
        })
    }

    pub fn set_outcome(
        &self,
        session_id: &str,
        episode_id: &str,
        outcome: SolveOutcome,
    ) -> Result<()> {
        self.update_episode(session_id, episode_id, |episode| {
            episode.status = outcome.status.clone();
            episode.outcome = Some(outcome.clone());
        })
    }

    pub fn build_context_block(
        &self,
        session_id: &str,
        query: &str,
        exclude_episode_id: Option<&str>,
    ) -> Result<Option<String>> {
        let state = self.load(session_id)?;
        let ranked = rank_episodes(&state.episodes, query, exclude_episode_id);
        if ranked.is_empty() {
            return Ok(None);
        }

        let mut lines = vec![
            "<solve-trace-context>".to_string(),
            "[System note: The following is distilled process memory from prior solve episodes. Treat it as optional problem-solving context and a reminder of prior approaches. Use it when it fits the current evidence, and ignore it when it does not.]".to_string(),
            String::new(),
        ];
        for episode in ranked.into_iter().take(MAX_CONTEXT_EPISODES) {
            lines.push(format!(
                "- [{}] goal={} | status={}",
                episode.turn_id, episode.goal, episode.status
            ));
            lines.push(format!(
                "  user_frame: {}",
                inline_clip(&episode.user_input, 140)
            ));
            if !episode.supplements.is_empty() {
                lines.push(format!(
                    "  supplements: {}",
                    episode
                        .supplements
                        .iter()
                        .take(3)
                        .map(|value| inline_clip(value, 100))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            if !episode.steps.is_empty() {
                lines.push(format!(
                    "  steps: {}",
                    episode
                        .steps
                        .iter()
                        .take(MAX_CONTEXT_STEPS)
                        .map(render_step_summary)
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            if !episode.decisions.is_empty() {
                lines.push(format!(
                    "  decisions: {}",
                    episode
                        .decisions
                        .iter()
                        .take(MAX_CONTEXT_DECISIONS)
                        .map(render_decision_summary)
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            if let Some(outcome) = &episode.outcome {
                lines.push(format!("  outcome: {}", inline_clip(&outcome.summary, 140)));
            }
        }
        lines.push("</solve-trace-context>".to_string());
        Ok(Some(lines.join("\n")))
    }

    fn update_episode(
        &self,
        session_id: &str,
        episode_id: &str,
        mut apply: impl FnMut(&mut SolveEpisode),
    ) -> Result<()> {
        let mut state = self.load(session_id)?;
        if let Some(episode) = state.episodes.iter_mut().find(|item| item.id == episode_id) {
            apply(episode);
            episode.updated_at_unix = unix_now();
            episode.normalize();
            self.save(session_id, &state)?;
        }
        Ok(())
    }

    fn state_path(&self, session_id: &str) -> PathBuf {
        self.root
            .join("solve_trace")
            .join(format!("{session_id}.json"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn render_step_summary(step: &SolveStep) -> String {
    format!(
        "{} [{}] {}",
        inline_clip(&step.action, 56),
        step.status,
        inline_clip(
            if step.observation.is_empty() {
                &step.trigger
            } else {
                &step.observation
            },
            90
        )
    )
}

fn render_decision_summary(decision: &SolveDecision) -> String {
    let rationale = if decision.rationale.is_empty() {
        String::new()
    } else {
        format!(
            " because {}",
            decision
                .rationale
                .iter()
                .take(2)
                .map(|item| inline_clip(item, 70))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!(
        "{} -> {}{}",
        inline_clip(&decision.question, 70),
        inline_clip(&decision.chosen, 80),
        rationale
    )
}

fn rank_episodes<'a>(
    episodes: &'a [SolveEpisode],
    query: &str,
    exclude_episode_id: Option<&str>,
) -> Vec<&'a SolveEpisode> {
    let tokens = tokenize(query);
    let mut ranked = episodes
        .iter()
        .filter(|episode| exclude_episode_id.is_none_or(|id| episode.id != id))
        .map(|episode| (score_episode(episode, &tokens), episode))
        .filter(|(score, _)| *score > 0)
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| right.1.updated_at_unix.cmp(&left.1.updated_at_unix))
    });
    ranked.into_iter().map(|(_, episode)| episode).collect()
}

fn score_episode(episode: &SolveEpisode, query_tokens: &HashSet<String>) -> usize {
    let mut haystack = vec![
        episode.goal.as_str(),
        episode.user_input.as_str(),
        episode.focus_goal_title.as_deref().unwrap_or_default(),
    ];
    haystack.extend(episode.supplements.iter().map(String::as_str));
    haystack.extend(episode.steps.iter().map(|step| step.action.as_str()));
    haystack.extend(episode.steps.iter().map(|step| step.observation.as_str()));
    haystack.extend(
        episode
            .decisions
            .iter()
            .map(|decision| decision.question.as_str()),
    );
    haystack.extend(
        episode
            .decisions
            .iter()
            .map(|decision| decision.chosen.as_str()),
    );
    if let Some(outcome) = &episode.outcome {
        haystack.push(outcome.summary.as_str());
    }

    haystack
        .into_iter()
        .map(tokenize)
        .map(|tokens| tokens.intersection(query_tokens).count())
        .sum::<usize>()
        + 1
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

fn normalize_state(state: &mut SolveTraceState) {
    for episode in &mut state.episodes {
        episode.normalize();
    }
    dedupe_episodes(&mut state.episodes);
}

fn dedupe_episodes(items: &mut Vec<SolveEpisode>) {
    let mut deduped: Vec<SolveEpisode> = Vec::with_capacity(items.len());
    for item in items.drain(..) {
        if let Some(existing) = deduped.iter_mut().find(|episode| episode.id == item.id) {
            *existing = item;
        } else {
            deduped.push(item);
        }
    }
    deduped.sort_by(|left, right| left.created_at_unix.cmp(&right.created_at_unix));
    *items = deduped;
}

fn dedupe_steps(items: &mut Vec<SolveStep>) {
    let mut deduped: Vec<SolveStep> = Vec::with_capacity(items.len());
    for item in items.drain(..) {
        if let Some(existing) = deduped.iter_mut().find(|step| step.id == item.id) {
            *existing = item;
        } else {
            deduped.push(item);
        }
    }
    deduped.sort_by(|left, right| left.created_at_unix.cmp(&right.created_at_unix));
    *items = deduped;
}

fn dedupe_decisions(items: &mut Vec<SolveDecision>) {
    let mut deduped: Vec<SolveDecision> = Vec::with_capacity(items.len());
    for item in items.drain(..) {
        if let Some(existing) = deduped.iter_mut().find(|decision| decision.id == item.id) {
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

fn normalize_optional(value: Option<String>) -> Option<String> {
    value
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
}

fn normalize_episode_status(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "completed" | "succeeded" => "completed".to_string(),
        "blocked" => "blocked".to_string(),
        "failed" => "failed".to_string(),
        "cancelled" | "canceled" => "cancelled".to_string(),
        _ => "in_progress".to_string(),
    }
}

fn normalize_step_status(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "completed" | "done" | "succeeded" => "completed".to_string(),
        "blocked" | "failed" | "error" => "blocked".to_string(),
        "cancelled" | "canceled" => "cancelled".to_string(),
        "in_progress" | "running" => "in_progress".to_string(),
        _ => "pending".to_string(),
    }
}

fn default_episode_status() -> String {
    "in_progress".to_string()
}

fn default_step_status() -> String {
    "pending".to_string()
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
    use super::{SolveDecision, SolveOutcome, SolveStep, SolveTraceStore};

    #[test]
    fn saves_and_renders_solve_trace_context() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = SolveTraceStore::new(tmp.path()).expect("store");
        store
            .start_episode(
                "session-a",
                "turn-1",
                "turn-1",
                "Fix build failures",
                "Please fix the Rust build",
                Some("goal-build"),
                Some("Fix build failures"),
            )
            .expect("start");
        store
            .append_supplements(
                "session-a",
                "turn-1",
                &[
                    "Errors cluster in the types layer".to_string(),
                    "User wants a fast path back to green builds".to_string(),
                ],
            )
            .expect("supplements");
        store
            .append_step(
                "session-a",
                "turn-1",
                SolveStep {
                    id: "step-1".to_string(),
                    kind: "tool".to_string(),
                    trigger: "build failure".to_string(),
                    action: "Run cargo check and group errors".to_string(),
                    observation: "Trait bound mismatches dominate the error set".to_string(),
                    evidence_refs: vec!["artifact://cargo/123".to_string()],
                    status: "completed".to_string(),
                    created_at_unix: 1,
                },
            )
            .expect("step");
        store
            .append_decision(
                "session-a",
                "turn-1",
                SolveDecision {
                    id: "decision-1".to_string(),
                    question: "What should we inspect first?".to_string(),
                    chosen: "Check the shared trait definition before fixing leaf implementations"
                        .to_string(),
                    rationale: vec!["The errors look like a cascade".to_string()],
                    created_at_unix: 1,
                },
            )
            .expect("decision");
        store
            .set_outcome(
                "session-a",
                "turn-1",
                SolveOutcome {
                    status: "completed".to_string(),
                    summary: "Root cause isolated to a shared trait signature change".to_string(),
                    next_focus: Some("Update every implementor signature".to_string()),
                    created_at_unix: 1,
                },
            )
            .expect("outcome");

        let block = store
            .build_context_block("session-a", "trait build failure", None)
            .expect("context")
            .expect("block");
        assert!(block.contains("Fix build failures"));
        assert!(block.contains("Trait bound mismatches"));
        assert!(block.contains("Check the shared trait definition"));
        assert!(block.contains("Root cause isolated"));
    }
}
