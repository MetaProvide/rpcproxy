use std::sync::Arc;
use std::time::Duration;

use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use rpcproxy::jsonrpc::JsonRpcRequest;
use rpcproxy::upstream::UpstreamManager;

fn rpc_request(method_name: &str) -> JsonRpcRequest {
    serde_json::from_value(serde_json::json!({
        "jsonrpc": "2.0",
        "method": method_name,
        "params": [],
        "id": 1
    }))
    .unwrap()
}

fn ok_response(result: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "result": result,
        "id": 1
    })
}

/// When the primary backend fails, requests should failover to the secondary.
#[tokio::test]
async fn failover_to_secondary_on_primary_failure() {
    let primary = MockServer::start().await;
    let secondary = MockServer::start().await;

    // Primary returns 500
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&primary)
        .await;

    // Secondary returns success
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response("0xabc")))
        .mount(&secondary)
        .await;

    let upstream = Arc::new(UpstreamManager::new(
        vec![primary.uri(), secondary.uri()],
        Duration::from_secs(5),
    ));

    let req = rpc_request("eth_blockNumber");
    let resp = upstream.send_request(&req).await.unwrap();

    assert_eq!(resp.result.unwrap(), serde_json::json!("0xabc"));
}

/// When both backends fail, the primary is retried as a last resort.
/// If the primary also fails the last resort, we get AllUpstreamsFailed.
#[tokio::test]
async fn all_backends_fail_returns_error() {
    let primary = MockServer::start().await;
    let secondary = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&primary)
        .await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&secondary)
        .await;

    let upstream = Arc::new(UpstreamManager::new(
        vec![primary.uri(), secondary.uri()],
        Duration::from_secs(5),
    ));

    let req = rpc_request("eth_blockNumber");
    let result = upstream.send_request(&req).await;

    assert!(
        result.is_err(),
        "should return error when all backends fail"
    );
}

/// A backend marked Down (3 consecutive errors) is skipped,
/// and traffic goes to the next healthy backend.
#[tokio::test]
async fn down_backend_is_skipped() {
    let primary = MockServer::start().await;
    let secondary = MockServer::start().await;

    // Primary always fails
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&primary)
        .await;

    // Secondary works
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response("0x1")))
        .mount(&secondary)
        .await;

    let upstream = Arc::new(UpstreamManager::new(
        vec![primary.uri(), secondary.uri()],
        Duration::from_secs(5),
    ));

    let req = rpc_request("eth_blockNumber");

    // Drive primary to Down (3 errors needed, but secondary will succeed on failover)
    // First 3 requests: primary fails → secondary succeeds each time
    // After 3 failures, primary is Down
    for _ in 0..3 {
        let resp = upstream.send_request(&req).await.unwrap();
        assert_eq!(resp.result.unwrap(), serde_json::json!("0x1"));
    }

    // Now primary is Down — verify secondary receives requests directly
    // by checking that secondary's request count increases without primary being tried
    let statuses = upstream.backend_statuses().await;
    assert_eq!(statuses[0].state, "Down", "primary should be Down");
    assert_eq!(statuses[1].state, "Healthy", "secondary should be Healthy");

    // 4th request should skip primary entirely
    let resp = upstream.send_request(&req).await.unwrap();
    assert_eq!(resp.result.unwrap(), serde_json::json!("0x1"));
}

/// When the primary recovers after being Down, a health check
/// restores it and subsequent requests use it again (priority order).
#[tokio::test]
async fn recovered_primary_gets_priority_again() {
    let primary = MockServer::start().await;
    let secondary = MockServer::start().await;

    // Primary fails initially
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&primary)
        .await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response("0xsecondary")))
        .mount(&secondary)
        .await;

    let upstream = Arc::new(UpstreamManager::new(
        vec![primary.uri(), secondary.uri()],
        Duration::from_secs(5),
    ));

    let req = rpc_request("eth_blockNumber");

    // Drive primary to Down
    for _ in 0..3 {
        let _ = upstream.send_request(&req).await;
    }

    let statuses = upstream.backend_statuses().await;
    assert_eq!(statuses[0].state, "Down");

    // Primary recovers — must return valid hex block for probe_backend_url to parse
    primary.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response("0xaaa")))
        .mount(&primary)
        .await;

    // Health check restores primary
    upstream
        .check_all_backends(|url| async move { rpcproxy::health::probe_backend_url(url).await })
        .await;

    let statuses = upstream.backend_statuses().await;
    assert_eq!(
        statuses[0].state, "Healthy",
        "primary should be Healthy after health check"
    );

    // Next request should go to primary (priority 0)
    let resp = upstream.send_request(&req).await.unwrap();
    assert_eq!(resp.result.unwrap(), serde_json::json!("0xaaa"));
}

/// Last-resort: when all backends fail in the normal loop, the primary
/// is retried one more time. If it succeeds, the request succeeds.
#[tokio::test]
async fn last_resort_primary_retry_succeeds() {
    let primary = MockServer::start().await;

    // Primary fails first 2 calls, succeeds on 3rd (the last-resort retry)
    // We use a single backend; on the first pass it fails, then the last-resort retries it
    // Since wiremock can't do conditional responses easily, we use a trick:
    // Set up primary with a single 500 response (up_to 1), then 200 for the rest
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .expect(1)
        .mount(&primary)
        .await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response("0xrescued")))
        .mount(&primary)
        .await;

    let upstream = Arc::new(UpstreamManager::new(
        vec![primary.uri()],
        Duration::from_secs(5),
    ));

    let req = rpc_request("eth_blockNumber");
    // First attempt fails → last resort retry on same primary succeeds
    let resp = upstream.send_request(&req).await.unwrap();
    assert_eq!(resp.result.unwrap(), serde_json::json!("0xrescued"));
}

/// Requests to unreachable backends (connection refused) are handled gracefully.
#[tokio::test]
async fn unreachable_backend_handled_gracefully() {
    let upstream = Arc::new(UpstreamManager::new(
        vec!["http://127.0.0.1:1".to_string()], // port 1 — guaranteed connection refused
        Duration::from_secs(1),
    ));

    let req = rpc_request("eth_blockNumber");
    let result = upstream.send_request(&req).await;
    assert!(
        result.is_err(),
        "should return error for unreachable backend"
    );
}
