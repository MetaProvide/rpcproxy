use std::fmt;

#[derive(Debug)]
pub enum RpcProxyError {
    /// All upstream backends failed to handle the request
    AllUpstreamsFailed,
    /// A single upstream request failed
    UpstreamRequest(String),
    /// HTTP status error from upstream
    UpstreamHttp(u16),
    /// Failed to serialize/deserialize JSON
    Json(serde_json::Error),
    /// Failed to read response body
    BodyRead(String),
    /// Health probe failed
    HealthProbe(String),
}

impl fmt::Display for RpcProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AllUpstreamsFailed => write!(f, "all upstream backends failed"),
            Self::UpstreamRequest(e) => write!(f, "upstream request failed: {e}"),
            Self::UpstreamHttp(status) => write!(f, "upstream HTTP {status}"),
            Self::Json(e) => write!(f, "JSON error: {e}"),
            Self::BodyRead(e) => write!(f, "body read error: {e}"),
            Self::HealthProbe(e) => write!(f, "health probe failed: {e}"),
        }
    }
}

impl std::error::Error for RpcProxyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(e) => Some(e),
            _ => None,
        }
    }
}

impl From<serde_json::Error> for RpcProxyError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
