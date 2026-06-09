use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserElement {
    #[serde(alias = "refId")]
    pub ref_id: String,
    pub kind: String,
    pub label: String,
    pub target: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub bbox: Option<BrowserRect>,
    #[serde(default)]
    pub disabled: Option<bool>,
    #[serde(default)]
    pub checked: Option<bool>,
    #[serde(default)]
    pub selected: Option<bool>,
    #[serde(default)]
    pub required: Option<bool>,
    #[serde(default)]
    #[serde(alias = "fieldName")]
    pub field_name: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    #[serde(alias = "formId")]
    pub form_id: Option<String>,
    #[serde(default)]
    #[serde(alias = "formAction")]
    pub form_action: Option<String>,
    #[serde(default)]
    #[serde(alias = "formMethod")]
    pub form_method: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserImage {
    pub src: String,
    pub alt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserPageState {
    pub url: String,
    pub final_url: String,
    pub content_type: String,
    pub title: Option<String>,
    pub content: String,
    #[serde(default)]
    pub elements: Vec<BrowserElement>,
    #[serde(default)]
    pub images: Vec<BrowserImage>,
    pub truncated_body: bool,
    pub fetched_at_unix: u64,
}

impl BrowserPageState {
    pub fn new(
        url: impl Into<String>,
        final_url: impl Into<String>,
        content_type: impl Into<String>,
        title: Option<String>,
        content: impl Into<String>,
        elements: Vec<BrowserElement>,
        images: Vec<BrowserImage>,
        truncated_body: bool,
    ) -> Self {
        Self {
            url: url.into(),
            final_url: final_url.into(),
            content_type: content_type.into(),
            title,
            content: content.into(),
            elements,
            images,
            truncated_body,
            fetched_at_unix: unix_now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSessionState {
    pub current: BrowserPageState,
    #[serde(default)]
    pub back_stack: Vec<BrowserPageState>,
    #[serde(default)]
    pub forward_stack: Vec<BrowserPageState>,
    #[serde(default)]
    pub focused_ref: Option<String>,
    #[serde(default)]
    pub scroll_offset: usize,
    #[serde(default)]
    pub console_messages: Vec<String>,
}

impl BrowserSessionState {
    pub fn new(current: BrowserPageState) -> Self {
        Self {
            current,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            focused_ref: None,
            scroll_offset: 0,
            console_messages: Vec::new(),
        }
    }

    pub fn current_page(&self) -> &BrowserPageState {
        &self.current
    }

    pub fn current_page_mut(&mut self) -> &mut BrowserPageState {
        &mut self.current
    }

    pub fn push_navigation(&mut self, next: BrowserPageState) {
        self.back_stack.push(self.current.clone());
        self.current = next;
        self.forward_stack.clear();
        self.focused_ref = None;
        self.scroll_offset = 0;
    }

    pub fn go_back(&mut self) -> bool {
        let Some(previous) = self.back_stack.pop() else {
            return false;
        };
        self.forward_stack.push(self.current.clone());
        self.current = previous;
        self.focused_ref = None;
        self.scroll_offset = 0;
        true
    }

    pub fn go_forward(&mut self) -> bool {
        let Some(next) = self.forward_stack.pop() else {
            return false;
        };
        self.back_stack.push(self.current.clone());
        self.current = next;
        self.focused_ref = None;
        self.scroll_offset = 0;
        true
    }

    pub fn focusable_refs(&self) -> Vec<String> {
        self.current
            .elements
            .iter()
            .map(|element| element.ref_id.clone())
            .collect()
    }

    pub fn focus_next(&mut self, reverse: bool) -> Option<String> {
        let refs = self.focusable_refs();
        if refs.is_empty() {
            self.focused_ref = None;
            return None;
        }

        let next_index = match self
            .focused_ref
            .as_ref()
            .and_then(|focused| refs.iter().position(|item| item == focused))
        {
            Some(index) if reverse => index.checked_sub(1).unwrap_or(refs.len() - 1),
            Some(index) => (index + 1) % refs.len(),
            None if reverse => refs.len() - 1,
            None => 0,
        };
        self.focused_ref = Some(refs[next_index].clone());
        self.focused_ref.clone()
    }

    pub fn set_focus(&mut self, reference: Option<String>) {
        self.focused_ref = reference;
    }

    pub fn scroll_by(&mut self, delta: isize, max_content_chars: usize) {
        let upper = self
            .current
            .content
            .chars()
            .count()
            .saturating_sub(max_content_chars);
        if delta.is_negative() {
            self.scroll_offset = self.scroll_offset.saturating_sub(delta.unsigned_abs());
        } else {
            self.scroll_offset =
                usize::min(self.scroll_offset.saturating_add(delta as usize), upper);
        }
    }

    pub fn clear_console(&mut self) {
        self.console_messages.clear();
    }

    pub fn log_console(&mut self, message: impl Into<String>) {
        self.console_messages.push(message.into());
    }

    pub fn form_fields(&self, form_id: &str) -> BTreeMap<String, String> {
        self.current
            .elements
            .iter()
            .filter(|element| element.form_id.as_deref() == Some(form_id))
            .filter_map(|element| {
                Some((
                    element.field_name.as_ref()?.trim().to_string(),
                    element.value.clone().unwrap_or_default(),
                ))
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct BrowserStateStore {
    root: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum PersistedBrowserState {
    Session(BrowserSessionState),
    Legacy(BrowserPageState),
}

impl BrowserStateStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(root.join("browser"))
            .with_context(|| format!("failed to create browser dir under {}", root.display()))?;
        Ok(Self { root })
    }

    pub fn load(&self, session_id: &str) -> Result<Option<BrowserSessionState>> {
        let path = self.page_path(session_id);
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("failed to read browser state {}", path.display()))?;
        let page = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse browser state {}", path.display()))?;
        Ok(Some(match page {
            PersistedBrowserState::Session(session) => session,
            PersistedBrowserState::Legacy(page) => BrowserSessionState::new(page),
        }))
    }

    pub fn save(&self, session_id: &str, page: &BrowserSessionState) -> Result<PathBuf> {
        let path = self.page_path(session_id);
        let tmp = path.with_extension("json.tmp");
        let raw =
            serde_json::to_string_pretty(page).context("failed to serialize browser state")?;
        fs::write(&tmp, raw).with_context(|| format!("failed to write {}", tmp.display()))?;
        fs::rename(&tmp, &path)
            .with_context(|| format!("failed to move {} to {}", tmp.display(), path.display()))?;
        Ok(path)
    }

    pub fn clear(&self, session_id: &str) -> Result<()> {
        let path = self.page_path(session_id);
        if path.exists() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
        Ok(())
    }

    fn page_path(&self, session_id: &str) -> PathBuf {
        self.root.join("browser").join(format!("{session_id}.json"))
    }

    pub fn root(&self) -> &Path {
        &self.root
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
        BrowserElement, BrowserImage, BrowserPageState, BrowserSessionState, BrowserStateStore,
    };
    use serde_json::json;

    #[test]
    fn saves_and_loads_browser_state() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = BrowserStateStore::new(tmp.path()).expect("store");
        let page = BrowserSessionState::new(BrowserPageState::new(
            "https://example.com",
            "https://example.com",
            "text/html",
            Some("Example".to_string()),
            "Hello world",
            vec![BrowserElement {
                ref_id: "@e1".to_string(),
                kind: "link".to_string(),
                label: "Docs".to_string(),
                target: Some("https://example.com/docs".to_string()),
                role: Some("link".to_string()),
                selector: Some("a".to_string()),
                bbox: None,
                disabled: None,
                checked: None,
                selected: None,
                required: None,
                field_name: None,
                value: None,
                form_id: None,
                form_action: None,
                form_method: None,
            }],
            vec![BrowserImage {
                src: "https://example.com/logo.png".to_string(),
                alt: "Logo".to_string(),
            }],
            false,
        ));
        store.save("demo", &page).expect("save");

        let loaded = store.load("demo").expect("load").expect("page");
        assert_eq!(loaded.current.url, "https://example.com");
        assert_eq!(loaded.current.title.as_deref(), Some("Example"));
        assert_eq!(loaded.current.elements.len(), 1);
        assert_eq!(loaded.current.images.len(), 1);
    }

    #[test]
    fn browser_element_accepts_electron_camel_case_fields() {
        let element: BrowserElement = serde_json::from_value(json!({
            "refId": "@e1",
            "kind": "input:text",
            "label": "Search",
            "target": null,
            "fieldName": "q",
            "value": "golang",
            "formId": "search-form",
            "formAction": "https://example.com/search",
            "formMethod": "get"
        }))
        .expect("deserialize browser element");

        assert_eq!(element.ref_id, "@e1");
        assert_eq!(element.field_name.as_deref(), Some("q"));
        assert_eq!(element.form_id.as_deref(), Some("search-form"));
        assert_eq!(
            element.form_action.as_deref(),
            Some("https://example.com/search")
        );
        assert_eq!(element.form_method.as_deref(), Some("get"));
    }
}
