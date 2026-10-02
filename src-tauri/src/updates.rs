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
    // Download only. Installing on Windows launches the MSI and exits this
    // process at once (`std::process::exit(0)` inside the updater), so the
    // hub must be stopped *before* `install`, or it is orphaned and keeps
    // the hub's ports until someone kills it.
    let bytes = match update.download(|_chunk, _total| {}, || log::info!("update downloaded; awaiting user confirmation")).await {
        Ok(bytes) => bytes,
        Err(e) => {
            log::warn!("update download failed: {e}");
            return;
        }
    };
    log::info!("update bytes downloaded: {}", bytes.len());

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
                    // Windows: runs the installer and exits. Elsewhere: swaps
                    // the bundle, then we relaunch.
                    if let Err(e) = update.install(&bytes) {
                        log::warn!("update install failed: {e}");
                        let _ = sup.start().await;
                        return;
                    }
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
