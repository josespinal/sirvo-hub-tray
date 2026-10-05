//! `hub.env`: extra environment variables for the hub, kept in the tray's
//! data folder and read on every hub start.
//!
//! The tray starts the hub itself (`node dist/index.js`, no `--env-file`), so
//! this file is the only way to tune the hub's other settings (log level,
//! backups, plugins…) per machine. What the tray manages — ports, database
//! path, the Odoo connection and `FISCAL_PLUGIN` — always wins: the file can
//! never move the hub to another Odoo or turn fiscal finalization off.

use std::collections::HashMap;
use std::path::Path;

/// Written on first use. All comments, so it changes nothing until edited.
pub const TEMPLATE: &str = "\
# Sirvo hub settings — extra environment variables for the hub.
#
# One KEY=VALUE per line. Lines starting with # are ignored; there are no
# inline comments (everything after the first = is the value).
# Changes apply the next time the hub starts (tray menu → Restart).
#
# The tray sets these itself and ignores them here: HUB_PORT, HUB_HTTP_PORT,
# HUB_ADMIN_PORT, HUB_DB_PATH, ODOO_URL, ODOO_DB, ODOO_USER, ODOO_PASSWORD,
# ODOO_RPC_HEADERS, FISCAL_PLUGIN. Change the Odoo connection from the tray
# menu (Connection to Odoo…).
#
# Examples:
# HUB_LOG_LEVEL=debug
# HUB_ENABLED_PLUGINS=reservations
# LITESTREAM_REPLICA_URL=s3://your-bucket/hubs/restaurant-1
# AWS_ACCESS_KEY_ID=
# AWS_SECRET_ACCESS_KEY=
";

fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn unquote(value: &str) -> &str {
    for quote in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            return &value[1..value.len() - 1];
        }
    }
    value
}

/// Parse `KEY=VALUE` lines. Returns the variables in file order and one
/// warning per line that could not be used (never echoing its value).
pub fn parse(contents: &str) -> (Vec<(String, String)>, Vec<String>) {
    let mut vars = Vec::new();
    let mut warnings = Vec::new();
    for (index, raw) in contents.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").map(str::trim_start).unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            warnings.push(format!("hub.env line {}: not KEY=VALUE, ignored", index + 1));
            continue;
        };
        let key = key.trim();
        if !is_valid_key(key) {
            warnings.push(format!("hub.env line {}: invalid variable name, ignored", index + 1));
            continue;
        }
        vars.push((key.to_string(), unquote(value.trim()).to_string()));
    }
    (vars, warnings)
}

/// The file's variables plus the tray-managed ones, which win. A repeated key
/// keeps its last value. Warns once per managed key the file tried to set.
pub fn merge(
    file: Vec<(String, String)>,
    managed: &[(&'static str, String)],
) -> (Vec<(String, String)>, Vec<String>) {
    let mut warnings = Vec::new();
    let mut order: Vec<String> = Vec::new();
    let mut values: HashMap<String, String> = HashMap::new();
    for (key, value) in file {
        if managed.iter().any(|(k, _)| *k == key) {
            let warning = format!("hub.env: {key} is set by the tray, ignored");
            if !warnings.contains(&warning) {
                warnings.push(warning);
            }
            continue;
        }
        if !values.contains_key(&key) {
            order.push(key.clone());
        }
        values.insert(key, value);
    }
    let mut env: Vec<(String, String)> = order
        .into_iter()
        .map(|key| {
            let value = values.remove(&key).unwrap_or_default();
            (key, value)
        })
        .collect();
    env.extend(managed.iter().map(|(k, v)| (k.to_string(), v.clone())));
    (env, warnings)
}

/// Read and parse `path`. A missing file is no extra settings.
pub fn load(path: &Path) -> (Vec<(String, String)>, Vec<String>) {
    match std::fs::read_to_string(path) {
        Ok(contents) => parse(&contents),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), Vec::new()),
        Err(e) => (Vec::new(), vec![format!("hub.env could not be read ({e}), ignored")]),
    }
}

/// Create `path` with the commented template unless it already exists.
pub fn ensure_exists(path: &Path) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, TEMPLATE)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(vars: &[(String, String)]) -> Vec<&str> {
        vars.iter().map(|(k, _)| k.as_str()).collect()
    }

    fn value<'a>(vars: &'a [(String, String)], key: &str) -> Option<&'a str> {
        vars.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    #[test]
    fn parse_reads_key_values_and_skips_comments_and_blanks() {
        let (vars, warnings) = parse("# comment\n\nHUB_LOG_LEVEL=debug\n  HUB_MDNS_ENABLED = false  \n");
        assert_eq!(keys(&vars), vec!["HUB_LOG_LEVEL", "HUB_MDNS_ENABLED"]);
        assert_eq!(value(&vars, "HUB_MDNS_ENABLED"), Some("false"));
        assert!(warnings.is_empty());
    }

    #[test]
    fn parse_strips_matching_quotes_and_export() {
        let (vars, _) = parse("export A=\"x y\"\nB='z'\nC=\"unbalanced\n");
        assert_eq!(value(&vars, "A"), Some("x y"));
        assert_eq!(value(&vars, "B"), Some("z"));
        assert_eq!(value(&vars, "C"), Some("\"unbalanced"));
    }

    #[test]
    fn parse_keeps_everything_after_the_first_equals() {
        let (vars, _) = parse("LITESTREAM_REPLICA_URL=s3://b/hubs?x=1#frag\n");
        assert_eq!(value(&vars, "LITESTREAM_REPLICA_URL"), Some("s3://b/hubs?x=1#frag"));
    }

    #[test]
    fn parse_handles_windows_line_endings_and_empty_values() {
        let (vars, warnings) = parse("A=1\r\nB=\r\n");
        assert_eq!(value(&vars, "A"), Some("1"));
        assert_eq!(value(&vars, "B"), Some(""));
        assert!(warnings.is_empty());
    }

    #[test]
    fn parse_warns_about_lines_it_cannot_use() {
        let (vars, warnings) = parse("no equals here\n1BAD=x\n=x\nGOOD=1\n");
        assert_eq!(keys(&vars), vec!["GOOD"]);
        assert_eq!(warnings.len(), 3);
        assert!(warnings[0].contains("line 1"));
        assert!(warnings[1].contains("line 2"));
    }

    #[test]
    fn merge_lets_the_tray_managed_values_win() {
        let file = vec![
            ("FISCAL_PLUGIN".to_string(), "none".to_string()),
            ("ODOO_PASSWORD".to_string(), "from-file".to_string()),
            ("HUB_LOG_LEVEL".to_string(), "debug".to_string()),
        ];
        let managed = vec![
            ("FISCAL_PLUGIN", "dr-ncf".to_string()),
            ("ODOO_PASSWORD", "from-keychain".to_string()),
        ];
        let (env, warnings) = merge(file, &managed);
        assert_eq!(value(&env, "FISCAL_PLUGIN"), Some("dr-ncf"));
        assert_eq!(value(&env, "ODOO_PASSWORD"), Some("from-keychain"));
        assert_eq!(value(&env, "HUB_LOG_LEVEL"), Some("debug"));
        assert_eq!(env.iter().filter(|(k, _)| k == "FISCAL_PLUGIN").count(), 1);
        assert_eq!(warnings.len(), 2);
        assert!(warnings.iter().any(|w| w.contains("FISCAL_PLUGIN")));
        // Never echo a value from the file: it may be a secret.
        assert!(warnings.iter().all(|w| !w.contains("from-file")));
    }

    #[test]
    fn merge_keeps_the_last_value_of_a_repeated_key() {
        let file = vec![
            ("HUB_LOG_LEVEL".to_string(), "info".to_string()),
            ("HUB_LOG_LEVEL".to_string(), "debug".to_string()),
        ];
        let (env, _) = merge(file, &[]);
        assert_eq!(env, vec![("HUB_LOG_LEVEL".to_string(), "debug".to_string())]);
    }

    #[test]
    fn load_treats_a_missing_file_as_empty() {
        let path = std::env::temp_dir().join(format!("hub-env-missing-{}.env", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let (vars, warnings) = load(&path);
        assert!(vars.is_empty());
        assert!(warnings.is_empty());
    }

    #[test]
    fn ensure_exists_writes_the_template_once_and_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("hub-env-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("hub.env");
        ensure_exists(&path).unwrap();
        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(written, TEMPLATE);
        // The template is all comments: it changes nothing until edited.
        assert!(parse(&written).0.is_empty());
        std::fs::write(&path, "HUB_LOG_LEVEL=debug\n").unwrap();
        ensure_exists(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "HUB_LOG_LEVEL=debug\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
