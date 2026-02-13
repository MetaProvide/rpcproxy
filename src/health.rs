use std::sync::Arc;
use std::time::Duration;

use tokio::time;
use tracing::info;

use crate::error::RpcProxyError;
use crate::upstream::UpstreamManager;

pub async fn start_health_checker(upstream: Arc<UpstreamManager>, interval_secs: u64) {
    let interval = Duration::from_secs(interval_secs);
    let notify = upstream.health_notify();

    info!(interval_secs = %interval_secs, "starting health checker");

    upstream.check_all_backends(probe_backend_url).await;

    let mut ticker = time::interval(interval);
    ticker.tick().await;

    loop {
        tokio::select! {
            _ = ticker.tick() => {},
            _ = notify.notified() => {
                info!("reactive health check triggered (backend went down)");
                ticker.reset();
            },
        }
        upstream.check_all_backends(probe_backend_url).await;
    }
}

pub async fn probe_backend_url(url: String) -> Result<u64, RpcProxyError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| RpcProxyError::HealthProbe(format!("client build: {e}")))?;

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });

    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| RpcProxyError::UpstreamRequest(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(RpcProxyError::UpstreamHttp(resp.status().as_u16()));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| RpcProxyError::BodyRead(e.to_string()))?;

    let result = json
        .get("result")
        .and_then(|v| v.as_str())
        .ok_or_else(|| RpcProxyError::HealthProbe("missing result field".into()))?;

    let block = u64::from_str_radix(result.trim_start_matches("0x"), 16)
        .map_err(|e| RpcProxyError::HealthProbe(format!("invalid block number: {e}")))?;

    Ok(block)
}
