mod auth;
pub mod rpc;
pub mod status;

use std::sync::Arc;

use crate::cache::RpcCache;
use crate::upstream::UpstreamManager;

#[derive(Clone)]
pub struct AppState {
    pub upstream: Arc<UpstreamManager>,
    pub cache: RpcCache,
    pub token: Option<String>,
}
