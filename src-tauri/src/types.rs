use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    pub key: String,
    pub id: String,
    pub name: String,
    pub native_name: Option<String>,
    pub cwd: String,
    pub project: String,
    pub path: String,
    pub created_at: String,
    pub updated_at: String,
    pub preview: String,
    pub model: String,
    pub provider: String,
    pub message_count: usize,
    pub tokens: u64,
    pub cost: f64,
    pub file_size: u64,
    pub branch_count: usize,
    pub starred: bool,
    pub archived: bool,
    pub malformed_lines: usize,
    pub version: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truncated: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    pub id: String,
    pub parent_id: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub role: String,
    pub timestamp: String,
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionIndex {
    pub sessions: Vec<SessionSummary>,
    pub warnings: Vec<String>,
    pub scanned_at: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    pub session: SessionSummary,
    pub entries: Vec<SessionEntry>,
    pub total: usize,
    pub has_more: bool,
    pub branch: String,
    pub warning: Option<String>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub session_root: String,
    pub data_dir: String,
    pub platform: String,
    pub terminal: String,
    pub terminal_preference: crate::terminal::TerminalPreference,
    pub terminal_options: Vec<crate::terminal::TerminalOption>,
    pub demo: bool,
    pub default_cwd: String,
    pub poll_interval: u64,
    pub version: String,
}
#[derive(Debug, Deserialize)]
pub struct BatchRequest {
    pub keys: Vec<String>,
    pub action: String,
    pub name: Option<String>,
}
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub name: Option<String>,
    pub starred: bool,
    pub archived: bool,
}
