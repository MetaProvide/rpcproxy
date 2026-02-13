use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use tracing::{error, warn};

use crate::cache::policy as cache_policy;
use crate::jsonrpc::{JsonRpcBody, JsonRpcRequest, JsonRpcResponse};

use super::AppState;

/// RPC handler for token-authenticated path: POST /<token>
pub async fn token_rpc_handler(
    State(state): State<AppState>,
    Path(path_token): Path<String>,
    body: String,
) -> impl IntoResponse {
    if let Some(expected_token) = &state.token
        && path_token != *expected_token
    {
        warn!("unauthorized RPC request (bad token path)");
        return (
            StatusCode::UNAUTHORIZED,
            Json(
                serde_json::to_value(JsonRpcResponse::error(
                    serde_json::Value::Null,
                    -32000,
                    "Unauthorized",
                ))
                .unwrap(),
            ),
        );
    }
    dispatch_rpc(&state, body).await
}

/// RPC handler for open access: POST /
pub async fn open_rpc_handler(State(state): State<AppState>, body: String) -> impl IntoResponse {
    if state.token.is_some() {
        warn!("unauthorized RPC request (missing token path)");
        return (
            StatusCode::UNAUTHORIZED,
            Json(
                serde_json::to_value(JsonRpcResponse::error(
                    serde_json::Value::Null,
                    -32000,
                    "Unauthorized",
                ))
                .unwrap(),
            ),
        );
    }
    dispatch_rpc(&state, body).await
}

async fn dispatch_rpc(state: &AppState, body: String) -> (StatusCode, Json<serde_json::Value>) {
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
            (
                StatusCode::OK,
                Json(serde_json::to_value(responses).unwrap()),
            )
        }
    }
}

async fn handle_single_request(state: &AppState, request: JsonRpcRequest) -> JsonRpcResponse {
    if !request.is_valid() {
        return JsonRpcResponse::invalid_request(request.id);
    }

    let original_id = request.id.clone();
    let cache_key = request.cache_key();
    let should_cache = cache_policy::should_cache(&request.method);

    // Check cache
    if should_cache {
        if let Some(cached) = state.cache.get(&cache_key).await {
            let mut resp = (*cached).clone();
            resp.id = original_id;
            return resp;
        }

        // Check for in-flight request (coalescing)
        if let Some(mut rx) = state.cache.subscribe_inflight(&cache_key).await
            && let Ok(resp) = rx.recv().await
        {
            let mut resp = (*resp).clone();
            resp.id = original_id;
            return resp;
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
                let ttl = cache_policy::ttl_for_request(&request, state.cache.default_ttl());
                let cached = Arc::new(response.clone());
                state
                    .cache
                    .insert(cache_key.clone(), cached.clone(), ttl)
                    .await;

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
