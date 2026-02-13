use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::Client;

use tokio::sync::RwLock;
use tracing::{debug, error, warn};

use crate::jsonrpc::{JsonRpcRequest, JsonRpcResponse};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendState {
    Healthy,
    Degraded,
    Down,
}

#[derive(Debug)]
pub struct BackendStatus {
    pub url: String,
    pub state: BackendState,
    pub consecutive_errors: u32,
    pub consecutive_successes: u32,
    pub last_error_at: Option<Instant>,
    pub last_success_at: Option<Instant>,
    pub latest_block: Option<u64>,
    pub avg_latency_ms: f64,
    pub total_requests: u64,
    pub total_errors: u64,
    pub started_at: Instant,
}

impl BackendStatus {
    pub fn new(url: String) -> Self {
        Self {
            url,
            state: BackendState::Healthy,
            consecutive_errors: 0,
            consecutive_successes: 0,
            last_error_at: None,
            last_success_at: None,
            latest_block: None,
            avg_latency_ms: 0.0,
            total_requests: 0,
            total_errors: 0,
            started_at: Instant::now(),
        }
    }

    pub fn record_success(&mut self, latency_ms: f64) {
        self.total_requests += 1;
        self.consecutive_errors = 0;
        self.consecutive_successes += 1;
        self.last_success_at = Some(Instant::now());
        self.state = BackendState::Healthy;
        if self.avg_latency_ms == 0.0 {
            self.avg_latency_ms = latency_ms;
        } else {
            self.avg_latency_ms = self.avg_latency_ms * 0.8 + latency_ms * 0.2;
        }
    }

    pub fn record_error(&mut self) {
        self.total_requests += 1;
        self.total_errors += 1;
        self.consecutive_successes = 0;
        self.consecutive_errors += 1;
        self.last_error_at = Some(Instant::now());
        if self.consecutive_errors >= 3 {
            self.state = BackendState::Down;
        } else {
            self.state = BackendState::Degraded;
        }
    }
}

pub struct UpstreamManager {
    pub backends: Vec<Arc<RwLock<BackendStatus>>>,
    client: Client,
}

impl UpstreamManager {
    pub fn new(urls: Vec<String>, request_timeout: Duration) -> Self {
        let client = Client::builder()
            .timeout(request_timeout)
            .pool_max_idle_per_host(20)
            .build()
            .expect("failed to build HTTP client");

        let backends = urls
            .into_iter()
            .map(|url| Arc::new(RwLock::new(BackendStatus::new(url))))
            .collect();

        Self { backends, client }
    }

    pub async fn send_request(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse, ()> {
        for backend_lock in &self.backends {
            let (url, state) = {
                let backend = backend_lock.read().await;
                (backend.url.clone(), backend.state)
            };

            if state == BackendState::Down {
                debug!(backend = %url, "skipping down backend");
                continue;
            }

            let start = Instant::now();
            match self.forward_to_backend(&url, request).await {
                Ok(response) => {
                    let latency = start.elapsed().as_secs_f64() * 1000.0;
                    let mut backend = backend_lock.write().await;
                    backend.record_success(latency);
                    debug!(backend = %url, latency_ms = %latency, "upstream success");
                    return Ok(response);
                }
                Err(e) => {
                    let mut backend = backend_lock.write().await;
                    backend.record_error();
                    warn!(backend = %url, error = %e, state = ?backend.state, "upstream error, trying next");
                }
            }
        }

        // All backends failed — last resort: try the first one anyway
        if let Some(backend_lock) = self.backends.first() {
            let url = backend_lock.read().await.url.clone();
            warn!(backend = %url, "all backends failed, last-resort attempt on primary");
            let start = Instant::now();
            if let Ok(response) = self.forward_to_backend(&url, request).await {
                let latency = start.elapsed().as_secs_f64() * 1000.0;
                let mut backend = backend_lock.write().await;
                backend.record_success(latency);
                return Ok(response);
            }
        }

        error!("all upstream backends failed");
        Err(())
    }

    async fn forward_to_backend(
        &self,
        url: &str,
        request: &JsonRpcRequest,
    ) -> Result<JsonRpcResponse, String> {
        let body = serde_json::to_string(request)
            .map_err(|e| format!("serialize error: {e}"))?;

        let resp = self
            .client
            .post(url)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| format!("request error: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }

        let text = resp.text().await.map_err(|e| format!("body read error: {e}"))?;

        let rpc_response: JsonRpcResponse = serde_json::from_str(&text)
            .map_err(|e| format!("invalid JSON-RPC response: {e}"))?;

        Ok(rpc_response)
    }

    pub async fn backend_statuses(&self) -> Vec<BackendHealthInfo> {
        let mut statuses = Vec::with_capacity(self.backends.len());
        for (i, backend_lock) in self.backends.iter().enumerate() {
            let b = backend_lock.read().await;
            statuses.push(BackendHealthInfo {
                url: b.url.clone(),
                priority: i,
                state: format!("{:?}", b.state),
                latency_ms: b.avg_latency_ms,
                latest_block: b.latest_block,
                total_requests: b.total_requests,
                total_errors: b.total_errors,
                uptime_secs: b.started_at.elapsed().as_secs(),
            });
        }
        statuses
    }

    pub async fn has_healthy_backend_with_block(&self) -> bool {
        for backend_lock in &self.backends {
            let b = backend_lock.read().await;
            if b.state == BackendState::Healthy && b.latest_block.is_some() {
                return true;
            }
        }
        false
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct BackendHealthInfo {
    pub url: String,
    pub priority: usize,
    pub state: String,
    pub latency_ms: f64,
    pub latest_block: Option<u64>,
    pub total_requests: u64,
    pub total_errors: u64,
    pub uptime_secs: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_state_transitions() {
        let mut backend = BackendStatus::new("http://localhost:8545".to_string());
        assert_eq!(backend.state, BackendState::Healthy);

        backend.record_error();
        assert_eq!(backend.state, BackendState::Degraded);
        assert_eq!(backend.consecutive_errors, 1);

        backend.record_error();
        assert_eq!(backend.state, BackendState::Degraded);

        backend.record_error();
        assert_eq!(backend.state, BackendState::Down);
        assert_eq!(backend.consecutive_errors, 3);

        backend.record_success(50.0);
        assert_eq!(backend.state, BackendState::Healthy);
        assert_eq!(backend.consecutive_errors, 0);
    }

    #[test]
    fn test_latency_tracking() {
        let mut backend = BackendStatus::new("http://localhost:8545".to_string());
        backend.record_success(100.0);
        assert_eq!(backend.avg_latency_ms, 100.0);

        backend.record_success(200.0);
        // 100 * 0.8 + 200 * 0.2 = 120
        assert!((backend.avg_latency_ms - 120.0).abs() < 0.01);
    }
}
