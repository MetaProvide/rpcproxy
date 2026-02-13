use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::routing::{get, post};
use tower::ServiceExt;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

use rpcproxy::cache::RpcCache;
use rpcproxy::handler;
use rpcproxy::handler::AppState;
use rpcproxy::upstream::UpstreamManager;

fn ok_response(result: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "result": result,
        "id": 1
    })
}

async fn setup(server_uri: &str, token: Option<&str>) -> Router {
    let upstream = Arc::new(UpstreamManager::new(
        vec![server_uri.to_string()],
        Duration::from_secs(5),
    ));
    let cache = RpcCache::new(1000, 2000);
    let state = AppState {
        upstream,
        cache,
        token: token.map(|t| t.to_string()),
    };

    Router::new()
        .route("/health", get(handler::status::health_handler))
        .route("/readiness", get(handler::status::readiness_handler))
        .route("/status", get(handler::status::status_handler))
        .route("/{token}", post(handler::rpc::token_rpc_handler))
        .fallback(post(handler::rpc::open_rpc_handler))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// Auth
// ---------------------------------------------------------------------------

/// Token-protected proxy rejects requests on the open endpoint.
#[tokio::test]
async fn auth_rejects_open_endpoint_when_token_set() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response("0x1")))
        .mount(&server)
        .await;

    let app = setup(&server.uri(), Some("secret")).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Token-protected proxy rejects requests with wrong token path.
#[tokio::test]
async fn auth_rejects_wrong_token_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response("0x1")))
        .mount(&server)
        .await;

    let app = setup(&server.uri(), Some("secret")).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/wrong-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Token-protected proxy accepts requests with correct token path.
#[tokio::test]
async fn auth_accepts_correct_token_path() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response("0x123")))
        .mount(&server)
        .await;

    let app = setup(&server.uri(), Some("secret")).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/secret")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["result"], "0x123");
}

/// Open proxy (no token) accepts requests on the fallback.
#[tokio::test]
async fn open_proxy_accepts_requests() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response("0xabc")))
        .mount(&server)
        .await;

    let app = setup(&server.uri(), None).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["result"], "0xabc");
}

// ---------------------------------------------------------------------------
// RPC dispatch
// ---------------------------------------------------------------------------

/// Invalid JSON body returns a JSON-RPC parse error.
#[tokio::test]
async fn invalid_json_returns_parse_error() {
    let server = MockServer::start().await;
    let app = setup(&server.uri(), None).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("content-type", "application/json")
                .body(Body::from("not valid json"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["error"]["code"], -32700);
}

/// Batch requests return an array of responses.
#[tokio::test]
async fn batch_request_returns_array() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response("0x1")))
        .mount(&server)
        .await;

    let app = setup(&server.uri(), None).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"[
                        {"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1},
                        {"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":2}
                    ]"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    let arr = body.as_array().expect("batch should return array");
    assert_eq!(arr.len(), 2);
}

/// Invalid request (empty method) returns -32600 invalid request error.
#[tokio::test]
async fn invalid_request_returns_error() {
    let server = MockServer::start().await;
    let app = setup(&server.uri(), None).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","method":"","params":[],"id":1}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert_eq!(body["error"]["code"], -32600);
}

// ---------------------------------------------------------------------------
// Status endpoints
// ---------------------------------------------------------------------------

/// /health returns 503 when no health check has run yet (no latest_block).
#[tokio::test]
async fn health_returns_503_before_first_probe() {
    let server = MockServer::start().await;
    let app = setup(&server.uri(), None).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

/// /status returns detailed backend info with auth.
#[tokio::test]
async fn status_returns_backend_info() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ok_response("0x1")))
        .mount(&server)
        .await;

    let app = setup(&server.uri(), Some("tok")).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/status")
                .header("authorization", "Bearer tok")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap(),
    )
    .unwrap();
    assert!(body["backends"].is_array());
    assert_eq!(body["total_backends"], 1);
}

/// /status rejects requests without valid bearer token.
#[tokio::test]
async fn status_rejects_without_auth() {
    let server = MockServer::start().await;
    let app = setup(&server.uri(), Some("tok")).await;

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
