//! Tauri commands: the only surface the frontend can call.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use crate::agent::{self, AgentContext};
use crate::contract::{
    ChatFrame, ChatMessage, ChatRunRequest, SettingsPatch, SettingsSnapshot, StoredThread, ThreadList,
};
use crate::settings;
use crate::store::{self, ThreadStore};

pub struct AppState {
    pub(crate) store: ThreadStore,
    runs: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl AppState {
    pub fn new(store: ThreadStore) -> Self {
        Self {
            store,
            runs: Mutex::new(HashMap::new()),
        }
    }
}

#[tauri::command]
pub fn list_threads(state: State<'_, AppState>, cursor: Option<String>) -> ThreadList {
    state.store.list(cursor.as_deref())
}

#[tauri::command]
pub fn create_thread(
    state: State<'_, AppState>,
    message: ChatMessage,
) -> Result<StoredThread, String> {
    state.store.create(message)
}

#[tauri::command]
pub fn get_messages(
    state: State<'_, AppState>,
    thread_id: String,
) -> Result<Vec<ChatMessage>, String> {
    state.store.messages(&thread_id)
}

#[tauri::command]
pub fn update_thread(
    state: State<'_, AppState>,
    thread: StoredThread,
) -> Result<StoredThread, String> {
    state.store.update(thread)
}

#[tauri::command]
pub fn delete_thread(state: State<'_, AppState>, thread_id: String) -> Result<(), String> {
    state.store.delete(&thread_id)
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<SettingsSnapshot, String> {
    let path = settings::settings_path(&app)?;
    settings::snapshot(&path)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, patch: SettingsPatch) -> Result<SettingsSnapshot, String> {
    let path = settings::settings_path(&app)?;
    settings::apply_patch(&path, patch)
}

/// Stops a run. The streaming loop checks the flag between network reads and
/// treats a cancellation as a clean stop rather than a failure.
#[tauri::command]
pub fn cancel_chat(state: State<'_, AppState>, run_id: String) {
    if let Some(flag) = state.runs.lock().ok().and_then(|runs| runs.get(&run_id).cloned()) {
        flag.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
pub async fn chat(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ChatRunRequest,
    on_event: Channel<ChatFrame>,
) -> Result<(), String> {
    let run_id = request.run_id.clone();
    let cancelled = Arc::new(AtomicBool::new(false));

    if let Ok(mut runs) = state.runs.lock() {
        runs.insert(run_id.clone(), cancelled.clone());
    }

    let result = run_chat(&app, &state, request, cancelled, &on_event).await;

    if let Ok(mut runs) = state.runs.lock() {
        runs.remove(&run_id);
    }

    let _ = on_event.send(ChatFrame::Done);
    result
}

async fn run_chat(
    app: &AppHandle,
    state: &AppState,
    request: ChatRunRequest,
    cancelled: Arc<AtomicBool>,
    on_event: &Channel<ChatFrame>,
) -> Result<(), String> {
    let resolved = settings::resolve(app)?;

    let Some(api_key) = resolved.api_key else {
        let message =
            "No API key is configured. Open Settings in the sidebar and add one to start chatting."
                .to_string();
        let _ = on_event.send(ChatFrame::Error {
            message: message.clone(),
        });
        // Nothing is persisted: a run that never reached the model must not
        // change the stored thread.
        return Err(message);
    };

    let client = reqwest::Client::builder()
        .user_agent("openui-desktop/0.1")
        .build()
        .map_err(|error| format!("Could not initialize the HTTP client: {error}"))?;

    let context = AgentContext {
        client,
        base_url: resolved.base_url,
        model: resolved.model,
        api_key,
        cancelled: cancelled.clone(),
    };

    // `create_thread` already stored the first user message, so only the tail
    // of the replayed history is genuinely new. Nothing is written twice.
    let stored = state.store.messages(&request.thread_id).unwrap_or_default();
    let turn = store::new_turn(&request.messages, &stored).to_vec();

    let mut emit = |frame: ChatFrame| {
        let _ = on_event.send(frame);
    };

    match agent::run(&context, &request.messages, &mut emit).await {
        Ok(outcome) => {
            if !outcome.produced.is_empty() {
                let mut appended = turn;
                appended.extend(outcome.produced);
                if let Err(error) = state.store.append_turn(&request.thread_id, &appended) {
                    eprintln!("failed to persist turn: {error}");
                }
            }
            Ok(())
        }
        // The agent already emitted `RUN_ERROR` for this run; reporting it a
        // second time would surface the same failure twice.
        Err(message) => Err(message),
    }
}
