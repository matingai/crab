use anyhow::{Context, Result, anyhow, bail};
use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize)]
pub struct CronJobSummary {
    pub id: String,
    pub schedule: String,
    pub prompt: String,
    pub prompt_preview: String,
    pub enabled: bool,
    pub next_run_at_unix: Option<u64>,
    pub last_run_at_unix: Option<u64>,
    pub last_status: Option<String>,
    pub last_session_id: Option<String>,
    pub recent_runs: Vec<CronJobRunRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobRunRecord {
    pub job_id: String,
    pub session_id: String,
    pub status: String,
    pub response_preview: String,
    pub updated_at_unix: u64,
}

#[derive(Debug, Clone)]
pub struct CronJobDefinition {
    pub id: String,
    pub schedule: String,
    pub prompt: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RootConfig {
    #[serde(default)]
    cron: CronConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CronConfig {
    #[serde(default)]
    jobs: Vec<CronJobEntry>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct CronJobEntry {
    id: String,
    schedule: String,
    prompt: String,
    enabled: Option<bool>,
}

pub fn load_cron_job_summaries(data_dir: &Path) -> Result<Vec<CronJobSummary>> {
    load_cron_job_definitions(data_dir)?
        .into_iter()
        .map(|job| {
            let run = load_run_record(data_dir, &job.id)?;
            let next_run_at_unix = next_run_at(&job, run.as_ref(), unix_now())?;
            let recent_runs = load_run_history(data_dir, Some(&job.id), 6)?;
            Ok(CronJobSummary {
                id: job.id,
                schedule: job.schedule,
                prompt: job.prompt.clone(),
                prompt_preview: truncate(job.prompt.trim(), 96),
                enabled: job.enabled,
                next_run_at_unix,
                last_run_at_unix: run.as_ref().map(|item| item.updated_at_unix),
                last_status: run.as_ref().map(|item| item.status.clone()),
                last_session_id: run.and_then(|item| {
                    if item.session_id.trim().is_empty() {
                        None
                    } else {
                        Some(item.session_id)
                    }
                }),
                recent_runs,
            })
        })
        .collect()
}

pub fn load_cron_job_definitions(data_dir: &Path) -> Result<Vec<CronJobDefinition>> {
    let config = load_root_config(data_dir)?;
    Ok(config
        .cron
        .jobs
        .into_iter()
        .filter(|job| !job.id.trim().is_empty())
        .map(|job| CronJobDefinition {
            id: job.id,
            schedule: job.schedule,
            prompt: job.prompt,
            enabled: job.enabled.unwrap_or(true),
        })
        .collect())
}

pub fn find_cron_job(data_dir: &Path, job_id: &str) -> Result<CronJobDefinition> {
    load_cron_job_definitions(data_dir)?
        .into_iter()
        .find(|job| job.id == job_id)
        .ok_or_else(|| anyhow!("cron job `{job_id}` not found"))
}

pub fn save_cron_job_definition(
    data_dir: &Path,
    previous_id: Option<&str>,
    job: &CronJobDefinition,
) -> Result<()> {
    let (config_path, mut root) = load_root_config_value(data_dir)?;
    let jobs = ensure_cron_jobs_sequence(&mut root)?;

    if let Some(previous_id) = previous_id.filter(|value| *value != job.id) {
        if let Some(index) = jobs
            .iter()
            .position(|item| yaml_job_id(item) == Some(previous_id))
        {
            jobs.remove(index);
        }
        rename_run_record(data_dir, previous_id, &job.id)?;
    }

    let replacement = cron_job_value(job);
    if let Some(index) = jobs
        .iter()
        .position(|item| yaml_job_id(item) == Some(job.id.as_str()))
    {
        jobs[index] = replacement;
    } else {
        jobs.push(replacement);
    }

    save_root_config_value(&config_path, &root)
}

pub fn delete_cron_job_definition(data_dir: &Path, job_id: &str) -> Result<()> {
    let (config_path, mut root) = load_root_config_value(data_dir)?;
    let jobs = ensure_cron_jobs_sequence(&mut root)?;
    let Some(index) = jobs
        .iter()
        .position(|item| yaml_job_id(item) == Some(job_id))
    else {
        bail!("cron job `{job_id}` not found");
    };
    jobs.remove(index);

    let record_path = run_record_path(data_dir, job_id);
    if record_path.exists() {
        fs::remove_file(&record_path)
            .with_context(|| format!("failed to remove {}", record_path.display()))?;
    }

    save_root_config_value(&config_path, &root)
}

pub fn list_due_jobs(data_dir: &Path, now_unix: u64) -> Result<Vec<CronJobDefinition>> {
    let mut due = Vec::new();
    for job in load_cron_job_definitions(data_dir)? {
        if !job.enabled {
            continue;
        }
        let run = load_run_record(data_dir, &job.id)?;
        if next_run_at(&job, run.as_ref(), now_unix)?.is_some_and(|value| value <= now_unix) {
            due.push(job);
        }
    }
    Ok(due)
}

pub fn save_run_record(data_dir: &Path, record: &CronJobRunRecord) -> Result<()> {
    let root = runtime_root(data_dir);
    fs::create_dir_all(&root)
        .with_context(|| format!("failed to create cron runtime dir {}", root.display()))?;
    let path = run_record_path(data_dir, &record.job_id);
    let raw =
        serde_json::to_string_pretty(record).context("failed to serialize cron run record")?;
    fs::write(&path, raw).with_context(|| format!("failed to write {}", path.display()))?;
    save_run_history_entry(data_dir, record)?;
    Ok(())
}

pub fn load_run_record(data_dir: &Path, job_id: &str) -> Result<Option<CronJobRunRecord>> {
    let path = run_record_path(data_dir, job_id);
    if !path.is_file() {
        return Ok(None);
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let record = serde_json::from_str(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(record))
}

pub fn load_run_history(
    data_dir: &Path,
    job_id: Option<&str>,
    limit: usize,
) -> Result<Vec<CronJobRunRecord>> {
    let root = history_root(data_dir);
    if !root.is_dir() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for entry in
        fs::read_dir(&root).with_context(|| format!("failed to read {}", root.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let item: CronJobRunRecord = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", path.display()))?;
        if job_id.is_none_or(|expected| item.job_id == expected) {
            items.push(item);
        }
    }

    items.sort_by(|a, b| {
        b.updated_at_unix
            .cmp(&a.updated_at_unix)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    if limit > 0 && items.len() > limit {
        items.truncate(limit);
    }
    Ok(items)
}

pub fn new_running_record(job_id: &str, session_id: &str) -> CronJobRunRecord {
    CronJobRunRecord {
        job_id: job_id.to_string(),
        session_id: session_id.to_string(),
        status: "running".to_string(),
        response_preview: String::new(),
        updated_at_unix: unix_now(),
    }
}

pub fn finalize_run_record(
    record: &mut CronJobRunRecord,
    status: &str,
    response_preview: impl Into<String>,
) {
    record.status = status.to_string();
    record.response_preview = response_preview.into();
    record.updated_at_unix = unix_now();
}

pub fn ensure_job_enabled(job: &CronJobDefinition) -> Result<()> {
    if !job.enabled {
        bail!("cron job `{}` is disabled", job.id);
    }
    Ok(())
}

pub fn next_run_at(
    job: &CronJobDefinition,
    run: Option<&CronJobRunRecord>,
    now_unix: u64,
) -> Result<Option<u64>> {
    if !job.enabled {
        return Ok(None);
    }

    let schedule = job.schedule.trim();
    if let Some(seconds) = parse_every_seconds(schedule) {
        return Ok(Some(
            run.map(|item| item.updated_at_unix.saturating_add(seconds))
                .unwrap_or(now_unix),
        ));
    }
    if let Some(timestamp) = parse_timestamp(schedule) {
        if run.is_some() {
            return Ok(None);
        }
        return Ok(Some(timestamp));
    }

    if schedule.split_whitespace().count() == 5 {
        let parsed = cron::Schedule::from_str(&format!("0 {schedule}"))
            .with_context(|| format!("invalid cron schedule `{schedule}`"))?;
        let start = Utc
            .timestamp_opt(
                run.map(|item| item.updated_at_unix).unwrap_or(now_unix) as i64,
                0,
            )
            .single()
            .ok_or_else(|| anyhow!("invalid cron base time"))?;
        return Ok(parsed
            .after(&start)
            .next()
            .map(|time| time.timestamp() as u64));
    }

    bail!("unsupported cron schedule `{schedule}`")
}

fn load_root_config(data_dir: &Path) -> Result<RootConfig> {
    let (_, raw) = load_root_config_raw(data_dir)?;
    let Some(raw) = raw else {
        return Ok(RootConfig::default());
    };
    serde_yaml::from_str(&raw).context("failed to parse cron root config")
}

fn load_root_config_raw(data_dir: &Path) -> Result<(PathBuf, Option<String>)> {
    let config_path = ["config.yaml", "config.yml"]
        .iter()
        .map(|name| data_dir.join(name))
        .find(|path| path.is_file())
        .unwrap_or_else(|| data_dir.join("config.yaml"));
    if !config_path.is_file() {
        return Ok((config_path, None));
    }
    let raw = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    Ok((config_path, Some(raw)))
}

fn load_root_config_value(data_dir: &Path) -> Result<(PathBuf, Value)> {
    let (config_path, raw) = load_root_config_raw(data_dir)?;
    let value = match raw {
        Some(raw) => serde_yaml::from_str(&raw)
            .with_context(|| format!("failed to parse {}", config_path.display()))?,
        None => Value::Mapping(Mapping::new()),
    };
    Ok((config_path, value))
}

fn save_root_config_value(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let raw = serde_yaml::to_string(value).context("failed to serialize root config")?;
    fs::write(path, raw).with_context(|| format!("failed to write {}", path.display()))
}

fn ensure_cron_jobs_sequence(root: &mut Value) -> Result<&mut Vec<Value>> {
    let root_map = ensure_mapping(root)?;
    let cron_value = root_map
        .entry(Value::String("cron".to_string()))
        .or_insert_with(|| Value::Mapping(Mapping::new()));
    let cron_map = ensure_mapping(cron_value)?;
    let jobs_value = cron_map
        .entry(Value::String("jobs".to_string()))
        .or_insert_with(|| Value::Sequence(Vec::new()));
    match jobs_value {
        Value::Sequence(items) => Ok(items),
        _ => bail!("cron.jobs must be a YAML sequence"),
    }
}

fn ensure_mapping(value: &mut Value) -> Result<&mut Mapping> {
    match value {
        Value::Mapping(map) => Ok(map),
        Value::Null => {
            *value = Value::Mapping(Mapping::new());
            match value {
                Value::Mapping(map) => Ok(map),
                _ => bail!("failed to build YAML mapping"),
            }
        }
        _ => bail!("root config must be a YAML mapping"),
    }
}

fn cron_job_value(job: &CronJobDefinition) -> Value {
    let mut map = Mapping::new();
    map.insert(
        Value::String("id".to_string()),
        Value::String(job.id.clone()),
    );
    map.insert(
        Value::String("schedule".to_string()),
        Value::String(job.schedule.clone()),
    );
    map.insert(
        Value::String("prompt".to_string()),
        Value::String(job.prompt.clone()),
    );
    map.insert(
        Value::String("enabled".to_string()),
        Value::Bool(job.enabled),
    );
    Value::Mapping(map)
}

fn yaml_job_id(value: &Value) -> Option<&str> {
    match value {
        Value::Mapping(map) => map
            .get(Value::String("id".to_string()))
            .and_then(Value::as_str),
        _ => None,
    }
}

fn rename_run_record(data_dir: &Path, previous_id: &str, next_id: &str) -> Result<()> {
    let previous_path = run_record_path(data_dir, previous_id);
    if !previous_path.exists() || previous_id == next_id {
        return Ok(());
    }
    let next_path = run_record_path(data_dir, next_id);
    if let Some(parent) = next_path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::rename(&previous_path, &next_path).with_context(|| {
        format!(
            "failed to rename {} to {}",
            previous_path.display(),
            next_path.display()
        )
    })?;

    let history_dir = history_root(data_dir);
    if history_dir.is_dir() {
        for entry in fs::read_dir(&history_dir)
            .with_context(|| format!("failed to read {}", history_dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let Some(filename) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let Some(suffix) = filename.strip_prefix(&format!("{previous_id}__")) else {
                continue;
            };
            let next_path = history_dir.join(format!("{next_id}__{suffix}"));
            fs::rename(&path, &next_path).with_context(|| {
                format!(
                    "failed to rename {} to {}",
                    path.display(),
                    next_path.display()
                )
            })?;
        }
    }

    Ok(())
}

fn runtime_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("cron")
}

fn run_record_path(data_dir: &Path, job_id: &str) -> PathBuf {
    runtime_root(data_dir).join(format!("{job_id}.json"))
}

fn history_root(data_dir: &Path) -> PathBuf {
    data_dir.join("runtime").join("cron-history")
}

fn history_record_path(data_dir: &Path, record: &CronJobRunRecord) -> PathBuf {
    let session = sanitize_filename(&record.session_id);
    history_root(data_dir).join(format!("{}__{}.json", record.job_id, session))
}

fn save_run_history_entry(data_dir: &Path, record: &CronJobRunRecord) -> Result<()> {
    let root = history_root(data_dir);
    fs::create_dir_all(&root)
        .with_context(|| format!("failed to create cron history dir {}", root.display()))?;
    let path = history_record_path(data_dir, record);
    let raw = serde_json::to_string_pretty(record)
        .context("failed to serialize cron run history record")?;
    fs::write(&path, raw).with_context(|| format!("failed to write {}", path.display()))
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    value.chars().take(max_chars).collect::<String>() + "..."
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn parse_every_seconds(schedule: &str) -> Option<u64> {
    let value = schedule.trim().strip_prefix("every ")?;
    parse_duration_seconds(value)
}

fn parse_timestamp(schedule: &str) -> Option<u64> {
    if !schedule.contains('T') {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(schedule)
        .ok()
        .map(|value| value.timestamp() as u64)
}

fn parse_duration_seconds(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    let (amount, unit) = trimmed.split_at(trimmed.len().saturating_sub(1));
    let amount = amount.parse::<u64>().ok()?;
    match unit {
        "m" => Some(amount * 60),
        "h" => Some(amount * 60 * 60),
        "d" => Some(amount * 60 * 60 * 24),
        _ => None,
    }
}

fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_job_enabled, finalize_run_record, find_cron_job, list_due_jobs,
        load_cron_job_summaries, load_run_record, new_running_record, next_run_at, save_run_record,
    };
    use std::fs;

    #[test]
    fn loads_cron_jobs_with_runtime_status() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("config.yaml"),
            r#"cron:
  jobs:
    - id: nightly-audit
      schedule: "0 2 * * *"
      prompt: "Audit the workspace and summarize risky changes."
"#,
        )
        .expect("write config");

        let mut record = new_running_record("nightly-audit", "cron.session");
        finalize_run_record(&mut record, "completed", "done");
        save_run_record(tmp.path(), &record).expect("save run");

        let jobs = load_cron_job_summaries(tmp.path()).expect("jobs");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, "nightly-audit");
        assert_eq!(jobs[0].last_status.as_deref(), Some("completed"));
        assert_eq!(jobs[0].last_session_id.as_deref(), Some("cron.session"));
        assert!(jobs[0].next_run_at_unix.is_some());
    }

    #[test]
    fn finds_and_validates_job() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("config.yaml"),
            r#"cron:
  jobs:
    - id: disabled
      schedule: "* * * * *"
      prompt: "noop"
      enabled: false
"#,
        )
        .expect("write config");

        let job = find_cron_job(tmp.path(), "disabled").expect("job");
        let error = ensure_job_enabled(&job).expect_err("disabled");
        assert!(error.to_string().contains("disabled"));
        assert!(
            load_run_record(tmp.path(), "missing")
                .expect("load")
                .is_none()
        );
    }

    #[test]
    fn computes_due_jobs_for_interval_schedule() {
        let tmp = tempfile::tempdir().expect("tempdir");
        fs::write(
            tmp.path().join("config.yaml"),
            r#"cron:
  jobs:
    - id: heartbeat
      schedule: "every 1m"
      prompt: "Ping"
"#,
        )
        .expect("write config");

        let job = find_cron_job(tmp.path(), "heartbeat").expect("job");
        assert!(next_run_at(&job, None, 100).expect("next").is_some());
        let due = list_due_jobs(tmp.path(), 161).expect("due");
        assert_eq!(due.len(), 1);
    }
}
