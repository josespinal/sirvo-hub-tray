use crate::commands::AppState;
use crate::i18n::{detect_locale, t, t_fmt};
use crate::settings;
use crate::status_client::HubStatus;
use crate::supervisor::HubState;
use parking_lot::Mutex;
use std::sync::Arc;
use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu},
    tray::{TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};

fn icon_for(state: HubState, app: &AppHandle) -> Option<Image<'static>> {
    let resource = app.path().resource_dir().ok()?;
    let file = match state {
        HubState::Running => "icons/tray-running.png",
        HubState::Errored | HubState::NeedsSetup | HubState::ConfigError => "icons/tray-error.png",
        _ => "icons/tray-stopped.png",
    };
    Image::from_path(resource.join(file)).ok()
}

#[derive(Clone)]
pub struct TrayController {
    app: AppHandle,
    lang: Arc<Mutex<String>>,
    last_status: Arc<Mutex<HubStatus>>,
    last_state: Arc<Mutex<HubState>>,
}

impl TrayController {
    pub fn new(app: AppHandle, initial_lang: String) -> Self {
        Self {
            app,
            lang: Arc::new(Mutex::new(initial_lang)),
            last_status: Arc::new(Mutex::new(HubStatus::default())),
            last_state: Arc::new(Mutex::new(HubState::Stopped)),
        }
    }

    pub fn set_language(&self, lang: String) {
        *self.lang.lock() = lang;
        let _ = self.rebuild_menu();
    }

    /// Called every poll (3s). Rebuild the menu only when something it shows
    /// changed: `uptime_sec` changes every time, and replacing the menu
    /// while it is open closes it or makes it flicker on Windows.
    pub fn update_status(&self, status: HubStatus) {
        let shown = |s: &HubStatus| (s.connected_terminals, s.lan_url.clone());
        let changed = {
            let mut last = self.last_status.lock();
            let changed = shown(&last) != shown(&status);
            *last = status;
            changed
        };
        if changed {
            let _ = self.rebuild_menu();
        }
    }

    pub fn update_state(&self, state: HubState) {
        let previous = std::mem::replace(&mut *self.last_state.lock(), state);
        // The hub can't run until the Odoo connection is fixed: put the form
        // in front of whoever is setting the machine up.
        if state != previous && matches!(state, HubState::NeedsSetup | HubState::ConfigError) {
            open_setup_window(&self.app, &self.lang.lock());
        }
        if let Some(tray) = self.app.tray_by_id("main") {
            if let Some(img) = icon_for(state, &self.app) {
                let _ = tray.set_icon(Some(img));
            }
        }
        let _ = self.rebuild_menu();
    }

    fn header_label(&self) -> String {
        let lang = self.lang.lock().clone();
        match *self.last_state.lock() {
            HubState::Running => t(&lang, "tray.running"),
            HubState::Stopped => t(&lang, "tray.stopped"),
            HubState::Starting => t(&lang, "tray.starting"),
            HubState::Restarting => t(&lang, "tray.restarting"),
            HubState::Errored => t(&lang, "tray.errored"),
            HubState::NeedsSetup => t(&lang, "tray.needsSetup"),
            HubState::ConfigError => t(&lang, "tray.configError"),
        }
    }

    fn rebuild_menu(&self) -> tauri::Result<()> {
        let lang = self.lang.lock().clone();
        let status = self.last_status.lock().clone();
        let state = *self.last_state.lock();

        let terminals_label = t_fmt(
            &lang,
            "tray.terminals",
            &[("count", &status.connected_terminals.unwrap_or(0).to_string())],
        );
        let lan_label = match status.lan_url.as_deref() {
            Some(url) => t_fmt(&lang, "tray.lanUrl", &[("url", url)]),
            None => t(&lang, "tray.lanUrlUnknown"),
        };

        let header   = MenuItem::with_id(&self.app, "header",    self.header_label(), false, None::<&str>)?;
        let terms    = MenuItem::with_id(&self.app, "terms",     terminals_label,    false, None::<&str>)?;
        let lan      = MenuItem::with_id(&self.app, "lan",       lan_label,          false, None::<&str>)?;
        let copy_lan = MenuItem::with_id(&self.app, "copy_lan",  t(&lang, "tray.copyLanUrl"), status.lan_url.is_some(), None::<&str>)?;
        let start    = MenuItem::with_id(&self.app, "start",     t(&lang, "tray.start"),    matches!(state, HubState::Stopped | HubState::Errored | HubState::NeedsSetup | HubState::ConfigError), None::<&str>)?;
        let restart  = MenuItem::with_id(&self.app, "restart",   t(&lang, "tray.restart"),  matches!(state, HubState::Running), None::<&str>)?;
        let stop     = MenuItem::with_id(&self.app, "stop",      t(&lang, "tray.stop"),     matches!(state, HubState::Running), None::<&str>)?;
        let odoo     = MenuItem::with_id(&self.app, "odoo",      t(&lang, "tray.odooConnection"), true, None::<&str>)?;
        let logs     = MenuItem::with_id(&self.app, "logs",      t(&lang, "tray.viewLogs"), true, None::<&str>)?;

        let settings_now = settings::load(&self.app);
        let autostart = CheckMenuItem::with_id(&self.app, "autostart", t(&lang, "tray.autostart"), true, settings_now.autostart, None::<&str>)?;

        let lang_auto = CheckMenuItem::with_id(&self.app, "lang_auto", t(&lang, "tray.languageAuto"), true, settings_now.language.is_none(), None::<&str>)?;
        let lang_en   = CheckMenuItem::with_id(&self.app, "lang_en",   t(&lang, "tray.languageEn"),   true, settings_now.language.as_deref() == Some("en"), None::<&str>)?;
        let lang_es   = CheckMenuItem::with_id(&self.app, "lang_es",   t(&lang, "tray.languageEs"),   true, settings_now.language.as_deref() == Some("es"), None::<&str>)?;
        let lang_sub  = Submenu::with_items(&self.app, t(&lang, "tray.language"), true, &[&lang_auto, &lang_en, &lang_es])?;

        let updates = MenuItem::with_id(&self.app, "check_updates", t(&lang, "tray.checkForUpdates"), true, None::<&str>)?;
        let quit    = MenuItem::with_id(&self.app, "quit", t(&lang, "tray.quit"), true, None::<&str>)?;

        let menu = Menu::with_items(
            &self.app,
            &[
                &header,
                &PredefinedMenuItem::separator(&self.app)?,
                &terms,
                &lan,
                &copy_lan,
                &PredefinedMenuItem::separator(&self.app)?,
                &start,
                &restart,
                &stop,
                &PredefinedMenuItem::separator(&self.app)?,
                &odoo,
                &logs,
                &autostart,
                &lang_sub,
                &updates,
                &PredefinedMenuItem::separator(&self.app)?,
                &quit,
            ],
        )?;

        if let Some(tray) = self.app.tray_by_id("main") {
            tray.set_menu(Some(menu))?;
        }
        Ok(())
    }
}

pub fn install(app: &AppHandle, initial_lang: String) -> tauri::Result<TrayController> {
    let controller = TrayController::new(app.clone(), initial_lang);
    let menu = Menu::with_items(app, &[&MenuItem::with_id(app, "boot", "Starting…", false, None::<&str>)?])?;

    let ctrl_for_events = controller.clone();
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().expect("default icon").clone())
        .menu(&menu)
        .on_menu_event(move |app, event| {
            handle_menu_event(app, event.id().as_ref(), &ctrl_for_events);
        })
        .on_tray_icon_event(|_app, event| {
            if let TrayIconEvent::Click { .. } = event {
                // No-op for v1; menu opens on right-click natively.
            }
        })
        .build(app)?;

    controller.rebuild_menu()?;
    Ok(controller)
}

fn handle_menu_event(app: &AppHandle, id: &str, ctrl: &TrayController) {
    let state: tauri::State<AppState> = app.state();
    match id {
        "start" => {
            let s = state.supervisor.clone();
            tauri::async_runtime::spawn(async move { let _ = s.start().await; });
        }
        "restart" => {
            let s = state.supervisor.clone();
            tauri::async_runtime::spawn(async move { let _ = s.restart().await; });
        }
        "stop" => {
            let lang = ctrl.lang.lock().clone();
            let app2 = app.clone();
            let s = state.supervisor.clone();
            app.dialog()
                .message(t(&lang, "dialog.stopBody"))
                .title(t(&lang, "dialog.stopTitle"))
                .buttons(MessageDialogButtons::OkCancelCustom(
                    t(&lang, "dialog.stopConfirm"),
                    t(&lang, "dialog.stopCancel"),
                ))
                .show(move |confirmed| {
                    if confirmed {
                        let _ = app2.run_on_main_thread(move || {
                            tauri::async_runtime::spawn(async move { let _ = s.stop().await; });
                        });
                    }
                });
        }
        "copy_lan" => {
            if let Some(url) = ctrl.last_status.lock().lan_url.clone() {
                let _ = app.clipboard().write_text(url);
            }
        }
        "odoo" => open_setup_window(app, &ctrl.lang.lock()),
        "logs" => {
            if let Some(win) = app.get_webview_window("logs") {
                let _ = win.show();
                let _ = win.set_focus();
            } else {
                let _ = WebviewWindowBuilder::new(app, "logs", WebviewUrl::default())
                    .title(t(&ctrl.lang.lock(), "logs.title"))
                    .inner_size(800.0, 500.0)
                    .build();
            }
        }
        "autostart" => {
            let app2 = app.clone();
            let mut s = settings::load(app);
            s.autostart = !s.autostart;
            let _ = settings::save(app, &s);
            tauri::async_runtime::spawn(async move {
                apply_autostart(&app2, s.autostart).await;
            });
            let _ = ctrl.rebuild_menu();
        }
        "lang_auto" => {
            let mut s = settings::load(app);
            s.language = None;
            let _ = settings::save(app, &s);
            ctrl.set_language(detect_locale().to_string());
        }
        "lang_en" => {
            let mut s = settings::load(app);
            s.language = Some("en".into());
            let _ = settings::save(app, &s);
            ctrl.set_language("en".into());
        }
        "lang_es" => {
            let mut s = settings::load(app);
            s.language = Some("es".into());
            let _ = settings::save(app, &s);
            ctrl.set_language("es".into());
        }
        "check_updates" => {
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                crate::updates::check_now(&app2).await;
            });
        }
        "quit" => {
            let s = state.supervisor.clone();
            let app2 = app.clone();
            tauri::async_runtime::spawn(async move {
                let _ = s.stop().await;
                app2.exit(0);
            });
        }
        _ => {}
    }
}

/// Show the "Connection to Odoo" form (`SetupWindow.tsx`), creating it on
/// first use.
pub fn open_setup_window(app: &AppHandle, lang: &str) {
    if let Some(win) = app.get_webview_window("setup") {
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "setup", WebviewUrl::default())
        .title(t(lang, "setup.title"))
        .inner_size(520.0, 600.0)
        .resizable(false)
        .build();
}

async fn apply_autostart(app: &AppHandle, enable: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let manager = app.autolaunch();
    if enable {
        let _ = manager.enable();
    } else {
        let _ = manager.disable();
    }
}
