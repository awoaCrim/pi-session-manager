mod parser;
mod store;
mod terminal;
mod tray;
mod types;

use crate::store::Store;
use crate::types::*;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

#[derive(Clone)]
struct AppState {
    store: Arc<Mutex<Store>>,
    launches: Arc<Mutex<HashMap<String, Instant>>>,
}
fn access<T>(
    state: &AppState,
    f: impl FnOnce(&mut Store) -> Result<T, String>,
) -> Result<T, String> {
    let mut store = state
        .store
        .lock()
        .map_err(|_| "会话数据库锁异常，请重启应用".to_string())?;
    f(&mut store)
}
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn bootstrap(state: tauri::State<'_, AppState>) -> Result<Bootstrap, String> {
    let state = state.inner().clone();
    blocking(move || access(&state, |s| Ok(s.bootstrap()))).await
}
#[tauri::command]
async fn list_sessions(state: tauri::State<'_, AppState>) -> Result<SessionIndex, String> {
    let state = state.inner().clone();
    blocking(move || access(&state, |s| Ok(s.index.clone()))).await
}
#[tauri::command]
async fn refresh_sessions(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<SessionIndex, String> {
    let state = state.inner().clone();
    blocking(move || {
        access(&state, |s| {
            if s.refresh()? {
                let _ = app.emit("sessions-changed", s.revision);
            }
            Ok(s.index.clone())
        })
    })
    .await
}
#[tauri::command]
async fn session_detail(
    state: tauri::State<'_, AppState>,
    key: String,
    branch: String,
    limit: usize,
) -> Result<SessionDetail, String> {
    let state = state.inner().clone();
    blocking(move || access(&state, |s| s.detail(&key, &branch, limit))).await
}
#[tauri::command]
async fn batch_sessions(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: BatchRequest,
) -> Result<(), String> {
    let state = state.inner().clone();
    blocking(move || {
        access(&state, |s| {
            s.batch(request)?;
            let _ = app.emit("sessions-changed", s.revision);
            Ok(())
        })
    })
    .await
}
#[tauri::command]
async fn set_session_root(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    root: Option<String>,
) -> Result<Bootstrap, String> {
    let state = state.inner().clone();
    blocking(move || {
        access(&state, |s| {
            let config = s.set_root(root)?;
            let _ = app.emit("sessions-changed", s.revision);
            Ok(config)
        })
    })
    .await
}
#[tauri::command]
async fn set_terminal(
    state: tauri::State<'_, AppState>,
    preference: terminal::TerminalPreference,
) -> Result<Bootstrap, String> {
    let state = state.inner().clone();
    blocking(move || access(&state, |s| s.set_terminal(preference))).await
}
#[tauri::command]
async fn launch_session(
    state: tauri::State<'_, AppState>,
    key: Option<String>,
    project_key: Option<String>,
    cwd: Option<String>,
    fork: Option<bool>,
) -> Result<serde_json::Value, String> {
    let state = state.inner().clone();
    blocking(move || {
        let (request, preference) = access(&state, |s| {
            if s.demo {
                return Err("演示模式不会启动真实终端。请以普通模式启动应用。".into());
            }
            let request = if let Some(key) = key {
                let session = s.get(&key)?;
                terminal::Launch {
                    cwd: session.cwd,
                    path: Some(session.path),
                    name: Some(session.name),
                    fork: fork.unwrap_or(false),
                }
            } else {
                let directory = if let Some(key) = project_key {
                    s.get(&key)?.cwd
                } else {
                    cwd.unwrap_or_else(|| s.bootstrap().default_cwd)
                };
                terminal::Launch {
                    cwd: directory,
                    path: None,
                    name: None,
                    fork: false,
                }
            };
            Ok((request, s.terminal_preference()))
        })?;
        let launch_key = request.path.clone().unwrap_or_else(|| request.cwd.clone());
        {
            let mut launches = state.launches.lock().map_err(|e| e.to_string())?;
            launches.retain(|_, at| at.elapsed() < Duration::from_secs(3));
            if launches.contains_key(&launch_key) {
                return Err("终端已在启动，请勿重复点击".into());
            }
            launches.insert(launch_key.clone(), Instant::now());
        }
        match terminal::launch(request, preference) {
            Ok(message) => Ok(serde_json::json!({"message":message})),
            Err(error) => {
                if let Ok(mut launches) = state.launches.lock() {
                    launches.remove(&launch_key);
                }
                Err(error)
            }
        }
    })
    .await
}
#[tauri::command]
async fn export_sessions(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    keys: Vec<String>,
) -> Result<serde_json::Value, String> {
    let state = state.inner().clone();
    blocking(move || {
        let (name, content, root, data_dir) = access(&state, |s| {
            let (name, content) = s.export_content(&keys)?;
            Ok((name, content, s.root.clone(), s.data_dir.clone()))
        })?;
        let filter = if name.ends_with(".jsonl") {
            "jsonl"
        } else {
            "json"
        };
        let selection = app
            .dialog()
            .file()
            .set_title("导出 Pi 会话")
            .set_file_name(&name)
            .add_filter("会话记录", &[filter])
            .blocking_save_file();
        let Some(selection) = selection else {
            return Ok(serde_json::json!({"saved":false}));
        };
        let target = selection.into_path().map_err(|e| e.to_string())?;
        let parent = target.parent().ok_or("无效的保存目录")?;
        let real_parent = std::fs::canonicalize(parent).map_err(|e| e.to_string())?;
        for protected in [root, data_dir] {
            if let Ok(protected) = std::fs::canonicalize(protected) {
                if real_parent.starts_with(protected) {
                    return Err("请导出到其他目录，不能覆盖原始会话或管理器数据库。".into());
                }
            }
        }
        if target.exists()
            && std::fs::symlink_metadata(&target)
                .map_err(|e| e.to_string())?
                .file_type()
                .is_symlink()
        {
            return Err("不能导出到符号链接文件".into());
        }
        use std::io::Write;
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        temp.write_all(&content).map_err(|e| e.to_string())?;
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        temp.persist(&target).map_err(|e| e.to_string())?;
        Ok(serde_json::json!({"saved":true,"path":target.to_string_lossy()}))
    })
    .await
}
fn watch_sessions(app: tauri::AppHandle, state: AppState) {
    std::thread::spawn(move || {
        use notify::{RecursiveMode, Watcher};
        let (send, receive) = std::sync::mpsc::channel();
        let mut watcher =
            notify::recommended_watcher(move |event: Result<notify::Event, notify::Error>| {
                if let Ok(event) = event {
                    if !event.kind.is_access() {
                        let _ = send.send(());
                    }
                }
            })
            .ok();
        let mut watched = PathBuf::new();
        loop {
            let root = match access(&state, |s| Ok(s.root.clone())) {
                Ok(v) => v,
                Err(_) => return,
            };
            if root != watched {
                if let Some(watcher) = &mut watcher {
                    let _ = watcher.unwatch(&watched);
                    if root.is_dir() {
                        let _ = watcher.watch(&root, RecursiveMode::Recursive);
                    }
                }
                watched = root;
            }
            let _ = receive.recv_timeout(Duration::from_secs(2));
            // Coalesce rapid append bursts without starving under a busy agent.
            let start = Instant::now();
            while start.elapsed() < Duration::from_millis(500)
                && receive.recv_timeout(Duration::from_millis(100)).is_ok()
            {}
            if let Err(error) = access(&state, |s| {
                if s.refresh()? {
                    let _ = app.emit("sessions-changed", s.revision);
                }
                Ok(())
            }) {
                let _ = app.emit("sync-error", error);
            }
        }
    });
}
fn seed_demo(root: &Path) -> Result<(), String> {
    let seed: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("demo.json")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    for (i, session) in seed.iter().enumerate() {
        let path = root.join(format!("demo-{i:02}.jsonl"));
        if path.exists() {
            continue;
        }
        let records = session.as_array().ok_or("无效的演示数据")?;
        let text = records
            .iter()
            .map(|v| serde_json::to_string(v).unwrap())
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        std::fs::write(path, text).map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            tray::show_main_window(app);
        }))
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(tray::window_state_flags())
                .build(),
        )
        .on_window_event(tray::on_window_event)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let demo = std::env::args().any(|arg| arg == "--demo");
            let default_data = app.path().app_data_dir()?;
            let data = if demo {
                default_data.join("demo")
            } else {
                std::env::var_os("PI_SESSION_MANAGER_DATA_DIR")
                    .map(PathBuf::from)
                    .unwrap_or(default_data)
            };
            let root = if demo {
                data.join("sessions")
            } else {
                store::default_session_root()
            };
            if demo {
                seed_demo(&root).map_err(std::io::Error::other)?;
            }
            let mut store = Store::new(root, data, demo).map_err(std::io::Error::other)?;
            store.refresh().map_err(std::io::Error::other)?;
            let state = AppState {
                store: Arc::new(Mutex::new(store)),
                launches: Arc::new(Mutex::new(HashMap::new())),
            };
            app.manage(state.clone());
            tray::setup(app.handle())?;
            watch_sessions(app.handle().clone(), state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            list_sessions,
            refresh_sessions,
            session_detail,
            batch_sessions,
            set_session_root,
            set_terminal,
            launch_session,
            export_sessions
        ])
        .build(tauri::generate_context!())
        .expect("无法启动 Pi Sessions 桌面应用")
        .run(tray::on_run_event);
}
