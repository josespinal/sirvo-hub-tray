use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// Per-target Node binary name relative to resources/node/<os>-<arch>/.
fn node_bin_relative() -> &'static str {
    if cfg!(target_os = "windows") {
        "node.exe"
    } else {
        "bin/node"
    }
}

fn target_dir_name() -> String {
    let os = if cfg!(target_os = "windows") { "win32" } else if cfg!(target_os = "macos") { "darwin" } else { "linux" };
    let arch = if cfg!(target_arch = "x86_64") { "x64" } else if cfg!(target_arch = "aarch64") { "arm64" } else { "unknown" };
    format!("{os}-{arch}")
}

pub fn node_binary(app: &AppHandle) -> PathBuf {
    let base = app.path().resource_dir().expect("resource_dir");
    base.join("resources/node").join(target_dir_name()).join(node_bin_relative())
}

pub fn hub_dir(app: &AppHandle) -> PathBuf {
    let base = app.path().resource_dir().expect("resource_dir");
    base.join("resources/hub")
}

pub fn hub_entry(app: &AppHandle) -> PathBuf {
    hub_dir(app).join("dist/index.js")
}

pub fn hub_db_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_data_dir().expect("app_data_dir");
    std::fs::create_dir_all(&dir).ok();
    dir.join("hub.sqlite")
}

pub fn log_file_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_log_dir().expect("app_log_dir");
    std::fs::create_dir_all(&dir).ok();
    dir.join("hub.log")
}

/// Extra hub environment variables, editable by the operator (`hub_env_file`).
pub fn hub_env_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_data_dir().expect("app_data_dir");
    std::fs::create_dir_all(&dir).ok();
    dir.join("hub.env")
}
