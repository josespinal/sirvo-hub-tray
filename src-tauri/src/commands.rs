use crate::log_buffer::LogBuffer;
use crate::odoo_conn::{self, ConnError, ConnView, SaveOutcome};
use crate::settings::{self, Settings};
use crate::status_client::{HubStatus, StatusClient};
use crate::supervisor::{HubState, Supervisor};
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

pub struct AppState {
    pub supervisor: Arc<Supervisor>,
    pub status_client: StatusClient,
    pub log_buffer: LogBuffer,
}

#[tauri::command]
pub async fn cmd_start(state: State<'_, AppState>) -> Result<(), String> {
    state.supervisor.start().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_stop(state: State<'_, AppState>) -> Result<(), String> {
    state.supervisor.stop().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cmd_restart(state: State<'_, AppState>) -> Result<(), String> {
    state.supervisor.restart().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cmd_get_state(state: State<'_, AppState>) -> HubState {
    state.supervisor.state()
}

#[tauri::command]
pub fn cmd_get_status(state: State<'_, AppState>) -> HubStatus {
    state.status_client.last()
}

#[tauri::command]
pub fn cmd_get_settings(app: AppHandle) -> Settings {
    settings::load(&app)
}

#[tauri::command]
pub fn cmd_save_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    // The Odoo connection is only changed through cmd_save_odoo_conn (it has
    // to be tested first); a caller that doesn't send it must not erase it.
    let odoo = settings::load(&app).odoo;
    settings::save(&app, &Settings { odoo, ..settings }).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cmd_get_odoo_conn(app: AppHandle) -> ConnView {
    odoo_conn::view(&app)
}

/// Test the connection against Odoo, save it, then (re)start the hub with it.
/// `password: None` or empty keeps the stored password.
#[tauri::command]
pub async fn cmd_save_odoo_conn(
    app: AppHandle,
    state: State<'_, AppState>,
    url: String,
    db: String,
    user: String,
    password: Option<String>,
) -> Result<SaveOutcome, ConnError> {
    let outcome = odoo_conn::test_and_save(&app, &url, &db, &user, password).await?;
    let supervisor = state.supervisor.clone();
    let restart = matches!(supervisor.state(), HubState::Running | HubState::Starting);
    tauri::async_runtime::spawn(async move {
        let _ = if restart { supervisor.restart().await } else { supervisor.start().await };
    });
    Ok(outcome)
}

/// The hub's last `hub_config_error` line, shown on the form after the hub
/// refused its settings (exit code 78). None when there is none.
#[tauri::command]
pub fn cmd_get_config_error(state: State<'_, AppState>) -> Option<String> {
    if state.supervisor.state() != HubState::ConfigError {
        return None;
    }
    state
        .log_buffer
        .snapshot()
        .into_iter()
        .rev()
        .find(|line| line.contains("hub_config_error") && !line.contains("Hub not started"))
        .map(|line| config_error_message(&line))
}

/// The hub logs pino JSON, captured as `[out] {...}`; keep just its `msg`.
fn config_error_message(line: &str) -> String {
    let json = line.split_once("] ").map_or(line, |(_, rest)| rest);
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| v.get("msg").and_then(|m| m.as_str()).map(str::to_string))
        .unwrap_or_else(|| json.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn extracts_the_hub_message() {
        let line = r#"[out] {"level":60,"event":"hub_config_error","msg":"ODOO_USER is empty."}"#;
        assert_eq!(super::config_error_message(line), "ODOO_USER is empty.");
        assert_eq!(super::config_error_message("[err] not json"), "not json");
    }
}

#[tauri::command]
pub fn cmd_get_log_snapshot(state: State<'_, AppState>) -> Vec<String> {
    state.log_buffer.snapshot()
}
