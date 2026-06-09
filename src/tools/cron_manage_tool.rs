use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeSet;

use crate::cron::{
    CronJobDefinition, delete_cron_job_definition, load_cron_job_definitions,
    load_cron_job_summaries, next_run_at, save_cron_job_definition,
};
use crate::tools::{Tool, ToolContext};
use crate::types::{ToolDefinition, object_schema};

pub struct CronManageTool;

#[derive(Debug, Deserialize)]
struct CronManageArgs {
    action: String,
    id: Option<String>,
    schedule: Option<String>,
    prompt: Option<String>,
    enabled: Option<bool>,
    previous_id: Option<String>,
}

#[async_trait]
impl Tool for CronManageTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "cron_manage",
            "List, create, update, enable, disable, or delete workspace cron jobs. Use this when the user asks to schedule or manage recurring tasks.",
            object_schema(
                json!({
                    "action": {
                        "type": "string",
                        "enum": ["list", "create", "update", "delete", "enable", "disable"]
                    },
                    "id": {
                        "type": "string",
                        "description": "Cron job id. Required for create, update, delete, enable, disable."
                    },
                    "schedule": {
                        "type": "string",
                        "description": "Schedule string like `every 1h`, `every 15m`, `2026-04-13T09:00:00+08:00`, or a 5-field cron expression."
                    },
                    "prompt": {
                        "type": "string",
                        "description": "Task prompt the scheduled run should execute."
                    },
                    "enabled": {
                        "type": "boolean",
                        "description": "Whether the job should run. Optional for create/update."
                    },
                    "previous_id": {
                        "type": "string",
                        "description": "Optional previous id when renaming a job during update."
                    }
                }),
                &["action"],
            ),
        )
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<String> {
        let args: CronManageArgs =
            serde_json::from_value(args).context("invalid cron_manage arguments")?;

        match args.action.as_str() {
            "list" => {
                let jobs = load_cron_job_summaries(&ctx.data_dir)?;
                if jobs.is_empty() {
                    return Ok("no cron jobs configured".to_string());
                }
                Ok(jobs
                    .into_iter()
                    .map(|job| {
                        format!(
                            "- {} [{}]{} :: {}",
                            job.id,
                            job.schedule,
                            if job.enabled { "" } else { " (disabled)" },
                            job.prompt_preview
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            "create" => {
                let schedule = required_trimmed(args.schedule, "schedule")?;
                let prompt = required_trimmed(args.prompt, "prompt")?;
                let id = resolve_create_job_id(args.id, &prompt, &ctx.data_dir)?;
                let job = CronJobDefinition {
                    id: id.clone(),
                    schedule: schedule.clone(),
                    prompt: prompt.clone(),
                    enabled: args.enabled.unwrap_or(true),
                };
                let validation_job = CronJobDefinition {
                    enabled: true,
                    ..job.clone()
                };
                next_run_at(&validation_job, None, current_unix())?;
                save_cron_job_definition(&ctx.data_dir, args.previous_id.as_deref(), &job)?;
                let next_run = next_run_at(&validation_job, None, current_unix())?
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                Ok(format!(
                    "cron job saved: {}\nschedule: {}\nenabled: {}\nnext_run_at_unix: {}",
                    job.id, job.schedule, job.enabled, next_run
                ))
            }
            "update" => {
                let schedule = required_trimmed(args.schedule, "schedule")?;
                let prompt = required_trimmed(args.prompt, "prompt")?;
                let previous_id = optional_trimmed(args.previous_id);
                let id = optional_trimmed(args.id)
                    .or_else(|| previous_id.clone())
                    .ok_or_else(|| anyhow::anyhow!("cron_manage `id` is required"))?;
                let lookup_id = previous_id.as_deref().unwrap_or(id.as_str());
                let existing = load_cron_job_definitions(&ctx.data_dir)?
                    .into_iter()
                    .find(|job| job.id == lookup_id)
                    .ok_or_else(|| anyhow::anyhow!("cron job `{lookup_id}` not found"))?;
                let job = CronJobDefinition {
                    id: id.clone(),
                    schedule: schedule.clone(),
                    prompt: prompt.clone(),
                    enabled: args.enabled.unwrap_or(existing.enabled),
                };
                let validation_job = CronJobDefinition {
                    enabled: true,
                    ..job.clone()
                };
                next_run_at(&validation_job, None, current_unix())?;
                save_cron_job_definition(&ctx.data_dir, previous_id.as_deref(), &job)?;
                let next_run = next_run_at(&validation_job, None, current_unix())?
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                Ok(format!(
                    "cron job saved: {}\nschedule: {}\nenabled: {}\nnext_run_at_unix: {}",
                    job.id, job.schedule, job.enabled, next_run
                ))
            }
            "enable" | "disable" => {
                let id = required_trimmed(args.id, "id")?;
                let jobs = load_cron_job_summaries(&ctx.data_dir)?;
                let existing = jobs
                    .into_iter()
                    .find(|job| job.id == id)
                    .ok_or_else(|| anyhow::anyhow!("cron job `{id}` not found"))?;
                let job = CronJobDefinition {
                    id: existing.id.clone(),
                    schedule: existing.schedule.clone(),
                    prompt: existing.prompt.clone(),
                    enabled: args.action == "enable",
                };
                save_cron_job_definition(&ctx.data_dir, None, &job)?;
                Ok(format!(
                    "cron job {}: {}",
                    if job.enabled { "enabled" } else { "disabled" },
                    job.id
                ))
            }
            "delete" => {
                let id = required_trimmed(args.id, "id")?;
                delete_cron_job_definition(&ctx.data_dir, &id)?;
                Ok(format!("cron job deleted: {id}"))
            }
            other => bail!("unsupported cron_manage action `{other}`"),
        }
    }
}

fn required_trimmed(value: Option<String>, field: &str) -> Result<String> {
    optional_trimmed(value).ok_or_else(|| anyhow::anyhow!("cron_manage `{field}` is required"))
}

fn optional_trimmed(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_create_job_id(
    id: Option<String>,
    prompt: &str,
    data_dir: &std::path::Path,
) -> Result<String> {
    let seed = optional_trimmed(id).unwrap_or_else(|| prompt.to_string());
    let mut base = slugify(&seed);
    if base.is_empty() {
        base = "cron-job".to_string();
    }
    if base.len() > 48 {
        base.truncate(48);
        base = base.trim_matches('-').to_string();
        if base.is_empty() {
            base = "cron-job".to_string();
        }
    }

    let existing = load_cron_job_definitions(data_dir)?
        .into_iter()
        .map(|job| job.id)
        .collect::<BTreeSet<_>>();
    if !existing.contains(&base) {
        return Ok(base);
    }

    for suffix in 2..=999 {
        let candidate = format!("{base}-{suffix}");
        if !existing.contains(&candidate) {
            return Ok(candidate);
        }
    }

    Ok(format!("cron-job-{}", current_unix()))
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;
    for ch in value.chars() {
        let normalized = if ch.is_ascii_alphanumeric() {
            Some(ch.to_ascii_lowercase())
        } else if ch == '-' || ch == '_' || ch.is_ascii_whitespace() {
            Some('-')
        } else {
            None
        };

        match normalized {
            Some('-') if !slug.is_empty() && !last_was_dash => {
                slug.push('-');
                last_was_dash = true;
            }
            Some(ch) => {
                slug.push(ch);
                last_was_dash = false;
            }
            None => {}
        }
    }
    slug.trim_matches('-').to_string()
}

fn current_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
