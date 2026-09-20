use chrono::{DateTime, NaiveDateTime, SecondsFormat, TimeZone, Utc};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const MAX_TEXT_CHARS: usize = 32_000;
const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
const MAX_RECORDS: usize = 200_000;
const PAGE_BUDGET_BYTES: usize = 16 * 1024 * 1024;
const MAX_BLOCKS: usize = 64;
const CHUNK_BYTES: usize = 64 * 1024;

/// The parsed form returned to the native store.
#[derive(Debug)]
pub struct ParsedSession {
    pub summary: crate::types::SessionSummary,
    pub entries: Vec<crate::types::SessionEntry>,
    pub total: usize,
    pub budget_limited: bool,
}

#[derive(Debug, Clone)]
struct HeaderInfo {
    id: String,
    cwd: String,
    created_at: String,
    version: u64,
}

#[derive(Debug, Clone)]
struct Node {
    id: String,
    parent_id: Option<String>,
    line: u64,
}

#[derive(Default)]
struct SummaryState {
    entry_count: usize,
    previous_id: Option<String>,
    last_id: Option<String>,
    native_name: Option<String>,
    first_user: String,
    preview: String,
    model: String,
    provider: String,
    message_count: usize,
    tokens: u64,
    cost: f64,
    children: HashMap<String, usize>,
    nodes: HashMap<String, Node>,
}

struct EntryBuffer {
    entries: VecDeque<crate::types::SessionEntry>,
    sizes: VecDeque<usize>,
    bytes: usize,
    limit: usize,
    budget_limited: bool,
}

impl EntryBuffer {
    fn new(limit: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            sizes: VecDeque::new(),
            bytes: 0,
            limit,
            budget_limited: false,
        }
    }

    fn push(&mut self, entry: crate::types::SessionEntry) {
        let size = estimate_entry_bytes(&entry);
        self.entries.push_back(entry);
        self.sizes.push_back(size);
        self.bytes = self.bytes.saturating_add(size);

        while self.entries.len() > self.limit || self.bytes > PAGE_BUDGET_BYTES {
            if self.bytes > PAGE_BUDGET_BYTES {
                self.budget_limited = true;
            }
            if let Some(size) = self.sizes.pop_front() {
                self.bytes = self.bytes.saturating_sub(size);
                self.entries.pop_front();
            } else {
                break;
            }
        }
    }

    fn finish(self) -> Vec<crate::types::SessionEntry> {
        self.entries.into_iter().collect()
    }
}

/// Parse one Pi JSONL session from a bounded snapshot of the open file.
///
/// `branch` must be `"all"` or `"active"`. Active means the persisted tree
/// ancestry of the last valid entry; it deliberately does not apply Pi's
/// compaction/context-building rules.
pub fn parse_session(
    file: &Path,
    include_entries: bool,
    limit: usize,
    branch: &str,
) -> Result<ParsedSession, String> {
    if !(1..=5_000).contains(&limit) {
        return Err("记录数量须为 1 到 5000".to_string());
    }
    if branch != "all" && branch != "active" {
        return Err("branch 必须是 all 或 active".to_string());
    }
    let active_branch = branch == "active";

    // Check the directory entry before opening. This rejects ordinary symlinks
    // without making the parser platform-specific; the open handle remains the
    // source of truth for the snapshot metadata below.
    let link_metadata =
        fs::symlink_metadata(file).map_err(|error| format!("无法读取会话文件: {error}"))?;
    if link_metadata.file_type().is_symlink() {
        return Err("不支持符号链接会话文件".to_string());
    }
    if !link_metadata.is_file() {
        return Err("不是普通会话文件".to_string());
    }

    let mut handle = OpenOptions::new()
        .read(true)
        .open(file)
        .map_err(|error| format!("无法打开会话文件: {error}"))?;
    let file_metadata = handle
        .metadata()
        .map_err(|error| format!("无法读取会话文件元数据: {error}"))?;
    if !file_metadata.is_file() {
        return Err("不是普通会话文件".to_string());
    }
    let snapshot_size = file_metadata.len();

    let absolute_path = absolute_path(file);
    let updated_at = system_time_string(file_metadata.modified());
    let mut header: Option<HeaderInfo> = None;
    let mut state = SummaryState::default();
    let mut retained = EntryBuffer::new(limit);

    let malformed_lines = scan_lines(&mut handle, snapshot_size, |line, record| {
        let record_type = record_type(record);
        if header.is_none() {
            if record_type != "session"
                || !record.get("id").is_some_and(Value::is_string)
                || !record.get("cwd").is_some_and(Value::is_string)
            {
                return Err("没有有效的 pi 会话头".to_string());
            }
            let created_at = timestamp(record.get("timestamp"), "");
            let version = version_value(record.get("version"));
            header = Some(HeaderInfo {
                id: string_value(record.get("id")),
                cwd: string_value(record.get("cwd")),
                created_at,
                version,
            });
            return Ok(());
        }

        // A later session line is not a tree entry. This mirrors Pi's header
        // semantics and keeps malformed/record counts stable for odd files.
        if record_type == "session" {
            return Ok(());
        }
        if state.entry_count >= MAX_RECORDS {
            return Err("会话超过 200,000 条记录，请直接在 pi 中查看".to_string());
        }
        state.entry_count += 1;

        let id = entry_id(record, state.entry_count);
        let parent_id = parent_id(record, state.previous_id.as_ref());
        state.previous_id = Some(id.clone());
        state.last_id = Some(id.clone());

        if let Some(parent) = parent_id.as_ref().filter(|value| !value.is_empty()) {
            *state.children.entry(parent.clone()).or_default() += 1;
        }
        if active_branch && include_entries {
            state.nodes.insert(
                id.clone(),
                Node {
                    id: id.clone(),
                    parent_id: parent_id.clone(),
                    line,
                },
            );
        }

        let usage = usage_of(record);
        state.tokens = state.tokens.saturating_add(usage.tokens);
        state.cost = (state.cost + usage.cost).min(f64::MAX);

        if record_type == "session_info" {
            if let Some(name) = record.get("name").and_then(Value::as_str) {
                let name = clip_chars(name, 160).0.trim().to_string();
                state.native_name = if name.is_empty() { None } else { Some(name) };
            }
        }
        if record_type == "model_change" {
            state.model = string_value(record.get("modelId"));
            state.provider = string_value(record.get("provider"));
        }

        let message = record.get("message").and_then(Value::as_object);
        if record_type == "message" {
            state.message_count += 1;
            if let Some(message) = message {
                if let Some(model) = message.get("model").filter(|value| js_truthy(value)) {
                    state.model = string_value(Some(model));
                }
                if let Some(provider) = message.get("provider").filter(|value| js_truthy(value)) {
                    state.provider = string_value(Some(provider));
                }
                let role = string_value(message.get("role"));
                if role == "user" || role == "assistant" {
                    let plain = preview_text(message.get("content"));
                    if role == "user" && state.first_user.is_empty() {
                        state.first_user = clip_chars(&plain, 160).0;
                    }
                    if !plain.is_empty() {
                        state.preview = clip_chars(&plain, 240).0;
                    }
                }
            }
        }

        if include_entries && !active_branch {
            let fallback = header
                .as_ref()
                .map(|value| value.created_at.as_str())
                .unwrap_or("");
            retained.push(normalize_record(record, &id, parent_id, fallback));
        }
        Ok(())
    })?;

    let header = header.ok_or_else(|| "没有有效的 pi 会话头".to_string())?;

    let (entries, total, budget_limited) = if include_entries && active_branch {
        let (selected, total) = select_active_nodes(&state, limit);
        let mut active_entries = EntryBuffer::new(limit);
        let fallback = header.created_at.as_str();
        // Re-read the same open file, from offset zero, but never beyond the
        // original metadata length. Appends after the snapshot cannot leak into
        // either pass or make line numbers disagree.
        let _ = scan_lines(&mut handle, snapshot_size, |line, record| {
            if let Some(node) = selected.get(&line) {
                active_entries.push(normalize_record(
                    record,
                    &node.id,
                    node.parent_id.clone(),
                    fallback,
                ));
            }
            Ok(())
        })?;
        let budget_limited = active_entries.budget_limited;
        (active_entries.finish(), total, budget_limited)
    } else if include_entries {
        let budget_limited = retained.budget_limited;
        (retained.finish(), state.entry_count, budget_limited)
    } else {
        (Vec::new(), state.entry_count, false)
    };

    let name = state.native_name.clone().unwrap_or_else(|| {
        if state.first_user.is_empty() {
            "未命名会话".to_string()
        } else {
            clip_chars(&state.first_user, 80).0
        }
    });
    let preview = if !state.preview.is_empty() {
        state.preview.clone()
    } else if !state.first_user.is_empty() {
        state.first_user.clone()
    } else {
        "还没有对话消息".to_string()
    };
    let summary = crate::types::SessionSummary {
        // The store owns SHA/key generation and persisted metadata.
        key: String::new(),
        id: header.id,
        name,
        native_name: state.native_name,
        cwd: header.cwd.clone(),
        project: project_name(&header.cwd),
        path: absolute_path,
        created_at: header.created_at,
        updated_at,
        preview,
        model: if state.model.is_empty() {
            "未知模型".to_string()
        } else {
            state.model
        },
        provider: state.provider,
        message_count: state.message_count,
        tokens: state.tokens,
        cost: state.cost,
        file_size: snapshot_size,
        branch_count: state.children.values().filter(|count| **count > 1).count(),
        starred: false,
        archived: false,
        malformed_lines,
        version: header.version,
    };

    Ok(ParsedSession {
        summary,
        entries,
        total,
        budget_limited,
    })
}

fn absolute_path(file: &Path) -> String {
    fs::canonicalize(file)
        .or_else(|_| {
            if file.is_absolute() {
                Ok(file.to_path_buf())
            } else {
                std::env::current_dir().map(|cwd| cwd.join(file))
            }
        })
        .unwrap_or_else(|_| file.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn system_time_string(value: io::Result<std::time::SystemTime>) -> String {
    value
        .ok()
        .map(|time| DateTime::<Utc>::from(time).to_rfc3339_opts(SecondsFormat::Millis, true))
        .unwrap_or_default()
}

fn record_type(record: &Map<String, Value>) -> &str {
    record.get("type").and_then(Value::as_str).unwrap_or("")
}

fn string_value(value: Option<&Value>) -> String {
    value.and_then(Value::as_str).unwrap_or("").to_string()
}

fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|number| number != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

fn version_value(value: Option<&Value>) -> u64 {
    let number = nonnegative_number(value);
    if number > 0.0 && number.is_finite() {
        number as u64
    } else {
        1
    }
}

fn nonnegative_number(value: Option<&Value>) -> f64 {
    value
        .and_then(Value::as_f64)
        .filter(|number| number.is_finite() && *number >= 0.0)
        .unwrap_or(0.0)
}

fn token_number(value: Option<&Value>) -> u64 {
    let number = nonnegative_number(value);
    if number >= u64::MAX as f64 {
        u64::MAX
    } else {
        number as u64
    }
}

struct Usage {
    tokens: u64,
    cost: f64,
}

fn usage_of(record: &Map<String, Value>) -> Usage {
    let message_usage = record
        .get("message")
        .and_then(Value::as_object)
        .and_then(|message| message.get("usage"))
        .and_then(Value::as_object);
    let usage = message_usage.or_else(|| record.get("usage").and_then(Value::as_object));
    let Some(usage) = usage else {
        return Usage {
            tokens: 0,
            cost: 0.0,
        };
    };

    let total_tokens = token_number(usage.get("totalTokens"));
    let tokens = if total_tokens > 0 {
        total_tokens
    } else {
        token_number(usage.get("input"))
            .saturating_add(token_number(usage.get("output")))
            .saturating_add(token_number(usage.get("cacheRead")))
            .saturating_add(token_number(usage.get("cacheWrite")))
    };
    let cost = usage
        .get("cost")
        .and_then(Value::as_object)
        .map(|cost| nonnegative_number(cost.get("total")))
        .unwrap_or(0.0);
    Usage { tokens, cost }
}

fn entry_id(record: &Map<String, Value>, entry_count: usize) -> String {
    let id = string_value(record.get("id"));
    if id.is_empty() {
        format!("legacy-{entry_count}")
    } else {
        id
    }
}

fn parent_id(record: &Map<String, Value>, previous_id: Option<&String>) -> Option<String> {
    if let Some(value) = record.get("parentId") {
        value.as_str().map(ToString::to_string)
    } else {
        previous_id.cloned()
    }
}

fn timestamp(value: Option<&Value>, fallback: &str) -> String {
    let Some(value) = value else {
        return fallback.to_string();
    };
    let parsed = match value {
        Value::String(value) => parse_timestamp_string(value),
        Value::Number(value) => value
            .as_f64()
            .and_then(|millis| timestamp_from_millis(millis)),
        _ => None,
    };
    parsed.unwrap_or_else(|| fallback.to_string())
}

fn parse_timestamp_string(value: &str) -> Option<String> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Some(
            parsed
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true),
        );
    }
    if let Ok(parsed) = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f") {
        return Some(
            Utc.from_utc_datetime(&parsed)
                .to_rfc3339_opts(SecondsFormat::Millis, true),
        );
    }
    if let Ok(millis) = value.parse::<f64>() {
        return timestamp_from_millis(millis);
    }
    None
}

fn timestamp_from_millis(millis: f64) -> Option<String> {
    if !millis.is_finite() || millis < i64::MIN as f64 || millis > i64::MAX as f64 {
        return None;
    }
    Utc.timestamp_millis_opt(millis as i64)
        .single()
        .map(|value| value.to_rfc3339_opts(SecondsFormat::Millis, true))
}

fn project_name(cwd: &str) -> String {
    let normalized = cwd.replace('\\', "/");
    let trimmed = normalized.trim_end_matches('/');
    trimmed
        .rsplit('/')
        .find(|part| !part.is_empty())
        .unwrap_or(if cwd.is_empty() { "未知项目" } else { cwd })
        .to_string()
}

fn clip_chars(value: &str, max: usize) -> (String, bool) {
    let mut chars = value.chars();
    let clipped: String = chars.by_ref().take(max).collect();
    let truncated = chars.next().is_some();
    (clipped, truncated)
}

fn clipped_block(value: &str) -> crate::types::ContentBlock {
    let (text, truncated) = clip_chars(value, MAX_TEXT_CHARS);
    crate::types::ContentBlock {
        kind: "text".to_string(),
        text: Some(text),
        name: None,
        arguments: None,
        mime_type: None,
        truncated: Some(truncated),
    }
}

fn simple_text_block(value: String) -> crate::types::ContentBlock {
    let (text, truncated) = clip_chars(&value, MAX_TEXT_CHARS);
    crate::types::ContentBlock {
        kind: "text".to_string(),
        text: Some(text),
        name: None,
        arguments: None,
        mime_type: None,
        truncated: Some(truncated),
    }
}

fn blocks(value: Option<&Value>) -> Vec<crate::types::ContentBlock> {
    let Some(value) = value else {
        return Vec::new();
    };
    if let Value::String(value) = value {
        return vec![clipped_block(value)];
    }
    let Value::Array(values) = value else {
        return Vec::new();
    };

    let mut result = Vec::new();
    for value in values.iter().take(MAX_BLOCKS) {
        let Some(block) = value.as_object() else {
            continue;
        };
        let kind = string_value(block.get("type"));
        match kind.as_str() {
            "thinking" => {
                let text = string_value(block.get("thinking"));
                let (text, truncated) = clip_chars(&text, MAX_TEXT_CHARS);
                result.push(crate::types::ContentBlock {
                    kind: "thinking".to_string(),
                    text: Some(text),
                    name: None,
                    arguments: None,
                    mime_type: None,
                    truncated: Some(truncated),
                });
            }
            "toolCall" | "tool_call" => {
                let arguments = printable(block.get("arguments"));
                let (arguments, truncated) = clip_chars(&arguments, MAX_TEXT_CHARS);
                result.push(crate::types::ContentBlock {
                    kind: "toolCall".to_string(),
                    text: None,
                    name: Some(string_value(block.get("name"))),
                    arguments: Some(arguments),
                    mime_type: None,
                    truncated: Some(truncated),
                });
            }
            "image" => {
                result.push(crate::types::ContentBlock {
                    kind: "image".to_string(),
                    text: None,
                    name: None,
                    arguments: None,
                    mime_type: Some(string_value(block.get("mimeType"))),
                    truncated: None,
                });
            }
            _ => result.push(clipped_block(&string_value(block.get("text")))),
        }
    }
    if values.len() > MAX_BLOCKS {
        result.push(crate::types::ContentBlock {
            kind: "text".to_string(),
            text: Some("额外内容块已省略，请导出原始记录查看。".to_string()),
            name: None,
            arguments: None,
            mime_type: None,
            truncated: Some(true),
        });
    }
    result
}

fn bounded_prune(value: &Value, budget: &mut isize, depth: usize) -> Value {
    if *budget <= 0 || depth > 8 {
        return Value::String("…".to_string());
    }
    match value {
        Value::String(value) => {
            let available = (*budget).max(0) as usize;
            let (mut result, truncated) = clip_chars(value, available);
            *budget -= result.chars().count() as isize;
            if truncated {
                result.push('…');
            }
            Value::String(result)
        }
        Value::Array(values) => {
            *budget -= 12;
            let mut result = values
                .iter()
                .take(MAX_BLOCKS)
                .map(|value| bounded_prune(value, budget, depth + 1))
                .collect::<Vec<_>>();
            if values.len() > MAX_BLOCKS {
                result.push(Value::String("…".to_string()));
            }
            Value::Array(result)
        }
        Value::Object(values) => {
            *budget -= 12;
            let mut result = Map::new();
            for (key, value) in values.iter().take(MAX_BLOCKS) {
                let (key, _) = clip_chars(key, 160);
                result.insert(key, bounded_prune(value, budget, depth + 1));
            }
            if values.len() > MAX_BLOCKS {
                result.insert("…".to_string(), Value::String("…".to_string()));
            }
            Value::Object(result)
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
    }
}

fn printable(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    if let Value::String(value) = value {
        let (mut result, truncated) = clip_chars(value, MAX_TEXT_CHARS);
        if truncated {
            result.push('…');
        }
        return result;
    }

    let mut budget = MAX_TEXT_CHARS as isize;
    let pruned = bounded_prune(value, &mut budget, 0);
    let serialized =
        serde_json::to_string_pretty(&pruned).unwrap_or_else(|_| "[无法展开的数据]".to_string());
    clip_chars(&serialized, MAX_TEXT_CHARS).0
}

fn preview_text(value: Option<&Value>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let raw = match value {
        Value::String(value) => clip_chars(value, 2_000).0,
        Value::Array(values) => values
            .iter()
            .take(MAX_BLOCKS)
            .filter_map(|value| value.as_object())
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
            .map(|block| clip_chars(&string_value(block.get("text")), 2_000).0)
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    };
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_record(
    record: &Map<String, Value>,
    id: &str,
    parent_id: Option<String>,
    fallback_time: &str,
) -> crate::types::SessionEntry {
    let record_kind = record_type(record);
    let message = record.get("message").and_then(Value::as_object);
    let mut role = message
        .and_then(|message| message.get("role"))
        .map(|value| string_value(Some(value)))
        .unwrap_or_default();
    if role.is_empty() {
        role = "system".to_string();
    }

    let content = match record_kind {
        "message" => {
            let mut content = blocks(message.and_then(|message| message.get("content")));
            if role == "bashExecution" {
                let command = string_value(message.and_then(|message| message.get("command")));
                let output = string_value(message.and_then(|message| message.get("output")));
                let (command, _) = clip_chars(&command, MAX_TEXT_CHARS);
                content = vec![
                    crate::types::ContentBlock {
                        kind: "toolCall".to_string(),
                        text: None,
                        name: Some("bash".to_string()),
                        arguments: Some(command),
                        mime_type: None,
                        truncated: None,
                    },
                    clipped_block(&output),
                ];
            }
            if let Some(message) = message {
                if message.get("summary").is_some_and(js_truthy) {
                    content.push(clipped_block(&string_value(message.get("summary"))));
                }
                if message.get("errorMessage").is_some_and(js_truthy) {
                    content.push(clipped_block(&string_value(message.get("errorMessage"))));
                }
            }
            content
        }
        "custom_message" => {
            role = "custom".to_string();
            blocks(record.get("content"))
        }
        "compaction" | "branch_summary" => {
            let mut content = vec![clipped_block(&string_value(record.get("summary")))];
            if record
                .get("retainedTail")
                .is_some_and(|value| value.is_array())
            {
                let retained = format!(
                    "保留的上下文快照：\n{}",
                    printable(record.get("retainedTail"))
                );
                content.push(clipped_block(&retained));
            }
            content
        }
        "model_change" => vec![simple_text_block(format!(
            "模型切换为 {}/{}",
            string_value(record.get("provider")),
            string_value(record.get("modelId"))
        ))],
        "thinking_level_change" => vec![simple_text_block(format!(
            "思考级别：{}",
            string_value(record.get("thinkingLevel"))
        ))],
        "session_info" => vec![simple_text_block(format!("会话名称：{}", {
            let name = string_value(record.get("name"));
            if name.is_empty() {
                "未命名".to_string()
            } else {
                name
            }
        }))],
        "label" => vec![simple_text_block(format!(
            "书签：{} → {}",
            {
                let label = string_value(record.get("label"));
                if label.is_empty() {
                    "已移除".to_string()
                } else {
                    label
                }
            },
            string_value(record.get("targetId"))
        ))],
        "custom" => vec![clipped_block(&printable(record.get("data")))],
        _ => vec![clipped_block(&printable(Some(&Value::Object(
            record.clone(),
        ))))],
    };

    let message_timestamp = message.and_then(|message| message.get("timestamp"));
    let timestamp_value = record
        .get("timestamp")
        .filter(|value| !value.is_null())
        .or(message_timestamp);
    let usage = usage_of(record);
    let is_error = message.is_some_and(|message| {
        message.get("isError").and_then(Value::as_bool) == Some(true)
            || message.get("stopReason").and_then(Value::as_str) == Some("error")
            || message.get("errorMessage").is_some_and(js_truthy)
    });

    crate::types::SessionEntry {
        id: id.to_string(),
        parent_id,
        kind: record_kind.to_string(),
        role,
        timestamp: timestamp(timestamp_value, fallback_time),
        content,
        model: message
            .and_then(|message| message.get("model"))
            .map(|value| string_value(Some(value)))
            .filter(|value| !value.is_empty()),
        tool_name: message
            .and_then(|message| message.get("toolName"))
            .map(|value| string_value(Some(value)))
            .filter(|value| !value.is_empty()),
        is_error: Some(is_error),
        tokens: (usage.tokens > 0).then_some(usage.tokens),
    }
}

fn estimate_entry_bytes(entry: &crate::types::SessionEntry) -> usize {
    let chars = entry
        .content
        .iter()
        .map(|block| {
            block.text.as_ref().map_or(0, |value| value.chars().count())
                + block
                    .arguments
                    .as_ref()
                    .map_or(0, |value| value.chars().count())
        })
        .sum::<usize>();
    chars.saturating_mul(2).saturating_add(512)
}

fn select_active_nodes(state: &SummaryState, limit: usize) -> (HashMap<u64, Node>, usize) {
    let mut selected = HashMap::new();
    let mut seen = HashSet::new();
    let mut current = state.last_id.as_ref().and_then(|id| state.nodes.get(id));
    let mut total = 0;
    while let Some(node) = current {
        if !seen.insert(node.id.clone()) {
            break;
        }
        total += 1;
        if total <= limit {
            selected.insert(node.line, node.clone());
        }
        current = node
            .parent_id
            .as_ref()
            .and_then(|parent| state.nodes.get(parent));
    }
    (selected, total)
}

fn is_line_blank(line: &[u8]) -> bool {
    line.iter().all(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
}

fn parse_line<F>(
    line: &[u8],
    line_number: u64,
    malformed: &mut usize,
    consume: &mut F,
) -> Result<(), String>
where
    F: FnMut(u64, &Map<String, Value>) -> Result<(), String>,
{
    if line.is_empty() || is_line_blank(line) {
        return Ok(());
    }
    let line = line.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(line);
    if line.is_empty() || is_line_blank(line) {
        return Ok(());
    }
    let value = match serde_json::from_slice::<Value>(line) {
        Ok(value) => value,
        Err(_) => {
            *malformed = malformed.saturating_add(1);
            return Ok(());
        }
    };
    let Value::Object(record) = value else {
        *malformed = malformed.saturating_add(1);
        return Ok(());
    };
    consume(line_number, &record)
}

/// Scan LF-delimited records without `read_line`/`read_until` allocation. A
/// line is retained only until 16 MiB; an oversized line is skipped through
/// its next LF and counted once as malformed.
fn scan_lines<F>(file: &mut File, size: u64, mut consume: F) -> Result<usize, String>
where
    F: FnMut(u64, &Map<String, Value>) -> Result<(), String>,
{
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("无法定位会话文件: {error}"))?;
    let mut remaining = size;
    let mut chunk = vec![0_u8; CHUNK_BYTES];
    let mut line = Vec::with_capacity(CHUNK_BYTES);
    let mut line_number = 0_u64;
    let mut malformed = 0_usize;
    let mut skipping = false;

    while remaining > 0 {
        let requested = remaining.min(chunk.len() as u64) as usize;
        let read = file
            .read(&mut chunk[..requested])
            .map_err(|error| format!("读取会话文件失败: {error}"))?;
        if read == 0 {
            break;
        }
        remaining -= read as u64;

        for byte in &chunk[..read] {
            if *byte == b'\n' {
                line_number += 1;
                if !skipping {
                    parse_line(&line, line_number, &mut malformed, &mut consume)?;
                }
                line.clear();
                skipping = false;
                continue;
            }
            if skipping {
                continue;
            }
            line.push(*byte);
            if line.len() > MAX_LINE_BYTES {
                malformed = malformed.saturating_add(1);
                line.clear();
                skipping = true;
            }
        }
    }

    // A JSONL file may end without LF. Keep a valid partial record, or count
    // the invalid partial tail as malformed instead of discarding it silently.
    if !skipping && !line.is_empty() {
        line_number += 1;
        parse_line(&line, line_number, &mut malformed, &mut consume)?;
    }
    Ok(malformed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fixture(rows: Vec<Value>, tail: &str) -> tempfile::NamedTempFile {
        use std::io::Write;
        let mut file = tempfile::NamedTempFile::new().unwrap();
        writeln!(file, "{}", json!({"type":"session","version":3,"id":"session-test","cwd":r"C:\用户\workspace","timestamp":"2026-09-19T00:00:00Z"})).unwrap();
        for row in rows {
            writeln!(file, "{row}").unwrap();
        }
        write!(file, "{tail}").unwrap();
        file
    }
    fn msg(id: &str, parent: Option<&str>, role: &str, content: Value) -> Value {
        json!({"type":"message","id":id,"parentId":parent,"message":{"role":role,"content":content}})
    }
    #[test]
    fn sessions_over_256_mib_keep_recent_messages_with_bounded_preview() {
        use std::io::Write;
        let mut file = fixture(vec![msg("root", None, "user", json!("large session"))], "");
        // Large image records reflect actual Pi sessions. Each line stays below
        // the per-record limit; the total file crosses the former 256 MiB gate.
        let payload = "A".repeat(8 * 1024 * 1024);
        for i in 0..33 {
            writeln!(file, "{{\"type\":\"message\",\"id\":\"image-{i}\",\"parentId\":\"root\",\"message\":{{\"role\":\"user\",\"content\":[{{\"type\":\"image\",\"mimeType\":\"image/png\",\"data\":\"{payload}\"}}]}}}}").unwrap();
        }
        writeln!(
            file,
            "{}",
            msg(
                "last",
                Some("image-32"),
                "assistant",
                json!("large session tail")
            )
        )
        .unwrap();
        let size = file.as_file().metadata().unwrap().len();
        assert!(size > 256 * 1024 * 1024);
        let parsed = parse_session(file.path(), true, 2, "all").unwrap();
        assert_eq!(parsed.summary.file_size, size);
        assert_eq!(parsed.summary.message_count, 35);
        assert_eq!(parsed.summary.malformed_lines, 0);
        assert_eq!(parsed.summary.preview, "large session tail");
        assert_eq!(parsed.entries.len(), 2);
        assert_eq!(parsed.entries[0].id, "image-32");
        assert_eq!(parsed.entries[1].id, "last");
        assert!(serde_json::to_vec(&parsed.entries).unwrap().len() < 4096);
    }
    #[test]
    fn reads_unicode_titles_and_all_usage() {
        let file = fixture(
            vec![
                msg("a", None, "user", json!("修复会话显示")),
                json!({"type":"message","id":"b","parentId":"a","message":{"role":"assistant","model":"model-test","provider":"test","content":[{"type":"text","text":"done"}],"usage":{"input":12,"output":8,"cacheRead":3,"cost":{"total":0.12}}}}),
                json!({"type":"compaction","id":"c","parentId":"b","summary":"summary","usage":{"totalTokens":7,"cost":{"total":0.03}},"retainedTail":[{"usage":{"totalTokens":500}}]}),
                json!({"type":"session_info","id":"d","parentId":"c","name":"中文会话"}),
            ],
            "",
        );
        let parsed = parse_session(file.path(), true, 100, "all").unwrap();
        assert_eq!(parsed.summary.name, "中文会话");
        assert_eq!(parsed.summary.project, "workspace");
        assert_eq!(parsed.summary.tokens, 30);
        assert!((parsed.summary.cost - 0.15).abs() < 0.0001);
        assert_eq!(parsed.summary.model, "model-test");
        assert_eq!(parsed.total, 4);
    }
    #[test]
    fn last_branch_excludes_abandoned_siblings() {
        let file = fixture(
            vec![
                msg("a", None, "user", json!("root")),
                msg("b", Some("a"), "assistant", json!("old branch")),
                msg("c", Some("a"), "assistant", json!("new branch")),
                msg("d", Some("c"), "user", json!("last")),
            ],
            "",
        );
        let result = parse_session(file.path(), true, 2, "active").unwrap();
        assert_eq!(result.total, 3);
        assert_eq!(
            result
                .entries
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c", "d"]
        );
        assert_eq!(result.summary.branch_count, 1);
    }
    #[test]
    fn legacy_records_get_linear_parents() {
        let file = fixture(
            vec![
                json!({"type":"message","message":{"role":"user","content":"one"}}),
                json!({"type":"message","message":{"role":"assistant","content":"two"}}),
            ],
            "",
        );
        let result = parse_session(file.path(), true, 100, "active").unwrap();
        assert_eq!(result.total, 2);
        assert_eq!(result.entries[1].parent_id.as_deref(), Some("legacy-1"));
    }
    #[test]
    fn trailing_partial_records_do_not_lose_healthy_messages() {
        let file = fixture(
            vec![msg("a", None, "user", json!("hello"))],
            "{\"type\":\"message\"",
        );
        let result = parse_session(file.path(), true, 100, "all").unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.summary.malformed_lines, 1);
    }
    #[test]
    fn valid_final_line_without_newline_is_retained() {
        let row = msg("a", None, "user", json!("hello")).to_string();
        let file = fixture(vec![], &row);
        let result = parse_session(file.path(), true, 100, "all").unwrap();
        assert_eq!(result.total, 1);
        assert_eq!(result.summary.malformed_lines, 0);
    }
    #[test]
    fn image_payloads_are_not_sent_to_frontend() {
        let file = fixture(
            vec![msg(
                "a",
                None,
                "user",
                json!([{"type":"image","mimeType":"image/png","data":"PRIVATE_BASE64"},{"type":"text","text":"中".repeat(33000)}]),
            )],
            "",
        );
        let result = parse_session(file.path(), true, 100, "all").unwrap();
        let serialized = serde_json::to_string(&result.entries).unwrap();
        assert!(!serialized.contains("PRIVATE_BASE64"));
        assert_eq!(
            result.entries[0].content[1]
                .text
                .as_ref()
                .unwrap()
                .chars()
                .count(),
            32000
        );
        assert_eq!(result.entries[0].content[1].truncated, Some(true));
    }
    #[test]
    fn limits_and_unknown_branch_are_rejected() {
        let file = fixture(vec![], "");
        assert!(parse_session(file.path(), true, 0, "all").is_err());
        assert!(parse_session(file.path(), true, 100, "invalid").is_err());
    }
    #[test]
    fn cycle_in_parent_chain_terminates() {
        let file = fixture(
            vec![
                msg("a", Some("b"), "user", json!("one")),
                msg("b", Some("a"), "assistant", json!("two")),
            ],
            "",
        );
        assert_eq!(
            parse_session(file.path(), true, 100, "active")
                .unwrap()
                .total,
            2
        );
    }
}
