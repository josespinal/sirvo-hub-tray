use crate::commands::AppState;
use crate::i18n::{detect_locale, t};
use crate::settings;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_updater::UpdaterExt;

pub async fn check_now(app: &AppHandle) {
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            log::warn!("updater unavailable: {e}");
            return;
        }
    };
    let update = match updater.check().await {
        Ok(Some(u)) => u,
        Ok(None) => {
            log::info!("no update available");
            return;
        }
        Err(e) => {
            log::warn!("update check failed: {e}");
            return;
        }
    };

    log::info!("downloading update {} silently", update.version);
    let mut downloaded: u64 = 0;
    if let Err(e) = update
        .download_and_install(
            |chunk_len, _content_len| {
                downloaded += chunk_len as u64;
            },
            || log::info!("update downloaded; awaiting user confirmation"),
        )
        .await
    {
        log::warn!("update download failed: {e}");
        return;
    }
    log::info!("update bytes downloaded: {downloaded}");

    // After install, Tauri replaces the binary on next launch. We need to
    // (1) stop the hub child, (2) ask the user, (3) restart the app.
    let lang = settings::load(app)
        .language
        .unwrap_or_else(|| detect_locale().to_string());
    let app2 = app.clone();
    app.dialog()
        .message(t(&lang, "dialog.updateReady"))
        .buttons(MessageDialogButtons::OkCancelCustom(
            t(&lang, "dialog.updateRestart"),
            t(&lang, "dialog.updateLater"),
        ))
        .show(move |confirmed| {
            if confirmed {
                let state: tauri::State<AppState> = app2.state();
                let sup = state.supervisor.clone();
                let a = app2.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = sup.stop().await;
                    a.restart();
                });
            }
        });
}

/// Schedule a periodic background update check (every 6 hours).
pub fn schedule_background(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Initial delay so app startup isn't blocked.
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        loop {
            check_now(&app).await;
            tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
        }
    });
}
