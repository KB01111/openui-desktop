//! Persisted user settings.
//!
//! One small JSON document in the app data directory. Writes go through a temp
//! file and a rename so a crash mid-write cannot truncate the stored settings.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::contract::{SettingsPatch, SettingsSnapshot};

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_MODEL: &str = "gpt-5.2";
const ENV_API_KEY: &str = "OPENAI_API_KEY";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSettings {
    base_url: String,
    model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
}

impl Default for StoredSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            api_key: None,
        }
    }
}

/// What the agent needs for one run, resolved after settings and env fallbacks.
#[derive(Debug, Clone)]
pub struct ResolvedSettings {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
}

pub fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| format!("Could not resolve the app data directory: {error}"))?;
    Ok(dir.join("settings.json"))
}

pub fn load(path: &Path) -> StoredSettings {
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => StoredSettings::default(),
    }
}

fn persist(path: &Path, settings: &StoredSettings) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "Settings path has no parent directory".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("Could not create the settings directory: {error}"))?;

    let serialized = serde_json::to_vec_pretty(settings)
        .map_err(|error| format!("Could not serialize settings: {error}"))?;
    write_atomic(path, &serialized)
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, bytes).map_err(|error| format!("Could not write {}: {error}", temp.display()))?;
    std::fs::rename(&temp, path).map_err(|error| format!("Could not replace {}: {error}", path.display()))
}

fn env_api_key() -> Option<String> {
    std::env::var(ENV_API_KEY)
        .ok()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
}

fn effective_key(settings: &StoredSettings) -> Option<String> {
    settings
        .api_key
        .as_ref()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
        .or_else(env_api_key)
}

pub fn snapshot(path: &Path) -> Result<SettingsSnapshot, String> {
    let settings = load(path);
    let api_key_set = effective_key(&settings).is_some();
    Ok(SettingsSnapshot {
        base_url: settings.base_url,
        model: settings.model,
        api_key_set,
    })
}

pub fn apply_patch(path: &Path, patch: SettingsPatch) -> Result<SettingsSnapshot, String> {
    let mut settings = load(path);

    if let Some(base_url) = patch.base_url {
        let trimmed = base_url.trim();
        if !trimmed.is_empty() {
            settings.base_url = trimmed.trim_end_matches('/').to_string();
        }
    }
    if let Some(model) = patch.model {
        let trimmed = model.trim();
        if !trimmed.is_empty() {
            settings.model = trimmed.to_string();
        }
    }
    if let Some(api_key) = patch.api_key {
        let trimmed = api_key.trim();
        if !trimmed.is_empty() {
            settings.api_key = Some(trimmed.to_string());
        }
    }

    persist(path, &settings)?;
    snapshot(path)
}

pub fn resolve(app: &AppHandle) -> Result<ResolvedSettings, String> {
    let path = settings_path(app)?;
    let settings = load(&path);
    let api_key = effective_key(&settings);
    Ok(ResolvedSettings {
        base_url: settings.base_url.trim_end_matches('/').to_string(),
        model: settings.model,
        api_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("openui-settings-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir.join("settings.json")
    }

    #[test]
    fn defaults_when_absent() {
        let path = temp_path("absent");
        let snapshot = snapshot(&path).expect("snapshot");
        assert_eq!(snapshot.base_url, DEFAULT_BASE_URL);
        assert_eq!(snapshot.model, DEFAULT_MODEL);
        assert_eq!(snapshot.api_key_set, env_api_key().is_some());
    }

    #[test]
    fn patch_never_returns_the_key_and_keeps_it_when_omitted() {
        let path = temp_path("patch");

        apply_patch(
            &path,
            SettingsPatch {
                api_key: Some("sk-test".into()),
                ..Default::default()
            },
        )
        .expect("first save");

        let snapshot = apply_patch(
            &path,
            SettingsPatch {
                model: Some("gpt-4o-mini".into()),
                ..Default::default()
            },
        )
        .expect("second save");

        assert_eq!(snapshot.model, "gpt-4o-mini");
        assert!(snapshot.api_key_set);
        assert!(!serde_json::to_string(&snapshot).expect("json").contains("sk-test"));
        assert_eq!(load(&path).api_key.as_deref(), Some("sk-test"));
    }

    #[test]
    fn blank_key_does_not_erase_an_existing_one() {
        let path = temp_path("blank");
        apply_patch(
            &path,
            SettingsPatch {
                api_key: Some("sk-keep".into()),
                ..Default::default()
            },
        )
        .expect("save");
        apply_patch(
            &path,
            SettingsPatch {
                api_key: Some("   ".into()),
                ..Default::default()
            },
        )
        .expect("blank save");
        assert_eq!(load(&path).api_key.as_deref(), Some("sk-keep"));
    }

    #[test]
    fn base_url_trailing_slash_is_normalized() {
        let path = temp_path("slash");
        let snapshot = apply_patch(
            &path,
            SettingsPatch {
                base_url: Some("http://127.0.0.1:11434/v1/".into()),
                ..Default::default()
            },
        )
        .expect("save");
        assert_eq!(snapshot.base_url, "http://127.0.0.1:11434/v1");
    }
}
