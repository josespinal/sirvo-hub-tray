use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub autostart: bool,
    pub language: Option<String>, // None = follow OS locale
}

impl Default for Settings {
    fn default() -> Self {
        Self { autostart: true, language: None }
    }
}

pub fn settings_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_config_dir().expect("app_config_dir");
    std::fs::create_dir_all(&dir).ok();
    dir.join("settings.json")
}

pub fn load(app: &AppHandle) -> Settings {
    let path = settings_path(app);
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<()> {
    let path = settings_path(app);
    std::fs::write(path, serde_json::to_string_pretty(settings)?)?;
    Ok(())
}
