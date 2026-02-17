use std::io::Write;
use std::net::TcpStream;
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


/// Perform an HTTP health check against the running instance using only std.
/// Returns 0 if the server responds with HTTP 200, 1 otherwise.
/// Used by `rpcproxy --health` for Docker HEALTHCHECK without curl.
pub fn run_health_check(port: u16) -> i32 {
    let addr = format!("127.0.0.1:{port}");
    let timeout = Duration::from_secs(5);

    let mut stream = match TcpStream::connect_timeout(&addr.parse().unwrap(), timeout) {
        Ok(s) => s,
        Err(_) => return 1,
    };

    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));

    let request = format!(
        "GET /health HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n"
    );
    if stream.write_all(request.as_bytes()).is_err() {
        return 1;
    }

    // Read enough bytes to capture the HTTP status line.
    let mut buf = [0u8; 32];
    let n = match std::io::Read::read(&mut stream, &mut buf) {
        Ok(n) if n > 0 => n,
        _ => return 1,
    };

    let response = String::from_utf8_lossy(&buf[..n]);
    if response.contains("200") {
        0
    } else {
        1
    }
}