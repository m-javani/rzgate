use reqwest::Client;
use serde_json::{Value, json};
use tokio::time::{Duration, sleep};
use tokio_util::sync::CancellationToken;
use tracing::Level;

use rzgate::config::Config;
use rzgate::error::RZError;
use rzgate::{async_main, init_logging};

pub struct TestHelper {
    http_addr: String,
    shutdown: CancellationToken,
    handle: Option<tokio::task::JoinHandle<()>>,
    client: Client,
}

impl TestHelper {
    pub async fn new() -> Self {
        // Setup test logging
        init_logging(Level::DEBUG);

        let config = Self::test_config();
        let http_addr = format!("http://{}:{}", config.listening_addr, config.http_port);

        let shutdown = CancellationToken::new();
        let shutdown_clone = shutdown.clone();

        // Spawn RzGate
        let handle = tokio::spawn(async move {
            let _ = async_main(config, shutdown_clone).await;
        });

        // Wait for HTTP server to be ready
        Self::wait_for_http(&http_addr).await;

        Self {
            http_addr,
            shutdown,
            handle: Some(handle),
            client: Client::new(),
        }
    }

    #[allow(unused)]
    pub async fn shutdown(&mut self) {
        self.shutdown.cancel();
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }

    fn test_config() -> Config {
        use clap::Parser;

        let args = vec![
            "rzgate",
            "--mode",
            "router",
            "--roomzin-addr",
            "172.20.0.60", // edge router IP
            "--roomzin-port",
            "9000",
            "--listening-addr",
            "127.0.0.1",
            "--http-port",
            "8777",
            "--timeout-sec",
            "2",
            "--keep-alive-sec",
            "30",
            "--conn-per-node",
            "10",
            "--max-active-conns",
            "10000",
        ];

        Config::parse_from(args)
    }

    async fn wait_for_http(addr: &str) {
        let client = Client::new();
        let max_attempts = 30;

        for attempt in 0..max_attempts {
            match client.get(format!("{}/health", addr)).send().await {
                Ok(resp) if resp.status().is_success() => {
                    tracing::info!("RzGate is ready on {}", addr);
                    return;
                }
                _ => {
                    if attempt == max_attempts - 1 {
                        panic!("RzGate failed to start after {} attempts", max_attempts);
                    }
                    sleep(Duration::from_millis(200)).await;
                }
            }
        }
    }

    pub async fn send_command(
        &self,
        command: &str,
        segment: &str,
        body: serde_json::Value,
    ) -> Result<Value, RZError> {
        let payload = json!({
            "command": command,
            "segment": segment,
            "body": body,
        });

        let response = self
            .client
            .post(format!("{}/api", self.http_addr))
            .json(&payload)
            .send()
            .await
            .map_err(|e| RZError::Internal(format!("Request failed: {}", e)))?;

        let status = response.status();
        let json: Value = response
            .json()
            .await
            .map_err(|e| RZError::Internal(format!("Response parse failed: {}", e)))?;

        if !status.is_success() {
            let error_msg = json
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            return Err(RZError::Internal(format!(
                "HTTP error {}: {}",
                status, error_msg
            )));
        }

        Ok(json)
    }

    pub async fn get_prop_room_day(
        &self,
        property_id: &str,
        room_type: &str,
        date: &str,
    ) -> Result<Value, RZError> {
        let body = json!({
            "property_id": property_id,
            "room_type": room_type,
            "date": date,
        });

        self.send_command("GETPROPROOMDAY", "segment_1", body).await
    }

    #[allow(unused)]
    pub fn http_addr(&self) -> &str {
        &self.http_addr
    }
}

impl Drop for TestHelper {
    fn drop(&mut self) {
        self.shutdown.cancel();
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}
