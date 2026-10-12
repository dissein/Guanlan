#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use guanlan_core::{
    agent::{self, Approvals},
    root, Session, Settings,
};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tauri::{ipc::Channel, Manager, State};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

struct Runtime {
    settings: Mutex<Settings>,
    session: Mutex<Session>,
    approvals: Arc<Approvals>,
    busy: AtomicBool,
    cancel: std::sync::Mutex<CancellationToken>,
    config: PathBuf,
    gate: Mutex<()>,
}
type Shared = Arc<Runtime>;
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn session_view(s: &Session) -> Value {
    json!({"id":s.id,"title":s.title,"messages":s.messages,"undo_count":s.undo.len()})
}
fn credential(base: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new("Guanlan", base).map_err(error)
}
fn key(base: &str) -> Result<String, String> {
    match credential(base)?.get_password() {
        Ok(k) => Ok(k),
        Err(keyring::Error::NoEntry) => Ok(String::new()),
        Err(e) => Err(error(e)),
    }
}
fn idle(s: &Runtime) -> Result<(), String> {
    if s.busy.load(Ordering::SeqCst) {
        Err("任务执行中，请先停止".into())
    } else {
        Ok(())
    }
}
#[tauri::command]
async fn bootstrap(state: State<'_, Shared>) -> Result<Value, String> {
    let settings = state.settings.lock().await.clone();
    let session = state.session.lock().await.clone();
    let sessions = if let Ok(r) = root(&settings.workspace) {
        guanlan_core::list_sessions(&r).map_err(error)?
    } else {
        vec![]
    };
    let has_key = !key(&settings.base_url)?.is_empty();
    Ok(
        json!({"settings":settings,"session":session_view(&session),"sessions":sessions,"has_key":has_key}),
    )
}
#[tauri::command]
async fn save_settings(
    settings: Settings,
    api_key: Option<String>,
    state: State<'_, Shared>,
) -> Result<(), String> {
    let _gate = state.gate.lock().await;
    idle(&state)?;
    agent::completion_url(&settings.base_url).map_err(error)?;
    if settings.model.trim().is_empty() {
        return Err("模型名不能为空".into());
    }
    if !matches!(settings.mode.as_str(), "build" | "plan") {
        return Err("无效模式".into());
    }
    if !settings.workspace.is_empty() {
        root(&settings.workspace).map_err(error)?;
    }
    if let Some(k) = api_key {
        let entry = credential(&settings.base_url)?;
        if k.is_empty() {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => (),
                Err(e) => return Err(error(e)),
            }
        } else {
            entry.set_password(&k).map_err(error)?
        }
    }
    let mut current = state.settings.lock().await;
    if current.workspace != settings.workspace {
        *state.session.lock().await = Session::default();
    }
    fs::write(
        &state.config,
        serde_json::to_vec_pretty(&settings).map_err(error)?,
    )
    .map_err(error)?;
    *current = settings;
    Ok(())
}
#[tauri::command]
async fn new_session(state: State<'_, Shared>) -> Result<Value, String> {
    let _gate = state.gate.lock().await;
    idle(&state)?;
    let s = Session::default();
    *state.session.lock().await = s.clone();
    Ok(session_view(&s))
}
#[tauri::command]
async fn load_session(id: String, state: State<'_, Shared>) -> Result<Value, String> {
    let _gate = state.gate.lock().await;
    idle(&state)?;
    let settings = state.settings.lock().await;
    let s = guanlan_core::load_session(&root(&settings.workspace).map_err(error)?, &id)
        .map_err(error)?;
    *state.session.lock().await = s.clone();
    Ok(session_view(&s))
}
#[tauri::command]
async fn start_run(
    prompt: String,
    on_event: Channel<Value>,
    state: State<'_, Shared>,
) -> Result<(), String> {
    let _gate = state.gate.lock().await;
    if prompt.trim().is_empty() {
        return Err("请输入消息".into());
    }
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err("已有任务正在执行".into());
    }
    let settings = state.settings.lock().await.clone();
    let api_key = match key(&settings.base_url) {
        Ok(k) => k,
        Err(e) => {
            state.busy.store(false, Ordering::SeqCst);
            return Err(e);
        }
    };
    if let Err(e) = root(&settings.workspace) {
        state.busy.store(false, Ordering::SeqCst);
        return Err(error(e));
    }
    let token = CancellationToken::new();
    *state.cancel.lock().map_err(error)? = token.clone();
    let shared = state.inner().clone();
    tauri::async_runtime::spawn(async move {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let channel = on_event.clone();
        let bridge = tokio::spawn(async move {
            while let Some(e) = rx.recv().await {
                let _ = channel.send(e);
            }
        });
        let mut session = shared.session.lock().await;
        let result = agent::run(
            &settings,
            &api_key,
            prompt,
            &mut session,
            shared.approvals.clone(),
            tx,
            token,
        )
        .await;
        let _ = bridge.await;
        let summary = json!({"type":"done","error":result.err().map(|e|e.to_string()),"session":session_view(&session)});
        drop(session);
        shared.busy.store(false, Ordering::SeqCst);
        let _ = on_event.send(summary);
    });
    Ok(())
}
#[tauri::command]
fn stop_run(state: State<'_, Shared>) -> Result<(), String> {
    state.cancel.lock().map_err(error)?.cancel();
    Ok(())
}
#[tauri::command]
async fn respond(id: String, answer: String, state: State<'_, Shared>) -> Result<(), String> {
    state.approvals.resolve(&id, answer).await.map_err(error)
}
#[tauri::command]
async fn undo(state: State<'_, Shared>) -> Result<usize, String> {
    let _gate = state.gate.lock().await;
    idle(&state)?;
    let settings = state.settings.lock().await;
    guanlan_core::undo(
        &root(&settings.workspace).map_err(error)?,
        &mut *state.session.lock().await,
    )
    .map_err(error)
}
#[tauri::command]
async fn workspace_files(state: State<'_, Shared>) -> Result<Vec<String>, String> {
    let s = state.settings.lock().await;
    let r = root(&s.workspace).map_err(error)?;
    let mut result = vec![];
    for e in fs::read_dir(r).map_err(error)? {
        let e = e.map_err(error)?;
        let name = e.file_name().to_string_lossy().into_owned();
        if name == ".guanlan" || name == ".git" {
            continue;
        }
        result.push(format!(
            "{}{}",
            name,
            if e.file_type().map_err(error)?.is_dir() {
                "/"
            } else {
                ""
            }
        ));
    }
    result.sort();
    Ok(result)
}
#[tauri::command]
async fn discover_models(base_url: String, api_key: Option<String>) -> Result<Vec<String>, String> {
    let endpoint = agent::completion_url(&base_url)
        .map_err(error)?
        .trim_end_matches("/chat/completions")
        .to_owned()
        + "/models";
    let mut r = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(error)?
        .get(endpoint);
    let k = match api_key {
        Some(k) => k,
        None => key(&base_url)?,
    };
    if !k.is_empty() {
        r = r.bearer_auth(k)
    }
    let v: Value = r
        .send()
        .await
        .map_err(error)?
        .error_for_status()
        .map_err(error)?
        .json()
        .await
        .map_err(error)?;
    Ok(v["data"]
        .as_array()
        .ok_or("服务没有返回 models 列表")?
        .iter()
        .filter_map(|m| m["id"].as_str().map(str::to_owned))
        .collect())
}
fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<Shared>();
                if state.busy.load(Ordering::SeqCst) {
                    api.prevent_close();
                    if let Ok(token) = state.cancel.lock() {
                        token.cancel();
                    }
                    let shared = state.inner().clone();
                    let window = window.clone();
                    tauri::async_runtime::spawn(async move {
                        while shared.busy.load(Ordering::SeqCst) {
                            tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                        }
                        let _ = window.destroy();
                    });
                }
            }
        })
        .setup(|app| {
            let dir = app.path().app_config_dir()?;
            fs::create_dir_all(&dir)?;
            let config = dir.join("settings.json");
            let settings: Settings = fs::read(&config)
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok())
                .unwrap_or_default();
            app.manage(Arc::new(Runtime {
                settings: Mutex::new(settings),
                session: Mutex::new(Session::default()),
                approvals: Arc::new(Approvals::default()),
                busy: AtomicBool::new(false),
                cancel: std::sync::Mutex::new(CancellationToken::new()),
                config,
                gate: Mutex::new(()),
            }));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            save_settings,
            new_session,
            load_session,
            start_run,
            stop_run,
            respond,
            undo,
            workspace_files,
            discover_models
        ])
        .run(tauri::generate_context!())
        .expect("无法启动观澜");
}
