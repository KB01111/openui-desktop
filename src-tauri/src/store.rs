//! Durable conversation storage.
//!
//! Threads are JSON documents under `threads/` in the app data directory. The
//! in-memory index is the source of truth between calls; every mutation writes
//! the affected thread atomically so a crash cannot leave a half-written file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::contract::{ChatMessage, Content, StoredThread, ThreadList};
use crate::settings::write_atomic;

const TITLE_LIMIT: usize = 40;
const DEFAULT_PAGE: usize = 50;

#[derive(Debug, Serialize, Deserialize)]
struct ThreadFile {
    thread: StoredThread,
    messages: Vec<ChatMessage>,
}

struct Index {
    threads: Vec<StoredThread>,
    messages: HashMap<String, Vec<ChatMessage>>,
}

pub struct ThreadStore {
    dir: PathBuf,
    index: Mutex<Index>,
}

fn threads_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve the app data directory: {error}"))?
        .join("threads");
    Ok(dir)
}

fn thread_file(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.json"))
}

/// First 40 characters of the first message, matching what the web template
/// derived its thread titles from.
pub fn derive_title(message: &ChatMessage) -> String {
    let text = message
        .content
        .as_ref()
        .map(Content::as_text)
        .unwrap_or_default();
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let title: String = collapsed.chars().take(TITLE_LIMIT).collect();
    if title.is_empty() {
        "New thread".to_string()
    } else {
        title
    }
}

fn load_file(dir: &Path, id: &str) -> Option<ThreadFile> {
    let raw = std::fs::read_to_string(thread_file(dir, id)).ok()?;
    serde_json::from_str(&raw).ok()
}

fn persist(dir: &Path, thread: &StoredThread, messages: &[ChatMessage]) -> Result<(), String> {
    std::fs::create_dir_all(dir)
        .map_err(|error| format!("Could not create the thread directory: {error}"))?;
    let file = ThreadFile {
        thread: thread.clone(),
        messages: messages.to_vec(),
    };
    let bytes = serde_json::to_vec(&file).map_err(|error| format!("Could not serialize thread: {error}"))?;
    write_atomic(&thread_file(dir, &thread.id), &bytes)
}

impl ThreadStore {
    /// Reads every thread file on startup so the list survives restarts.
    pub fn load(app: &AppHandle) -> Result<Self, String> {
        let dir = threads_dir(app)?;
        let mut threads = Vec::new();
        let mut messages = HashMap::new();

        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
                    continue;
                }
                let Ok(raw) = std::fs::read_to_string(&path) else {
                    continue;
                };
                let Ok(file) = serde_json::from_str::<ThreadFile>(&raw) else {
                    continue;
                };
                messages.insert(file.thread.id.clone(), file.messages);
                threads.push(file.thread);
            }
        }

        threads.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(Self {
            dir,
            index: Mutex::new(Index { threads, messages }),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Index> {
        // A poisoned lock means another call panicked while holding it. The
        // files on disk are still intact, so continue with the in-memory state.
        self.index.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn list(&self, cursor: Option<&str>) -> ThreadList {
        let index = self.lock();
        let offset = cursor
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        let threads: Vec<_> = index
            .threads
            .iter()
            .skip(offset)
            .take(DEFAULT_PAGE)
            .cloned()
            .collect();
        let next = offset + threads.len();
        ThreadList {
            threads,
            next_cursor: (next < index.threads.len()).then(|| next.to_string()),
        }
    }

    pub fn create(&self, message: ChatMessage) -> Result<StoredThread, String> {
        let thread = StoredThread {
            id: uuid::Uuid::new_v4().to_string(),
            title: derive_title(&message),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let messages = vec![message];

        persist(&self.dir, &thread, &messages)?;

        let mut index = self.lock();
        index.threads.insert(0, thread.clone());
        index.messages.insert(thread.id.clone(), messages);
        Ok(thread)
    }

    pub fn messages(&self, id: &str) -> Result<Vec<ChatMessage>, String> {
        if let Some(messages) = self.lock().messages.get(id) {
            return Ok(messages.clone());
        }
        match load_file(&self.dir, id) {
            Some(file) => Ok(file.messages),
            None => Err(format!("Thread {id} was not found")),
        }
    }

    pub fn update(&self, thread: StoredThread) -> Result<StoredThread, String> {
        let messages = self.messages(&thread.id)?;
        persist(&self.dir, &thread, &messages)?;

        let mut index = self.lock();
        match index.threads.iter_mut().find(|existing| existing.id == thread.id) {
            Some(existing) => *existing = thread.clone(),
            None => return Err(format!("Thread {} was not found", thread.id)),
        }
        Ok(thread)
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        match std::fs::remove_file(thread_file(&self.dir, id)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Could not delete thread {id}: {error}")),
        }
        let mut index = self.lock();
        index.threads.retain(|thread| thread.id != id);
        index.messages.remove(id);
        Ok(())
    }

    /// Appends one completed turn. `create` already stored the first user
    /// message, so the caller passes only what the run added beyond it.
    pub fn append_turn(&self, id: &str, turn: &[ChatMessage]) -> Result<(), String> {
        if turn.is_empty() {
            return Ok(());
        }

        let mut index = self.lock();
        let Some(existing) = index.messages.get(id) else {
            return Err(format!("Thread {id} was not found"));
        };
        let Some(thread) = index.threads.iter().find(|thread| thread.id == id).cloned() else {
            return Err(format!("Thread {id} was not found"));
        };

        let mut merged = existing.clone();
        merged.extend_from_slice(turn);
        persist(&self.dir, &thread, &merged)?;
        index.messages.insert(id.to_string(), merged);
        Ok(())
    }
}

/// Keeps only the messages a run produced, given that the store already holds
/// the history the run was started from.
///
/// The frontend replays the whole thread on every send, so the tail of the
/// incoming history is what is genuinely new. Matching on message id keeps this
/// correct even when the store was seeded by `create_thread`.
pub fn new_turn<'a>(history: &'a [ChatMessage], stored: &[ChatMessage]) -> &'a [ChatMessage] {
    if history.len() <= stored.len() {
        return &[];
    }
    &history[stored.len()..]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(text: &str) -> ChatMessage {
        ChatMessage::text("m", "user", text)
    }

    #[test]
    fn titles_come_from_the_first_message_and_fall_back() {
        assert_eq!(derive_title(&user("What's the weather in Berlin?")), "What's the weather in Berlin?");
        assert_eq!(
            derive_title(&user(&"a".repeat(100))),
            "a".repeat(TITLE_LIMIT),
        );
        assert_eq!(derive_title(&user("   ")), "New thread");
    }

    #[test]
    fn only_messages_beyond_the_stored_history_are_new() {
        let stored = vec![user("one")];
        let history = vec![user("one"), user("two")];
        assert_eq!(new_turn(&history, &stored).len(), 1);

        assert!(new_turn(&stored, &stored).is_empty());
        assert!(new_turn(&[], &stored).is_empty());
    }

    #[test]
    fn store_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("openui-store-{}", uuid::Uuid::new_v4()));
        let store = ThreadStore {
            dir: dir.clone(),
            index: Mutex::new(Index {
                threads: Vec::new(),
                messages: HashMap::new(),
            }),
        };

        let thread = store.create(user("hello there")).expect("create");
        assert_eq!(thread.title, "hello there");
        assert_eq!(store.messages(&thread.id).expect("messages").len(), 1);

        store
            .append_turn(
                &thread.id,
                &[ChatMessage::text("a1", "assistant", "hi")],
            )
            .expect("append");
        assert_eq!(store.messages(&thread.id).expect("messages").len(), 2);

        let reloaded = load_file(&dir, &thread.id).expect("persisted file");
        assert_eq!(reloaded.messages.len(), 2);
        assert_eq!(reloaded.thread.title, "hello there");

        store.update(StoredThread { title: "renamed".into(), ..thread.clone() }).expect("rename");
        assert_eq!(store.list(None).threads[0].title, "renamed");

        store.delete(&thread.id).expect("delete");
        assert!(store.list(None).threads.is_empty());
        assert!(load_file(&dir, &thread.id).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn listing_pages_without_repeating_entries() {
        let dir = std::env::temp_dir().join(format!("openui-page-{}", uuid::Uuid::new_v4()));
        let store = ThreadStore {
            dir: dir.clone(),
            index: Mutex::new(Index {
                threads: Vec::new(),
                messages: HashMap::new(),
            }),
        };
        for index in 0..(DEFAULT_PAGE + 2) {
            store.create(user(&format!("thread {index}"))).expect("create");
        }

        let first = store.list(None);
        assert_eq!(first.threads.len(), DEFAULT_PAGE);
        let expected_cursor = DEFAULT_PAGE.to_string();
        assert_eq!(first.next_cursor.as_deref(), Some(expected_cursor.as_str()));

        let second = store.list(first.next_cursor.as_deref());
        assert_eq!(second.threads.len(), 2);
        assert!(second.next_cursor.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
