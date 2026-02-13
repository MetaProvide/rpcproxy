use std::sync::Arc;
use std::time::Duration;

use tokio::time;
use tracing::{debug, info, warn};

use crate::upstream::{BackendState, UpstreamManager};

pub async fn start_health_checker(upstream: Arc<UpstreamManager>, interval_secs: u64) {
    let interval = Duration::from_secs(interval_secs);
    info!(interval_secs = %interval_secs, "starting health checker");

    let mut ticker = time::interval(interval);
    // Skip the first immediate tick
    ticker.tick().await;

    loop {
        ticker.tick().await;
        check_all_backends(&upstream).await;
    }
}

async fn check_all_backends(upstream: &UpstreamManager) {
    let mut best_block: Option<u64> = None;

    for backend_lock in &upstream.backends {
        let url = backend_lock.read().await.url.clone();
        match probe_backend(&url).await {
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
        for backend_lock in &upstream.backends {
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

async fn probe_backend(url: &str) -> Result<u64, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("client build error: {e}"))?;

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });

    let resp = client
        .post(url)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request error: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("json parse error: {e}"))?;

    let result = json
        .get("result")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "missing result field".to_string())?;

    let block = u64::from_str_radix(result.trim_start_matches("0x"), 16)
        .map_err(|e| format!("invalid block number: {e}"))?;

    Ok(block)
}
