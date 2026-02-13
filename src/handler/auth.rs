use axum::http::HeaderMap;

use super::AppState;

/// Returns true if no token is configured or if the Authorization header matches.
pub fn check_bearer_token(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(expected) = &state.token else { return true };
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|t| t == expected.as_str())
        .unwrap_or(false)
}
