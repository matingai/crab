use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub stream: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: Some(Value::String(text.into())),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: Some(Value::String(text.into())),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: "assistant".to_string(),
            content: Some(Value::String(text.into())),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    pub fn tool(tool_call_id: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            role: "tool".to_string(),
            content: Some(Value::String(text.into())),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }

    pub fn content_text(&self) -> String {
        match &self.content {
            Some(Value::String(text)) => text.clone(),
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(""),
            Some(other) => other.to_string(),
            None => String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionDefinition,
}

impl ToolDefinition {
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: Value,
    ) -> Self {
        Self {
            kind: "function".to_string(),
            function: FunctionDefinition {
                name: name.into(),
                description: description.into(),
                parameters,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: ToolFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolFunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub choices: Vec<ChatChoice>,
}

impl ChatResponse {
    pub fn first_message(self) -> Result<ChatMessage> {
        if let Some(choice) = self.choices.into_iter().next() {
            return Ok(choice.message);
        }
        bail!("model response did not contain any choices")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatChoice {
    pub message: ChatMessage,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatStreamChunk {
    pub choices: Vec<ChatStreamChoice>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatStreamChoice {
    pub delta: ChatStreamDelta,
    pub index: usize,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ChatStreamDelta {
    pub role: Option<String>,
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ChatStreamToolCallDelta>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatStreamToolCallDelta {
    pub index: usize,
    pub id: Option<String>,
    #[serde(rename = "type")]
    pub kind: Option<String>,
    pub function: Option<ChatStreamToolFunctionDelta>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ChatStreamToolFunctionDelta {
    pub name: Option<String>,
    pub arguments: Option<String>,
}

pub fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn is_false(value: &bool) -> bool {
    !*value
}
