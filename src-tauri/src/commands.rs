use crate::log_buffer::LogBuffer;
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
    settings::save(&app, &settings).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cmd_get_log_snapshot(state: State<'_, AppState>) -> Vec<String> {
    state.log_buffer.snapshot()
}
