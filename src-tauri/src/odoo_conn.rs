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
/// The header the WAF in front of Odoo checks.
pub const WAF_HEADER: &str = "x-api-token";

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
    /// Sent as `x-api-token` on every request to Odoo (`ODOO_RPC_HEADERS`).
    pub waf_token: Option<String>,
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
    pub has_waf_token: bool,
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

/// The hub reads `ODOO_RPC_HEADERS` as `Key:Value,Key2:Value2`, so a comma
/// would split the token; whitespace and control characters can't go in a
/// header at all.
pub fn check_waf_token(token: &str) -> Result<(), ConnError> {
    if token.chars().any(|c| c == ',' || c.is_whitespace() || c.is_control()) {
        return Err(ConnError::new("invalidWafToken"));
    }
    Ok(())
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

// ─── Secret storage ─────────────────────────────────────────────────────────

/// What is kept secret per account: the password and the optional WAF token.
/// Stored as one JSON value, in the keychain or the fallback file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Secrets {
    password: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    waf_token: Option<String>,
}

impl Secrets {
    /// Tray 0.3.0–0.3.1 stored the bare password in the keychain.
    fn parse(raw: &str) -> Self {
        serde_json::from_str(raw).unwrap_or_else(|_| Self { password: raw.to_string(), waf_token: None })
    }
}

#[derive(Serialize, Deserialize)]
struct SecretFile {
    account: String,
    #[serde(flatten)]
    secrets: Secrets,
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

fn store_secrets(app: &AppHandle, conn: &OdooConn, secrets: &Secrets) -> Result<Storage, ConnError> {
    let path = secret_file_path(app);
    let value = serde_json::to_string(secrets).map_err(|e| ConnError::with("storeFailed", e))?;
    match keyring_entry(conn).and_then(|e| e.set_password(&value)) {
        Ok(()) => {
            // A fallback copy from an earlier save would outlive a password
            // change; the keychain is now the only copy.
            let _ = std::fs::remove_file(&path);
            Ok(Storage::Keychain)
        }
        Err(err) => {
            log::warn!("keychain unavailable ({err}); storing the Odoo password in {}", path.display());
            let body = serde_json::to_string(&SecretFile { account: account(conn), secrets: secrets.clone() })
                .map_err(|e| ConnError::with("storeFailed", e))?;
            write_secret_file(&path, &body).map_err(|e| ConnError::with("storeFailed", e))?;
            Ok(Storage::File)
        }
    }
}

/// The stored secrets for `conn`, and where they came from. A fallback file
/// written for another server, database or user is ignored.
fn load_secrets(app: &AppHandle, conn: &OdooConn) -> Option<(Secrets, Storage)> {
    match keyring_entry(conn).and_then(|e| e.get_password()) {
        Ok(raw) => return Some((Secrets::parse(&raw), Storage::Keychain)),
        Err(keyring::Error::NoEntry) => {}
        Err(err) => log::warn!("keychain read failed ({err}); trying the fallback file"),
    }
    let raw = std::fs::read_to_string(secret_file_path(app)).ok()?;
    let file: SecretFile = serde_json::from_str(&raw).ok()?;
    (file.account == account(conn)).then_some((file.secrets, Storage::File))
}

// ─── Odoo ───────────────────────────────────────────────────────────────────

async fn jsonrpc(
    client: &reqwest::Client,
    url: &str,
    waf_token: Option<&str>,
    service: &str,
    method: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, ConnError> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "call",
        "params": { "service": service, "method": method, "args": args },
        "id": 1,
    });
    let mut request = client.post(format!("{url}/jsonrpc")).json(&body);
    if let Some(token) = waf_token {
        request = request.header(WAF_HEADER, token);
    }
    let response = request
        .send()
        .await
        .map_err(|e| ConnError::with("unreachable", e))?;
    // Odoo itself answers RPC errors with 200; a 401/403 comes from the WAF
    // (or a proxy) in front of it.
    if matches!(response.status().as_u16(), 401 | 403) {
        let code = if waf_token.is_some() { "wafRejected" } else { "wafBlocked" };
        return Err(ConnError::with(code, format!("HTTP {}", response.status())));
    }
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
pub async fn test_login(conn: &OdooConn, password: &str, waf_token: Option<&str>) -> Result<bool, ConnError> {
    let client = reqwest::Client::builder()
        .timeout(ODOO_TIMEOUT)
        .build()
        .map_err(|e| ConnError::with("unreachable", e))?;
    let uid = jsonrpc(&client, &conn.url, waf_token, "common", "login", serde_json::json!([conn.db, conn.user, password])).await?;
    let Some(uid) = uid.as_i64().filter(|uid| *uid > 0) else {
        return Err(ConnError::new("badLogin"));
    };
    for group in [HUB_SERVICE_GROUP, ADMIN_GROUP] {
        let has = jsonrpc(
            &client,
            &conn.url,
            waf_token,
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
    let stored = conn.as_ref().and_then(|c| load_secrets(app, c));
    ConnView {
        has_password: stored.is_some(),
        has_waf_token: stored.as_ref().is_some_and(|(s, _)| s.waf_token.is_some()),
        storage: stored.map(|(_, storage)| storage),
        conn,
    }
}

/// What to do with the WAF token on save.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", content = "value", rename_all = "lowercase")]
pub enum WafTokenChange {
    /// Keep the stored token (or none).
    Keep,
    Set(String),
    Remove,
}

/// Test the connection, then save it. Nothing is stored unless Odoo accepted
/// the login. `password: None` keeps the stored one, and `WafTokenChange::Keep`
/// the stored token — only while the server, database and user are unchanged,
/// since secrets are stored per account.
pub async fn test_and_save(
    app: &AppHandle,
    url: &str,
    db: &str,
    user: &str,
    password: Option<String>,
    waf_token: WafTokenChange,
) -> Result<SaveOutcome, ConnError> {
    let conn = normalize(url, db, user)?;
    let previous = settings::load(app).odoo;
    let stored = load_secrets(app, &conn).map(|(s, _)| s);
    let password = match password.filter(|p| !p.is_empty()) {
        Some(p) => p,
        None => match &stored {
            Some(s) => s.password.clone(),
            None => return Err(ConnError::new("missingPassword")),
        },
    };
    check_password(&password)?;
    let waf_token = match waf_token {
        WafTokenChange::Keep => stored.and_then(|s| s.waf_token),
        WafTokenChange::Remove => None,
        WafTokenChange::Set(t) => Some(t.trim().to_string()).filter(|t| !t.is_empty()),
    };
    if let Some(token) = &waf_token {
        check_waf_token(token)?;
    }

    let hub_service_group = test_login(&conn, &password, waf_token.as_deref()).await?;
    let storage = store_secrets(app, &conn, &Secrets { password, waf_token })?;

    let mut s = settings::load(app);
    s.odoo = Some(conn.clone());
    settings::save(app, &s).map_err(|e| ConnError::with("storeFailed", e))?;

    // The old account's secrets would otherwise stay in the keychain.
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
    let (secrets, _) = load_secrets(app, &conn)?;
    Some(HubCredentials {
        url: conn.url,
        db: conn.db,
        user: conn.user,
        password: secrets.password,
        waf_token: secrets.waf_token,
    })
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
    fn rejects_waf_tokens_that_cannot_travel_in_odoo_rpc_headers() {
        assert!(check_waf_token("abc123_-.~+/=").is_ok());
        for bad in ["a,b", "a b", "a\nb"] {
            assert_eq!(check_waf_token(bad).unwrap_err().code, "invalidWafToken");
        }
    }

    #[test]
    fn reads_secrets_stored_by_older_trays_as_a_bare_password() {
        assert_eq!(Secrets::parse("hunter2-long"), Secrets { password: "hunter2-long".into(), waf_token: None });
        let json = r#"{"password":"p","waf_token":"t"}"#;
        assert_eq!(Secrets::parse(json), Secrets { password: "p".into(), waf_token: Some("t".into()) });
    }

    #[test]
    fn fallback_file_keeps_the_account_next_to_the_secrets() {
        let file = SecretFile { account: "a".into(), secrets: Secrets { password: "p".into(), waf_token: Some("t".into()) } };
        let json = serde_json::to_string(&file).unwrap();
        assert_eq!(json, r#"{"account":"a","password":"p","waf_token":"t"}"#);
        let back: SecretFile = serde_json::from_str(r#"{"account":"a","password":"p"}"#).unwrap();
        assert_eq!(back.secrets.waf_token, None);
    }

    #[test]
    fn account_identifies_server_db_and_user() {
        let a = OdooConn { url: "https://a".into(), db: "pos".into(), user: "hub".into() };
        let b = OdooConn { db: "other".into(), ..a.clone() };
        assert_ne!(account(&a), account(&b));
    }
}
