use crate::log_buffer::LogBuffer;
use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::oneshot;

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

pub struct SupervisorConfig {
    pub node_binary: PathBuf,
    pub hub_entry: PathBuf,
    pub hub_dir: PathBuf,
    pub db_path: PathBuf,
    pub log_file: PathBuf,
    pub admin_port: u16,
    pub ws_port: u16,
    pub http_port: u16,
}

pub struct Supervisor {
    state: Arc<Mutex<HubState>>,
    child: Arc<Mutex<Option<Child>>>,
    /// When set, stop() has been called and the watcher should not flip to Errored.
    intentional_stop: Arc<AtomicBool>,
    /// Signal channel for the watcher to wake up when stop() runs.
    /// Wrapped in Mutex<Option<>> so we can replace per run.
    shutdown_tx: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    log_buffer: LogBuffer,
    config: SupervisorConfig,
    app: AppHandle,
}

impl Supervisor {
    pub fn new(app: AppHandle, config: SupervisorConfig, log_buffer: LogBuffer) -> Self {
        Self {
            state: Arc::new(Mutex::new(HubState::Stopped)),
            child: Arc::new(Mutex::new(None)),
            intentional_stop: Arc::new(AtomicBool::new(false)),
            shutdown_tx: Arc::new(Mutex::new(None)),
            log_buffer,
            config,
            app,
        }
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

        let mut cmd = Command::new(&self.config.node_binary);
        cmd.arg(&self.config.hub_entry)
            .current_dir(&self.config.hub_dir)
            .env("HUB_PORT", self.config.ws_port.to_string())
            .env("HUB_HTTP_PORT", self.config.http_port.to_string())
            .env("HUB_ADMIN_PORT", self.config.admin_port.to_string())
            .env("HUB_DB_PATH", &self.config.db_path)
            .env("ODOO_URL", &odoo.url)
            .env("ODOO_DB", &odoo.db)
            .env("ODOO_USER", &odoo.user)
            .env("ODOO_PASSWORD", &odoo.password)
            // Every request the hub makes to Odoo carries it (needs hub
            // v0.15+; older hubs send it on XML-RPC only).
            .env(
                "ODOO_RPC_HEADERS",
                odoo.waf_token.as_deref().map(|t| format!("{}:{t}", crate::odoo_conn::WAF_HEADER)).unwrap_or_default(),
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        // On Windows, suppress the console window that would otherwise pop
        // up alongside node.exe. 0x08000000 = CREATE_NO_WINDOW.
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }

        let mut child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                self.set_state(HubState::Errored);
                return Err(anyhow!("spawn node: {e}"));
            }
        };

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
        spawn_reader(
            stdout,
            self.log_buffer.clone(),
            self.app.clone(),
            "out",
            self.config.log_file.clone(),
        );
        spawn_reader(
            stderr,
            self.log_buffer.clone(),
            self.app.clone(),
            "err",
            self.config.log_file.clone(),
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
                        let next = if intentional.load(Ordering::SeqCst) {
                            HubState::Stopped
                        } else {
                            match status {
                                Ok(s) if s.success() => HubState::Stopped,
                                Ok(s) if s.code() == Some(EXIT_CONFIG_ERROR) => HubState::ConfigError,
                                _ => HubState::Errored,
                            }
                        };
                        *state.lock() = next;
                        let _ = app.emit("hub-state", next);
                        log::info!("hub child (pid={pid:?}) exited; state={next:?}");
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
            let _ = c.start_kill();
            let _ = c.wait().await;
        }
        self.set_state(HubState::Stopped);
        Ok(())
    }

    pub async fn restart(&self) -> Result<()> {
        self.set_state(HubState::Restarting);
        self.stop().await?;
        // Small yield so any pending watcher emit is processed before start.
        tokio::task::yield_now().await;
        self.start().await
    }
}

fn spawn_reader<R>(
    stream: R,
    buf: LogBuffer,
    app: AppHandle,
    stream_name: &'static str,
    log_file: std::path::PathBuf,
) where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let reader = BufReader::new(stream);
        let mut lines = reader.lines();
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_file)
            .await
            .ok();
        while let Ok(Some(line)) = lines.next_line().await {
            let stamped = format!("[{stream_name}] {line}");
            buf.push(stamped.clone());
            if let Some(f) = file.as_mut() {
                use tokio::io::AsyncWriteExt;
                let _ = f.write_all(stamped.as_bytes()).await;
                let _ = f.write_all(b"\n").await;
            }
            let _ = app.emit("hub-log-line", stamped);
        }
    });
}
