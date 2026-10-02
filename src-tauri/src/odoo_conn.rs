//! The hub's Odoo connection: checking it, and keeping the password out of
//! plain files. See docs/spec_odoo_credentials.md.
//!
//! URL, database and user live in `settings.json`. The password goes to the
//! OS keychain (Windows Credential Manager, macOS Keychain, Linux Secret
//! Service). When the keychain can't be used — typically a Linux machine with
//! no unlocked Secret Service — it falls back to `odoo-secret` in the app
//! config folder, readable only by this OS user, and says so in the UI.

use crate::settings::{self, OdooConn};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager};

const KEYRING_SERVICE: &str = "sirvo-hub-tray";
const SECRET_FILE: &str = "odoo-secret";
/// Same rule as the hub's `validateConfig` (nu_pos_hub/src/config.ts): the
/// hub exits with code 78 on these, so refuse them before saving.
const WELL_KNOWN_PASSWORDS: &[&str] = &["admin"];
/// Offline logins need the hub's user in this group (or to be an admin).
const HUB_SERVICE_GROUP: &str = "nu_restaurant_pos.group_pos_hub_service";
const ADMIN_GROUP: &str = "base.group_system";
const ODOO_TIMEOUT: Duration = Duration::from_secs(10);

/// Where the password ended up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Storage {
    Keychain,
    File,
}

/// Everything the hub process needs.
pub struct HubCredentials {
    pub url: String,
    pub db: String,
    pub user: String,
    pub password: String,
}

/// A failure the setup form shows. `code` maps to `setup.errors.<code>` in
/// i18n; `detail` is the raw cause, shown under it when present.
#[derive(Debug, Clone, Serialize)]
pub struct ConnError {
    pub code: &'static str,
    pub detail: Option<String>,
}

impl ConnError {
    fn new(code: &'static str) -> Self {
        Self { code, detail: None }
    }
    fn with(code: &'static str, detail: impl ToString) -> Self {
        Self { code, detail: Some(detail.to_string()) }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveOutcome {
    pub storage: Storage,
    /// False = the hub runs, but without offline logins.
    pub hub_service_group: bool,
}

/// What the setup form shows when it opens. The password itself never
/// leaves the Rust side.
#[derive(Debug, Clone, Serialize)]
pub struct ConnView {
    pub conn: Option<OdooConn>,
    pub has_password: bool,
    pub storage: Option<Storage>,
}

fn account(conn: &OdooConn) -> String {
    format!("{}|{}|{}", conn.url, conn.db, conn.user)
}

// ─── Input ──────────────────────────────────────────────────────────────────

/// Trim and check the form's fields. The URL loses any trailing slash so the
/// keychain account and the hub's `ODOO_URL` are the same for the same server.
pub fn normalize(url: &str, db: &str, user: &str) -> Result<OdooConn, ConnError> {
    let url = url.trim().trim_end_matches('/').to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) || url.len() <= "https://".len() {
        return Err(ConnError::new("invalidUrl"));
    }
    let db = db.trim().to_string();
    if db.is_empty() {
        return Err(ConnError::new("missingDb"));
    }
    let user = user.trim().to_string();
    if user.is_empty() {
        return Err(ConnError::new("missingUser"));
    }
    Ok(OdooConn { url, db, user })
}

pub fn check_password(password: &str) -> Result<(), ConnError> {
    if password.is_empty() {
        return Err(ConnError::new("missingPassword"));
    }
    if WELL_KNOWN_PASSWORDS.contains(&password.to_lowercase().as_str()) {
        return Err(ConnError::new("defaultPassword"));
    }
    Ok(())
}

// ─── Password storage ───────────────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
struct SecretFile {
    account: String,
    password: String,
}

fn secret_file_path(app: &AppHandle) -> PathBuf {
    let dir = app.path().app_config_dir().expect("app_config_dir");
    std::fs::create_dir_all(&dir).ok();
    dir.join(SECRET_FILE)
}

fn keyring_entry(conn: &OdooConn) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, &account(conn))
}

fn write_secret_file(path: &PathBuf, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    // `mode` only applies when the file is created; tighten an existing one.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(contents.as_bytes())
}

fn store_password(app: &AppHandle, conn: &OdooConn, password: &str) -> Result<Storage, ConnError> {
    let path = secret_file_path(app);
    match keyring_entry(conn).and_then(|e| e.set_password(password)) {
        Ok(()) => {
            // A fallback copy from an earlier save would outlive a password
            // change; the keychain is now the only copy.
            let _ = std::fs::remove_file(&path);
            Ok(Storage::Keychain)
        }
        Err(err) => {
            log::warn!("keychain unavailable ({err}); storing the Odoo password in {}", path.display());
            let body = serde_json::to_string(&SecretFile { account: account(conn), password: password.to_string() })
                .map_err(|e| ConnError::with("storeFailed", e))?;
            write_secret_file(&path, &body).map_err(|e| ConnError::with("storeFailed", e))?;
            Ok(Storage::File)
        }
    }
}

/// The stored password for `conn`, and where it came from. A fallback file
/// written for another server, database or user is ignored.
fn load_password(app: &AppHandle, conn: &OdooConn) -> Option<(String, Storage)> {
    match keyring_entry(conn).and_then(|e| e.get_password()) {
        Ok(password) => return Some((password, Storage::Keychain)),
        Err(keyring::Error::NoEntry) => {}
        Err(err) => log::warn!("keychain read failed ({err}); trying the fallback file"),
    }
    let raw = std::fs::read_to_string(secret_file_path(app)).ok()?;
    let file: SecretFile = serde_json::from_str(&raw).ok()?;
    (file.account == account(conn)).then_some((file.password, Storage::File))
}

// ─── Odoo ───────────────────────────────────────────────────────────────────

async fn jsonrpc(client: &reqwest::Client, url: &str, service: &str, method: &str, args: serde_json::Value) -> Result<serde_json::Value, ConnError> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "call",
        "params": { "service": service, "method": method, "args": args },
        "id": 1,
    });
    let response = client
        .post(format!("{url}/jsonrpc"))
        .json(&body)
        .send()
        .await
        .map_err(|e| ConnError::with("unreachable", e))?;
    if !response.status().is_success() {
        return Err(ConnError::with("unreachable", format!("HTTP {}", response.status())));
    }
    let data: serde_json::Value = response.json().await.map_err(|e| ConnError::with("notOdoo", e))?;
    if let Some(error) = data.get("error") {
        let message = error
            .pointer("/data/message")
            .or_else(|| error.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("unknown error");
        return Err(ConnError::with("odooError", message));
    }
    Ok(data.get("result").cloned().unwrap_or(serde_json::Value::Null))
}

/// Log in the way the hub does (JSON-RPC, user + password) and check the
/// offline-login group. Returns whether the user can read the offline-login
/// data (POS Hub Service or admin).
pub async fn test_login(conn: &OdooConn, password: &str) -> Result<bool, ConnError> {
    let client = reqwest::Client::builder()
        .timeout(ODOO_TIMEOUT)
        .build()
        .map_err(|e| ConnError::with("unreachable", e))?;
    let uid = jsonrpc(&client, &conn.url, "common", "login", serde_json::json!([conn.db, conn.user, password])).await?;
    let Some(uid) = uid.as_i64().filter(|uid| *uid > 0) else {
        return Err(ConnError::new("badLogin"));
    };
    for group in [HUB_SERVICE_GROUP, ADMIN_GROUP] {
        let has = jsonrpc(
            &client,
            &conn.url,
            "object",
            "execute_kw",
            serde_json::json!([conn.db, uid, password, "res.users", "has_group", [group]]),
        )
        .await?;
        if has.as_bool() == Some(true) {
            return Ok(true);
        }
    }
    Ok(false)
}

// ─── Public API ─────────────────────────────────────────────────────────────

pub fn view(app: &AppHandle) -> ConnView {
    let conn = settings::load(app).odoo;
    let stored = conn.as_ref().and_then(|c| load_password(app, c));
    ConnView {
        has_password: stored.is_some(),
        storage: stored.map(|(_, storage)| storage),
        conn,
    }
}

/// Test the connection, then save it. Nothing is stored unless Odoo accepted
/// the login. `password: None` keeps the stored one — only allowed while the
/// server, database and user are unchanged, since it is stored per account.
pub async fn test_and_save(app: &AppHandle, url: &str, db: &str, user: &str, password: Option<String>) -> Result<SaveOutcome, ConnError> {
    let conn = normalize(url, db, user)?;
    let previous = settings::load(app).odoo;
    let password = match password.filter(|p| !p.is_empty()) {
        Some(p) => p,
        None => match load_password(app, &conn) {
            Some((p, _)) => p,
            None => return Err(ConnError::new("missingPassword")),
        },
    };
    check_password(&password)?;

    let hub_service_group = test_login(&conn, &password).await?;
    let storage = store_password(app, &conn, &password)?;

    let mut s = settings::load(app);
    s.odoo = Some(conn.clone());
    settings::save(app, &s).map_err(|e| ConnError::with("storeFailed", e))?;

    // The old account's password would otherwise stay in the keychain.
    if let Some(old) = previous.filter(|old| *old != conn) {
        if let Ok(entry) = keyring_entry(&old) {
            let _ = entry.delete_credential();
        }
    }
    Ok(SaveOutcome { storage, hub_service_group })
}

/// What the supervisor passes to the hub. None = not set up (or the password
/// is gone from the keychain): the hub must not start.
pub fn load_for_hub(app: &AppHandle) -> Option<HubCredentials> {
    let conn = settings::load(app).odoo?;
    let (password, _) = load_password(app, &conn)?;
    Some(HubCredentials { url: conn.url, db: conn.db, user: conn.user, password })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_the_form_fields() {
        let conn = normalize("  https://erp.example.com/// ", " pos ", " hub-barilo ").unwrap();
        assert_eq!(conn, OdooConn { url: "https://erp.example.com".into(), db: "pos".into(), user: "hub-barilo".into() });
    }

    #[test]
    fn rejects_bad_fields() {
        assert_eq!(normalize("erp.example.com", "pos", "u").unwrap_err().code, "invalidUrl");
        assert_eq!(normalize("https://", "pos", "u").unwrap_err().code, "invalidUrl");
        assert_eq!(normalize("http://odoo:8069", " ", "u").unwrap_err().code, "missingDb");
        assert_eq!(normalize("http://odoo:8069", "pos", "").unwrap_err().code, "missingUser");
    }

    #[test]
    fn refuses_the_passwords_the_hub_refuses() {
        assert_eq!(check_password("").unwrap_err().code, "missingPassword");
        assert_eq!(check_password("admin").unwrap_err().code, "defaultPassword");
        assert_eq!(check_password("ADMIN").unwrap_err().code, "defaultPassword");
        assert!(check_password("a-long-random-password").is_ok());
    }

    #[test]
    fn account_identifies_server_db_and_user() {
        let a = OdooConn { url: "https://a".into(), db: "pos".into(), user: "hub".into() };
        let b = OdooConn { db: "other".into(), ..a.clone() };
        assert_ne!(account(&a), account(&b));
    }
}
