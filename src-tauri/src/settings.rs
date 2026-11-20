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
pub const DEFAULT_TEMPERATURE: f64 = 0.7;
const MAX_TOKENS_LIMIT: u32 = 1_000_000;
const ENV_API_KEY: &str = "OPENAI_API_KEY";

fn default_temperature() -> f64 {
    DEFAULT_TEMPERATURE
}

fn default_weather_tool() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSettings {
    base_url: String,
    model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
    #[serde(default = "default_temperature")]
    temperature: f64,
    #[serde(default)]
    max_tokens: Option<u32>,
    #[serde(default)]
    instructions: Option<String>,
    #[serde(default = "default_weather_tool")]
    weather_tool: bool,
}

impl Default for StoredSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            api_key: None,
            temperature: DEFAULT_TEMPERATURE,
            max_tokens: None,
            instructions: None,
            weather_tool: true,
        }
    }
}

/// What the agent needs for one run, resolved after settings and env fallbacks.
#[derive(Debug, Clone)]
pub struct ResolvedSettings {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub temperature: f64,
    pub max_tokens: Option<u32>,
    pub instructions: Option<String>,
    pub weather_tool: bool,
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
    std::fs::write(&temp, bytes)
        .map_err(|error| format!("Could not write {}: {error}", temp.display()))?;
    std::fs::rename(&temp, path)
        .map_err(|error| format!("Could not replace {}: {error}", path.display()))
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
        temperature: settings.temperature.clamp(0.0, 2.0),
        max_tokens: settings.max_tokens,
        instructions: settings.instructions,
        weather_tool: settings.weather_tool,
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
    if patch.remove_api_key == Some(true) {
        settings.api_key = None;
    } else if let Some(api_key) = patch.api_key {
        let trimmed = api_key.trim();
        if !trimmed.is_empty() {
            settings.api_key = Some(trimmed.to_string());
        }
    }
    if let Some(temperature) = patch.temperature {
        settings.temperature = temperature.clamp(0.0, 2.0);
    }
    if let Some(max_tokens) = patch.max_tokens {
        settings.max_tokens = if max_tokens == 0 {
            None
        } else {
            Some(max_tokens.min(MAX_TOKENS_LIMIT))
        };
    }
    if let Some(instructions) = patch.instructions {
        let trimmed = instructions.trim();
        settings.instructions = (!trimmed.is_empty()).then(|| trimmed.to_string());
    }
    if let Some(weather_tool) = patch.weather_tool {
        settings.weather_tool = weather_tool;
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
        temperature: settings.temperature.clamp(0.0, 2.0),
        max_tokens: settings.max_tokens,
        instructions: settings
            .instructions
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty()),
        weather_tool: settings.weather_tool,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("helios-settings-{name}-{}", uuid::Uuid::new_v4()));
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
        assert_eq!(snapshot.temperature, DEFAULT_TEMPERATURE);
        assert_eq!(snapshot.max_tokens, None);
        assert_eq!(snapshot.instructions, None);
        assert!(snapshot.weather_tool);
    }

    #[test]
    fn behavior_settings_persist_and_can_be_cleared() {
        let path = temp_path("behavior");
        let snapshot = apply_patch(
            &path,
            SettingsPatch {
                temperature: Some(1.4),
                max_tokens: Some(2_048),
                instructions: Some("  Be terse.  ".into()),
                weather_tool: Some(false),
                ..Default::default()
            },
        )
        .expect("save");

        assert_eq!(snapshot.temperature, 1.4);
        assert_eq!(snapshot.max_tokens, Some(2_048));
        assert_eq!(snapshot.instructions.as_deref(), Some("Be terse."));
        assert!(!snapshot.weather_tool);

        let clamped = apply_patch(
            &path,
            SettingsPatch {
                temperature: Some(9.0),
                ..Default::default()
            },
        )
        .expect("clamp");
        assert_eq!(clamped.temperature, 2.0);

        let cleared = apply_patch(
            &path,
            SettingsPatch {
                max_tokens: Some(0),
                instructions: Some("   ".into()),
                ..Default::default()
            },
        )
        .expect("clear");
        assert_eq!(cleared.max_tokens, None);
        assert_eq!(cleared.instructions, None);
    }

    #[test]
    fn older_settings_files_gain_default_behavior_fields() {
        let path = temp_path("legacy");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(
            &path,
            r#"{"baseUrl":"http://localhost:11434/v1","model":"llama3"}"#,
        )
        .expect("legacy settings");

        let snapshot = snapshot(&path).expect("snapshot");
        assert_eq!(snapshot.temperature, DEFAULT_TEMPERATURE);
        assert!(snapshot.max_tokens.is_none());
        assert!(snapshot.instructions.is_none());
        assert!(snapshot.weather_tool);
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
        assert!(!serde_json::to_string(&snapshot)
            .expect("json")
            .contains("sk-test"));
        assert_eq!(load(&path).api_key.as_deref(), Some("sk-test"));
    }

    #[test]
    fn an_explicit_flag_removes_the_key() {
        let path = temp_path("remove");
        apply_patch(
            &path,
            SettingsPatch {
                api_key: Some("sk-live".into()),
                ..Default::default()
            },
        )
        .expect("save");

        let snapshot = apply_patch(
            &path,
            SettingsPatch {
                remove_api_key: Some(true),
                ..Default::default()
            },
        )
        .expect("remove");
        assert!(!snapshot.api_key_set);
        assert_eq!(load(&path).api_key, None);
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
