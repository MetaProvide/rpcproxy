mod cache;
mod config;
mod error;
mod health;
mod jsonrpc;
mod upstream;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use axum::Router;
use clap::Parser;
use tracing::{error, info, warn};

use cache::RpcCache;
use config::Config;
use jsonrpc::{JsonRpcBody, JsonRpcRequest, JsonRpcResponse};
use upstream::UpstreamManager;

#[derive(Clone)]
struct AppState {
    upstream: Arc<UpstreamManager>,
    cache: RpcCache,
    token: Option<String>,
}

#[tokio::main]
async fn main() {
    let config = Config::parse();

    let log_level = if config.verbose { "debug,hyper=info,reqwest=info" } else { "warn,rpcproxy=info" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level)),
        )
        .init();

    let token = config.token.clone().filter(|t| !t.is_empty());

    info!(
        port = %config.port,
        targets = ?config.targets,
        cache_ttl = %config.cache_ttl,
        health_interval = %config.health_interval,
        auth = token.is_some(),
        verbose = config.verbose,
        "starting rpcproxy"
    );

    if let Some(ref t) = token {
        info!(path = %format!("/{t}"), "token auth enabled via URL path");
    }

    let upstream = Arc::new(UpstreamManager::new(
        config.targets.clone(),
        Duration::from_secs(config.request_timeout),
    ));

    let cache = RpcCache::new(config.cache_max_size, config.cache_ttl);

    let state = AppState {
        upstream: upstream.clone(),
        cache,
        token,
    };

    // Spawn health checker
    tokio::spawn(health::start_health_checker(
        upstream.clone(),
        config.health_interval,
    ));

    let app = Router::new()
        .route("/health", get(health_handler))
        .route("/readiness", get(readiness_handler))
        .route("/status", get(status_handler))
        .route("/{token}", post(token_rpc_handler))
        .fallback(post(open_rpc_handler))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind");

    info!(addr = %addr, "rpcproxy listening");
    axum::serve(listener, app).await.expect("server error");
}

/// Lightweight health check for Docker HEALTHCHECK.
/// Returns 200 only if at least one backend is healthy AND has returned a real block number.
async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let ok = state.upstream.has_healthy_backend_with_block().await;
    if ok {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "unavailable")
    }
}

/// Readiness probe — same as health but returns JSON detail.
async fn readiness_handler(State(state): State<AppState>) -> impl IntoResponse {
    let statuses = state.upstream.backend_statuses().await;
    let ok = statuses.iter().any(|s| s.state == "Healthy" && s.latest_block.is_some());

    let body = serde_json::json!({
        "status": if ok { "ok" } else { "unavailable" },
        "backends": statuses,
    });

    if ok {
        (StatusCode::OK, Json(body))
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(body))
    }
}

/// Detailed status endpoint showing all targets, their states, and usage statistics.
async fn status_handler(State(state): State<AppState>) -> impl IntoResponse {
    let statuses = state.upstream.backend_statuses().await;
    let cache_entries = state.cache.entry_count().await;

    let healthy_count = statuses.iter().filter(|s| s.state == "Healthy").count();
    let total = statuses.len();

    let body = serde_json::json!({
        "healthy_backends": healthy_count,
        "total_backends": total,
        "cache_entries": cache_entries,
        "backends": statuses,
    });

    (StatusCode::OK, Json(body))
}

/// RPC handler for token-authenticated path: POST /<token>
async fn token_rpc_handler(
    State(state): State<AppState>,
    Path(path_token): Path<String>,
    body: String,
) -> impl IntoResponse {
    if let Some(expected_token) = &state.token {
        if path_token != *expected_token {
            warn!("unauthorized RPC request (bad token path)");
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::to_value(
                    JsonRpcResponse::error(serde_json::Value::Null, -32000, "Unauthorized"),
                ).unwrap()),
            );
        }
    }
    dispatch_rpc(&state, body).await
}

/// RPC handler for open access: POST /
async fn open_rpc_handler(
    State(state): State<AppState>,
    body: String,
) -> impl IntoResponse {
    if state.token.is_some() {
        warn!("unauthorized RPC request (missing token path)");
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::to_value(
                JsonRpcResponse::error(serde_json::Value::Null, -32000, "Unauthorized"),
            ).unwrap()),
        );
    }
    dispatch_rpc(&state, body).await
}

async fn dispatch_rpc(
    state: &AppState,
    body: String,
) -> (StatusCode, Json<serde_json::Value>) {
    let parsed = match serde_json::from_str::<JsonRpcBody>(&body) {
        Ok(parsed) => parsed,
        Err(_) => {
            let resp = JsonRpcResponse::parse_error();
            return (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()));
        }
    };

    match parsed {
        JsonRpcBody::Single(request) => {
            let resp = handle_single_request(state, request).await;
            (StatusCode::OK, Json(serde_json::to_value(resp).unwrap()))
        }
        JsonRpcBody::Batch(requests) => {
            let mut responses = Vec::with_capacity(requests.len());
            for request in requests {
                let resp = handle_single_request(state, request).await;
                responses.push(resp);
            }
            (StatusCode::OK, Json(serde_json::to_value(responses).unwrap()))
        }
    }
}

async fn handle_single_request(state: &AppState, request: JsonRpcRequest) -> JsonRpcResponse {
    if !request.is_valid() {
        return JsonRpcResponse::invalid_request(request.id);
    }

    let original_id = request.id.clone();
    let cache_key = request.cache_key();
    let should_cache = RpcCache::should_cache(&request.method);

    // Check cache
    if should_cache {
        if let Some(cached) = state.cache.get(&cache_key).await {
            let mut resp = (*cached).clone();
            resp.id = original_id;
            return resp;
        }

        // Check for in-flight request (coalescing)
        if let Some(mut rx) = state.cache.subscribe_inflight(&cache_key).await {
            if let Ok(resp) = rx.recv().await {
                let mut resp = (*resp).clone();
                resp.id = original_id;
                return resp;
            }
        }
    }

    // Register in-flight
    let tx = if should_cache {
        Some(state.cache.register_inflight(&cache_key).await)
    } else {
        None
    };

    // Forward to upstream
    let result = state.upstream.send_request(&request).await;

    match result {
        Ok(mut response) => {
            response.id = original_id;

            if should_cache && response.error.is_none() {
                let ttl = RpcCache::ttl_for_request(&request, state.cache.default_ttl());
                let cached = Arc::new(response.clone());
                state.cache.insert(cache_key.clone(), cached.clone(), ttl).await;

                if let Some(tx) = tx {
                    let _ = tx.send(cached);
                }
                state.cache.remove_inflight(&cache_key).await;
            } else if let Some(_tx) = tx {
                state.cache.remove_inflight(&cache_key).await;
            }

            response
        }
        Err(e) => {
            if let Some(_tx) = tx {
                state.cache.remove_inflight(&cache_key).await;
            }
            error!(method = %request.method, error = %e, "all upstreams failed");
            JsonRpcResponse::internal_error(request.id)
        }
    }
}
