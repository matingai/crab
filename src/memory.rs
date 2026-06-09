use anyhow::{Context, Result, bail};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const MEMORY_CHAR_LIMIT: usize = 2200;
const USER_CHAR_LIMIT: usize = 1375;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    #[serde(default = "default_target")]
    pub target: String,
    pub content: String,
    pub created_at_unix: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct MemoryFile {
    entries: Vec<MemoryEntry>,
}

#[derive(Debug, Clone)]
pub struct MemoryStore {
    path: PathBuf,
    lock_path: PathBuf,
    index_path: PathBuf,
}

impl MemoryStore {
    pub fn new(data_dir: impl AsRef<Path>) -> Result<Self> {
        let data_dir = data_dir.as_ref();
        fs::create_dir_all(data_dir)
            .with_context(|| format!("failed to create data dir {}", data_dir.display()))?;
        Ok(Self {
            path: data_dir.join("memory.json"),
            lock_path: data_dir.join("memory.json.lock"),
            index_path: data_dir.join("memory_fts.sqlite"),
        })
    }

    pub fn add(&self, content: impl Into<String>) -> Result<MemoryEntry> {
        self.add_to("memory", content)
    }

    pub fn add_to(&self, target: &str, content: impl Into<String>) -> Result<MemoryEntry> {
        let target = normalize_target(target)?;
        let content = content.into().trim().to_string();
        if content.is_empty() {
            bail!("memory content cannot be empty");
        }

        self.with_file_lock(|store| {
            let mut file = store.load_file_unlocked()?;
            normalize_file(&mut file);
            file.entries
                .retain(|entry| !(entry.target == target && entry.content.trim() == content));

            let entry = MemoryEntry {
                id: Uuid::new_v4().to_string(),
                target: target.to_string(),
                content,
                created_at_unix: unix_now(),
            };
            file.entries.push(entry.clone());
            prune_target(&mut file.entries, target);
            store.save_file_unlocked(&file)?;
            store.rebuild_index_unlocked(&file)?;
            Ok(entry)
        })
    }

    pub fn replace(
        &self,
        target: Option<&str>,
        id: Option<&str>,
        old_text: Option<&str>,
        content: impl Into<String>,
    ) -> Result<MemoryEntry> {
        let target = normalize_optional_target(target)?;
        let content = content.into().trim().to_string();
        if content.is_empty() {
            bail!("replacement memory content cannot be empty");
        }

        self.with_file_lock(|store| {
            let mut file = store.load_file_unlocked()?;
            normalize_file(&mut file);
            let index = find_entry_index(&file.entries, target, id, old_text)?;
            file.entries[index].content = content.clone();
            let replaced = file.entries[index].clone();
            dedupe_entries(&mut file.entries);
            prune_all_targets(&mut file.entries);
            store.save_file_unlocked(&file)?;
            store.rebuild_index_unlocked(&file)?;
            Ok(replaced)
        })
    }

    pub fn remove(
        &self,
        target: Option<&str>,
        id: Option<&str>,
        old_text: Option<&str>,
    ) -> Result<MemoryEntry> {
        let target = normalize_optional_target(target)?;

        self.with_file_lock(|store| {
            let mut file = store.load_file_unlocked()?;
            normalize_file(&mut file);
            let index = find_entry_index(&file.entries, target, id, old_text)?;
            let removed = file.entries.remove(index);
            store.save_file_unlocked(&file)?;
            store.rebuild_index_unlocked(&file)?;
            Ok(removed)
        })
    }

    pub fn list(&self) -> Result<Vec<MemoryEntry>> {
        self.list_target(None)
    }

    pub fn list_target(&self, target: Option<&str>) -> Result<Vec<MemoryEntry>> {
        let target = normalize_optional_target(target)?;
        let mut file = self.load_file_unlocked()?;
        normalize_file(&mut file);
        Ok(file
            .entries
            .into_iter()
            .filter(|entry| target.is_none_or(|target| entry.target == target))
            .collect())
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryEntry>> {
        self.search_target(query, None, limit)
    }

    pub fn search_target(
        &self,
        query: &str,
        target: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let target = normalize_optional_target(target)?;
        if let Ok(items) = self.search_index(query, target, limit) {
            if !items.is_empty() {
                return Ok(items);
            }
        }
        self.search_substring(query, target, limit)
    }

    fn search_substring(
        &self,
        query: &str,
        target: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let query_tokens = tokenize(query);
        let mut file = self.load_file_unlocked()?;
        normalize_file(&mut file);
        let mut scored = file
            .entries
            .into_iter()
            .filter(|entry| target.is_none_or(|target| entry.target == target))
            .map(|entry| {
                let score = score_entry(&entry.content, &query_tokens);
                (score, entry)
            })
            .filter(|(score, _)| *score > 0)
            .collect::<Vec<_>>();

        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| b.1.created_at_unix.cmp(&a.1.created_at_unix))
        });
        Ok(scored
            .into_iter()
            .take(limit)
            .map(|(_, entry)| entry)
            .collect())
    }

    pub fn build_context_block(&self, query: &str, limit: usize) -> Result<String> {
        let items = self.search(query, limit)?;
        if items.is_empty() {
            return Ok(String::new());
        }

        let body = items
            .into_iter()
            .map(|item| format!("- [{}] {}", item.target, item.content))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(format!(
            "<memory-context>\n[System note: The following is recalled memory context, NOT new user input. Treat it as background information.]\n\n{}\n</memory-context>",
            body
        ))
    }

    fn load_file_unlocked(&self) -> Result<MemoryFile> {
        if !self.path.exists() {
            return Ok(MemoryFile::default());
        }
        let raw = fs::read_to_string(&self.path)
            .with_context(|| format!("failed to read {}", self.path.display()))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse {}", self.path.display()))
    }

    fn save_file_unlocked(&self, file: &MemoryFile) -> Result<()> {
        let tmp = self.path.with_extension("json.tmp");
        let raw = serde_json::to_string_pretty(file).context("failed to serialize memory file")?;
        fs::write(&tmp, raw).with_context(|| format!("failed to write {}", tmp.display()))?;
        fs::rename(&tmp, &self.path).with_context(|| {
            format!(
                "failed to move {} to {}",
                tmp.display(),
                self.path.display()
            )
        })
    }

    fn with_file_lock<T>(&self, action: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        let _guard = LockFile::acquire(&self.lock_path)?;
        action(self)
    }

    fn open_index(&self) -> Result<Connection> {
        let conn = Connection::open(&self.index_path)
            .with_context(|| format!("failed to open {}", self.index_path.display()))?;
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(
                id UNINDEXED,
                target UNINDEXED,
                content,
                created_at_unix UNINDEXED,
                tokenize='unicode61'
            );",
        )
        .context("failed to initialize memory FTS index")?;
        Ok(conn)
    }

    fn rebuild_index_unlocked(&self, file: &MemoryFile) -> Result<()> {
        let mut conn = self.open_index()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM memory_fts", [])?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO memory_fts (id, target, content, created_at_unix)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for entry in &file.entries {
                stmt.execute(params![
                    entry.id,
                    entry.target,
                    entry.content,
                    entry.created_at_unix.to_string()
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn ensure_index_current(&self) -> Result<()> {
        let mut file = self.load_file_unlocked()?;
        normalize_file(&mut file);
        self.rebuild_index_unlocked(&file)
    }

    fn search_index(
        &self,
        query: &str,
        target: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let Some(match_query) = fts_query(query) else {
            return Ok(Vec::new());
        };
        if !self.index_path.exists() {
            self.ensure_index_current()?;
        }
        if self.index_is_stale()? {
            self.ensure_index_current()?;
        }

        let conn = self.open_index()?;
        let mut rows = if let Some(target) = target {
            let mut stmt = conn.prepare(
                "SELECT id, target, content, created_at_unix
                 FROM memory_fts
                 WHERE memory_fts MATCH ?1 AND target = ?2
                 ORDER BY bm25(memory_fts), created_at_unix DESC
                 LIMIT ?3",
            )?;
            let mapped = stmt.query_map(params![match_query, target, limit as i64], map_fts_row)?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, target, content, created_at_unix
                 FROM memory_fts
                 WHERE memory_fts MATCH ?1
                 ORDER BY bm25(memory_fts), created_at_unix DESC
                 LIMIT ?2",
            )?;
            let mapped = stmt.query_map(params![match_query, limit as i64], map_fts_row)?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        };

        if rows.is_empty() {
            self.ensure_index_current()?;
            rows = self.search_index_without_rebuild(query, target, limit)?;
        }
        Ok(rows)
    }

    fn index_is_stale(&self) -> Result<bool> {
        if !self.path.exists() || !self.index_path.exists() {
            return Ok(false);
        }
        let json_modified = fs::metadata(&self.path)
            .with_context(|| format!("failed to stat {}", self.path.display()))?
            .modified()
            .with_context(|| format!("failed to read modified time for {}", self.path.display()))?;
        let index_modified = fs::metadata(&self.index_path)
            .with_context(|| format!("failed to stat {}", self.index_path.display()))?
            .modified()
            .with_context(|| {
                format!(
                    "failed to read modified time for {}",
                    self.index_path.display()
                )
            })?;
        Ok(json_modified > index_modified)
    }

    fn search_index_without_rebuild(
        &self,
        query: &str,
        target: Option<&str>,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let Some(match_query) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let conn = self.open_index()?;
        if let Some(target) = target {
            let mut stmt = conn.prepare(
                "SELECT id, target, content, created_at_unix
                 FROM memory_fts
                 WHERE memory_fts MATCH ?1 AND target = ?2
                 ORDER BY bm25(memory_fts), created_at_unix DESC
                 LIMIT ?3",
            )?;
            let mapped = stmt.query_map(params![match_query, target, limit as i64], map_fts_row)?;
            Ok(mapped.collect::<rusqlite::Result<Vec<_>>>()?)
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, target, content, created_at_unix
                 FROM memory_fts
                 WHERE memory_fts MATCH ?1
                 ORDER BY bm25(memory_fts), created_at_unix DESC
                 LIMIT ?2",
            )?;
            let mapped = stmt.query_map(params![match_query, limit as i64], map_fts_row)?;
            Ok(mapped.collect::<rusqlite::Result<Vec<_>>>()?)
        }
    }
}

fn normalize_file(file: &mut MemoryFile) {
    for entry in &mut file.entries {
        if normalize_target(&entry.target).is_err() {
            entry.target = default_target();
        }
    }
    dedupe_entries(&mut file.entries);
    prune_all_targets(&mut file.entries);
}

fn dedupe_entries(entries: &mut Vec<MemoryEntry>) {
    let mut seen = HashSet::new();
    let mut kept_reversed = Vec::new();
    for entry in entries.iter().rev() {
        let key = (
            entry.target.clone(),
            entry.content.trim().to_lowercase().to_string(),
        );
        if seen.insert(key) {
            kept_reversed.push(entry.clone());
        }
    }
    kept_reversed.reverse();
    *entries = kept_reversed;
}

fn prune_all_targets(entries: &mut Vec<MemoryEntry>) {
    prune_target(entries, "memory");
    prune_target(entries, "user");
}

fn prune_target(entries: &mut Vec<MemoryEntry>, target: &str) {
    let limit = char_limit_for_target(target);
    let mut used = 0usize;
    let mut keep = HashSet::new();

    for (index, entry) in entries
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, entry)| entry.target == target)
    {
        let entry_chars = entry.content.chars().count();
        if keep.is_empty() || used + entry_chars <= limit {
            keep.insert(index);
            used += entry_chars;
        }
    }

    entries.retain_with_index(|index, entry| entry.target != target || keep.contains(&index));
}

fn find_entry_index(
    entries: &[MemoryEntry],
    target: Option<&str>,
    id: Option<&str>,
    old_text: Option<&str>,
) -> Result<usize> {
    let id = id.map(str::trim).filter(|value| !value.is_empty());
    let old_text = old_text.map(str::trim).filter(|value| !value.is_empty());
    if id.is_none() && old_text.is_none() {
        bail!("memory replace/remove requires `id` or `old_text`");
    }

    let matches = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| target.is_none_or(|target| entry.target == target))
        .filter(|(_, entry)| {
            id.is_some_and(|id| entry.id == id)
                || old_text.is_some_and(|old_text| entry.content.contains(old_text))
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();

    match matches.as_slice() {
        [] => bail!("no matching memory entry found"),
        [index] => Ok(*index),
        _ => bail!("multiple memory entries matched; use `id` or a more specific `old_text`"),
    }
}

fn normalize_target(target: &str) -> Result<&str> {
    match target.trim() {
        "" | "memory" => Ok("memory"),
        "user" => Ok("user"),
        other => bail!("unsupported memory target `{other}`"),
    }
}

fn normalize_optional_target(target: Option<&str>) -> Result<Option<&str>> {
    target.map(normalize_target).transpose()
}

fn char_limit_for_target(target: &str) -> usize {
    if target == "user" {
        USER_CHAR_LIMIT
    } else {
        MEMORY_CHAR_LIMIT
    }
}

fn default_target() -> String {
    "memory".to_string()
}

fn map_fts_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryEntry> {
    let created_at_raw: String = row.get(3)?;
    Ok(MemoryEntry {
        id: row.get(0)?,
        target: row.get(1)?,
        content: row.get(2)?,
        created_at_unix: created_at_raw.parse::<u64>().unwrap_or_default(),
    })
}

fn fts_query(query: &str) -> Option<String> {
    let terms = query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(str::trim)
        .filter(|part| part.chars().count() >= 2)
        .map(|part| format!("\"{}\"", part.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

fn tokenize(query: &str) -> HashSet<String> {
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .map(|part| part.trim().to_lowercase())
        .filter(|part| part.len() >= 2)
        .collect()
}

fn score_entry(content: &str, query_tokens: &HashSet<String>) -> usize {
    if query_tokens.is_empty() {
        return 0;
    }
    let haystack = content.to_lowercase();
    query_tokens
        .iter()
        .filter(|token| haystack.contains(token.as_str()))
        .count()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

struct LockFile {
    path: PathBuf,
    _file: File,
}

impl LockFile {
    fn acquire(path: &Path) -> Result<Self> {
        for attempt in 0..100 {
            match OpenOptions::new().write(true).create_new(true).open(path) {
                Ok(file) => {
                    return Ok(Self {
                        path: path.to_path_buf(),
                        _file: file,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    thread::sleep(Duration::from_millis(10 + attempt));
                }
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("failed to create lock {}", path.display()));
                }
            }
        }
        bail!("timed out waiting for memory lock {}", path.display())
    }
}

impl Drop for LockFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

trait RetainWithIndex<T> {
    fn retain_with_index(&mut self, keep: impl FnMut(usize, &T) -> bool);
}

impl<T> RetainWithIndex<T> for Vec<T> {
    fn retain_with_index(&mut self, mut keep: impl FnMut(usize, &T) -> bool) {
        let mut index = 0usize;
        self.retain(|item| {
            let should_keep = keep(index, item);
            index += 1;
            should_keep
        });
    }
}

#[cfg(test)]
mod tests {
    use super::MemoryStore;

    #[test]
    fn adds_and_searches_memory() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = MemoryStore::new(tmp.path()).expect("store");
        store
            .add("User prefers concise answers and Rust examples.")
            .expect("add");
        let results = store.search("Rust concise", 5).expect("search");
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("Rust"));
        assert_eq!(results[0].target, "memory");
    }

    #[test]
    fn supports_user_target_replace_and_remove() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = MemoryStore::new(tmp.path()).expect("store");
        let entry = store
            .add_to("user", "User prefers terse progress updates.")
            .expect("add user");

        let replaced = store
            .replace(
                Some("user"),
                Some(&entry.id),
                None,
                "User prefers concise Mandarin progress updates.",
            )
            .expect("replace");
        assert_eq!(replaced.target, "user");
        assert!(replaced.content.contains("Mandarin"));

        let search_results = store
            .search_target("Mandarin", Some("user"), 5)
            .expect("search user");
        assert_eq!(search_results.len(), 1);

        let removed = store
            .remove(Some("user"), None, Some("Mandarin"))
            .expect("remove");
        assert_eq!(removed.id, entry.id);
        assert!(store.list_target(Some("user")).expect("list").is_empty());
    }

    #[test]
    fn deduplicates_exact_target_content() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = MemoryStore::new(tmp.path()).expect("store");
        store.add("Stable project fact.").expect("first");
        store.add("Stable project fact.").expect("second");
        assert_eq!(store.list().expect("list").len(), 1);
    }

    #[test]
    fn creates_and_uses_fts_index() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = MemoryStore::new(tmp.path()).expect("store");
        store
            .add_to("memory", "Project uses rusqlite FTS for durable recall.")
            .expect("add");

        assert!(tmp.path().join("memory_fts.sqlite").is_file());
        let results = store
            .search_target("rusqlite durable", Some("memory"), 5)
            .expect("search");
        assert_eq!(results.len(), 1);
        assert!(results[0].content.contains("FTS"));
    }

    #[test]
    fn rebuilds_stale_fts_index_when_json_changes() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = MemoryStore::new(tmp.path()).expect("store");
        store
            .add("Obsolete memory entry that should disappear.")
            .expect("add");
        assert_eq!(store.search("obsolete", 5).expect("search").len(), 1);

        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(tmp.path().join("memory.json"), "{\"entries\":[]}")
            .expect("manual json edit");

        let results = store.search("obsolete", 5).expect("search stale");
        assert!(results.is_empty());
    }
}
