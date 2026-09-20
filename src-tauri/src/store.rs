use crate::parser::parse_session;
use crate::terminal::{self, TerminalPreference};
use crate::types::*;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

struct Cached {
    modified: SystemTime,
    len: u64,
    summary: SessionSummary,
}
pub struct Store {
    pub root: PathBuf,
    default_root: PathBuf,
    pub data_dir: PathBuf,
    pub demo: bool,
    connection: Connection,
    terminal_preference: TerminalPreference,
    cache: HashMap<PathBuf, Cached>,
    pub index: SessionIndex,
    pub revision: u64,
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn expand(value: PathBuf) -> PathBuf {
    let s = value.to_string_lossy();
    if s == "~" {
        return dirs::home_dir().unwrap_or(value);
    }
    if let Some(tail) = s.strip_prefix("~/").or_else(|| s.strip_prefix("~\\")) {
        if let Some(home) = dirs::home_dir() {
            return home.join(tail);
        }
    }
    if value.is_absolute() {
        value
    } else {
        std::env::current_dir().unwrap_or_default().join(value)
    }
}
pub fn default_session_root() -> PathBuf {
    if let Some(root) = std::env::var_os("PI_CODING_AGENT_SESSION_DIR") {
        return expand(PathBuf::from(root));
    }
    let agent = std::env::var_os("PI_CODING_AGENT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join(".pi")
                .join("agent")
        });
    expand(agent).join("sessions")
}
fn key_for(file: &Path) -> String {
    let path = file.to_string_lossy().replace('\\', "/");
    let normalized = if cfg!(windows) {
        path.to_lowercase()
    } else {
        path
    };
    format!("{:x}", Sha256::digest(normalized.as_bytes()))[..24].to_string()
}
fn valid_key(key: &str) -> bool {
    key.len() == 24 && key.bytes().all(|v| v.is_ascii_hexdigit())
}
fn meta_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionMeta> {
    Ok(SessionMeta {
        name: row.get(0)?,
        starred: row.get(1)?,
        archived: row.get(2)?,
    })
}
impl Store {
    pub fn new(default_root: PathBuf, data_dir: PathBuf, demo: bool) -> Result<Self, String> {
        fs::create_dir_all(&data_dir).map_err(err)?;
        let connection = Connection::open(data_dir.join("sessions.sqlite3")).map_err(err)?;
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(err)?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;
            CREATE TABLE IF NOT EXISTS metadata (key TEXT PRIMARY KEY, name TEXT, starred INTEGER NOT NULL DEFAULT 0, archived INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            PRAGMA user_version=1;").map_err(err)?;
        let saved: Option<String> = connection
            .query_row(
                "SELECT value FROM settings WHERE key='sessionRoot'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(err)?;
        let root = if demo {
            default_root.clone()
        } else {
            saved
                .map(PathBuf::from)
                .unwrap_or_else(|| default_root.clone())
        };
        let saved_terminal: Option<String> = connection
            .query_row("SELECT value FROM settings WHERE key='terminal'", [], |r| {
                r.get(0)
            })
            .optional()
            .map_err(err)?;
        let terminal_preference = saved_terminal
            .as_deref()
            .map(TerminalPreference::from_id)
            .transpose()?
            .unwrap_or_default();
        Ok(Self {
            root,
            default_root,
            data_dir,
            demo,
            connection,
            terminal_preference,
            cache: HashMap::new(),
            index: SessionIndex::default(),
            revision: 0,
        })
    }
    pub fn bootstrap(&self) -> Bootstrap {
        let terminal = terminal::configuration(self.terminal_preference);
        Bootstrap {
            session_root: self.root.to_string_lossy().into(),
            data_dir: self.data_dir.to_string_lossy().into(),
            platform: std::env::consts::OS.into(),
            terminal: terminal.label,
            terminal_preference: self.terminal_preference,
            terminal_options: terminal.options,
            demo: self.demo,
            default_cwd: dirs::home_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            poll_interval: 2000,
            version: env!("CARGO_PKG_VERSION").into(),
        }
    }
    pub fn terminal_preference(&self) -> TerminalPreference {
        self.terminal_preference
    }
    pub fn set_terminal(&mut self, preference: TerminalPreference) -> Result<Bootstrap, String> {
        terminal::validate(preference)?;
        if preference == TerminalPreference::Auto {
            self.connection
                .execute("DELETE FROM settings WHERE key='terminal'", [])
                .map_err(err)?;
        } else {
            self.connection.execute("INSERT INTO settings(key,value) VALUES('terminal',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [preference.id()]).map_err(err)?;
        }
        self.terminal_preference = preference;
        Ok(self.bootstrap())
    }
    pub fn set_root(&mut self, root: Option<String>) -> Result<Bootstrap, String> {
        if self.demo {
            return Err("演示模式不能修改真实会话目录".into());
        }
        let reset = root.is_none();
        let path = root
            .map(|r| expand(PathBuf::from(r)))
            .unwrap_or_else(|| self.default_root.clone());
        if path.exists() && !path.is_dir() {
            return Err("请选择一个目录".into());
        }
        if reset {
            self.connection
                .execute("DELETE FROM settings WHERE key='sessionRoot'", [])
                .map_err(err)?;
        } else {
            self.connection.execute("INSERT INTO settings(key,value) VALUES('sessionRoot',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [path.to_string_lossy().as_ref()]).map_err(err)?;
        }
        self.root = path;
        self.cache.clear();
        self.refresh()?;
        Ok(self.bootstrap())
    }
    fn metadata(&self, key: &str) -> Result<SessionMeta, String> {
        self.connection
            .query_row(
                "SELECT name,starred,archived FROM metadata WHERE key=?1",
                [key],
                meta_from_row,
            )
            .optional()
            .map(|m| m.unwrap_or_default())
            .map_err(err)
    }
    pub fn refresh(&mut self) -> Result<bool, String> {
        let mut files = Vec::new();
        let mut warnings = Vec::new();
        if !self.root.exists() {
            warnings.push("尚未找到 pi 会话目录。可在设置中选择目录，或先用 pi 创建会话。".into());
        } else if let Err(error) = walk(&self.root, 0, &mut files, &mut warnings) {
            warnings.push(format!("无法读取会话目录：{error}"));
        }
        files.sort();
        let mut cache = HashMap::new();
        let mut sessions = Vec::new();
        for file in files {
            let metadata = match fs::metadata(&file) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let cached = self
                .cache
                .remove(&file)
                .filter(|c| c.modified == modified && c.len == metadata.len());
            let mut entry = if let Some(entry) = cached {
                entry
            } else {
                match parse_session(&file, false, 100, "all") {
                    Ok(parsed) => Cached {
                        modified,
                        len: metadata.len(),
                        summary: parsed.summary,
                    },
                    Err(error) => {
                        warnings.push(format!(
                            "{}：{error}",
                            file.file_name().unwrap_or_default().to_string_lossy()
                        ));
                        continue;
                    }
                }
            };
            entry.summary.key = key_for(&file);
            let mut summary = entry.summary.clone();
            let meta = self.metadata(&summary.key)?;
            if let Some(name) = meta.name {
                summary.name = name;
            }
            summary.starred = meta.starred;
            summary.archived = meta.archived;
            if summary.malformed_lines > 0 {
                warnings.push(format!(
                    "{}：跳过 {} 条不完整、损坏或过大的记录",
                    summary.name, summary.malformed_lines
                ));
            }
            sessions.push(summary);
            cache.insert(file, entry);
        }
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.key.cmp(&b.key)));
        let changed = serde_json::to_vec(&(&sessions, &warnings)).map_err(err)?
            != serde_json::to_vec(&(&self.index.sessions, &self.index.warnings)).map_err(err)?;
        self.cache = cache;
        self.index = SessionIndex {
            sessions,
            warnings,
            scanned_at: Utc::now().to_rfc3339(),
        };
        if changed {
            self.revision += 1;
        }
        Ok(changed)
    }
    pub fn get(&self, key: &str) -> Result<SessionSummary, String> {
        if !valid_key(key) {
            return Err("无效的会话标识".into());
        }
        let session = self
            .index
            .sessions
            .iter()
            .find(|s| s.key == key)
            .ok_or("会话不存在或已移走")?
            .clone();
        let file = Path::new(&session.path);
        if fs::symlink_metadata(file)
            .map_err(err)?
            .file_type()
            .is_symlink()
        {
            return Err("不允许通过符号链接读取会话".into());
        }
        let canonical =
            fs::canonicalize(file).map_err(|_| "会话文件不存在或无法读取".to_string())?;
        let root = fs::canonicalize(&self.root).map_err(err)?;
        if !canonical.starts_with(&root) || !canonical.is_file() {
            return Err("会话文件不在配置的会话目录内".into());
        }
        Ok(session)
    }
    pub fn detail(&self, key: &str, branch: &str, limit: usize) -> Result<SessionDetail, String> {
        if !["all", "active"].contains(&branch) || !(1..=5000).contains(&limit) {
            return Err("无效的分支或消息数量".into());
        }
        let session = self.get(key)?;
        let parsed = parse_session(Path::new(&session.path), true, limit, branch)?;
        let mut summary = parsed.summary;
        summary.key = session.key.clone();
        summary.starred = session.starred;
        summary.archived = session.archived;
        if let Some(name) = self.metadata(key)?.name {
            summary.name = name;
        }
        let mut warnings = Vec::new();
        if summary.malformed_lines > 0 {
            warnings.push(format!(
                "跳过 {} 条损坏或尚未写完的记录。原文件未修改。",
                summary.malformed_lines
            ));
        }
        if parsed.budget_limited {
            warnings.push("消息内容过大，已限制预览内存。完整内容请导出查看。".into());
        }
        Ok(SessionDetail {
            session: summary,
            has_more: parsed.total > parsed.entries.len(),
            entries: parsed.entries,
            total: parsed.total,
            branch: branch.into(),
            warning: if warnings.is_empty() {
                None
            } else {
                Some(warnings.join(" "))
            },
        })
    }
    pub fn batch(&mut self, request: BatchRequest) -> Result<(), String> {
        if request.keys.is_empty() || request.keys.len() > 500 {
            return Err("每次请选择 1 到 500 个会话".into());
        }
        if !["star", "unstar", "archive", "restore", "rename"].contains(&request.action.as_str()) {
            return Err("不支持的批量操作".into());
        }
        let mut seen = HashSet::new();
        let keys: Vec<_> = request
            .keys
            .into_iter()
            .filter(|k| seen.insert(k.clone()))
            .collect();
        let mut updates = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            let session = self.get(key)?;
            let mut meta = self.metadata(key)?;
            match request.action.as_str() {
                "star" => meta.starred = true,
                "unstar" => meta.starred = false,
                "archive" => meta.archived = true,
                "restore" => meta.archived = false,
                "rename" => {
                    let template = request.name.as_deref().ok_or("请输入会话名称")?;
                    // Substitute placeholders in the template, never reinterpret text inside an existing name.
                    let name = apply_template(template, &session.name, i + 1)
                        .trim()
                        .to_string();
                    if name.is_empty()
                        || name.chars().count() > 160
                        || name.chars().any(char::is_control)
                    {
                        return Err("名称须为 1 到 160 个字符，不能包含控制字符".into());
                    }
                    meta.name = Some(name);
                }
                _ => unreachable!(),
            }
            updates.push((key.clone(), meta));
        }
        let tx = self.connection.transaction().map_err(err)?;
        for (key, meta) in &updates {
            tx.execute("INSERT INTO metadata(key,name,starred,archived) VALUES(?1,?2,?3,?4) ON CONFLICT(key) DO UPDATE SET name=excluded.name,starred=excluded.starred,archived=excluded.archived", params![key, meta.name, meta.starred, meta.archived]).map_err(err)?;
        }
        tx.commit().map_err(err)?;
        self.refresh()?;
        Ok(())
    }
    pub fn export_content(&self, keys: &[String]) -> Result<(String, Vec<u8>), String> {
        if keys.is_empty() || keys.len() > 100 {
            return Err("每次可导出 1 到 100 个会话".into());
        }
        let mut sessions = Vec::new();
        let mut seen = HashSet::new();
        let mut total = 0u64;
        for key in keys {
            if !seen.insert(key) {
                continue;
            }
            let session = self.get(key)?;
            total = total.saturating_add(fs::metadata(&session.path).map_err(err)?.len());
            if total > 64 * 1024 * 1024 {
                return Err("导出内容超过 64 MiB，请减少选择或直接复制会话文件".into());
            }
            sessions.push(session);
        }
        if sessions.len() == 1 {
            return Ok((
                format!("pi-session-{}.jsonl", sessions[0].key),
                fs::read(&sessions[0].path).map_err(err)?,
            ));
        }
        let mut bundle = Vec::new();
        for session in sessions {
            let jsonl = fs::read_to_string(&session.path).map_err(err)?;
            bundle.push(serde_json::json!({"session":session,"jsonl":jsonl}));
        }
        Ok(("pi-sessions.json".into(), serde_json::to_vec_pretty(&serde_json::json!({"version":1,"exportedAt":Utc::now().to_rfc3339(),"sessions":bundle})).map_err(err)?))
    }
}
fn walk(
    dir: &Path,
    depth: usize,
    files: &mut Vec<PathBuf>,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    if depth > 12 {
        warnings.push(format!("目录层级超过扫描上限：{}", dir.display()));
        return Ok(());
    }
    if files.len() >= 50_000 {
        return Err("超过 50,000 个会话文件，请选择更小的目录".into());
    }
    for entry in fs::read_dir(dir).map_err(err)? {
        if files.len() >= 50_000 {
            return Err("超过 50,000 个会话文件，请选择更小的目录".into());
        }
        let entry = entry.map_err(err)?;
        let kind = entry.file_type().map_err(err)?;
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            if let Err(e) = walk(&path, depth + 1, files, warnings) {
                warnings.push(e);
            }
        } else if kind.is_file() && path.extension().is_some_and(|v| v == "jsonl") {
            files.push(path);
        }
    }
    Ok(())
}
fn apply_template(template: &str, name: &str, number: usize) -> String {
    let mut result = String::new();
    let mut rest = template;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix("{name}") {
            result.push_str(name);
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("{n}") {
            result.push_str(&number.to_string());
            rest = tail;
        } else {
            let ch = rest.chars().next().unwrap();
            result.push(ch);
            rest = &rest[ch.len_utf8()..];
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, Store, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("sessions");
        fs::create_dir(&root).unwrap();
        let file = root.join("sample.jsonl");
        fs::write(&file, "{\"type\":\"session\",\"version\":3,\"id\":\"test\",\"cwd\":\"/project\",\"timestamp\":\"2026-09-19T00:00:00Z\"}\n{\"type\":\"message\",\"id\":\"a\",\"parentId\":null,\"message\":{\"role\":\"user\",\"content\":\"Hello\"}}\n").unwrap();
        let mut store = Store::new(root, dir.path().join("data"), false).unwrap();
        store.refresh().unwrap();
        (dir, store, file)
    }
    #[test]
    fn metadata_persists_and_never_changes_jsonl() {
        let (_dir, mut store, file) = fixture();
        let before = fs::read(&file).unwrap();
        let key = store.index.sessions[0].key.clone();
        store
            .batch(BatchRequest {
                keys: vec![key.clone()],
                action: "rename".into(),
                name: Some("测试 {n}".into()),
            })
            .unwrap();
        store
            .batch(BatchRequest {
                keys: vec![key.clone()],
                action: "archive".into(),
                name: None,
            })
            .unwrap();
        assert_eq!(before, fs::read(&file).unwrap());
        let root = store.root.clone();
        let data = store.data_dir.clone();
        drop(store);
        let mut restored = Store::new(root, data, false).unwrap();
        restored.refresh().unwrap();
        assert_eq!(restored.index.sessions[0].name, "测试 1");
        assert!(restored.index.sessions[0].archived);
    }
    #[test]
    fn terminal_default_selection_and_reset_survive_restart() {
        let (_dir, mut store, file) = fixture();
        let before = fs::read(&file).unwrap();
        assert_eq!(
            store.bootstrap().terminal_preference,
            TerminalPreference::Auto
        );
        let choice = store.bootstrap().terminal_options.first().unwrap().id;
        store.set_terminal(choice).unwrap();
        let root = store.root.clone();
        let data = store.data_dir.clone();
        drop(store);
        let mut restored = Store::new(root.clone(), data.clone(), false).unwrap();
        assert_eq!(restored.bootstrap().terminal_preference, choice);
        assert_eq!(restored.terminal_preference(), choice);
        restored.set_terminal(TerminalPreference::Auto).unwrap();
        drop(restored);
        let reset = Store::new(root, data, false).unwrap();
        assert_eq!(reset.terminal_preference(), TerminalPreference::Auto);
        assert_eq!(before, fs::read(file).unwrap());
    }
    #[test]
    fn unavailable_terminal_does_not_replace_saved_setting() {
        let (_dir, mut store, _) = fixture();
        let unavailable = if cfg!(windows) {
            TerminalPreference::System
        } else {
            TerminalPreference::Powershell
        };
        assert!(store.set_terminal(unavailable).is_err());
        assert_eq!(store.terminal_preference(), TerminalPreference::Auto);
        assert!(TerminalPreference::from_id("cmd.exe & echo unsafe").is_err());
    }
    #[test]
    fn invalid_batch_is_atomic() {
        let (_dir, mut store, _) = fixture();
        let key = store.index.sessions[0].key.clone();
        assert!(store
            .batch(BatchRequest {
                keys: vec![key, "invalid".into()],
                action: "archive".into(),
                name: None
            })
            .is_err());
        assert!(!store.index.sessions[0].archived);
    }
    #[test]
    fn append_is_detected_and_readable() {
        use std::io::Write;
        let (_dir, mut store, file) = fixture();
        let key = store.index.sessions[0].key.clone();
        fs::OpenOptions::new().append(true).open(&file).unwrap().write_all(b"{\"type\":\"message\",\"id\":\"b\",\"parentId\":\"a\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"updated\"}]}}\n").unwrap();
        assert!(store.refresh().unwrap());
        assert_eq!(store.detail(&key, "all", 100).unwrap().total, 2);
    }
    #[test]
    fn placeholders_in_names_are_literal() {
        assert_eq!(apply_template("{n} {name}", "old {n}", 2), "2 old {n}");
    }
    #[test]
    fn missing_root_is_non_destructive() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("missing");
        let mut store = Store::new(root.clone(), dir.path().join("db"), false).unwrap();
        store.refresh().unwrap();
        assert!(!root.exists());
        assert!(!store.index.warnings.is_empty());
    }
    #[test]
    fn export_preserves_bytes() {
        let (_dir, store, file) = fixture();
        let (_, bytes) = store
            .export_content(&[store.index.sessions[0].key.clone()])
            .unwrap();
        assert_eq!(bytes, fs::read(file).unwrap());
    }
}
