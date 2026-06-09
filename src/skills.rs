use anyhow::{Context, Result, bail};
use regex::Regex;
use serde::Deserialize;
use std::collections::{BTreeMap, HashSet};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SkillActivation {
    pub task_kinds: Vec<String>,
    pub requires_tools: Vec<String>,
    pub requires_shell: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSummary {
    pub category: String,
    pub name: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub activation: SkillActivation,
    pub path: PathBuf,
    pub updated_at_unix: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct SkillDocument {
    pub summary: SkillSummary,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillLinkedFile {
    pub path: String,
    pub size_bytes: u64,
    pub file_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillEnvRequirement {
    pub name: String,
    pub prompt: String,
    pub help: Option<String>,
    pub required_for: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillConfigRequirement {
    pub key: String,
    pub description: String,
    pub prompt: Option<String>,
    pub default_value: Option<String>,
    pub resolved_value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillReadiness {
    pub required_environment_variables: Vec<SkillEnvRequirement>,
    pub missing_required_environment_variables: Vec<String>,
    pub required_commands: Vec<String>,
    pub missing_required_commands: Vec<String>,
    pub config_requirements: Vec<SkillConfigRequirement>,
    pub setup_needed: bool,
    pub readiness_status: String,
}

#[derive(Debug, Clone)]
pub struct SkillView {
    pub summary: SkillSummary,
    pub file_path: String,
    pub file_type: String,
    pub content: String,
    pub is_binary: bool,
    pub linked_files: BTreeMap<String, Vec<SkillLinkedFile>>,
    pub readiness: SkillReadiness,
}

#[derive(Debug, Clone)]
pub struct SkillMatch {
    pub document: SkillDocument,
    pub score: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SkillQueryContext {
    pub available_tools: HashSet<String>,
    pub shell_enabled: bool,
    pub task_kinds: HashSet<String>,
}

impl SkillQueryContext {
    pub fn from_query(
        query: &str,
        available_tools: impl IntoIterator<Item = String>,
        shell_enabled: bool,
    ) -> Self {
        Self {
            available_tools: available_tools
                .into_iter()
                .map(|value| normalize_tag(&value))
                .filter(|value| !value.is_empty())
                .collect(),
            shell_enabled,
            task_kinds: detect_task_kinds(query),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SkillStore {
    local_root: PathBuf,
    roots: Vec<PathBuf>,
    disabled_skills: HashSet<String>,
    config_settings: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OfficeWorkflowTarget {
    Xlsx,
    Docx,
    Pptx,
    Generic,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct RootConfig {
    #[serde(default)]
    skills: SkillsConfig,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct SkillsConfig {
    #[serde(default)]
    include_bundled: Option<bool>,
    #[serde(default)]
    disabled: Vec<String>,
    #[serde(default)]
    platform_disabled: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    external_dirs: Vec<String>,
    #[serde(default)]
    config: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Default, Deserialize)]
struct SkillFrontmatter {
    description: Option<String>,
    keywords: Option<StringListField>,
    task_kinds: Option<StringListField>,
    requires_tools: Option<StringListField>,
    requires_shell: Option<BoolField>,
    platforms: Option<StringListField>,
    required_environment_variables: Option<Vec<EnvRequirementField>>,
    required_commands: Option<StringListField>,
    prerequisites: Option<SkillPrerequisites>,
    updated_at_unix: Option<u64>,
    metadata: Option<SkillMetadata>,
}

#[derive(Debug, Default, Deserialize)]
struct SkillMetadata {
    #[serde(default)]
    hermes: HermesMetadata,
}

#[derive(Debug, Default, Deserialize)]
struct HermesMetadata {
    tags: Option<StringListField>,
    #[serde(default)]
    config: Vec<SkillConfigEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum StringListField {
    String(String),
    List(Vec<String>),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum BoolField {
    Bool(bool),
    String(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum EnvRequirementField {
    String(String),
    Object(EnvRequirement),
}

#[derive(Debug, Clone, Default, Deserialize)]
struct EnvRequirement {
    name: String,
    prompt: Option<String>,
    help: Option<String>,
    required_for: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct SkillPrerequisites {
    env_vars: Option<StringListField>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct SkillConfigEntry {
    key: String,
    description: Option<String>,
    prompt: Option<String>,
    default: Option<serde_yaml::Value>,
}

impl SkillStore {
    pub fn new(data_dir: impl AsRef<Path>) -> Result<Self> {
        Self::new_with_platform(data_dir, env::var("HERMES_RS_PLATFORM").ok().as_deref())
    }

    pub fn new_with_platform(data_dir: impl AsRef<Path>, platform: Option<&str>) -> Result<Self> {
        let local_root = data_dir.as_ref().join("skills");
        fs::create_dir_all(&local_root)
            .with_context(|| format!("failed to create skills dir {}", local_root.display()))?;

        let config = load_root_config(data_dir.as_ref())?;
        let SkillsConfig {
            include_bundled,
            disabled,
            platform_disabled,
            external_dirs,
            config: config_settings,
        } = config;
        let mut roots = vec![local_root.clone()];
        for raw_dir in external_dirs {
            let path = expand_config_path(&raw_dir);
            if !path.is_dir() {
                continue;
            }
            if roots.iter().any(|existing| existing == &path) {
                continue;
            }
            roots.push(path);
        }
        let include_bundled = include_bundled.unwrap_or(true);
        if include_bundled {
            let bundled_root = bundled_skills_root();
            if bundled_root.is_dir() && !roots.iter().any(|existing| existing == &bundled_root) {
                roots.push(bundled_root);
            }
        }

        let normalized_platform = platform
            .map(normalize_tag)
            .filter(|value| !value.is_empty());
        let mut disabled_skills = disabled
            .into_iter()
            .map(|value| normalize_tag(&value))
            .filter(|value| !value.is_empty())
            .collect::<HashSet<_>>();
        if let Some(platform_name) = normalized_platform.as_deref() {
            if let Some(values) = platform_disabled.get(platform_name) {
                disabled_skills.extend(
                    values
                        .iter()
                        .map(|value| normalize_tag(value))
                        .filter(|value| !value.is_empty()),
                );
            }
        }

        Ok(Self {
            local_root,
            roots,
            disabled_skills,
            config_settings,
        })
    }

    pub fn root(&self) -> &Path {
        &self.local_root
    }

    pub fn list(&self) -> Result<Vec<SkillSummary>> {
        let mut deduped = BTreeMap::<(String, String), SkillSummary>::new();
        for root in &self.roots {
            let mut collected = Vec::new();
            self.collect_skills(root, root, &mut collected)?;
            for skill in collected {
                let key = (skill.category.clone(), skill.name.clone());
                deduped.entry(key).or_insert(skill);
            }
        }
        let mut skills = deduped.into_values().collect::<Vec<_>>();
        skills.sort_by(|a, b| {
            a.category
                .cmp(&b.category)
                .then_with(|| a.name.cmp(&b.name))
        });
        Ok(skills)
    }

    pub fn view(&self, name: &str, category: Option<&str>) -> Result<SkillDocument> {
        let view = self.view_with_file(name, category, None)?;
        Ok(SkillDocument {
            summary: view.summary,
            content: view.content,
        })
    }

    pub fn view_with_file(
        &self,
        name: &str,
        category: Option<&str>,
        file_path: Option<&str>,
    ) -> Result<SkillView> {
        let skill = self.resolve_skill(name, category)?;
        let skill_dir = skill
            .path
            .parent()
            .context("skill path is missing a parent directory")?;
        let linked_files = collect_linked_files(skill_dir)?;
        let raw_skill = fs::read_to_string(&skill.path)
            .with_context(|| format!("failed to read {}", skill.path.display()))?;
        let (frontmatter, _) = parse_frontmatter(&raw_skill);
        let (resolved_file_path, target_file) = resolve_skill_file_path(skill_dir, file_path)?;
        let (content, is_binary) = read_skill_file_content(&target_file)?;
        Ok(SkillView {
            summary: skill,
            file_path: resolved_file_path,
            file_type: file_type_label(&target_file),
            content,
            is_binary,
            linked_files,
            readiness: build_skill_readiness(&frontmatter, &self.config_settings),
        })
    }

    fn resolve_skill(&self, name: &str, category: Option<&str>) -> Result<SkillSummary> {
        let skills = self.list()?;
        let matches = skills
            .into_iter()
            .filter(|skill| {
                skill.name == name
                    && category
                        .map(|value| value == skill.category)
                        .unwrap_or(true)
            })
            .collect::<Vec<_>>();

        match matches.as_slice() {
            [] => bail!("skill `{name}` not found"),
            [skill] => Ok(skill.clone()),
            _ => bail!("multiple skills named `{name}` found; specify `category`"),
        }
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SkillMatch>> {
        self.search_with_context(query, limit, None)
    }

    pub fn search_with_context(
        &self,
        query: &str,
        limit: usize,
        context: Option<&SkillQueryContext>,
    ) -> Result<Vec<SkillMatch>> {
        let tokens = tokenize(query);
        if tokens.is_empty() {
            return Ok(Vec::new());
        }

        let mut matches = Vec::new();
        for summary in self.list()? {
            let content = fs::read_to_string(&summary.path)
                .with_context(|| format!("failed to read {}", summary.path.display()))?;
            let score = score_skill(&summary, &content, query, &tokens, context);
            if score == 0 {
                continue;
            }
            matches.push(SkillMatch {
                document: SkillDocument { summary, content },
                score,
            });
        }

        matches.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| {
                    a.document
                        .summary
                        .category
                        .cmp(&b.document.summary.category)
                })
                .then_with(|| a.document.summary.name.cmp(&b.document.summary.name))
        });
        matches.truncate(limit);
        Ok(matches)
    }

    pub fn build_context_block(&self, query: &str, limit: usize) -> Result<Option<String>> {
        self.build_context_block_with_context(query, limit, None)
    }

    pub fn build_context_block_with_context(
        &self,
        query: &str,
        limit: usize,
        context: Option<&SkillQueryContext>,
    ) -> Result<Option<String>> {
        let matches = self.search_with_context(query, limit, context)?;
        if matches.is_empty() {
            return Ok(None);
        }

        let mut sections = Vec::new();
        if let Some(route_note) = build_route_note(query, &matches) {
            sections.push(route_note);
        }
        sections.push(
            matches
                .iter()
                .map(|matched| {
                    let summary = &matched.document.summary;
                    let body = truncate_skill_body(&matched.document.content, 2_500);
                    let activation = render_activation_summary(&summary.activation);
                    format!(
                        "## {}/{}\n{}\n{}\n\n{}",
                        summary.category, summary.name, summary.description, activation, body
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n"),
        );

        Ok(Some(format!(
            "<skills-context>\n[System note: The following are potentially relevant local skills. Reuse them when appropriate.]\n\n{}\n</skills-context>",
            sections.join("\n\n")
        )))
    }

    pub fn build_brief_context_block_with_context(
        &self,
        query: &str,
        limit: usize,
        context: Option<&SkillQueryContext>,
    ) -> Result<Option<String>> {
        let matches = self.search_with_context(query, limit, context)?;
        if matches.is_empty() {
            return Ok(None);
        }

        let mut sections = Vec::new();
        if let Some(route_note) = build_route_note(query, &matches) {
            sections.push(route_note);
        }
        sections.push(
            matches
                .iter()
                .map(|matched| {
                    let summary = &matched.document.summary;
                    let activation = render_activation_summary(&summary.activation);
                    format!(
                        "## {}/{}\n{}\n{}\nDetail hint: use `skill_view` for `{}` in category `{}` if you need the full workflow.",
                        summary.category,
                        summary.name,
                        summary.description,
                        activation,
                        summary.name,
                        summary.category
                    )
                })
                .collect::<Vec<_>>()
                .join("\n\n"),
        );

        Ok(Some(format!(
            "<skills-context>\n[System note: The following are concise skill briefs. Use them as routing hints first. If a skill seems relevant, inspect it on demand with `skill_view` instead of assuming the full workflow.]\n\n{}\n</skills-context>",
            sections.join("\n\n")
        )))
    }

    pub fn save(
        &self,
        category: &str,
        name: &str,
        description: &str,
        keywords: &[String],
        body: &str,
    ) -> Result<PathBuf> {
        self.save_with_metadata(
            category,
            name,
            description,
            keywords,
            &SkillActivation::default(),
            body,
        )
    }

    pub fn save_with_metadata(
        &self,
        category: &str,
        name: &str,
        description: &str,
        keywords: &[String],
        activation: &SkillActivation,
        body: &str,
    ) -> Result<PathBuf> {
        let dir = self.local_root.join(category).join(name);
        fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
        let path = dir.join("SKILL.md");
        let now = unix_now();
        let created_at = if path.is_file() {
            fs::read_to_string(&path)
                .ok()
                .map(|raw| parse_frontmatter(&raw).0.updated_at_unix.unwrap_or(now))
                .unwrap_or(now)
        } else {
            now
        };
        let content = render_skill_markdown(
            name,
            description,
            keywords,
            activation,
            body,
            created_at,
            now,
        )?;
        fs::write(&path, content).with_context(|| format!("failed to write {}", path.display()))?;
        Ok(path)
    }

    pub fn append(&self, category: &str, name: &str, content: &str) -> Result<PathBuf> {
        let path = self.local_root.join(category).join(name).join("SKILL.md");
        if !path.is_file() {
            bail!("skill `{category}/{name}` does not exist");
        }
        let mut existing = fs::read_to_string(&path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if !existing.ends_with('\n') {
            existing.push('\n');
        }
        existing.push('\n');
        existing.push_str(content.trim());
        existing.push('\n');
        fs::write(&path, existing)
            .with_context(|| format!("failed to write {}", path.display()))?;
        Ok(path)
    }

    pub fn delete(&self, name: &str, category: Option<&str>) -> Result<PathBuf> {
        let skill = self.resolve_skill(name, category)?;
        let skill_dir = skill
            .path
            .parent()
            .context("skill path is missing a parent directory")?
            .to_path_buf();
        fs::remove_dir_all(&skill_dir)
            .with_context(|| format!("failed to remove {}", skill_dir.display()))?;
        Ok(skill_dir)
    }

    pub fn patch(
        &self,
        name: &str,
        category: Option<&str>,
        old_string: &str,
        new_string: &str,
        file_path: Option<&str>,
        replace_all: bool,
    ) -> Result<PathBuf> {
        if old_string.trim().is_empty() {
            bail!("old_string cannot be empty");
        }

        let (_, target_path) = self.resolve_mutation_target(name, category, file_path, true)?;
        let original = fs::read_to_string(&target_path)
            .with_context(|| format!("failed to read {}", target_path.display()))?;
        let matches = original.matches(old_string).count();
        if matches == 0 {
            bail!(
                "old_string not found in {}",
                target_path
                    .file_name()
                    .map(|value| value.to_string_lossy().to_string())
                    .unwrap_or_else(|| target_path.display().to_string())
            );
        }
        if !replace_all && matches > 1 {
            bail!("old_string matched multiple locations; set replace_all=true");
        }

        let updated = if replace_all {
            original.replace(old_string, new_string)
        } else {
            original.replacen(old_string, new_string, 1)
        };
        fs::write(&target_path, updated)
            .with_context(|| format!("failed to write {}", target_path.display()))?;
        Ok(target_path)
    }

    pub fn write_supporting_file(
        &self,
        name: &str,
        category: Option<&str>,
        file_path: &str,
        content: &str,
    ) -> Result<PathBuf> {
        let (_, target_path) =
            self.resolve_mutation_target(name, category, Some(file_path), false)?;
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(&target_path, content)
            .with_context(|| format!("failed to write {}", target_path.display()))?;
        Ok(target_path)
    }

    pub fn remove_supporting_file(
        &self,
        name: &str,
        category: Option<&str>,
        file_path: &str,
    ) -> Result<PathBuf> {
        let (_, target_path) =
            self.resolve_mutation_target(name, category, Some(file_path), true)?;
        fs::remove_file(&target_path)
            .with_context(|| format!("failed to remove {}", target_path.display()))?;
        prune_empty_skill_dirs(
            target_path
                .parent()
                .context("target path is missing a parent directory")?,
        )?;
        Ok(target_path)
    }

    fn resolve_mutation_target(
        &self,
        name: &str,
        category: Option<&str>,
        file_path: Option<&str>,
        require_exists: bool,
    ) -> Result<(PathBuf, PathBuf)> {
        let skill = self.resolve_skill(name, category)?;
        let skill_dir = skill
            .path
            .parent()
            .context("skill path is missing a parent directory")?
            .to_path_buf();
        let (resolved_file_path, target_path) =
            resolve_mutation_file_path(&skill_dir, file_path, require_exists)?;
        Ok((PathBuf::from(resolved_file_path), target_path))
    }

    fn collect_skills(&self, root: &Path, dir: &Path, out: &mut Vec<SkillSummary>) -> Result<()> {
        for entry in
            fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                let name = entry.file_name().to_string_lossy().to_string();
                if is_excluded_dir(&name) {
                    continue;
                }
                self.collect_skills(root, &path, out)?;
                continue;
            }
            if entry.file_name() != "SKILL.md" {
                continue;
            }

            let raw = fs::read_to_string(&path)
                .with_context(|| format!("failed to read {}", path.display()))?;
            let (frontmatter, _) = parse_frontmatter(&raw);
            if !skill_matches_os(&normalize_string_list(frontmatter.platforms.clone())) {
                continue;
            }

            let rel = path
                .strip_prefix(root)
                .unwrap_or(path.as_path())
                .components()
                .map(|component| component.as_os_str().to_string_lossy().to_string())
                .collect::<Vec<_>>();
            let (category, name) = match rel.as_slice() {
                [category, name, file] if file == "SKILL.md" => (category.clone(), name.clone()),
                [name, file] if file == "SKILL.md" => ("general".to_string(), name.clone()),
                _ => continue,
            };
            if self.disabled_skills.contains(&normalize_tag(&name)) {
                continue;
            }

            let mut keywords = normalize_string_list(frontmatter.keywords);
            keywords.extend(normalize_string_list(
                frontmatter.metadata.and_then(|value| value.hermes.tags),
            ));
            dedupe_vec(&mut keywords);

            let description = frontmatter
                .description
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| extract_description(&raw));
            let activation = SkillActivation {
                task_kinds: normalize_string_list(frontmatter.task_kinds),
                requires_tools: normalize_string_list(frontmatter.requires_tools),
                requires_shell: parse_boolish(frontmatter.requires_shell),
            };

            out.push(SkillSummary {
                category,
                name,
                description,
                keywords,
                activation,
                path: path.clone(),
                updated_at_unix: frontmatter.updated_at_unix,
            });
        }
        Ok(())
    }
}

fn load_root_config(data_dir: &Path) -> Result<SkillsConfig> {
    let config_path = ["config.yaml", "config.yml"]
        .iter()
        .map(|name| data_dir.join(name))
        .find(|path| path.is_file());
    let Some(config_path) = config_path else {
        return Ok(SkillsConfig::default());
    };

    let raw = fs::read_to_string(&config_path)
        .with_context(|| format!("failed to read {}", config_path.display()))?;
    let parsed: RootConfig = serde_yaml::from_str(&raw)
        .with_context(|| format!("failed to parse {}", config_path.display()))?;
    Ok(parsed.skills)
}

fn build_skill_readiness(
    frontmatter: &SkillFrontmatter,
    config_settings: &BTreeMap<String, serde_yaml::Value>,
) -> SkillReadiness {
    let required_environment_variables = collect_required_env_vars(frontmatter);
    let missing_required_environment_variables = required_environment_variables
        .iter()
        .map(|item| item.name.clone())
        .filter(|name| env::var_os(name).is_none())
        .collect::<Vec<_>>();

    let required_commands = normalize_string_list(frontmatter.required_commands.clone());
    let missing_required_commands = required_commands
        .iter()
        .filter(|command| !command_exists(command))
        .cloned()
        .collect::<Vec<_>>();

    let config_requirements = frontmatter
        .metadata
        .as_ref()
        .map(|value| {
            value
                .hermes
                .config
                .iter()
                .filter(|item| !item.key.trim().is_empty())
                .map(|item| SkillConfigRequirement {
                    key: item.key.trim().to_string(),
                    description: item
                        .description
                        .clone()
                        .filter(|value| !value.trim().is_empty())
                        .unwrap_or_else(|| "No description provided.".to_string()),
                    prompt: item.prompt.clone().filter(|value| !value.trim().is_empty()),
                    default_value: item.default.as_ref().and_then(yaml_value_to_string),
                    resolved_value: config_settings
                        .get(item.key.trim())
                        .and_then(yaml_value_to_string)
                        .or_else(|| item.default.as_ref().and_then(yaml_value_to_string)),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let setup_needed =
        !missing_required_environment_variables.is_empty() || !missing_required_commands.is_empty();
    SkillReadiness {
        required_environment_variables,
        missing_required_environment_variables,
        required_commands,
        missing_required_commands,
        config_requirements,
        setup_needed,
        readiness_status: if setup_needed {
            "setup_needed".to_string()
        } else {
            "available".to_string()
        },
    }
}

fn collect_required_env_vars(frontmatter: &SkillFrontmatter) -> Vec<SkillEnvRequirement> {
    let mut items = frontmatter
        .required_environment_variables
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| match item {
            EnvRequirementField::String(name) => {
                let trimmed = name.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(SkillEnvRequirement {
                        name: trimmed.to_string(),
                        prompt: format!("Enter value for {trimmed}"),
                        help: None,
                        required_for: None,
                    })
                }
            }
            EnvRequirementField::Object(item) => {
                let name = item.name.trim();
                if name.is_empty() {
                    None
                } else {
                    Some(SkillEnvRequirement {
                        name: name.to_string(),
                        prompt: item
                            .prompt
                            .filter(|value| !value.trim().is_empty())
                            .unwrap_or_else(|| format!("Enter value for {name}")),
                        help: item.help.filter(|value| !value.trim().is_empty()),
                        required_for: item.required_for.filter(|value| !value.trim().is_empty()),
                    })
                }
            }
        })
        .collect::<Vec<_>>();

    for legacy_name in frontmatter
        .prerequisites
        .as_ref()
        .and_then(|value| value.env_vars.clone())
        .map(|value| normalize_string_list(Some(value)))
        .unwrap_or_default()
    {
        if items.iter().any(|item| item.name == legacy_name) {
            continue;
        }
        items.push(SkillEnvRequirement {
            prompt: format!("Enter value for {legacy_name}"),
            name: legacy_name,
            help: None,
            required_for: None,
        });
    }

    items.sort_by(|a, b| a.name.cmp(&b.name));
    items
}

fn render_skill_markdown(
    name: &str,
    description: &str,
    keywords: &[String],
    activation: &SkillActivation,
    body: &str,
    created_at_unix: u64,
    updated_at_unix: u64,
) -> Result<String> {
    #[derive(serde::Serialize)]
    struct Frontmatter<'a> {
        name: &'a str,
        description: &'a str,
        keywords: &'a [String],
        task_kinds: &'a [String],
        requires_tools: &'a [String],
        requires_shell: bool,
        created_at_unix: u64,
        updated_at_unix: u64,
    }

    let frontmatter = Frontmatter {
        name: name.trim(),
        description: description.trim(),
        keywords,
        task_kinds: &activation.task_kinds,
        requires_tools: &activation.requires_tools,
        requires_shell: activation.requires_shell,
        created_at_unix,
        updated_at_unix,
    };
    let yaml = serde_yaml::to_string(&frontmatter).context("failed to serialize skill metadata")?;
    Ok(format!("---\n{}---\n\n{}", yaml, body.trim()))
}

fn parse_frontmatter(content: &str) -> (SkillFrontmatter, String) {
    if !content.starts_with("---\n") {
        return (SkillFrontmatter::default(), content.to_string());
    }
    let Some(end) = content[4..].find("\n---") else {
        return (SkillFrontmatter::default(), content.to_string());
    };

    let yaml = &content[4..4 + end];
    let body = content[(4 + end + 4)..]
        .trim_start_matches('\n')
        .to_string();
    let parsed = serde_yaml::from_str::<SkillFrontmatter>(yaml).unwrap_or_default();
    (parsed, body)
}

fn extract_description(content: &str) -> String {
    content
        .lines()
        .find(|line| {
            !line.trim().is_empty()
                && !line.trim().starts_with("---")
                && !line.trim().starts_with('#')
        })
        .map(|line| line.trim().to_string())
        .unwrap_or_default()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn tokenize(query: &str) -> HashSet<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(normalize_tag)
        .filter(|part| part.len() >= 2)
        .collect()
}

fn detect_office_workflow(query: &str) -> Option<OfficeWorkflowTarget> {
    let lowered = query.to_lowercase();
    if contains_any(
        &lowered,
        &[
            ".xlsx",
            " excel",
            "excel ",
            "spreadsheet",
            "workbook",
            "sheet",
            "表格",
            "工作簿",
        ],
    ) {
        return Some(OfficeWorkflowTarget::Xlsx);
    }
    if contains_any(
        &lowered,
        &[
            ".docx",
            " word",
            "word ",
            "paragraph",
            "document",
            "文档",
            "段落",
        ],
    ) {
        return Some(OfficeWorkflowTarget::Docx);
    }
    if is_explicit_pptx_request(&lowered) {
        return Some(OfficeWorkflowTarget::Pptx);
    }
    if contains_any(
        &lowered,
        &[
            "office", "文件", "excel", "word", "ppt", "pptx", "xlsx", "docx",
        ],
    ) {
        return Some(OfficeWorkflowTarget::Generic);
    }
    None
}

fn is_explicit_pptx_request(lowered: &str) -> bool {
    contains_any(
        lowered,
        &[
            ".pptx",
            "powerpoint",
            " pptx",
            "pptx ",
            " ppt ",
            "ppt ",
            "ppt文件",
        ],
    )
}

fn has_slide_terms(lowered: &str) -> bool {
    contains_any(
        lowered,
        &[
            "slidev",
            "presentation",
            "slides",
            "slide deck",
            "pitch deck",
            "deck",
            "keynote",
            "talk",
            "幻灯片",
            "演示",
            "路演",
        ],
    )
}

fn is_slide_authoring_request(query: &str) -> bool {
    let lowered = query.to_lowercase();
    if is_explicit_pptx_request(&lowered) || !has_slide_terms(&lowered) {
        return false;
    }
    contains_any(
        &lowered,
        &[
            "slidev", "create", "make", "prepare", "draft", "generate", "build", "new", "做",
            "生成", "创建", "准备", "起草",
        ],
    )
}

fn preferred_office_skill_name(target: OfficeWorkflowTarget) -> &'static str {
    match target {
        OfficeWorkflowTarget::Xlsx => "xlsx-edit",
        OfficeWorkflowTarget::Docx => "docx-edit",
        OfficeWorkflowTarget::Pptx => "pptx-review",
        OfficeWorkflowTarget::Generic => "office-router",
    }
}

fn build_route_note(query: &str, matches: &[SkillMatch]) -> Option<String> {
    let has_slidev_match = matches.iter().any(|matched| {
        matched.document.summary.category == "slides"
            && matched.document.summary.name == "slidev-deck"
    });
    if has_slidev_match && is_slide_authoring_request(query) {
        return Some(
            "[Routing note: This looks like a new presentation or deck request. Prefer `slides/slidev-deck` and let Slidev preview start by default unless the user explicitly needs `.pptx` or an existing PowerPoint file.]"
                .to_string(),
        );
    }

    let target = detect_office_workflow(query)?;
    let preferred = preferred_office_skill_name(target);
    let has_office_match = matches
        .iter()
        .any(|matched| matched.document.summary.category == "office");
    if !has_office_match {
        return None;
    }
    Some(match target {
        OfficeWorkflowTarget::Xlsx => format!(
            "[Routing note: The request looks spreadsheet-oriented. Prefer `office/{preferred}` over `office/office-router` unless the format is still uncertain.]"
        ),
        OfficeWorkflowTarget::Docx => format!(
            "[Routing note: The request looks document-oriented. Prefer `office/{preferred}` over `office/office-router` unless the format is still uncertain.]"
        ),
        OfficeWorkflowTarget::Pptx => format!(
            "[Routing note: The request looks presentation-oriented. Prefer `office/{preferred}` over `office/office-router` unless the format is still uncertain.]"
        ),
        OfficeWorkflowTarget::Generic => format!(
            "[Routing note: The request looks Office-related but the target format is not fully clear. Start with `office/{preferred}`.]"
        ),
    })
}

fn score_skill(
    summary: &SkillSummary,
    content: &str,
    query: &str,
    tokens: &HashSet<String>,
    context: Option<&SkillQueryContext>,
) -> usize {
    if !activation_matches(&summary.activation, context) {
        return 0;
    }

    let name = summary.name.to_lowercase();
    let description = summary.description.to_lowercase();
    let category = summary.category.to_lowercase();
    let keywords = summary
        .keywords
        .iter()
        .map(|value| value.to_lowercase())
        .collect::<Vec<_>>();
    let body = content.to_lowercase();
    let mut score: usize = tokens
        .iter()
        .map(|token| {
            let mut score = 0usize;
            if name.contains(token) {
                score += 5;
            }
            if description.contains(token) {
                score += 4;
            }
            if category.contains(token) {
                score += 2;
            }
            if keywords.iter().any(|keyword| keyword.contains(token)) {
                score += 6;
            }
            if body.contains(token) {
                score += 1;
            }
            score
        })
        .sum();

    if let Some(context) = context {
        let matched_task_kinds = summary
            .activation
            .task_kinds
            .iter()
            .map(|value| normalize_tag(value))
            .filter(|value| context.task_kinds.contains(value))
            .count();
        score += matched_task_kinds * 6;

        if !summary.activation.requires_tools.is_empty() {
            score += summary.activation.requires_tools.len() * 2;
        }
        if summary.activation.requires_shell && context.shell_enabled {
            score += 2;
        }
    }

    score += slidev_workflow_bonus(summary, query);
    score += office_workflow_bonus(summary, query);

    score
}

fn slidev_workflow_bonus(summary: &SkillSummary, query: &str) -> usize {
    if summary.category != "slides" || summary.name != "slidev-deck" {
        return 0;
    }

    if is_slide_authoring_request(query) {
        return 42;
    }

    let lowered = query.to_lowercase();
    if !is_explicit_pptx_request(&lowered) && has_slide_terms(&lowered) {
        return 12;
    }

    0
}

fn office_workflow_bonus(summary: &SkillSummary, query: &str) -> usize {
    if summary.category != "office" {
        return 0;
    }

    match detect_office_workflow(query) {
        Some(OfficeWorkflowTarget::Xlsx) => match summary.name.as_str() {
            "xlsx-edit" => 40,
            "office-router" => 12,
            _ => 0,
        },
        Some(OfficeWorkflowTarget::Docx) => match summary.name.as_str() {
            "docx-edit" => 40,
            "office-router" => 12,
            _ => 0,
        },
        Some(OfficeWorkflowTarget::Pptx) => match summary.name.as_str() {
            "pptx-review" => 40,
            "office-router" => 12,
            _ => 0,
        },
        Some(OfficeWorkflowTarget::Generic) => match summary.name.as_str() {
            "office-router" => 18,
            "xlsx-edit" | "docx-edit" | "pptx-review" => 6,
            _ => 0,
        },
        None => 0,
    }
}

fn normalize_string_list(value: Option<StringListField>) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    match value {
        StringListField::String(value) => value
            .split(',')
            .map(normalize_tag)
            .filter(|value| !value.is_empty())
            .filter(|value| seen.insert(value.clone()))
            .collect(),
        StringListField::List(values) => values
            .into_iter()
            .map(|value| normalize_tag(&value))
            .filter(|value| !value.is_empty())
            .filter(|value| seen.insert(value.clone()))
            .collect(),
    }
}

fn truncate_skill_body(content: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        return content.to_string();
    }
    let mut clipped = content.chars().take(max_chars).collect::<String>();
    clipped.push_str("\n...[truncated]...");
    clipped
}

fn parse_boolish(value: Option<BoolField>) -> bool {
    match value {
        Some(BoolField::Bool(value)) => value,
        Some(BoolField::String(value)) => {
            matches!(value.trim(), "1" | "true" | "TRUE" | "yes" | "on")
        }
        None => false,
    }
}

fn normalize_tag(value: &str) -> String {
    value.trim().to_lowercase()
}

fn render_activation_summary(activation: &SkillActivation) -> String {
    let mut fields = Vec::new();
    if !activation.task_kinds.is_empty() {
        fields.push(format!("task_kinds={}", activation.task_kinds.join(", ")));
    }
    if !activation.requires_tools.is_empty() {
        fields.push(format!(
            "requires_tools={}",
            activation.requires_tools.join(", ")
        ));
    }
    if activation.requires_shell {
        fields.push("requires_shell=true".to_string());
    }

    if fields.is_empty() {
        "_Activation: general_".to_string()
    } else {
        format!("_Activation: {}_", fields.join("; "))
    }
}

fn activation_matches(activation: &SkillActivation, context: Option<&SkillQueryContext>) -> bool {
    let Some(context) = context else {
        return true;
    };

    if activation.requires_shell && !context.shell_enabled {
        return false;
    }

    if activation
        .requires_tools
        .iter()
        .map(|value| normalize_tag(value))
        .any(|tool| !context.available_tools.contains(&tool))
    {
        return false;
    }

    if !activation.task_kinds.is_empty()
        && !activation
            .task_kinds
            .iter()
            .map(|value| normalize_tag(value))
            .any(|kind| context.task_kinds.contains(&kind))
    {
        return false;
    }

    true
}

fn detect_task_kinds(query: &str) -> HashSet<String> {
    let lowered = query.to_lowercase();
    let mut kinds = HashSet::new();

    if contains_any(
        &lowered,
        &["fix", "bug", "debug", "failing", "error", "broken", "issue"],
    ) {
        kinds.insert("debugging".to_string());
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
        kinds.insert("coding".to_string());
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
        kinds.insert("analysis".to_string());
    }
    if contains_any(&lowered, &["plan", "design", "architecture", "approach"]) {
        kinds.insert("planning".to_string());
    }
    if contains_any(&lowered, &["doc", "readme", "document"]) {
        kinds.insert("documentation".to_string());
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
        kinds.insert("operations".to_string());
    }

    if kinds.is_empty() {
        kinds.insert("general".to_string());
    }

    kinds
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn skill_matches_os(platforms: &[String]) -> bool {
    if platforms.is_empty() {
        return true;
    }
    let current = current_os_tag();
    platforms
        .iter()
        .map(|value| normalize_tag(value))
        .any(|value| value == current)
}

fn current_os_tag() -> String {
    match env::consts::OS {
        "macos" => "macos".to_string(),
        "windows" => "windows".to_string(),
        _ => "linux".to_string(),
    }
}

fn is_excluded_dir(name: &str) -> bool {
    matches!(name, ".git" | ".github" | ".hub")
}

fn expand_config_path(raw: &str) -> PathBuf {
    let expanded_home = if let Some(rest) = raw.strip_prefix("~/") {
        env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("~"))
            .join(rest)
    } else {
        PathBuf::from(raw)
    };
    let raw_string = expanded_home.to_string_lossy().to_string();
    let expanded_env =
        env_var_regex().replace_all(&raw_string, |captures: &regex::Captures<'_>| {
            captures
                .get(1)
                .or_else(|| captures.get(2))
                .and_then(|value| env::var(value.as_str()).ok())
                .unwrap_or_default()
        });
    PathBuf::from(expanded_env.into_owned())
}

fn bundled_skills_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bundled-skills")
}

fn command_exists(command: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };
    env::split_paths(&paths).any(|directory| {
        let candidate = directory.join(command);
        if candidate.is_file() {
            return true;
        }
        #[cfg(windows)]
        {
            for ext in ["exe", "cmd", "bat"] {
                if directory.join(format!("{command}.{ext}")).is_file() {
                    return true;
                }
            }
        }
        false
    })
}

fn yaml_value_to_string(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::Null => None,
        serde_yaml::Value::Bool(value) => Some(value.to_string()),
        serde_yaml::Value::Number(value) => Some(value.to_string()),
        serde_yaml::Value::String(value) => Some(value.clone()),
        other => serde_yaml::to_string(other)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()),
    }
}

fn collect_linked_files(skill_dir: &Path) -> Result<BTreeMap<String, Vec<SkillLinkedFile>>> {
    let mut linked_files = BTreeMap::new();
    for group in ["references", "templates", "scripts", "assets"] {
        let group_dir = skill_dir.join(group);
        if !group_dir.is_dir() {
            continue;
        }

        let mut files = WalkDir::new(&group_dir)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| {
                let path = entry.path();
                let metadata = entry
                    .metadata()
                    .with_context(|| format!("failed to read metadata for {}", path.display()))?;
                Ok(SkillLinkedFile {
                    path: relative_slash_path(skill_dir, path),
                    size_bytes: metadata.len(),
                    file_type: file_type_label(path),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        files.sort_by(|a, b| a.path.cmp(&b.path));

        if !files.is_empty() {
            linked_files.insert(group.to_string(), files);
        }
    }
    Ok(linked_files)
}

fn resolve_skill_file_path(skill_dir: &Path, file_path: Option<&str>) -> Result<(String, PathBuf)> {
    resolve_mutation_file_path(skill_dir, file_path, true)
}

fn resolve_mutation_file_path(
    skill_dir: &Path,
    file_path: Option<&str>,
    require_exists: bool,
) -> Result<(String, PathBuf)> {
    match file_path.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(("SKILL.md".to_string(), skill_dir.join("SKILL.md"))),
        Some(file_path) => {
            let requested = Path::new(file_path);
            if requested.is_absolute() {
                bail!("file_path must be relative to the skill directory");
            }
            if requested.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            }) {
                bail!("file_path must stay within the skill directory");
            }
            let first_component = requested
                .components()
                .next()
                .map(|component| component.as_os_str().to_string_lossy().to_string())
                .unwrap_or_default();
            if !ALLOWED_SKILL_SUBDIRS
                .iter()
                .any(|value| *value == first_component.as_str())
            {
                bail!(
                    "file_path must live under one of: {}",
                    ALLOWED_SKILL_SUBDIRS.join(", ")
                );
            }
            if requested.components().count() < 2 {
                bail!("file_path must include a file name inside the skill subdirectory");
            }

            let target = skill_dir.join(requested);
            if require_exists && !target.is_file() {
                bail!("skill file `{file_path}` not found");
            }

            if require_exists {
                let resolved_skill_dir = skill_dir
                    .canonicalize()
                    .with_context(|| format!("failed to resolve {}", skill_dir.display()))?;
                let resolved_target = target
                    .canonicalize()
                    .with_context(|| format!("failed to resolve {}", target.display()))?;
                if !resolved_target.starts_with(&resolved_skill_dir) {
                    bail!("file_path escapes skill directory boundary");
                }
            }

            Ok((relative_slash_path(skill_dir, &target), target))
        }
    }
}

fn prune_empty_skill_dirs(start_dir: &Path) -> Result<()> {
    let mut current = start_dir.to_path_buf();
    while let Some(name) = current
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
    {
        if !ALLOWED_SKILL_SUBDIRS
            .iter()
            .any(|value| *value == name.as_str())
        {
            break;
        }
        let is_empty = fs::read_dir(&current)
            .with_context(|| format!("failed to read {}", current.display()))?
            .next()
            .is_none();
        if !is_empty {
            break;
        }
        let parent = current
            .parent()
            .context("skill support directory is missing a parent")?
            .to_path_buf();
        fs::remove_dir(&current)
            .with_context(|| format!("failed to remove {}", current.display()))?;
        current = parent;
    }
    Ok(())
}

fn read_skill_file_content(path: &Path) -> Result<(String, bool)> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    match String::from_utf8(bytes) {
        Ok(content) => Ok((content, false)),
        Err(error) => {
            let bytes = error.into_bytes();
            let file_name = path
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            Ok((
                format!("[Binary file: {file_name}, size: {} bytes]", bytes.len()),
                true,
            ))
        }
    }
}

fn relative_slash_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn file_type_label(path: &Path) -> String {
    path.extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

const ALLOWED_SKILL_SUBDIRS: [&str; 4] = ["references", "templates", "scripts", "assets"];

fn env_var_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\$\{([^}]+)\}|\$([A-Za-z_][A-Za-z0-9_]*)").expect("regex"))
}

fn dedupe_vec(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

#[cfg(test)]
mod tests {
    use super::{SkillActivation, SkillQueryContext, SkillStore};
    use std::fs;

    fn isolated_store(data_dir: &std::path::Path) -> SkillStore {
        let config_path = data_dir.join("config.yaml");
        if !config_path.exists() {
            fs::write(&config_path, "skills:\n  include_bundled: false\n").expect("write config");
        }
        SkillStore::new(data_dir).expect("store")
    }

    fn isolated_store_with_platform(data_dir: &std::path::Path, platform: &str) -> SkillStore {
        let config_path = data_dir.join("config.yaml");
        if !config_path.exists() {
            fs::write(&config_path, "skills:\n  include_bundled: false\n").expect("write config");
        }
        SkillStore::new_with_platform(data_dir, Some(platform)).expect("store")
    }

    fn write_skill(
        root: &std::path::Path,
        category: &str,
        name: &str,
        description: &str,
        keywords: &[&str],
    ) {
        let skill_dir = root.join("skills").join(category).join(name);
        fs::create_dir_all(&skill_dir).expect("mkdir skill");
        let keywords_yaml = if keywords.is_empty() {
            String::new()
        } else {
            format!(
                "keywords:\n{}",
                keywords
                    .iter()
                    .map(|keyword| format!("  - {keyword}\n"))
                    .collect::<String>()
            )
        };
        fs::write(
            skill_dir.join("SKILL.md"),
            format!(
                "---\nname: {name}\ndescription: {description}\n{keywords_yaml}---\n\n# {name}\n\nWorkflow.\n"
            ),
        )
        .expect("write skill");
    }

    #[test]
    fn saves_and_lists_skills() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = isolated_store(tmp.path());
        store
            .save_with_metadata(
                "coding",
                "rust-review",
                "Review Rust code carefully.",
                &["rust".to_string(), "review".to_string()],
                &SkillActivation {
                    task_kinds: vec!["analysis".to_string()],
                    requires_tools: vec!["read_file".to_string()],
                    requires_shell: false,
                },
                "# Rust Review\n\nCheck ownership and errors.",
            )
            .expect("save");

        let skills = store.list().expect("list");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].category, "coding");
        assert_eq!(skills[0].name, "rust-review");
        assert_eq!(
            skills[0].keywords,
            vec!["rust".to_string(), "review".to_string()]
        );
        assert_eq!(
            skills[0].activation.task_kinds,
            vec!["analysis".to_string()]
        );
        assert_eq!(
            skills[0].activation.requires_tools,
            vec!["read_file".to_string()]
        );
        assert!(skills[0].updated_at_unix.is_some());
    }

    #[test]
    fn searches_matching_skills() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = isolated_store(tmp.path());
        store
            .save(
                "coding",
                "rust-review",
                "Review Rust code carefully.",
                &["rust".to_string(), "review".to_string()],
                "# Rust Review\n\nCheck ownership and errors.",
            )
            .expect("save");
        store
            .save(
                "ops",
                "docker-cleanup",
                "Cleanup docker resources.",
                &["docker".to_string(), "cleanup".to_string()],
                "# Docker Cleanup\n\nRemove old images.",
            )
            .expect("save");

        let matches = store.search("review rust module", 5).expect("search");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].document.summary.name, "rust-review");
    }

    #[test]
    fn search_respects_activation_requirements() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = isolated_store(tmp.path());
        store
            .save_with_metadata(
                "coding",
                "shell-debug",
                "Debug shell-heavy failures.",
                &["debug".to_string(), "shell".to_string()],
                &SkillActivation {
                    task_kinds: vec!["debugging".to_string()],
                    requires_tools: vec!["terminal".to_string()],
                    requires_shell: true,
                },
                "# Shell Debug\n\nInspect processes and logs.",
            )
            .expect("save");

        let without_shell =
            SkillQueryContext::from_query("debug failing build", ["terminal".to_string()], false);
        let matches = store
            .search_with_context("debug failing build", 5, Some(&without_shell))
            .expect("search");
        assert!(matches.is_empty());

        let with_shell =
            SkillQueryContext::from_query("debug failing build", ["terminal".to_string()], true);
        let matches = store
            .search_with_context("debug failing build", 5, Some(&with_shell))
            .expect("search");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].document.summary.name, "shell-debug");
    }

    #[test]
    fn search_filters_mismatched_task_kind() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = isolated_store(tmp.path());
        store
            .save_with_metadata(
                "docs",
                "release-notes",
                "Write release notes.",
                &["release".to_string(), "notes".to_string()],
                &SkillActivation {
                    task_kinds: vec!["documentation".to_string()],
                    requires_tools: Vec::new(),
                    requires_shell: false,
                },
                "# Release Notes\n\nCapture changes clearly.",
            )
            .expect("save");

        let context = SkillQueryContext::from_query(
            "implement auth middleware",
            ["read_file".to_string(), "write_file".to_string()],
            false,
        );
        let matches = store
            .search_with_context("implement auth middleware", 5, Some(&context))
            .expect("search");
        assert!(matches.is_empty());
    }

    #[test]
    fn search_prefers_specific_office_workflow_over_router() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_skill(
            tmp.path(),
            "office",
            "office-router",
            "Route office requests.",
            &["office", "excel", "word", "powerpoint"],
        );
        write_skill(
            tmp.path(),
            "office",
            "xlsx-edit",
            "Edit xlsx workbooks.",
            &["xlsx", "excel", "spreadsheet", "workbook"],
        );
        write_skill(
            tmp.path(),
            "office",
            "docx-edit",
            "Edit docx documents.",
            &["docx", "word", "document"],
        );

        let matches = isolated_store(tmp.path())
            .search("Please update revenue.xlsx and add a new summary sheet", 3)
            .expect("search");
        assert_eq!(matches[0].document.summary.name, "xlsx-edit");
        assert_eq!(matches[1].document.summary.name, "office-router");
    }

    #[test]
    fn build_context_block_adds_office_route_note() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_skill(
            tmp.path(),
            "office",
            "office-router",
            "Route office requests.",
            &["office", "excel", "word", "powerpoint"],
        );
        write_skill(
            tmp.path(),
            "office",
            "pptx-review",
            "Review pptx decks.",
            &["pptx", "powerpoint", "presentation", "slides"],
        );

        let block = isolated_store(tmp.path())
            .build_context_block(
                "Please review this pitch deck.pptx and summarize the slides",
                3,
            )
            .expect("context")
            .expect("some context");
        assert!(block.contains("Routing note"));
        assert!(block.contains("office/pptx-review"));
    }

    #[test]
    fn build_brief_context_block_uses_skill_briefs_and_detail_hint() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_skill(
            tmp.path(),
            "office",
            "docx-edit",
            "Edit docx documents.",
            &["docx", "word", "document"],
        );

        let block = isolated_store(tmp.path())
            .build_brief_context_block_with_context("Please update this contract.docx", 3, None)
            .expect("context")
            .expect("some context");
        assert!(block.contains("office/docx-edit"));
        assert!(block.contains("Detail hint: use `skill_view`"));
        assert!(!block.contains("Workflow."));
    }

    #[test]
    fn search_prefers_slidev_for_new_presentation_requests() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_skill(
            tmp.path(),
            "slides",
            "slidev-deck",
            "Create Slidev decks.",
            &["slidev", "slides", "presentation", "pitch deck"],
        );
        write_skill(
            tmp.path(),
            "office",
            "office-router",
            "Route office requests.",
            &["office", "excel", "word", "powerpoint"],
        );
        write_skill(
            tmp.path(),
            "office",
            "pptx-review",
            "Review pptx decks.",
            &["pptx", "powerpoint", "presentation", "slides"],
        );

        let matches = isolated_store(tmp.path())
            .search("Create a pitch deck for our product launch", 3)
            .expect("search");
        assert_eq!(matches[0].document.summary.name, "slidev-deck");
    }

    #[test]
    fn build_context_block_prefers_slidev_route_note_for_new_decks() {
        let tmp = tempfile::tempdir().expect("tempdir");
        write_skill(
            tmp.path(),
            "slides",
            "slidev-deck",
            "Create Slidev decks.",
            &["slidev", "slides", "presentation", "pitch deck"],
        );
        write_skill(
            tmp.path(),
            "office",
            "office-router",
            "Route office requests.",
            &["office", "excel", "word", "powerpoint"],
        );
        write_skill(
            tmp.path(),
            "office",
            "pptx-review",
            "Review pptx decks.",
            &["pptx", "powerpoint", "presentation", "slides"],
        );

        let block = isolated_store(tmp.path())
            .build_context_block("Please create a presentation for next week's launch", 3)
            .expect("context")
            .expect("some context");
        assert!(block.contains("Routing note"));
        assert!(block.contains("slides/slidev-deck"));
    }

    #[test]
    fn parses_yaml_arrays_and_metadata_tags() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let skill_dir = tmp.path().join("skills").join("coding").join("deep-review");
        fs::create_dir_all(&skill_dir).expect("mkdirs");
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: deep-review
description: Deep review skill
keywords:
  - rust
  - safety
task_kinds: [analysis, coding]
requires_tools:
  - read_file
platforms: [linux, macos]
metadata:
  hermes:
    tags: [ownership, review]
updated_at_unix: 123
---

# Deep Review

Inspect ownership.
"#,
        )
        .expect("write");

        let store = isolated_store(tmp.path());
        let skills = store.list().expect("list");
        assert_eq!(skills.len(), 1);
        assert_eq!(
            skills[0].keywords,
            vec![
                "rust".to_string(),
                "safety".to_string(),
                "ownership".to_string(),
                "review".to_string()
            ]
        );
        assert_eq!(
            skills[0].activation.task_kinds,
            vec!["analysis".to_string(), "coding".to_string()]
        );
        assert_eq!(skills[0].updated_at_unix, Some(123));
    }

    #[test]
    fn honors_disabled_skills_and_external_dirs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let external = tempfile::tempdir().expect("external");
        let external_skill_dir = external.path().join("ops").join("docker-audit");
        fs::create_dir_all(&external_skill_dir).expect("mkdir external");
        fs::write(
            external_skill_dir.join("SKILL.md"),
            r#"---
name: docker-audit
description: Inspect docker state
---

# Docker Audit
"#,
        )
        .expect("write external");

        let local_skill_dir = tmp.path().join("skills").join("coding").join("rust-review");
        fs::create_dir_all(&local_skill_dir).expect("mkdir local");
        fs::write(
            local_skill_dir.join("SKILL.md"),
            r#"---
name: rust-review
description: Review rust
---

# Rust Review
"#,
        )
        .expect("write local");

        fs::write(
            tmp.path().join("config.yaml"),
            format!(
                "skills:\n  include_bundled: false\n  disabled: [rust-review]\n  external_dirs:\n    - {}\n",
                external.path().display()
            ),
        )
        .expect("write config");

        let store = isolated_store(tmp.path());
        let skills = store.list().expect("list");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "docker-audit");
    }

    #[test]
    fn honors_platform_disabled_for_selected_platform() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let first = tmp.path().join("skills").join("coding").join("keep-me");
        let second = tmp.path().join("skills").join("coding").join("hide-me");
        fs::create_dir_all(&first).expect("mkdir first");
        fs::create_dir_all(&second).expect("mkdir second");
        fs::write(
            first.join("SKILL.md"),
            "---\nname: keep-me\ndescription: Keep\n---\n\n# Keep\n",
        )
        .expect("write first");
        fs::write(
            second.join("SKILL.md"),
            "---\nname: hide-me\ndescription: Hide\n---\n\n# Hide\n",
        )
        .expect("write second");
        fs::write(
            tmp.path().join("config.yaml"),
            "skills:\n  include_bundled: false\n  platform_disabled:\n    desktop: [hide-me]\n",
        )
        .expect("write config");

        let store = isolated_store_with_platform(tmp.path(), "desktop");
        let skills = store.list().expect("list");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "keep-me");
    }

    #[test]
    fn views_skill_and_linked_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let skill_dir = tmp.path().join("skills").join("coding").join("rust-review");
        fs::create_dir_all(skill_dir.join("references")).expect("mkdir refs");
        fs::create_dir_all(skill_dir.join("scripts")).expect("mkdir scripts");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: rust-review\ndescription: Review rust\n---\n\n# Rust Review\n",
        )
        .expect("write skill");
        fs::write(
            skill_dir.join("references").join("api.md"),
            "# API\n\nReference content\n",
        )
        .expect("write ref");
        fs::write(
            skill_dir.join("scripts").join("run.sh"),
            "#!/bin/sh\necho review\n",
        )
        .expect("write script");

        let store = isolated_store(tmp.path());
        let root_view = store
            .view_with_file("rust-review", Some("coding"), None)
            .expect("root view");
        assert_eq!(root_view.file_path, "SKILL.md");
        assert!(root_view.linked_files.contains_key("references"));
        assert!(root_view.linked_files.contains_key("scripts"));

        let ref_view = store
            .view_with_file("rust-review", Some("coding"), Some("references/api.md"))
            .expect("ref view");
        assert_eq!(ref_view.file_path, "references/api.md");
        assert!(ref_view.content.contains("Reference content"));
    }

    #[test]
    fn blocks_skill_file_traversal() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let skill_dir = tmp.path().join("skills").join("coding").join("rust-review");
        fs::create_dir_all(&skill_dir).expect("mkdir skill");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: rust-review\ndescription: Review rust\n---\n\n# Rust Review\n",
        )
        .expect("write skill");
        fs::write(tmp.path().join("secret.env"), "SECRET=1").expect("write secret");

        let store = isolated_store(tmp.path());
        let error = store
            .view_with_file("rust-review", Some("coding"), Some("../secret.env"))
            .expect_err("expected traversal error");
        assert!(error.to_string().contains("skill directory"));
    }

    #[test]
    fn patches_skill_body() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = isolated_store(tmp.path());
        store
            .save(
                "coding",
                "rust-review",
                "Review rust",
                &["rust".to_string()],
                "# Rust Review\n\nCheck ownership.\n",
            )
            .expect("save");

        let path = store
            .patch(
                "rust-review",
                Some("coding"),
                "Check ownership.",
                "Check ownership and lifetimes.",
                None,
                false,
            )
            .expect("patch");

        let content = fs::read_to_string(path).expect("read");
        assert!(content.contains("Check ownership and lifetimes."));
    }

    #[test]
    fn writes_and_removes_supporting_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = isolated_store(tmp.path());
        store
            .save(
                "coding",
                "rust-review",
                "Review rust",
                &["rust".to_string()],
                "# Rust Review\n\nCheck ownership.\n",
            )
            .expect("save");

        let written = store
            .write_supporting_file(
                "rust-review",
                Some("coding"),
                "references/checklist.md",
                "Review checklist",
            )
            .expect("write file");
        assert!(written.is_file());

        let removed = store
            .remove_supporting_file("rust-review", Some("coding"), "references/checklist.md")
            .expect("remove file");
        assert!(!removed.exists());
        assert!(!written.parent().expect("parent").exists());
    }

    #[test]
    fn blocks_supporting_file_traversal() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = isolated_store(tmp.path());
        store
            .save(
                "coding",
                "rust-review",
                "Review rust",
                &["rust".to_string()],
                "# Rust Review\n\nCheck ownership.\n",
            )
            .expect("save");

        let error = store
            .write_supporting_file("rust-review", Some("coding"), "../secret.env", "SECRET=1")
            .expect_err("expected traversal error");
        assert!(error.to_string().contains("skill directory"));
    }

    #[test]
    fn deletes_skill_directory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = isolated_store(tmp.path());
        store
            .save(
                "coding",
                "rust-review",
                "Review rust",
                &["rust".to_string()],
                "# Rust Review\n\nCheck ownership.\n",
            )
            .expect("save");

        let skill_dir = store.delete("rust-review", Some("coding")).expect("delete");
        assert!(!skill_dir.exists());
        assert!(store.list().expect("list").is_empty());
    }

    #[test]
    fn surfaces_skill_readiness_requirements() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let skill_dir = tmp.path().join("skills").join("ops").join("deploy");
        fs::create_dir_all(&skill_dir).expect("mkdir skill");
        fs::write(
            tmp.path().join("config.yaml"),
            "skills:\n  include_bundled: false\n  config:\n    wiki.path: /tmp/wiki\n",
        )
        .expect("write config");
        fs::write(
            skill_dir.join("SKILL.md"),
            r#"---
name: deploy
description: Deploy service
required_environment_variables:
  - name: FAKE_DEPLOY_KEY
    prompt: Enter deploy key
required_commands: [git, definitely-missing-command]
metadata:
  hermes:
    config:
      - key: wiki.path
        description: Wiki path
        default: ~/wiki
---

# Deploy
"#,
        )
        .expect("write skill");

        let store = isolated_store(tmp.path());
        let view = store
            .view_with_file("deploy", Some("ops"), None)
            .expect("view");
        assert!(view.readiness.setup_needed);
        assert_eq!(
            view.readiness.missing_required_environment_variables,
            vec!["FAKE_DEPLOY_KEY".to_string()]
        );
        assert!(
            view.readiness
                .missing_required_commands
                .contains(&"definitely-missing-command".to_string())
        );
        assert_eq!(view.readiness.config_requirements.len(), 1);
        assert_eq!(
            view.readiness.config_requirements[0]
                .resolved_value
                .as_deref(),
            Some("/tmp/wiki")
        );
    }
}
