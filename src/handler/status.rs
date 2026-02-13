use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json};
use tracing::warn;

use super::auth::check_bearer_token;
use super::AppState;

/// Lightweight health check for Docker HEALTHCHECK.
/// Returns 200 only if at least one backend is healthy AND has returned a real block number.
pub async fn health_handler(State(state): State<AppState>) -> impl IntoResponse {
    let ok = state.upstream.has_healthy_backend_with_block().await;
    if ok {
        (StatusCode::OK, "ok")
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, "unavailable")
    }
}

/// Readiness probe — same as health but returns JSON detail.
pub async fn readiness_handler(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if !check_bearer_token(&state, &headers) {
        warn!("unauthorized readiness request (missing or bad token)");
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "Unauthorized" })));
    }

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
pub async fn status_handler(State(state): State<AppState>, headers: HeaderMap) -> impl IntoResponse {
    if !check_bearer_token(&state, &headers) {
        warn!("unauthorized status request (missing or bad token)");
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "Unauthorized" })),
        );
    }

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
