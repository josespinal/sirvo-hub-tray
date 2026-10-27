use crate::backup::{self, HubExit, Plan};
use crate::log_buffer::LogBuffer;
use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HubState {
    Stopped,
    Starting,
    Running,
    Restarting,
    Errored,
    /// No Odoo connection saved yet; the hub was not started.
    NeedsSetup,
    /// The hub exited with `EXIT_CONFIG_ERROR` (78): it refused its Odoo
    /// settings. Restarting won't help until they are fixed.
    ConfigError,
}

/// `EXIT_CONFIG_ERROR` in nu_pos_hub/src/config.ts.
const EXIT_CONFIG_ERROR: i32 = 78;
/// A hub that ran at least this long before crashing restarts from the
/// shortest delay again.
const HEALTHY_RUN: Duration = Duration::from_secs(120);
const MAX_RESTART_DELAY: Duration = Duration::from_secs(60);
/// `hub.log` is rotated to `hub.log.1` (replacing it) past this size.
const LOG_ROTATE_BYTES: u64 = 10 * 1024 * 1024;
/// A restore that takes longer is treated as failed (backup destination
/// unreachable), so the hub still starts.
const RESTORE_TIMEOUT: Duration = Duration::from_secs(300);

/// Delay before the `attempt`-th restart in a row (0-based): 2s, 4s, 8s…
/// capped at a minute.
fn restart_delay(attempt: u32) -> Duration {
    let secs = 2u64.saturating_mul(1u64 << attempt.min(6));
    Duration::from_secs(secs).min(MAX_RESTART_DELAY)
}

pub struct SupervisorConfig {
    pub node_binary: PathBuf,
    pub hub_entry: PathBuf,
    pub hub_dir: PathBuf,
    pub db_path: PathBuf,
    pub log_file: PathBuf,
    pub admin_port: u16,
    pub ws_port: u16,
    pub http_port: u16,
    /// Extra hub environment (see `hub_env_file`), read on every start.
    pub hub_env_file: PathBuf,
    /// Bundled Litestream (Windows only) and its config (see `backup`).
    pub litestream_binary: PathBuf,
    pub litestream_config: PathBuf,
}

/// The hub's environment. Set explicitly so a value inherited from the
/// desktop session can never decide how the hub talks to Odoo.
fn hub_env(config: &SupervisorConfig, odoo: &crate::odoo_conn::HubCredentials) -> Vec<(&'static str, String)> {
    vec![
        ("HUB_PORT", config.ws_port.to_string()),
        ("HUB_HTTP_PORT", config.http_port.to_string()),
        ("HUB_ADMIN_PORT", config.admin_port.to_string()),
        ("HUB_DB_PATH", config.db_path.to_string_lossy().into_owned()),
        ("ODOO_URL", odoo.url.clone()),
        ("ODOO_DB", odoo.db.clone()),
        ("ODOO_USER", odoo.user.clone()),
        ("ODOO_PASSWORD", odoo.password.clone()),
        // Every request the hub makes to Odoo carries it (needs hub
        // v0.15+; older hubs send it on XML-RPC only).
        (
            "ODOO_RPC_HEADERS",
            odoo.waf_token
                .as_deref()
                .map(|t| format!("{}:{t}", crate::odoo_conn::WAF_HEADER))
                .unwrap_or_default(),
        ),
        // Dominican fiscal: payments are finalized in Odoo synchronously, with
        // NCF, invoice and reservation advances. Without it the hub falls back
        // to "none" (older hubs) or refuses to start (rost_pos_restaurant#181).
        ("FISCAL_PLUGIN", "dr-ncf".to_string()),
    ]
}

/// The hub's full environment: `hub.env` plus the managed values, which win.
/// Returns the warnings about the file's unusable or ignored lines.
fn spawn_env(
    config: &SupervisorConfig,
    odoo: &crate::odoo_conn::HubCredentials,
) -> (Vec<(String, String)>, Vec<String>) {
    let (file, mut warnings) = crate::hub_env_file::load(&config.hub_env_file);
    let (env, merge_warnings) = crate::hub_env_file::merge(file, &hub_env(config, odoo));
    warnings.extend(merge_warnings);
    (env, warnings)
}

pub struct Supervisor {
    state: Arc<Mutex<HubState>>,
    child: Arc<Mutex<Option<Child>>>,
    /// When set, stop() has been called and the watcher should not flip to Errored.
    intentional_stop: Arc<AtomicBool>,
    /// Unexpected exits in a row, for the restart backoff.
    crashes: Arc<AtomicU32>,
    /// Restart requests (after a delay) from the watcher to the task started
    /// by `spawn_auto_restart`. None until that task runs.
    restart_tx: Arc<Mutex<Option<mpsc::UnboundedSender<Duration>>>>,
    /// Signal channel for the watcher to wake up when stop() runs.
    /// Wrapped in Mutex<Option<>> so we can replace per run.
    shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    /// The running hub was started under `litestream replicate`.
    under_litestream: Arc<AtomicBool>,
    /// Litestream failed to run this session: run the hub without it until
    /// the operator restarts it.
    litestream_failed: Arc<AtomicBool>,
    /// The hub's exit as Litestream reported it on stderr.
    hub_exit: Arc<Mutex<Option<HubExit>>>,
    log_buffer: LogBuffer,
    config: SupervisorConfig,
    app: AppHandle,
    /// Windows: a job object that kills its processes when its last handle
    /// closes — i.e. when the tray process ends, however it ends.
    #[cfg(windows)]
    job: Option<win32job::Job>,
}

/// `kill_on_drop` only covers a tray that exits normally. When Windows ends
/// the tray (crash, Task Manager, the updater's `exit(0)`), the hub would
/// live on holding its ports and every new hub would fail with EADDRINUSE.
#[cfg(windows)]
fn kill_on_close_job() -> Option<win32job::Job> {
    let job = win32job::Job::create().map_err(|e| log::warn!("job object: {e}")).ok()?;
    let mut info = job.query_extended_limit_info().map_err(|e| log::warn!("job object: {e}")).ok()?;
    info.limit_kill_on_job_close();
    job.set_extended_limit_info(&mut info).map_err(|e| log::warn!("job object: {e}")).ok()?;
    Some(job)
}

impl Supervisor {
    pub fn new(app: AppHandle, config: SupervisorConfig, log_buffer: LogBuffer) -> Self {
        Self {
            state: Arc::new(Mutex::new(HubState::Stopped)),
            child: Arc::new(Mutex::new(None)),
            intentional_stop: Arc::new(AtomicBool::new(false)),
            shutdown_tx: Arc::new(Mutex::new(None)),
            crashes: Arc::new(AtomicU32::new(0)),
            restart_tx: Arc::new(Mutex::new(None)),
            under_litestream: Arc::new(AtomicBool::new(false)),
            litestream_failed: Arc::new(AtomicBool::new(false)),
            hub_exit: Arc::new(Mutex::new(None)),
            log_buffer,
            config,
            app,
            #[cfg(windows)]
            job: kill_on_close_job(),
        }
    }

    /// Restart the hub after it crashes. The watcher can't call `start`
    /// itself (its future would contain its own type), so it sends the
    /// delay here and this task, which owns an `Arc`, does the restart.
    pub fn spawn_auto_restart(self: &Arc<Self>) {
        let (tx, mut rx) = mpsc::unbounded_channel::<Duration>();
        *self.restart_tx.lock() = Some(tx);
        let me = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            while let Some(delay) = rx.recv().await {
                tokio::time::sleep(delay).await;
                // Someone may have started or stopped it meanwhile.
                if me.state() == HubState::Errored {
                    let _ = me.start().await;
                }
            }
        });
    }

    pub fn state(&self) -> HubState {
        *self.state.lock()
    }

    fn set_state(&self, next: HubState) {
        *self.state.lock() = next;
        let _ = self.app.emit("hub-state", next);
    }

    pub async fn start(&self) -> Result<()> {
        if matches!(self.state(), HubState::Starting | HubState::Running) {
            return Ok(());
        }
        let Some(odoo) = crate::odoo_conn::load_for_hub(&self.app) else {
            // Starting without credentials would just fail; ask for them.
            self.set_state(HubState::NeedsSetup);
            return Ok(());
        };
        self.set_state(HubState::Starting);
        self.intentional_stop.store(false, Ordering::SeqCst);

        let (mut env, env_warnings) = spawn_env(&self.config, &odoo);
        for warning in env_warnings {
            log::warn!("{warning}");
            self.log_buffer.push(format!("[tray] {warning}"));
        }
        // Set per start below; a value from hub.env would misreport backups.
        env.retain(|(k, _)| k != backup::BACKUP_MODE_VAR);

        // One writer for the restore's and the hub's output, so rotation has
        // a single owner.
        let log_tx = spawn_log_writer(self.config.log_file.clone(), LOG_ROTATE_BYTES);

        let mut mode = self.backup_mode(&env, &log_tx).await;
        if self.intentional_stop.load(Ordering::SeqCst) {
            // Stopped while restoring.
            return Ok(());
        }
        let mut litestream = mode == backup::MODE_LITESTREAM;
        let spawned = match self.spawn_hub(&env, mode, litestream) {
            Err(e) if litestream => {
                // The hub must run even if Litestream can't.
                self.tray_log(&log_tx, format!("could not start Litestream ({e}); starting the hub WITHOUT backups"));
                self.litestream_failed.store(true, Ordering::SeqCst);
                mode = backup::MODE_UNAVAILABLE;
                litestream = false;
                self.spawn_hub(&env, mode, false)
            }
            other => other,
        };
        let mut child = match spawned {
            Ok(c) => c,
            Err(e) => {
                self.set_state(HubState::Errored);
                return Err(anyhow!("spawn node: {e}"));
            }
        };
        self.under_litestream.store(litestream, Ordering::SeqCst);
        *self.hub_exit.lock() = None;

        #[cfg(windows)]
        if let (Some(job), Some(handle)) = (&self.job, child.raw_handle()) {
            if let Err(e) = job.assign_process(handle as isize) {
                log::warn!("could not tie the hub to the tray's lifetime: {e}");
            }
        }

        let stdout = match child.stdout.take() {
            Some(s) => s,
            None => {
                self.set_state(HubState::Errored);
                return Err(anyhow!("no stdout"));
            }
        };
        let stderr = match child.stderr.take() {
            Some(s) => s,
            None => {
                self.set_state(HubState::Errored);
                return Err(anyhow!("no stderr"));
            }
        };
        spawn_reader(stdout, self.log_buffer.clone(), self.app.clone(), "out", log_tx.clone(), None);
        let stderr_done = spawn_reader(
            stderr,
            self.log_buffer.clone(),
            self.app.clone(),
            "err",
            log_tx,
            litestream.then(|| self.hub_exit.clone()),
        );

        let pid = child.id();
        let (tx, mut rx) = oneshot::channel::<()>();
        *self.shutdown_tx.lock() = Some(tx);
        *self.child.lock() = Some(child);
        self.set_state(HubState::Running);

        let state = self.state.clone();
        let app = self.app.clone();
        let child_slot = self.child.clone();
        let intentional = self.intentional_stop.clone();
        let crashes = self.crashes.clone();
        let restart_tx = self.restart_tx.clone();
        let log_buffer = self.log_buffer.clone();
        let started = Instant::now();
        let hub_exit = self.hub_exit.clone();
        let litestream_failed = self.litestream_failed.clone();
        let mut stderr_done = Some(stderr_done);

        tokio::spawn(async move {
            // Race the child's exit against an explicit shutdown signal.
            // The shutdown signal arrives from stop() AFTER start_kill();
            // by the time we observe it, the child is already exiting.
            loop {
                // Briefly take the child out for `.wait()`, then put it back if
                // not finished. We have to be careful: tokio::process::Child::wait
                // takes &mut self, so we own the child for the duration of wait.
                let taken = { child_slot.lock().take() };
                let Some(mut c) = taken else {
                    // Child has been removed externally (only stop() does that
                    // after start_kill). Treat as clean stop.
                    log::info!("hub watcher (pid={pid:?}): child slot empty, exiting");
                    break;
                };

                tokio::select! {
                    status = c.wait() => {
                        if litestream {
                            // Litestream's last stderr line says how the hub
                            // ended; let the reader get to it.
                            if let Some(done) = stderr_done.take() {
                                let _ = tokio::time::timeout(Duration::from_secs(2), done).await;
                            }
                        }
                        let next = if intentional.load(Ordering::SeqCst) {
                            HubState::Stopped
                        } else {
                            let code = match &status {
                                Ok(s) if s.success() => None,
                                // Litestream exits 1 whatever the hub's code was.
                                Ok(s) if litestream => match *hub_exit.lock() {
                                    Some(HubExit::Code(code)) => Some(code),
                                    Some(HubExit::Signal) => None,
                                    None => {
                                        litestream_failed.store(true, Ordering::SeqCst);
                                        let line = format!(
                                            "[tray] Litestream failed ({s}) without running the hub; \
                                             the next start runs the hub WITHOUT backups (Restart to retry)"
                                        );
                                        log_buffer.push(line.clone());
                                        let _ = app.emit("hub-log-line", line);
                                        None
                                    }
                                },
                                Ok(s) => s.code(),
                                Err(_) => None,
                            };
                            match &status {
                                Ok(s) if s.success() => HubState::Stopped,
                                _ if code == Some(EXIT_CONFIG_ERROR) => HubState::ConfigError,
                                _ => HubState::Errored,
                            }
                        };
                        *state.lock() = next;
                        let _ = app.emit("hub-state", next);
                        log::info!("hub child (pid={pid:?}) exited; state={next:?}");
                        // A crash at 2 a.m. must not leave the restaurant
                        // without a hub until someone clicks Start. Not for
                        // ConfigError (78): restarting can't fix the config.
                        if next == HubState::Errored {
                            let attempt = if started.elapsed() >= HEALTHY_RUN {
                                crashes.store(1, Ordering::SeqCst);
                                0
                            } else {
                                crashes.fetch_add(1, Ordering::SeqCst)
                            };
                            let delay = restart_delay(attempt);
                            let line = format!(
                                "[tray] hub exited unexpectedly ({}); restarting in {}s",
                                status.as_ref().map(|s| s.to_string()).unwrap_or_else(|e| e.to_string()),
                                delay.as_secs()
                            );
                            log_buffer.push(line.clone());
                            let _ = app.emit("hub-log-line", line);
                            if let Some(tx) = restart_tx.lock().as_ref() {
                                let _ = tx.send(delay);
                            }
                        }
                        // Leave child_slot empty since the process is gone.
                        break;
                    }
                    _ = &mut rx => {
                        // stop() told us to wake up — put the child back so stop()
                        // can find it and kill it, then loop and re-await.
                        *child_slot.lock() = Some(c);
                        // rx is consumed; replace with a never-completing future
                        // by using a dummy channel for the next loop iteration.
                        let (_dummy_tx, dummy_rx) = oneshot::channel::<()>();
                        rx = dummy_rx;
                        // Now re-enter the loop; this time only the wait() arm
                        // can complete (rx will never fire).
                    }
                }
            }
        });

        Ok(())
    }

    pub async fn stop(&self) -> Result<()> {
        self.intentional_stop.store(true, Ordering::SeqCst);

        // Wake the watcher so it puts the child back in the slot (or no-op if
        // the slot already has the child).
        if let Some(tx) = self.shutdown_tx.lock().take() {
            let _ = tx.send(());
        }

        // Give the watcher a chance to re-park the child so we can take it.
        // The watcher's select! arm is fast; a few yields are enough.
        for _ in 0..50 {
            if self.child.lock().is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }

        // Take the child and kill it. start_kill is non-blocking; we then
        // explicitly wait so we know the process is reaped before returning.
        let taken = { self.child.lock().take() };
        if let Some(mut c) = taken {
            kill_hub(&mut c, self.under_litestream.load(Ordering::SeqCst)).await;
        }
        self.set_state(HubState::Stopped);
        Ok(())
    }

    /// Also retries Litestream after it failed earlier in this session.
    pub async fn restart(&self) -> Result<()> {
        self.litestream_failed.store(false, Ordering::SeqCst);
        self.set_state(HubState::Restarting);
        self.stop().await?;
        // Small yield so any pending watcher emit is processed before start.
        tokio::task::yield_now().await;
        self.start().await
    }

    /// A tray message in the logs window, the log buffer and `hub.log`.
    fn tray_log(&self, log_tx: &mpsc::UnboundedSender<String>, msg: impl AsRef<str>) {
        let line = format!("[tray] {}", msg.as_ref());
        log::info!("{line}");
        self.log_buffer.push(line.clone());
        let _ = log_tx.send(line.clone());
        let _ = self.app.emit("hub-log-line", line);
    }

    /// Decide how backups run this start (see `backup`), restoring a missing
    /// database first. Returns the mode to give the hub.
    async fn backup_mode(&self, env: &[(String, String)], log_tx: &mpsc::UnboundedSender<String>) -> &'static str {
        let db = &self.config.db_path;
        let marker = backup::restore_failed_marker(db);
        let plan = backup::plan(&backup::Facts {
            replica_configured: backup::replica_url(env).is_some(),
            litestream_present: self.config.litestream_binary.is_file(),
            restore_failed_marker: marker.exists(),
            litestream_failed: self.litestream_failed.load(Ordering::SeqCst),
            db_exists: db.exists(),
        });
        match plan {
            Plan::Direct(backup::MODE_NONE) => backup::MODE_NONE,
            Plan::Direct(backup::MODE_RESTORE_FAILED) => self.restore_failed(log_tx),
            Plan::Direct(mode) => {
                self.tray_log(log_tx, "LITESTREAM_REPLICA_URL is set but Litestream is not available: running WITHOUT backups");
                mode
            }
            Plan::Replicate => {
                self.tray_log(log_tx, format!("backing up {} with Litestream", db.display()));
                backup::MODE_LITESTREAM
            }
            Plan::RestoreThenReplicate => {
                // A new or replaced machine: rebuild the database from the
                // backup before the hub creates an empty one.
                self.tray_log(log_tx, format!("no database at {}: restoring it from the backup, if one exists", db.display()));
                if self.restore(env, log_tx).await {
                    self.tray_log(log_tx, format!("backing up {} with Litestream", db.display()));
                    backup::MODE_LITESTREAM
                } else {
                    if let Err(e) = backup::write_restore_failed_marker(db) {
                        self.tray_log(log_tx, format!("could not write {}: {e}", marker.display()));
                    }
                    self.restore_failed(log_tx)
                }
            }
        }
    }

    fn restore_failed(&self, log_tx: &mpsc::UnboundedSender<String>) -> &'static str {
        let c = &self.config;
        for line in backup::restore_failed_help(&c.litestream_binary, &c.litestream_config, &c.db_path) {
            self.tray_log(log_tx, line);
        }
        backup::MODE_RESTORE_FAILED
    }

    /// `litestream restore`: a no-op on the very first start, when there is no
    /// backup yet (-if-replica-exists). False when it failed or timed out,
    /// typically because the backup destination is unreachable at boot.
    async fn restore(&self, env: &[(String, String)], log_tx: &mpsc::UnboundedSender<String>) -> bool {
        let mut cmd = Command::new(&self.config.litestream_binary);
        cmd.arg("restore")
            .arg("-config")
            .arg(&self.config.litestream_config)
            .args(["-if-db-not-exists", "-if-replica-exists"])
            .arg(&self.config.db_path)
            .current_dir(&self.config.hub_dir)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        hide_console(&mut cmd);
        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.tray_log(log_tx, format!("could not start the Litestream restore: {e}"));
                return false;
            }
        };
        if let Some(out) = child.stdout.take() {
            spawn_reader(out, self.log_buffer.clone(), self.app.clone(), "restore", log_tx.clone(), None);
        }
        if let Some(err) = child.stderr.take() {
            spawn_reader(err, self.log_buffer.clone(), self.app.clone(), "restore", log_tx.clone(), None);
        }
        match tokio::time::timeout(RESTORE_TIMEOUT, child.wait()).await {
            Ok(Ok(status)) if status.success() => true,
            Ok(Ok(status)) => {
                self.tray_log(log_tx, format!("Litestream restore failed ({status})"));
                false
            }
            Ok(Err(e)) => {
                self.tray_log(log_tx, format!("Litestream restore failed: {e}"));
                false
            }
            Err(_) => {
                let _ = child.start_kill();
                self.tray_log(log_tx, format!("Litestream restore timed out after {}s", RESTORE_TIMEOUT.as_secs()));
                false
            }
        }
    }

    /// The hub, or Litestream running the hub as its child.
    fn spawn_hub(&self, env: &[(String, String)], mode: &str, litestream: bool) -> std::io::Result<Child> {
        let c = &self.config;
        let mut cmd = if litestream {
            let mut cmd = Command::new(&c.litestream_binary);
            cmd.arg("replicate").arg("-config").arg(&c.litestream_config).arg("-exec").arg(
                backup::exec_command(&[&c.node_binary.to_string_lossy(), &c.hub_entry.to_string_lossy()]),
            );
            cmd
        } else {
            let mut cmd = Command::new(&c.node_binary);
            cmd.arg(&c.hub_entry);
            cmd
        };
        cmd.current_dir(&c.hub_dir)
            .envs(env.iter().map(|(k, v)| (k, v)))
            .env(backup::BACKUP_MODE_VAR, mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        hide_console(&mut cmd);
        cmd.spawn()
    }
}

/// On Windows, suppress the console window that would otherwise pop up
/// alongside node.exe or litestream.exe. 0x08000000 = CREATE_NO_WINDOW.
fn hide_console(cmd: &mut Command) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = cmd;
}

/// Kill the hub and reap it. Under Litestream the hub is Litestream's child:
/// killing Litestream alone would leave the hub running on its ports.
async fn kill_hub(c: &mut Child, under_litestream: bool) {
    if under_litestream {
        #[cfg(windows)]
        if let Some(pid) = c.id() {
            let mut taskkill = Command::new("taskkill");
            taskkill.args(["/PID", &pid.to_string(), "/T", "/F"]);
            hide_console(&mut taskkill);
            let _ = taskkill.status().await;
        }
        // Litestream forwards SIGTERM to the hub and waits for it.
        #[cfg(unix)]
        if let Some(pid) = c.id() {
            let _ = Command::new("kill").args(["-TERM", &pid.to_string()]).status().await;
            if tokio::time::timeout(Duration::from_secs(10), c.wait()).await.is_ok() {
                return;
            }
        }
    }
    let _ = c.start_kill();
    let _ = c.wait().await;
}

/// Forwards `stream`'s lines to the logs until EOF. With `hub_exit`, also
/// records how the hub ended from Litestream's `Error: exit status N` line.
fn spawn_reader<R>(
    stream: R,
    buf: LogBuffer,
    app: AppHandle,
    stream_name: &'static str,
    log_tx: mpsc::UnboundedSender<String>,
    hub_exit: Option<Arc<Mutex<Option<HubExit>>>>,
) -> tokio::task::JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let reader = BufReader::new(stream);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(slot) = &hub_exit {
                if let Some(exit) = backup::hub_exit_from_litestream_line(&line) {
                    *slot.lock() = Some(exit);
                }
            }
            let stamped = format!("[{stream_name}] {line}");
            buf.push(stamped.clone());
            let _ = log_tx.send(stamped.clone());
            let _ = app.emit("hub-log-line", stamped);
        }
    })
}

/// Appends lines to `path`, moving it to `<path>.1` (replacing an older one)
/// once it passes `max_bytes`, so the log can't fill the disk. Ends when
/// every sender is dropped (both readers finished).
fn spawn_log_writer(path: PathBuf, max_bytes: u64) -> mpsc::UnboundedSender<String> {
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        use tokio::io::AsyncWriteExt;
        let open = |p: PathBuf| async move {
            tokio::fs::OpenOptions::new().create(true).append(true).open(p).await.ok()
        };
        let mut size = tokio::fs::metadata(&path).await.map(|m| m.len()).unwrap_or(0);
        let mut file = open(path.clone()).await;
        while let Some(line) = rx.recv().await {
            if size >= max_bytes {
                drop(file.take());
                let mut old = path.clone().into_os_string();
                old.push(".1");
                let _ = tokio::fs::remove_file(&old).await;
                let _ = tokio::fs::rename(&path, &old).await;
                file = open(path.clone()).await;
                size = 0;
            }
            if let Some(f) = file.as_mut() {
                if f.write_all(line.as_bytes()).await.is_ok() && f.write_all(b"\n").await.is_ok() {
                    size += line.len() as u64 + 1;
                }
            }
        }
        if let Some(f) = file.as_mut() {
            let _ = f.flush().await;
        }
    });
    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> SupervisorConfig {
        SupervisorConfig {
            node_binary: PathBuf::from("node"),
            hub_entry: PathBuf::from("dist/index.js"),
            hub_dir: PathBuf::from("."),
            db_path: PathBuf::from("hub.sqlite"),
            log_file: PathBuf::from("hub.log"),
            admin_port: 8767,
            ws_port: 8765,
            http_port: 8766,
            hub_env_file: PathBuf::from("hub.env"),
            litestream_binary: PathBuf::from("litestream"),
            litestream_config: PathBuf::from("litestream.yml"),
        }
    }

    fn test_creds(waf_token: Option<&str>) -> crate::odoo_conn::HubCredentials {
        crate::odoo_conn::HubCredentials {
            url: "https://odoo.example".into(),
            db: "pos".into(),
            user: "pos_hub".into(),
            password: "secret".into(),
            waf_token: waf_token.map(String::from),
        }
    }

    fn env_value<'a>(env: &'a [(&'static str, String)], key: &str) -> Option<&'a str> {
        env.iter().find(|(k, _)| *k == key).map(|(_, v)| v.as_str())
    }

    // Hubs from rost_pos_restaurant#181 on refuse to start without
    // FISCAL_PLUGIN; older ones fell back to "none": payments forwarded with
    // no NCF, invoice or reservation advances.
    #[test]
    fn hub_env_sets_the_dominican_fiscal_plugin() {
        let env = hub_env(&test_config(), &test_creds(None));
        assert_eq!(env_value(&env, "FISCAL_PLUGIN"), Some("dr-ncf"));
    }

    #[test]
    fn hub_env_passes_ports_db_and_odoo_credentials() {
        let env = hub_env(&test_config(), &test_creds(Some("tok")));
        assert_eq!(env_value(&env, "HUB_PORT"), Some("8765"));
        assert_eq!(env_value(&env, "HUB_HTTP_PORT"), Some("8766"));
        assert_eq!(env_value(&env, "HUB_ADMIN_PORT"), Some("8767"));
        assert_eq!(env_value(&env, "HUB_DB_PATH"), Some("hub.sqlite"));
        assert_eq!(env_value(&env, "ODOO_URL"), Some("https://odoo.example"));
        assert_eq!(env_value(&env, "ODOO_DB"), Some("pos"));
        assert_eq!(env_value(&env, "ODOO_USER"), Some("pos_hub"));
        assert_eq!(env_value(&env, "ODOO_PASSWORD"), Some("secret"));
        assert_eq!(
            env_value(&env, "ODOO_RPC_HEADERS"),
            Some(format!("{}:tok", crate::odoo_conn::WAF_HEADER).as_str())
        );
    }

    #[test]
    fn spawn_env_adds_hub_env_file_values_but_keeps_the_managed_ones() {
        let dir = std::env::temp_dir().join(format!("tray-spawn-env-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut config = test_config();
        config.hub_env_file = dir.join("hub.env");
        std::fs::write(&config.hub_env_file, "HUB_LOG_LEVEL=debug\nFISCAL_PLUGIN=none\nbroken line\n").unwrap();

        let (env, warnings) = spawn_env(&config, &test_creds(None));
        let get = |key: &str| env.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
        assert_eq!(get("HUB_LOG_LEVEL"), Some("debug"));
        assert_eq!(get("FISCAL_PLUGIN"), Some("dr-ncf"));
        assert_eq!(get("ODOO_USER"), Some("pos_hub"));
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spawn_env_without_a_hub_env_file_is_just_the_managed_env() {
        let mut config = test_config();
        config.hub_env_file = std::env::temp_dir().join(format!("tray-no-hub-env-{}.env", std::process::id()));
        let _ = std::fs::remove_file(&config.hub_env_file);
        let (env, warnings) = spawn_env(&config, &test_creds(None));
        let managed: Vec<(String, String)> = hub_env(&config, &test_creds(None))
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        assert_eq!(env, managed);
        assert!(warnings.is_empty());
    }

    #[test]
    fn hub_env_sends_empty_rpc_headers_without_a_waf_token() {
        let env = hub_env(&test_config(), &test_creds(None));
        assert_eq!(env_value(&env, "ODOO_RPC_HEADERS"), Some(""));
    }

    #[test]
    fn restart_delay_backs_off_and_caps_at_a_minute() {
        let secs: Vec<u64> = (0..8).map(|n| restart_delay(n).as_secs()).collect();
        assert_eq!(secs, vec![2, 4, 8, 16, 32, 60, 60, 60]);
        assert_eq!(restart_delay(u32::MAX), MAX_RESTART_DELAY);
    }

    #[tokio::test]
    async fn log_writer_rotates_past_the_size_limit() {
        let dir = std::env::temp_dir().join(format!("tray-log-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("hub.log");

        let tx = spawn_log_writer(path.clone(), 20);
        for line in ["0123456789", "abcdefghij", "after-rotation"] {
            tx.send(line.to_string()).unwrap();
        }
        drop(tx);
        // The writer task ends once the channel drains.
        for _ in 0..50 {
            if std::fs::read_to_string(&path).map(|s| s.contains("after-rotation")).unwrap_or(false) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        let current = std::fs::read_to_string(&path).unwrap();
        let rotated = std::fs::read_to_string(dir.join("hub.log.1")).unwrap();
        assert_eq!(current, "after-rotation\n");
        assert_eq!(rotated, "0123456789\nabcdefghij\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
