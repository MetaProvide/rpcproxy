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

fn block_number_response(hex_block: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "result": hex_block,
        "id": 1
    })
}

#[tokio::test]
async fn notify_fires_when_backend_goes_down() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let upstream = Arc::new(UpstreamManager::new(
        vec![server.uri()],
        Duration::from_secs(5),
    ));

    let notify = upstream.health_notify();
    let req = rpc_request("eth_blockNumber");

    for _ in 0..3 {
        let _ = upstream.send_request(&req).await;
    }

    let fired = tokio::time::timeout(Duration::from_millis(100), notify.notified()).await;
    assert!(
        fired.is_ok(),
        "health_notify should fire when backend goes Down"
    );
}

#[tokio::test]
async fn reactive_check_recovers_backend() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(block_number_response("0x100")))
        .expect(1..)
        .mount(&server)
        .await;

    let upstream = Arc::new(UpstreamManager::new(
        vec![server.uri()],
        Duration::from_secs(5),
    ));

    upstream
        .check_all_backends(|url| async move { rpcproxy::health::probe_backend_url(url).await })
        .await;

    assert!(
        upstream.has_healthy_backend_with_block().await,
        "should be healthy after initial probe"
    );

    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let req = rpc_request("eth_blockNumber");
    for _ in 0..3 {
        let _ = upstream.send_request(&req).await;
    }

    assert!(
        !upstream.has_healthy_backend_with_block().await,
        "should be unhealthy after backend goes Down"
    );

    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(block_number_response("0x200")))
        .mount(&server)
        .await;

    upstream
        .check_all_backends(|url| async move { rpcproxy::health::probe_backend_url(url).await })
        .await;

    assert!(
        upstream.has_healthy_backend_with_block().await,
        "should be healthy again after reactive health check"
    );
}

#[tokio::test]
async fn checker_reacts_to_notify_signal() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(block_number_response("0x100")))
        .mount(&server)
        .await;

    let upstream = Arc::new(UpstreamManager::new(
        vec![server.uri()],
        Duration::from_secs(5),
    ));

    let health_upstream = upstream.clone();
    tokio::spawn(async move {
        rpcproxy::health::start_health_checker(health_upstream, 3600).await;
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(
        upstream.has_healthy_backend_with_block().await,
        "should be healthy after startup probe"
    );

    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let req = rpc_request("eth_blockNumber");
    for _ in 0..3 {
        let _ = upstream.send_request(&req).await;
    }

    assert!(
        !upstream.has_healthy_backend_with_block().await,
        "should be unhealthy after 3 failures"
    );

    tokio::time::sleep(Duration::from_millis(300)).await;

    server.reset().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(block_number_response("0x300")))
        .mount(&server)
        .await;

    upstream.health_notify().notify_one();

    tokio::time::sleep(Duration::from_millis(500)).await;

    assert!(
        upstream.has_healthy_backend_with_block().await,
        "health checker should have reactively re-probed and recovered the backend"
    );
}
