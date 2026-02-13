use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;
use moka::Expiry;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tracing::trace;

use crate::jsonrpc::JsonRpcResponse;

#[derive(Clone)]
struct CacheEntry {
    response: Arc<JsonRpcResponse>,
    ttl: Duration,
}

struct PerEntryExpiry;

impl Expiry<String, CacheEntry> for PerEntryExpiry {
    fn expire_after_create(
        &self,
        _key: &String,
        value: &CacheEntry,
        _current_time: std::time::Instant,
    ) -> Option<Duration> {
        Some(value.ttl)
    }
}

#[derive(Clone)]
pub struct RpcCache {
    cache: Cache<String, CacheEntry>,
    default_ttl: Duration,
    inflight:
        Arc<RwLock<std::collections::HashMap<String, broadcast::Sender<Arc<JsonRpcResponse>>>>>,
}

impl RpcCache {
    pub fn new(max_size: u64, default_ttl_ms: u64) -> Self {
        let cache = Cache::builder()
            .max_capacity(max_size)
            .expire_after(PerEntryExpiry)
            .build();

        Self {
            cache,
            default_ttl: Duration::from_millis(default_ttl_ms),
            inflight: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    pub async fn get(&self, key: &str) -> Option<Arc<JsonRpcResponse>> {
        let result = self.cache.get(key).await;
        if let Some(entry) = &result {
            trace!(key = %key, "cache hit");
            return Some(entry.response.clone());
        }
        None
    }

    pub async fn insert(&self, key: String, response: Arc<JsonRpcResponse>, ttl: Duration) {
        self.cache.insert(key, CacheEntry { response, ttl }).await;
    }

    pub async fn subscribe_inflight(
        &self,
        key: &str,
    ) -> Option<broadcast::Receiver<Arc<JsonRpcResponse>>> {
        let inflight = self.inflight.read().await;
        inflight.get(key).map(|tx| tx.subscribe())
    }

    pub async fn register_inflight(&self, key: &str) -> broadcast::Sender<Arc<JsonRpcResponse>> {
        let (tx, _) = broadcast::channel(1);
        let mut inflight = self.inflight.write().await;
        inflight.insert(key.to_string(), tx.clone());
        tx
    }

    pub async fn remove_inflight(&self, key: &str) {
        let mut inflight = self.inflight.write().await;
        inflight.remove(key);
    }

    pub fn default_ttl(&self) -> Duration {
        self.default_ttl
    }

    pub async fn entry_count(&self) -> u64 {
        self.cache.entry_count()
    }
}
