mod commands;
mod i18n;
mod log_buffer;
mod paths;
mod settings;
mod status_client;
mod supervisor;
mod tray;
mod updates;

use crate::commands::AppState;
use crate::log_buffer::LogBuffer;
use crate::status_client::StatusClient;
use crate::supervisor::{Supervisor, SupervisorConfig};
use std::sync::Arc;
use tauri::{Listener, Manager};

const ADMIN_PORT: u16 = 8767;
const WS_PORT: u16 = 8765;
const HTTP_PORT: u16 = 8766;

pub fn run() {
    env_logger::init();
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::cmd_start,
            commands::cmd_stop,
            commands::cmd_restart,
            commands::cmd_get_state,
            commands::cmd_get_status,
            commands::cmd_get_settings,
            commands::cmd_save_settings,
            commands::cmd_get_log_snapshot,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // Resolve initial language
            let saved = settings::load(&handle);
            let lang = saved
                .language
                .clone()
                .unwrap_or_else(|| i18n::detect_locale().to_string());

            // Build supervisor
            let log_buffer = LogBuffer::new(5000);
            let cfg = SupervisorConfig {
                node_binary: paths::node_binary(&handle),
                hub_entry: paths::hub_entry(&handle),
                hub_dir: paths::hub_dir(&handle),
                db_path: paths::hub_db_path(&handle),
                log_file: paths::log_file_path(&handle),
                admin_port: ADMIN_PORT,
                ws_port: WS_PORT,
                http_port: HTTP_PORT,
            };
            let supervisor = Arc::new(Supervisor::new(handle.clone(), cfg, log_buffer.clone()));
            let status_client = StatusClient::new(handle.clone(), ADMIN_PORT);

            handle.manage(AppState {
                supervisor: supervisor.clone(),
                status_client: status_client.clone(),
                log_buffer,
            });

            // Install tray
            let controller = tray::install(&handle, lang.clone())?;

            // Forward hub-state events into the tray controller
            {
                let c = controller.clone();
                handle.listen("hub-state", move |event| {
                    if let Ok(state) =
                        serde_json::from_str::<supervisor::HubState>(event.payload())
                    {
                        c.update_state(state);
                    }
                });
            }
            // Forward hub-status events into the tray controller
            {
                let c = controller.clone();
                handle.listen("hub-status", move |event| {
                    if let Ok(status) =
                        serde_json::from_str::<status_client::HubStatus>(event.payload())
                    {
                        c.update_status(status);
                    }
                });
            }

            // Start supervisor + status polling
            let sup = supervisor.clone();
            tauri::async_runtime::spawn(async move {
                let _ = sup.start().await;
            });
            let sc = status_client.clone();
            tauri::async_runtime::spawn(async move {
                sc.start_polling().await;
            });

            updates::schedule_background(handle.clone());

            // Apply autostart preference
            {
                use tauri_plugin_autostart::ManagerExt;
                let mgr = handle.autolaunch();
                if saved.autostart {
                    let _ = mgr.enable();
                } else {
                    let _ = mgr.disable();
                }
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
