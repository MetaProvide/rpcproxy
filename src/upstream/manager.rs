use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::Client;
use tokio::sync::RwLock;
use tracing::{debug, error, warn};

use crate::error::RpcProxyError;
use crate::jsonrpc::{JsonRpcRequest, JsonRpcResponse};

use super::backend::{BackendHealthInfo, BackendState, BackendStatus};

pub struct UpstreamManager {
    backends: Vec<Arc<RwLock<BackendStatus>>>,
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

    pub async fn send_request(&self, request: &JsonRpcRequest) -> Result<JsonRpcResponse, RpcProxyError> {
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
        Err(RpcProxyError::AllUpstreamsFailed)
    }

    async fn forward_to_backend(
        &self,
        url: &str,
        request: &JsonRpcRequest,
    ) -> Result<JsonRpcResponse, RpcProxyError> {
        let body = serde_json::to_string(request)?;

        let resp = self
            .client
            .post(url)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| RpcProxyError::UpstreamRequest(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(RpcProxyError::UpstreamHttp(resp.status().as_u16()));
        }

        let text = resp
            .text()
            .await
            .map_err(|e| RpcProxyError::BodyRead(e.to_string()))?;

        let rpc_response: JsonRpcResponse = serde_json::from_str(&text)?;

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

    /// Runs a health probe on each backend and updates their state.
    /// Used by the health checker — keeps backend mutation encapsulated.
    pub async fn check_all_backends<F, Fut>(&self, probe: F)
    where
        F: Fn(String) -> Fut,
        Fut: std::future::Future<Output = Result<u64, RpcProxyError>>,
    {
        let mut best_block: Option<u64> = None;

        for backend_lock in &self.backends {
            let url = backend_lock.read().await.url.clone();
            match probe(url.clone()).await {
                Ok(block_number) => {
                    let mut backend = backend_lock.write().await;
                    backend.latest_block = Some(block_number);
                    backend.record_success(0.0);
                    debug!(backend = %url, block = %block_number, "health check passed");

                    match best_block {
                        Some(best) if block_number > best => best_block = Some(block_number),
                        None => best_block = Some(block_number),
                        _ => {}
                    }
                }
                Err(e) => {
                    let mut backend = backend_lock.write().await;
                    backend.record_error();
                    warn!(backend = %url, error = %e, state = ?backend.state, "health check failed");
                }
            }
        }

        // Mark backends with stale blocks as degraded
        if let Some(best) = best_block {
            for backend_lock in &self.backends {
                let mut backend = backend_lock.write().await;
                if let Some(block) = backend.latest_block {
                    if best > block && best - block > 10 {
                        if backend.state == BackendState::Healthy {
                            backend.state = BackendState::Degraded;
                            warn!(
                                backend = %backend.url,
                                block = %block,
                                best_block = %best,
                                "backend is stale, marking degraded"
                            );
                        }
                    }
                }
            }
        }
    }
}
