use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_RENDERED_GOALS: usize = 8;
const MAX_RENDERED_COGNITION: usize = 6;
const MAX_RENDERED_HOT_DATA: usize = 6;
const MAX_BRIEF_RENDERED_GOALS: usize = 4;
const MAX_BRIEF_RENDERED_COGNITION: usize = 3;
const MAX_BRIEF_RENDERED_HOT_DATA: usize = 3;
const EVIDENCE_FRESHNESS_WINDOW_SECS: u64 = 7 * 24 * 3600;

#[derive(Debug, Clone, PartialEq, Eq)]
struct EvidenceSummary {
    supports: usize,
    conflicts: usize,
    context: usize,
    distinct_sources: usize,
    latest_observed_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceItem {
    #[serde(default = "default_evidence_source_type")]
    pub source_type: String,
    #[serde(default)]
    pub source: String,
    pub summary: String,
    #[serde(default = "default_evidence_relation")]
    pub relation: String,
    #[serde(default)]
    pub observed_at_unix: u64,
}

impl EvidenceItem {
    pub fn normalize(&mut self) {
        self.source_type = normalize_evidence_source_type(&self.source_type);
        self.source = self.source.trim().to_string();
        self.summary = normalized_text(&self.summary, "(unspecified evidence)");
        self.relation = normalize_evidence_relation(&self.relation);
        if self.observed_at_unix == 0 {
            self.observed_at_unix = unix_now();
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoalItem {
    pub id: String,
    pub title: String,
    #[serde(default = "default_goal_level")]
    pub level: String,
    #[serde(default = "default_goal_status")]
    pub status: String,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub evidence_items: Vec<EvidenceItem>,
    #[serde(default)]
    pub updated_at_unix: u64,
}

impl GoalItem {
    pub fn normalize(&mut self) {
        self.id = normalized_id(&self.id, "goal");
        self.title = normalized_text(&self.title, "(untitled goal)");
        self.level = normalize_goal_level(&self.level);
        self.status = normalize_goal_status(&self.status);
        self.confidence = normalize_confidence(self.confidence);
        self.parent_id = self
            .parent_id
            .take()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.summary = self.summary.trim().to_string();
        self.evidence = self.evidence.trim().to_string();
        normalize_evidence_items(
            &mut self.evidence_items,
            self.updated_at_unix,
            &self.evidence,
        );
        if self.updated_at_unix == 0 {
            self.updated_at_unix = unix_now();
        }
        self.confidence = calibrate_confidence_from_evidence(self.confidence, &self.evidence_items);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CognitionItem {
    pub id: String,
    #[serde(default = "default_cognition_kind")]
    pub kind: String,
    pub content: String,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub evidence_items: Vec<EvidenceItem>,
    #[serde(default)]
    pub updated_at_unix: u64,
}

impl CognitionItem {
    pub fn normalize(&mut self) {
        self.id = normalized_id(&self.id, "cognition");
        self.kind = normalize_cognition_kind(&self.kind);
        self.content = normalized_text(&self.content, "(empty cognition)");
        self.confidence = normalize_confidence(self.confidence);
        self.evidence = self.evidence.trim().to_string();
        normalize_evidence_items(
            &mut self.evidence_items,
            self.updated_at_unix,
            &self.evidence,
        );
        if self.updated_at_unix == 0 {
            self.updated_at_unix = unix_now();
        }
        self.confidence = calibrate_confidence_from_evidence(self.confidence, &self.evidence_items);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HotDataItem {
    pub id: String,
    pub content: String,
    #[serde(default = "default_confidence")]
    pub confidence: f32,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub goal_id: Option<String>,
    #[serde(default)]
    pub expires_at_unix: Option<u64>,
    #[serde(default)]
    pub evidence_items: Vec<EvidenceItem>,
    #[serde(default)]
    pub updated_at_unix: u64,
}

impl HotDataItem {
    pub fn normalize(&mut self) {
        self.id = normalized_id(&self.id, "hot");
        self.content = normalized_text(&self.content, "(empty hot data)");
        self.confidence = normalize_confidence(self.confidence);
        self.source = self.source.trim().to_string();
        self.goal_id = self
            .goal_id
            .take()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.expires_at_unix = self.expires_at_unix.filter(|value| *value > 0);
        normalize_evidence_items(&mut self.evidence_items, self.updated_at_unix, "");
        if self.updated_at_unix == 0 {
            self.updated_at_unix = unix_now();
        }
        self.confidence = calibrate_confidence_from_evidence(self.confidence, &self.evidence_items);
    }

    pub fn is_stale(&self, now: u64) -> bool {
        self.expires_at_unix
            .is_some_and(|expires_at| expires_at <= now)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoalState {
    #[serde(default)]
    pub mission: String,
    #[serde(default = "default_goal_phase")]
    pub phase: String,
    #[serde(default)]
    pub current_focus_goal_id: Option<String>,
    #[serde(default)]
    pub reflection: String,
    #[serde(default)]
    pub goals: Vec<GoalItem>,
    #[serde(default)]
    pub cognition: Vec<CognitionItem>,
    #[serde(default)]
    pub hot_data: Vec<HotDataItem>,
    #[serde(default)]
    pub updated_at_unix: u64,
}

impl GoalState {
    pub fn normalize(&mut self) {
        self.mission = self.mission.trim().to_string();
        self.phase = normalize_goal_phase(&self.phase);
        self.current_focus_goal_id = self
            .current_focus_goal_id
            .take()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        self.reflection = self.reflection.trim().to_string();

        for goal in &mut self.goals {
            goal.normalize();
        }
        dedupe_goals(&mut self.goals);

        for item in &mut self.cognition {
            item.normalize();
        }
        dedupe_cognition(&mut self.cognition);

        for item in &mut self.hot_data {
            item.normalize();
        }
        dedupe_hot_data(&mut self.hot_data);

        if self.updated_at_unix == 0 {
            self.updated_at_unix = unix_now();
        }
        if self
            .current_focus_goal_id
            .as_ref()
            .is_some_and(|goal_id| !self.goals.iter().any(|goal| goal.id == *goal_id))
        {
            self.current_focus_goal_id = select_focus_goal_id(&self.goals);
        }
    }
}

#[derive(Debug, Clone)]
pub struct GoalStateStore {
    root: PathBuf,
}

impl GoalStateStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("goal_state"))
            .with_context(|| format!("failed to create goal_state dir under {}", root.display()))?;
        Ok(Self { root })
    }

    pub fn load(&self, session_id: &str) -> Result<GoalState> {
        let path = self.state_path(session_id);
        if !path.exists() {
            return Ok(GoalState {
                mission: String::new(),
                phase: default_goal_phase(),
                current_focus_goal_id: None,
                reflection: String::new(),
                goals: Vec::new(),
                cognition: Vec::new(),
                hot_data: Vec::new(),
                updated_at_unix: unix_now(),
            });
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read goal state {}", path.display()))?;
        let mut state: GoalState = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse goal state {}", path.display()))?;
        state.normalize();
        Ok(state)
    }

    pub fn save(&self, session_id: &str, state: &GoalState) -> Result<PathBuf> {
        let path = self.state_path(session_id);
        let tmp = path.with_extension("json.tmp");
        let mut normalized = state.clone();
        normalized.updated_at_unix = unix_now();
        normalized.normalize();
        let raw =
            serde_json::to_string_pretty(&normalized).context("failed to serialize goal state")?;
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

    pub fn replace(&self, session_id: &str, mut state: GoalState) -> Result<GoalState> {
        state.updated_at_unix = unix_now();
        state.normalize();
        self.save(session_id, &state)?;
        Ok(state)
    }

    pub fn merge(
        &self,
        session_id: &str,
        mission: Option<String>,
        phase: Option<String>,
        current_focus_goal_id: Option<Option<String>>,
        reflection: Option<String>,
        goals: Vec<GoalItem>,
        cognition: Vec<CognitionItem>,
        hot_data: Vec<HotDataItem>,
    ) -> Result<GoalState> {
        let mut state = self.load(session_id)?;

        for goal in goals {
            merge_goal_item(&mut state.goals, goal);
        }
        for item in cognition {
            merge_cognition_item(&mut state.cognition, item);
        }
        for item in hot_data {
            merge_hot_data_item(&mut state.hot_data, item);
        }
        if let Some(mission) = mission {
            state.mission = mission;
        }
        if let Some(phase) = phase {
            state.phase = phase;
        }
        if let Some(current_focus_goal_id) = current_focus_goal_id {
            state.current_focus_goal_id = current_focus_goal_id;
        }
        if let Some(reflection) = reflection {
            state.reflection = reflection;
        }

        state.updated_at_unix = unix_now();
        state.normalize();
        self.save(session_id, &state)?;
        Ok(state)
    }

    pub fn set_focus(&self, session_id: &str, goal_id: Option<&str>) -> Result<GoalState> {
        let mut state = self.load(session_id)?;
        let goal_id = goal_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        if let Some(goal_id) = goal_id.as_deref() {
            if !state.goals.iter().any(|goal| goal.id == goal_id) {
                bail!("focus goal `{goal_id}` was not found in current goal state");
            }
        }
        state.current_focus_goal_id = goal_id;
        state.normalize();
        self.save(session_id, &state)?;
        Ok(state)
    }

    pub fn build_context_block(&self, session_id: &str) -> Result<Option<String>> {
        let state = self.load(session_id)?;
        Ok(render_goal_state_block(&state))
    }

    pub fn build_brief_context_block(&self, session_id: &str) -> Result<Option<String>> {
        let state = self.load(session_id)?;
        Ok(render_goal_state_brief_block(&state))
    }

    fn state_path(&self, session_id: &str) -> PathBuf {
        self.root
            .join("goal_state")
            .join(format!("{session_id}.json"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

fn render_goal_state_block(state: &GoalState) -> Option<String> {
    if state.goals.is_empty() && state.cognition.is_empty() && state.hot_data.is_empty() {
        return None;
    }

    let now = unix_now();
    let focus_id = state
        .current_focus_goal_id
        .clone()
        .or_else(|| select_focus_goal_id(&state.goals));
    let focus_goal = focus_id
        .as_ref()
        .and_then(|goal_id| state.goals.iter().find(|goal| goal.id == *goal_id));

    let mut lines = vec![
        "<goal-state>".to_string(),
        "[System note: The following is the agent's maintained goal state and working cognition. Use it as background context, update it when plans or beliefs change, and align actions with the active focus goal.]".to_string(),
        String::new(),
    ];

    if !state.mission.is_empty() {
        lines.push(format!("mission: {}", inline_clip(&state.mission, 180)));
    }
    lines.push(format!("phase: {}", state.phase));

    if let Some(goal) = focus_goal {
        let evidence = render_evidence_annotation("", &goal.evidence, &goal.evidence_items, 64);
        lines.push(format!(
            "focus_goal: [{}] {} | level={} | status={} | confidence={:.2}{}",
            goal.id, goal.title, goal.level, goal.status, goal.confidence, evidence
        ));
        if !goal.summary.is_empty() {
            lines.push(format!("focus_summary: {}", goal.summary));
        }
    } else {
        lines.push("focus_goal: (none)".to_string());
    }

    let selected_goals = prioritized_goals(&state.goals, focus_goal.map(|goal| goal.id.as_str()));
    if !selected_goals.is_empty() {
        lines.push("goals:".to_string());
        for goal in selected_goals.into_iter().take(MAX_RENDERED_GOALS) {
            let relation = goal
                .parent_id
                .as_deref()
                .map(|parent| format!(" parent={parent}"))
                .unwrap_or_default();
            let evidence =
                render_evidence_annotation(" evidence", &goal.evidence, &goal.evidence_items, 80);
            lines.push(format!(
                "- [{}] {} | level={} | status={} | confidence={:.2}{}{}",
                goal.id, goal.title, goal.level, goal.status, goal.confidence, relation, evidence
            ));
        }
    }

    let cognition = prioritized_cognition(&state.cognition);
    if !cognition.is_empty() {
        lines.push("cognition:".to_string());
        for item in cognition.into_iter().take(MAX_RENDERED_COGNITION) {
            let evidence =
                render_evidence_annotation(" evidence", &item.evidence, &item.evidence_items, 70);
            lines.push(format!(
                "- [{}] {} | confidence={:.2}{}",
                item.kind,
                inline_clip(&item.content, 160),
                item.confidence,
                evidence
            ));
        }
    }

    let hot_data = prioritized_hot_data(
        &state.hot_data,
        focus_goal.map(|goal| goal.id.as_str()),
        now,
    );
    if !hot_data.is_empty() {
        lines.push("hot_data:".to_string());
        for item in hot_data.into_iter().take(MAX_RENDERED_HOT_DATA) {
            let source = if item.source.is_empty() {
                String::new()
            } else {
                format!(" source={}", inline_clip(&item.source, 48))
            };
            let evidence = render_evidence_annotation("", "", &item.evidence_items, 64);
            let freshness = match item.expires_at_unix {
                Some(expires_at_unix) if expires_at_unix <= now => " stale".to_string(),
                Some(expires_at_unix) => format!(" expires_at={expires_at_unix}"),
                None => String::new(),
            };
            let goal_marker = item
                .goal_id
                .as_deref()
                .map(|goal_id| format!(" goal={goal_id}"))
                .unwrap_or_default();
            lines.push(format!(
                "- {} | confidence={:.2}{}{}{}",
                inline_clip(&item.content, 160),
                item.confidence,
                source,
                goal_marker,
                format!("{evidence}{freshness}")
            ));
        }
    }

    if !state.reflection.is_empty() {
        lines.push(format!(
            "reflection: {}",
            inline_clip(&state.reflection, 180)
        ));
    }

    lines.push("</goal-state>".to_string());
    Some(lines.join("\n"))
}

fn render_goal_state_brief_block(state: &GoalState) -> Option<String> {
    if state.goals.is_empty() && state.cognition.is_empty() && state.hot_data.is_empty() {
        return None;
    }

    let now = unix_now();
    let focus_id = state
        .current_focus_goal_id
        .clone()
        .or_else(|| select_focus_goal_id(&state.goals));
    let focus_goal = focus_id
        .as_ref()
        .and_then(|goal_id| state.goals.iter().find(|goal| goal.id == *goal_id));
    let prioritized_goals =
        prioritized_goals(&state.goals, focus_goal.map(|goal| goal.id.as_str()));
    let prioritized_cognition = prioritized_cognition(&state.cognition);
    let prioritized_hot_data = prioritized_hot_data(
        &state.hot_data,
        focus_goal.map(|goal| goal.id.as_str()),
        now,
    );

    let mut lines = vec![
        "<goal-state>".to_string(),
        "[System note: The following is a brief goal-state digest. Use it as lightweight working context; trust current evidence over stale details, and expand only when needed.]".to_string(),
        String::new(),
    ];

    if !state.mission.is_empty() {
        lines.push(format!("mission: {}", inline_clip(&state.mission, 140)));
    }
    lines.push(format!("phase: {}", state.phase));

    if let Some(goal) = focus_goal {
        lines.push(format!(
            "focus_goal: [{}] {} | status={} | confidence={:.2}",
            goal.id,
            inline_clip(&goal.title, 96),
            goal.status,
            goal.confidence
        ));
        if !goal.summary.is_empty() {
            lines.push(format!(
                "focus_summary: {}",
                inline_clip(&goal.summary, 160)
            ));
        }
        if goal.status == "blocked" {
            let blocker = if !goal.summary.trim().is_empty() {
                goal.summary.as_str()
            } else if !goal.evidence.trim().is_empty() {
                goal.evidence.as_str()
            } else {
                "Focus goal is blocked."
            };
            lines.push(format!("current_blocker: {}", inline_clip(blocker, 160)));
        }
    } else {
        lines.push("focus_goal: (none)".to_string());
    }

    let supporting_goals = prioritized_goals
        .into_iter()
        .filter(|goal| focus_goal.is_none_or(|focus| goal.id != focus.id))
        .filter(|goal| matches!(goal.status.as_str(), "pending" | "in_progress" | "blocked"))
        .take(MAX_BRIEF_RENDERED_GOALS)
        .collect::<Vec<_>>();
    if !supporting_goals.is_empty() {
        lines.push("supporting_goals:".to_string());
        for goal in supporting_goals {
            lines.push(format!(
                "- {} [{}] c={:.2}",
                inline_clip(&goal.title, 88),
                goal.status,
                goal.confidence
            ));
        }
    }

    let next_step = prioritized_hot_data
        .iter()
        .find(|item| match (focus_goal, item.goal_id.as_deref()) {
            (Some(goal), Some(goal_id)) => goal_id == goal.id,
            (_, None) => true,
            _ => false,
        })
        .or_else(|| prioritized_hot_data.first());
    if let Some(next_step) = next_step {
        lines.push(format!(
            "next_step_hint: {}",
            inline_clip(&next_step.content, 160)
        ));
    }

    if !prioritized_cognition.is_empty() {
        lines.push("key_beliefs:".to_string());
        for item in prioritized_cognition
            .into_iter()
            .take(MAX_BRIEF_RENDERED_COGNITION)
        {
            lines.push(format!(
                "- [{}] {} | c={:.2}",
                item.kind,
                inline_clip(&item.content, 120),
                item.confidence
            ));
        }
    }

    if !prioritized_hot_data.is_empty() {
        lines.push("recent_signals:".to_string());
        for item in prioritized_hot_data
            .into_iter()
            .take(MAX_BRIEF_RENDERED_HOT_DATA)
        {
            let source = if item.source.trim().is_empty() {
                String::new()
            } else {
                format!(" | source={}", inline_clip(&item.source, 36))
            };
            lines.push(format!(
                "- {} | c={:.2}{}",
                inline_clip(&item.content, 120),
                item.confidence,
                source
            ));
        }
    }

    if !state.reflection.is_empty() {
        lines.push(format!(
            "reflection: {}",
            inline_clip(&state.reflection, 140)
        ));
    }

    lines.push("</goal-state>".to_string());
    Some(lines.join("\n"))
}

fn prioritized_goals<'a>(goals: &'a [GoalItem], focus_goal_id: Option<&str>) -> Vec<&'a GoalItem> {
    let mut selected = Vec::new();
    let mut seen = HashSet::new();

    if let Some(focus_goal_id) = focus_goal_id {
        if let Some(goal) = goals.iter().find(|goal| goal.id == focus_goal_id) {
            push_goal(&mut selected, &mut seen, goal);
            let mut current_parent = goal.parent_id.as_deref();
            while let Some(parent_id) = current_parent {
                if let Some(parent) = goals.iter().find(|goal| goal.id == parent_id) {
                    push_goal(&mut selected, &mut seen, parent);
                    current_parent = parent.parent_id.as_deref();
                } else {
                    break;
                }
            }

            for child in goals
                .iter()
                .filter(|item| item.parent_id.as_deref() == Some(focus_goal_id))
            {
                push_goal(&mut selected, &mut seen, child);
            }
        }
    }

    let mut others = goals.iter().collect::<Vec<_>>();
    others.sort_by(|left, right| {
        goal_status_rank(&left.status)
            .cmp(&goal_status_rank(&right.status))
            .then_with(|| goal_level_rank(&left.level).cmp(&goal_level_rank(&right.level)))
            .then_with(|| {
                evidence_priority(&right.evidence_items)
                    .cmp(&evidence_priority(&left.evidence_items))
            })
            .then_with(|| {
                right
                    .confidence
                    .partial_cmp(&left.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| right.updated_at_unix.cmp(&left.updated_at_unix))
    });
    for goal in others {
        push_goal(&mut selected, &mut seen, goal);
    }
    selected
}

fn prioritized_cognition(items: &[CognitionItem]) -> Vec<&CognitionItem> {
    let mut items = items.iter().collect::<Vec<_>>();
    items.sort_by(|left, right| {
        cognition_kind_rank(&left.kind)
            .cmp(&cognition_kind_rank(&right.kind))
            .then_with(|| {
                evidence_priority(&right.evidence_items)
                    .cmp(&evidence_priority(&left.evidence_items))
            })
            .then_with(|| {
                right
                    .confidence
                    .partial_cmp(&left.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| right.updated_at_unix.cmp(&left.updated_at_unix))
    });
    items
}

fn prioritized_hot_data<'a>(
    items: &'a [HotDataItem],
    focus_goal_id: Option<&str>,
    now: u64,
) -> Vec<&'a HotDataItem> {
    let mut items = items
        .iter()
        .filter(|item| !item.is_stale(now))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        hot_data_focus_rank(left.goal_id.as_deref(), focus_goal_id)
            .cmp(&hot_data_focus_rank(
                right.goal_id.as_deref(),
                focus_goal_id,
            ))
            .then_with(|| {
                evidence_priority(&right.evidence_items)
                    .cmp(&evidence_priority(&left.evidence_items))
            })
            .then_with(|| {
                right
                    .confidence
                    .partial_cmp(&left.confidence)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| right.updated_at_unix.cmp(&left.updated_at_unix))
    });
    items
}

fn push_goal<'a>(selected: &mut Vec<&'a GoalItem>, seen: &mut HashSet<String>, goal: &'a GoalItem) {
    if seen.insert(goal.id.clone()) {
        selected.push(goal);
    }
}

fn merge_goal_item(items: &mut Vec<GoalItem>, mut incoming: GoalItem) {
    incoming.updated_at_unix = unix_now();
    incoming.normalize();
    if let Some(existing) = items.iter_mut().find(|item| item.id == incoming.id) {
        *existing = incoming;
    } else {
        items.push(incoming);
    }
}

fn merge_cognition_item(items: &mut Vec<CognitionItem>, mut incoming: CognitionItem) {
    incoming.updated_at_unix = unix_now();
    incoming.normalize();
    if let Some(existing) = items.iter_mut().find(|item| item.id == incoming.id) {
        *existing = incoming;
    } else {
        items.push(incoming);
    }
}

fn merge_hot_data_item(items: &mut Vec<HotDataItem>, mut incoming: HotDataItem) {
    incoming.updated_at_unix = unix_now();
    incoming.normalize();
    if let Some(existing) = items.iter_mut().find(|item| item.id == incoming.id) {
        *existing = incoming;
    } else {
        items.push(incoming);
    }
}

fn dedupe_goals(items: &mut Vec<GoalItem>) {
    let mut seen = HashSet::new();
    items.retain(|item| seen.insert(item.id.clone()));
}

fn dedupe_cognition(items: &mut Vec<CognitionItem>) {
    let mut deduped = Vec::<CognitionItem>::new();
    let mut seen = HashSet::new();
    for item in items.drain(..).rev() {
        if seen.insert(item.id.clone()) {
            deduped.push(item);
        }
    }
    deduped.reverse();
    *items = deduped;
}

fn dedupe_hot_data(items: &mut Vec<HotDataItem>) {
    let mut deduped = Vec::<HotDataItem>::new();
    let mut seen = HashSet::new();
    for item in items.drain(..).rev() {
        if seen.insert(item.id.clone()) {
            deduped.push(item);
        }
    }
    deduped.reverse();
    *items = deduped;
}

fn select_focus_goal_id(goals: &[GoalItem]) -> Option<String> {
    goals
        .iter()
        .min_by(|left, right| {
            goal_status_rank(&left.status)
                .cmp(&goal_status_rank(&right.status))
                .then_with(|| goal_level_rank(&left.level).cmp(&goal_level_rank(&right.level)))
                .then_with(|| right.updated_at_unix.cmp(&left.updated_at_unix))
        })
        .map(|goal| goal.id.clone())
}

fn normalize_goal_level(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "long_term" => "long_term".to_string(),
        "current" => "current".to_string(),
        "subgoal" => "subgoal".to_string(),
        _ => "current".to_string(),
    }
}

fn normalize_goal_status(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "pending" => "pending".to_string(),
        "in_progress" => "in_progress".to_string(),
        "blocked" => "blocked".to_string(),
        "succeeded" | "success" | "completed" => "succeeded".to_string(),
        "failed" => "failed".to_string(),
        "transferred" => "transferred".to_string(),
        "cancelled" | "canceled" => "cancelled".to_string(),
        _ => "pending".to_string(),
    }
}

fn normalize_cognition_kind(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "fact" => "fact".to_string(),
        "assumption" => "assumption".to_string(),
        "unknown" => "unknown".to_string(),
        "risk" => "risk".to_string(),
        "decision" => "decision".to_string(),
        _ => "fact".to_string(),
    }
}

fn normalize_evidence_source_type(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "user_input" => "user_input".to_string(),
        "tool_output" => "tool_output".to_string(),
        "file_verification" => "file_verification".to_string(),
        "model_inference" => "model_inference".to_string(),
        "assistant_response" => "assistant_response".to_string(),
        "system_state" => "system_state".to_string(),
        "session_history" => "session_history".to_string(),
        "legacy" => "legacy".to_string(),
        _ => default_evidence_source_type(),
    }
}

fn normalize_evidence_relation(value: &str) -> String {
    match value.trim().to_ascii_lowercase().as_str() {
        "supports" => "supports".to_string(),
        "conflicts" => "conflicts".to_string(),
        "context" => "context".to_string(),
        _ => default_evidence_relation(),
    }
}

fn goal_status_rank(status: &str) -> usize {
    match status {
        "in_progress" => 0,
        "blocked" => 1,
        "pending" => 2,
        "succeeded" => 3,
        "failed" => 4,
        "transferred" => 5,
        "cancelled" => 6,
        _ => 7,
    }
}

fn goal_level_rank(level: &str) -> usize {
    match level {
        "current" => 0,
        "subgoal" => 1,
        "long_term" => 2,
        _ => 3,
    }
}

fn cognition_kind_rank(kind: &str) -> usize {
    match kind {
        "fact" => 0,
        "decision" => 1,
        "assumption" => 2,
        "risk" => 3,
        "unknown" => 4,
        _ => 5,
    }
}

fn hot_data_focus_rank(goal_id: Option<&str>, focus_goal_id: Option<&str>) -> usize {
    match (goal_id, focus_goal_id) {
        (Some(goal_id), Some(focus_goal_id)) if goal_id == focus_goal_id => 0,
        (Some(_), _) => 1,
        _ => 2,
    }
}

fn normalized_id(value: &str, prefix: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        format!("{prefix}-{}", unix_now())
    } else {
        trimmed.to_string()
    }
}

fn normalized_text(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_confidence(value: f32) -> f32 {
    if !value.is_finite() {
        return default_confidence();
    }
    value.clamp(0.0, 1.0)
}

fn normalize_evidence_items(items: &mut Vec<EvidenceItem>, observed_at_unix: u64, legacy: &str) {
    for item in items.iter_mut() {
        if item.observed_at_unix == 0 && observed_at_unix > 0 {
            item.observed_at_unix = observed_at_unix;
        }
        item.normalize();
    }
    dedupe_evidence_items(items);
    if items.is_empty() && !legacy.trim().is_empty() {
        items.push(EvidenceItem {
            source_type: "legacy".to_string(),
            source: String::new(),
            summary: legacy.trim().to_string(),
            relation: "supports".to_string(),
            observed_at_unix: if observed_at_unix > 0 {
                observed_at_unix
            } else {
                unix_now()
            },
        });
    }
}

fn dedupe_evidence_items(items: &mut Vec<EvidenceItem>) {
    let mut deduped = Vec::<EvidenceItem>::new();
    let mut seen = HashSet::new();
    for item in items.drain(..).rev() {
        let key = format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}",
            item.source_type, item.source, item.summary, item.relation
        );
        if seen.insert(key) {
            deduped.push(item);
        }
    }
    deduped.reverse();
    *items = deduped;
}

fn summarize_evidence(items: &[EvidenceItem]) -> EvidenceSummary {
    let mut source_kinds = HashSet::new();
    let mut latest_observed_at_unix = 0;
    let mut supports = 0;
    let mut conflicts = 0;
    let mut context = 0;
    for item in items {
        source_kinds.insert(item.source_type.clone());
        latest_observed_at_unix = latest_observed_at_unix.max(item.observed_at_unix);
        match item.relation.as_str() {
            "supports" => supports += 1,
            "conflicts" => conflicts += 1,
            "context" => context += 1,
            _ => {}
        }
    }
    EvidenceSummary {
        supports,
        conflicts,
        context,
        distinct_sources: source_kinds.len(),
        latest_observed_at_unix,
    }
}

fn calibrate_confidence_from_evidence(base_confidence: f32, items: &[EvidenceItem]) -> f32 {
    let base_confidence = normalize_confidence(base_confidence);
    if items.is_empty() {
        return base_confidence;
    }

    let summary = summarize_evidence(items);
    let freshness_bonus = if summary.latest_observed_at_unix > 0 {
        let age = unix_now().saturating_sub(summary.latest_observed_at_unix);
        if age <= EVIDENCE_FRESHNESS_WINDOW_SECS {
            0.04
        } else {
            0.0
        }
    } else {
        0.0
    };
    let support_score = (summary.supports as f32 * 0.08).min(0.24);
    let source_score = (summary.distinct_sources as f32 * 0.04).min(0.12);
    let context_score = (summary.context as f32 * 0.02).min(0.06);
    let conflict_penalty = (summary.conflicts as f32 * 0.12).min(0.36);
    let evidence_score = (0.5 + support_score + source_score + context_score + freshness_bonus
        - conflict_penalty)
        .clamp(0.05, 0.95);

    normalize_confidence(base_confidence * 0.6 + evidence_score * 0.4)
}

fn render_evidence_annotation(
    prefix: &str,
    legacy: &str,
    items: &[EvidenceItem],
    max_chars: usize,
) -> String {
    if items.is_empty() && legacy.trim().is_empty() {
        return String::new();
    }
    let summary = summarize_evidence(items);
    let supports = summary.supports;
    let conflicts = summary.conflicts;
    let context = summary.context;
    let mut parts = Vec::new();
    if supports > 0 {
        parts.push(format!("s={supports}"));
    }
    if conflicts > 0 {
        parts.push(format!("c={conflicts}"));
    }
    if context > 0 {
        parts.push(format!("x={context}"));
    }
    let detail = if let Some(latest) = items.iter().max_by_key(|item| item.observed_at_unix) {
        inline_clip(&latest.summary, max_chars)
    } else {
        inline_clip(legacy, max_chars)
    };
    if parts.is_empty() {
        format!("{prefix}={detail}")
    } else {
        format!("{prefix}[{}]={detail}", parts.join(" "))
    }
}

fn evidence_priority(items: &[EvidenceItem]) -> (usize, usize, usize, u64) {
    let summary = summarize_evidence(items);
    (
        summary.conflicts,
        summary.supports,
        summary.context,
        summary.latest_observed_at_unix,
    )
}

fn inline_clip(value: &str, max_chars: usize) -> String {
    let normalized = value.replace('\n', " ").trim().to_string();
    if normalized.chars().count() <= max_chars {
        normalized
    } else {
        normalized.chars().take(max_chars).collect::<String>() + "..."
    }
}

fn default_goal_level() -> String {
    "current".to_string()
}

fn default_goal_status() -> String {
    "pending".to_string()
}

fn default_cognition_kind() -> String {
    "fact".to_string()
}

fn default_confidence() -> f32 {
    0.5
}

fn default_goal_phase() -> String {
    "understand".to_string()
}

fn default_evidence_source_type() -> String {
    "system_state".to_string()
}

fn default_evidence_relation() -> String {
    "supports".to_string()
}

fn normalize_goal_phase(value: &str) -> String {
    match value.trim() {
        "understand" | "investigate" | "act" | "verify" | "finalize" => value.trim().to_string(),
        _ => default_goal_phase(),
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
    use super::{
        CognitionItem, EvidenceItem, GoalItem, GoalState, GoalStateStore, HotDataItem,
        render_goal_state_block, render_goal_state_brief_block, unix_now,
    };

    #[test]
    fn saves_and_loads_goal_state() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = GoalStateStore::new(tmp.path()).expect("store");
        let state = GoalState {
            mission: "Ship first version".to_string(),
            phase: "act".to_string(),
            current_focus_goal_id: Some("g1".to_string()),
            reflection: "Session-scoped state is enough for v1.".to_string(),
            goals: vec![GoalItem {
                id: "g1".to_string(),
                title: "Ship first version".to_string(),
                level: "current".to_string(),
                status: "in_progress".to_string(),
                confidence: 0.9,
                parent_id: None,
                summary: "Need a minimal closed loop".to_string(),
                evidence: "User explicitly asked for first version".to_string(),
                evidence_items: vec![],
                updated_at_unix: 1,
            }],
            cognition: vec![CognitionItem {
                id: "c1".to_string(),
                kind: "assumption".to_string(),
                content: "Session-scoped state is enough for v1".to_string(),
                confidence: 0.6,
                evidence: "Reduces scope".to_string(),
                evidence_items: vec![],
                updated_at_unix: 1,
            }],
            hot_data: vec![HotDataItem {
                id: "h1".to_string(),
                content: "todo already exists for execution tasks".to_string(),
                confidence: 0.85,
                source: "code inspection".to_string(),
                goal_id: Some("g1".to_string()),
                expires_at_unix: None,
                evidence_items: vec![],
                updated_at_unix: 1,
            }],
            updated_at_unix: 1,
        };
        store.save("session-1", &state).expect("save");
        let loaded = store.load("session-1").expect("load");
        assert_eq!(loaded.current_focus_goal_id.as_deref(), Some("g1"));
        assert_eq!(loaded.goals.len(), 1);
        assert_eq!(loaded.cognition.len(), 1);
        assert_eq!(loaded.hot_data.len(), 1);
    }

    #[test]
    fn merge_updates_existing_items() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = GoalStateStore::new(tmp.path()).expect("store");
        store
            .merge(
                "session-1",
                Some("Initial goal".to_string()),
                Some("understand".to_string()),
                Some(Some("g1".to_string())),
                None,
                vec![GoalItem {
                    id: "g1".to_string(),
                    title: "Initial goal".to_string(),
                    level: "current".to_string(),
                    status: "pending".to_string(),
                    confidence: 0.4,
                    parent_id: None,
                    summary: String::new(),
                    evidence: String::new(),
                    evidence_items: vec![],
                    updated_at_unix: 0,
                }],
                vec![],
                vec![],
            )
            .expect("merge seed");

        let merged = store
            .merge(
                "session-1",
                None,
                Some("act".to_string()),
                None,
                Some("The goal now has enough evidence to move forward.".to_string()),
                vec![GoalItem {
                    id: "g1".to_string(),
                    title: "Initial goal".to_string(),
                    level: "current".to_string(),
                    status: "in_progress".to_string(),
                    confidence: 0.8,
                    parent_id: None,
                    summary: "Updated".to_string(),
                    evidence: "More evidence".to_string(),
                    evidence_items: vec![],
                    updated_at_unix: 0,
                }],
                vec![],
                vec![],
            )
            .expect("merge update");

        assert_eq!(merged.goals.len(), 1);
        assert_eq!(merged.goals[0].status, "in_progress");
        assert_eq!(merged.goals[0].summary, "Updated");
        assert_eq!(merged.phase, "act");
        assert!(merged.reflection.contains("move forward"));
    }

    #[test]
    fn renders_context_block_with_focus() {
        let state = GoalState {
            mission: "Build a maintained goal loop".to_string(),
            phase: "act".to_string(),
            current_focus_goal_id: Some("g2".to_string()),
            reflection: "Current focus is implementation.".to_string(),
            goals: vec![
                GoalItem {
                    id: "g1".to_string(),
                    title: "Long term".to_string(),
                    level: "long_term".to_string(),
                    status: "pending".to_string(),
                    confidence: 0.7,
                    parent_id: None,
                    summary: String::new(),
                    evidence: String::new(),
                    evidence_items: vec![],
                    updated_at_unix: 1,
                },
                GoalItem {
                    id: "g2".to_string(),
                    title: "Current focus".to_string(),
                    level: "current".to_string(),
                    status: "in_progress".to_string(),
                    confidence: 0.9,
                    parent_id: Some("g1".to_string()),
                    summary: "Focus summary".to_string(),
                    evidence: "User request".to_string(),
                    evidence_items: vec![],
                    updated_at_unix: 2,
                },
            ],
            cognition: vec![CognitionItem {
                id: "c1".to_string(),
                kind: "fact".to_string(),
                content: "Need goal-state loop".to_string(),
                confidence: 0.95,
                evidence: "user said so".to_string(),
                evidence_items: vec![],
                updated_at_unix: 2,
            }],
            hot_data: vec![HotDataItem {
                id: "h1".to_string(),
                content: "todo already covers task checklists".to_string(),
                confidence: 0.8,
                source: "code".to_string(),
                goal_id: Some("g2".to_string()),
                expires_at_unix: None,
                evidence_items: vec![],
                updated_at_unix: 2,
            }],
            updated_at_unix: 2,
        };

        let block = render_goal_state_block(&state).expect("block");
        assert!(block.contains("focus_goal: [g2] Current focus"));
        assert!(block.contains("cognition:"));
        assert!(block.contains("hot_data:"));
    }

    #[test]
    fn keeps_distinct_hot_data_without_id_collision() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = GoalStateStore::new(tmp.path()).expect("store");
        let merged = store
            .merge(
                "session-1",
                None,
                None,
                Some(Some("g1".to_string())),
                None,
                vec![],
                vec![],
                vec![
                    HotDataItem {
                        id: "h1".to_string(),
                        content: "Need todo sync for current goal loop".to_string(),
                        confidence: 0.8,
                        source: "tool:todo".to_string(),
                        goal_id: Some("g1".to_string()),
                        expires_at_unix: None,
                        evidence_items: vec![],
                        updated_at_unix: 0,
                    },
                    HotDataItem {
                        id: "h2".to_string(),
                        content: "Current goal loop still needs todo synchronization".to_string(),
                        confidence: 0.9,
                        source: "tool:todo".to_string(),
                        goal_id: Some("g1".to_string()),
                        expires_at_unix: None,
                        evidence_items: vec![],
                        updated_at_unix: 0,
                    },
                ],
            )
            .expect("merge");
        assert_eq!(merged.hot_data.len(), 2);
    }

    #[test]
    fn keeps_distinct_cognition_without_id_collision() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = GoalStateStore::new(tmp.path()).expect("store");
        let merged = store
            .merge(
                "session-1",
                None,
                None,
                None,
                None,
                vec![],
                vec![
                    CognitionItem {
                        id: "c1".to_string(),
                        kind: "fact".to_string(),
                        content: "Verified todo sync is needed for the focus goal".to_string(),
                        confidence: 0.8,
                        evidence: "tool result".to_string(),
                        evidence_items: vec![],
                        updated_at_unix: 0,
                    },
                    CognitionItem {
                        id: "c2".to_string(),
                        kind: "fact".to_string(),
                        content: "Focus goal still needs todo synchronization".to_string(),
                        confidence: 0.9,
                        evidence: "second tool result".to_string(),
                        evidence_items: vec![],
                        updated_at_unix: 0,
                    },
                ],
                vec![],
            )
            .expect("merge");
        assert_eq!(merged.cognition.len(), 2);
    }

    #[test]
    fn normalizes_legacy_evidence_into_structured_items() {
        let mut goal = GoalItem {
            id: "g1".to_string(),
            title: "Current".to_string(),
            level: "current".to_string(),
            status: "in_progress".to_string(),
            confidence: 0.8,
            parent_id: None,
            summary: String::new(),
            evidence: "User request".to_string(),
            evidence_items: vec![],
            updated_at_unix: 10,
        };
        goal.normalize();
        assert_eq!(goal.evidence_items.len(), 1);
        assert_eq!(goal.evidence_items[0].source_type, "legacy");
        assert_eq!(goal.evidence_items[0].relation, "supports");
    }

    #[test]
    fn preserves_structured_evidence_and_counts_in_render() {
        let state = GoalState {
            mission: "Render evidence-rich goal state".to_string(),
            phase: "investigate".to_string(),
            current_focus_goal_id: Some("g1".to_string()),
            reflection: "Evidence counters should stay visible.".to_string(),
            goals: vec![GoalItem {
                id: "g1".to_string(),
                title: "Current".to_string(),
                level: "current".to_string(),
                status: "blocked".to_string(),
                confidence: 0.4,
                parent_id: None,
                summary: String::new(),
                evidence: String::new(),
                evidence_items: vec![
                    EvidenceItem {
                        source_type: "tool_output".to_string(),
                        source: "tool:search".to_string(),
                        summary: "Search found a blocker".to_string(),
                        relation: "supports".to_string(),
                        observed_at_unix: 1,
                    },
                    EvidenceItem {
                        source_type: "model_inference".to_string(),
                        source: "goal_state_reconcile".to_string(),
                        summary: "Alternative path may still exist".to_string(),
                        relation: "conflicts".to_string(),
                        observed_at_unix: 2,
                    },
                ],
                updated_at_unix: 2,
            }],
            cognition: vec![],
            hot_data: vec![],
            updated_at_unix: 2,
        };
        let block = render_goal_state_block(&state).expect("block");
        assert!(block.contains("s=1 c=1"));
        assert!(block.contains("Alternative path may still exist"));
    }

    #[test]
    fn renders_brief_goal_state_digest() {
        let state = GoalState {
            mission: "Shrink prompt pressure".to_string(),
            phase: "verify".to_string(),
            current_focus_goal_id: Some("g2".to_string()),
            reflection: "Brief digests should stay shorter than full state.".to_string(),
            goals: vec![
                GoalItem {
                    id: "g1".to_string(),
                    title: "Long-term platform quality".to_string(),
                    level: "long_term".to_string(),
                    status: "in_progress".to_string(),
                    confidence: 0.7,
                    parent_id: None,
                    summary: "Keep the agent stable".to_string(),
                    evidence: String::new(),
                    evidence_items: vec![],
                    updated_at_unix: 1,
                },
                GoalItem {
                    id: "g2".to_string(),
                    title: "Shrink prompt pressure".to_string(),
                    level: "current".to_string(),
                    status: "blocked".to_string(),
                    confidence: 0.86,
                    parent_id: Some("g1".to_string()),
                    summary: "Skills and goal-state blocks are too large.".to_string(),
                    evidence: "Latest debug snapshot shows prompt bloat".to_string(),
                    evidence_items: vec![],
                    updated_at_unix: 3,
                },
                GoalItem {
                    id: "g3".to_string(),
                    title: "Reduce skill payload".to_string(),
                    level: "subgoal".to_string(),
                    status: "in_progress".to_string(),
                    confidence: 0.82,
                    parent_id: Some("g2".to_string()),
                    summary: String::new(),
                    evidence: String::new(),
                    evidence_items: vec![],
                    updated_at_unix: 2,
                },
            ],
            cognition: vec![CognitionItem {
                id: "c1".to_string(),
                kind: "risk".to_string(),
                content: "Large prompt blocks may hide the user's immediate request.".to_string(),
                confidence: 0.91,
                evidence: String::new(),
                evidence_items: vec![],
                updated_at_unix: 4,
            }],
            hot_data: vec![HotDataItem {
                id: "h1".to_string(),
                content: "Switch goal-state to a brief digest before model compaction.".to_string(),
                confidence: 0.88,
                source: "latest debug review".to_string(),
                goal_id: Some("g2".to_string()),
                expires_at_unix: None,
                evidence_items: vec![],
                updated_at_unix: 5,
            }],
            updated_at_unix: 5,
        };

        let full = render_goal_state_block(&state).expect("full block");
        let brief = render_goal_state_brief_block(&state).expect("brief block");

        assert!(brief.contains("current_blocker:"));
        assert!(brief.contains("next_step_hint:"));
        assert!(brief.contains("supporting_goals:"));
        assert!(brief.contains("mission: Shrink prompt pressure"));
        assert!(brief.contains("phase: verify"));
        assert!(brief.contains("reflection:"));
        assert!(brief.len() < full.len());
    }

    #[test]
    fn evidence_supports_raise_confidence() {
        let mut cognition = CognitionItem {
            id: "c1".to_string(),
            kind: "fact".to_string(),
            content: "Verified implementation path".to_string(),
            confidence: 0.5,
            evidence: String::new(),
            evidence_items: vec![
                EvidenceItem {
                    source_type: "user_input".to_string(),
                    source: "current_turn".to_string(),
                    summary: "The user explicitly requested this change.".to_string(),
                    relation: "supports".to_string(),
                    observed_at_unix: unix_now(),
                },
                EvidenceItem {
                    source_type: "tool_output".to_string(),
                    source: "read_file".to_string(),
                    summary: "Code inspection confirmed the target path.".to_string(),
                    relation: "supports".to_string(),
                    observed_at_unix: unix_now(),
                },
            ],
            updated_at_unix: unix_now(),
        };
        cognition.normalize();
        assert!(cognition.confidence > 0.5);
    }

    #[test]
    fn evidence_conflicts_lower_confidence() {
        let mut cognition = CognitionItem {
            id: "c1".to_string(),
            kind: "assumption".to_string(),
            content: "The current path is still valid".to_string(),
            confidence: 0.8,
            evidence: String::new(),
            evidence_items: vec![
                EvidenceItem {
                    source_type: "tool_output".to_string(),
                    source: "search".to_string(),
                    summary: "The expected file was not found.".to_string(),
                    relation: "conflicts".to_string(),
                    observed_at_unix: unix_now(),
                },
                EvidenceItem {
                    source_type: "tool_output".to_string(),
                    source: "memory_query".to_string(),
                    summary: "No supporting result was found.".to_string(),
                    relation: "conflicts".to_string(),
                    observed_at_unix: unix_now(),
                },
            ],
            updated_at_unix: unix_now(),
        };
        cognition.normalize();
        assert!(cognition.confidence < 0.8);
    }
}
