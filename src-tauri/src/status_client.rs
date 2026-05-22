use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HubStatus {
    pub hub_version: Option<String>,
    pub uptime_sec: Option<u64>,
    pub connected_terminals: Option<u32>,
    pub lan_url: Option<String>,
    pub reachable: bool,
}

#[derive(Clone)]
pub struct StatusClient {
    inner: Arc<Inner>,
}

struct Inner {
    app: AppHandle,
    admin_port: u16,
    last: Mutex<HubStatus>,
}

impl StatusClient {
    pub fn new(app: AppHandle, admin_port: u16) -> Self {
        Self {
            inner: Arc::new(Inner {
                app,
                admin_port,
                last: Mutex::new(HubStatus::default()),
            }),
        }
    }

    pub fn last(&self) -> HubStatus {
        self.inner.last.lock().clone()
    }

    pub async fn start_polling(self) {
        // Single fixed poll cadence in v1; menu/window visibility was an
        // optimization not worth the complexity yet.
        let mut interval = tokio::time::interval(Duration::from_secs(3));
        loop {
            interval.tick().await;
            let status = self.fetch_once().await;
            *self.inner.last.lock() = status.clone();
            let _ = self.inner.app.emit("hub-status", status);
        }
    }

    async fn fetch_once(&self) -> HubStatus {
        let url = format!("http://127.0.0.1:{}/admin/status", self.inner.admin_port);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(1500))
            .build()
            .expect("reqwest client");
        match client.get(&url).send().await {
            Ok(res) if res.status().is_success() => {
                if let Ok(body) = res.json::<RawStatus>().await {
                    return HubStatus {
                        hub_version: Some(body.hub_version),
                        uptime_sec: Some(body.uptime_sec),
                        connected_terminals: Some(body.connected_terminals),
                        lan_url: body.lan_url,
                        reachable: true,
                    };
                }
                HubStatus { reachable: false, ..Default::default() }
            }
            _ => HubStatus { reachable: false, ..Default::default() },
        }
    }
}

#[derive(Deserialize)]
struct RawStatus {
    #[serde(rename = "hubVersion")]
    hub_version: String,
    #[serde(rename = "uptimeSec")]
    uptime_sec: u64,
    #[serde(rename = "connectedTerminals")]
    connected_terminals: u32,
    #[serde(rename = "lanUrl")]
    lan_url: Option<String>,
}
