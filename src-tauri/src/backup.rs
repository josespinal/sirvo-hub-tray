//! Litestream backups of the hub's SQLite database, the tray's version of
//! nu_pos_hub/scripts/start-hub.sh.
//!
//! When `hub.env` sets `LITESTREAM_REPLICA_URL`, the supervisor runs the hub
//! as `litestream replicate -exec "<node> <dist/index.js>"`. It tells the hub
//! which case applies through `HUB_BACKUP_MODE`, which the hub reports on
//! `GET /health` as `backup.mode` and the tray menu shows.
//!
//! The hub always starts. A backup problem must never keep a restaurant from
//! taking orders; it is reported instead.

use std::path::{Path, PathBuf};

/// Backups running.
pub const MODE_LITESTREAM: &str = "litestream";
/// No destination configured.
pub const MODE_NONE: &str = "none";
/// Destination configured, but Litestream is missing or failed to run.
pub const MODE_UNAVAILABLE: &str = "unavailable";
/// The database was missing and could not be restored: the hub runs WITHOUT
/// backups so it cannot publish its new, empty database over the good copy.
pub const MODE_RESTORE_FAILED: &str = "restore_failed";

pub const REPLICA_URL_VAR: &str = "LITESTREAM_REPLICA_URL";
pub const BACKUP_MODE_VAR: &str = "HUB_BACKUP_MODE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Run the hub directly, reporting this mode.
    Direct(&'static str),
    /// No database yet: restore it from the backup, then replicate.
    RestoreThenReplicate,
    Replicate,
}

pub struct Facts {
    pub replica_configured: bool,
    pub litestream_present: bool,
    pub restore_failed_marker: bool,
    /// Litestream itself failed earlier in this tray session.
    pub litestream_failed: bool,
    pub db_exists: bool,
}

pub fn plan(f: &Facts) -> Plan {
    if !f.replica_configured {
        Plan::Direct(MODE_NONE)
    } else if !f.litestream_present {
        Plan::Direct(MODE_UNAVAILABLE)
    } else if f.restore_failed_marker {
        Plan::Direct(MODE_RESTORE_FAILED)
    } else if f.litestream_failed {
        Plan::Direct(MODE_UNAVAILABLE)
    } else if !f.db_exists {
        Plan::RestoreThenReplicate
    } else {
        Plan::Replicate
    }
}

/// The replica URL from the hub's environment (last value wins, as when it is
/// applied); an empty value means none.
pub fn replica_url(env: &[(String, String)]) -> Option<&str> {
    env.iter()
        .rev()
        .find(|(k, _)| k == REPLICA_URL_VAR)
        .map(|(_, v)| v.trim())
        .filter(|v| !v.is_empty())
}

/// Set when a restore failed. The hub then created a fresh, empty database,
/// and replicating that would publish it over the good backup, so while this
/// file exists backups stay off, across restarts, until an operator restores
/// by hand and deletes it.
pub fn restore_failed_marker(db_path: &Path) -> PathBuf {
    let mut p = db_path.as_os_str().to_owned();
    p.push(".restore-failed");
    PathBuf::from(p)
}

pub fn write_restore_failed_marker(db_path: &Path) -> std::io::Result<()> {
    let stamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    std::fs::write(restore_failed_marker(db_path), format!("{stamp}\n"))
}

/// What to do to recover from a failed restore, for the log.
pub fn restore_failed_help(litestream: &Path, config: &Path, db_path: &Path) -> Vec<String> {
    vec![
        "RESTORE FAILED earlier: running WITHOUT backups to protect the existing backup.".into(),
        format!("To recover: stop the hub, move {} aside, run", db_path.display()),
        format!("  \"{}\" restore -config \"{}\" \"{}\"", litestream.display(), config.display(), db_path.display()),
        format!("then delete {} and start the hub.", restore_failed_marker(db_path).display()),
    ]
}

/// Litestream takes the hub command as ONE string and splits it with
/// go-shellwords, so each argument is single-quoted. Backslashes are literal
/// inside single quotes there, so Windows paths pass unchanged.
pub fn exec_command(args: &[&str]) -> String {
    args.iter()
        .map(|a| format!("'{}'", a.replace('\'', "'\\''")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// How the hub ended, as Litestream reports it. Litestream exits 1 whatever
/// the hub's exit code was, printing `Error: exit status N` (or `Error:
/// signal: …`) on stderr; any other failure is Litestream's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubExit {
    Code(i32),
    Signal,
}

pub fn hub_exit_from_litestream_line(line: &str) -> Option<HubExit> {
    let rest = line.trim().strip_prefix("Error: ")?;
    if let Some(code) = rest.strip_prefix("exit status ") {
        return code.trim().parse().ok().map(HubExit::Code);
    }
    rest.starts_with("signal: ").then_some(HubExit::Signal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts() -> Facts {
        Facts {
            replica_configured: true,
            litestream_present: true,
            restore_failed_marker: false,
            litestream_failed: false,
            db_exists: true,
        }
    }

    #[test]
    fn no_replica_url_runs_without_backups() {
        let f = Facts { replica_configured: false, litestream_present: false, ..facts() };
        assert_eq!(plan(&f), Plan::Direct(MODE_NONE));
    }

    #[test]
    fn missing_litestream_binary_is_unavailable() {
        assert_eq!(plan(&Facts { litestream_present: false, ..facts() }), Plan::Direct(MODE_UNAVAILABLE));
    }

    #[test]
    fn restore_failed_marker_keeps_backups_off_even_with_a_database() {
        let f = Facts { restore_failed_marker: true, db_exists: false, ..facts() };
        assert_eq!(plan(&f), Plan::Direct(MODE_RESTORE_FAILED));
        assert_eq!(plan(&Facts { restore_failed_marker: true, ..facts() }), Plan::Direct(MODE_RESTORE_FAILED));
    }

    #[test]
    fn litestream_failure_falls_back_to_running_the_hub_alone() {
        assert_eq!(plan(&Facts { litestream_failed: true, ..facts() }), Plan::Direct(MODE_UNAVAILABLE));
    }

    #[test]
    fn missing_database_is_restored_first() {
        assert_eq!(plan(&Facts { db_exists: false, ..facts() }), Plan::RestoreThenReplicate);
        assert_eq!(plan(&facts()), Plan::Replicate);
    }

    #[test]
    fn replica_url_takes_the_last_value_and_ignores_empty_ones() {
        let env = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
        };
        assert_eq!(replica_url(&env(&[("HUB_LOG_LEVEL", "debug")])), None);
        assert_eq!(replica_url(&env(&[(REPLICA_URL_VAR, "  ")])), None);
        assert_eq!(
            replica_url(&env(&[(REPLICA_URL_VAR, "s3://a/1"), (REPLICA_URL_VAR, "file:///mnt/usb/hub")])),
            Some("file:///mnt/usb/hub")
        );
    }

    #[test]
    fn marker_sits_next_to_the_database() {
        assert_eq!(
            restore_failed_marker(Path::new("/data/hub.sqlite")),
            PathBuf::from("/data/hub.sqlite.restore-failed")
        );
    }

    #[test]
    fn exec_command_quotes_windows_paths_spaces_and_quotes() {
        assert_eq!(
            exec_command(&[r"C:\Program Files\Sirvo Hub\resources\node\win32-x64\node.exe", r"C:\Program Files\Sirvo Hub\resources\hub\dist\index.js"]),
            r"'C:\Program Files\Sirvo Hub\resources\node\win32-x64\node.exe' 'C:\Program Files\Sirvo Hub\resources\hub\dist\index.js'"
        );
        assert_eq!(exec_command(&["/Users/o'brien/node"]), r"'/Users/o'\''brien/node'");
    }

    #[test]
    fn reads_the_hub_exit_from_litestream_errors() {
        assert_eq!(hub_exit_from_litestream_line("Error: exit status 78"), Some(HubExit::Code(78)));
        assert_eq!(hub_exit_from_litestream_line("Error: signal: killed"), Some(HubExit::Signal));
        assert_eq!(hub_exit_from_litestream_line("Error: cannot parse exec command: invalid"), None);
        assert_eq!(hub_exit_from_litestream_line("time=… level=INFO msg=\"litestream shut down\""), None);
    }
}
