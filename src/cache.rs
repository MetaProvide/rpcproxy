use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;
use moka::Expiry;
use tokio::sync::broadcast;
use tokio::sync::RwLock;
use tracing::trace;

use crate::jsonrpc::{JsonRpcRequest, JsonRpcResponse};

const IMMUTABLE_TTL_SECS: u64 = 3600;
const NEVER_CACHE_METHODS: &[&str] = &[
    "eth_sendRawTransaction",
    "eth_sendTransaction",
    "personal_sign",
    "personal_unlockAccount",
    "personal_sendTransaction",
    "admin_addPeer",
    "admin_removePeer",
    "miner_start",
    "miner_stop",
    "debug_traceTransaction",
];

const IMMUTABLE_METHODS: &[&str] = &[
    "eth_getBlockByHash",
    "eth_getTransactionByHash",
    "eth_getTransactionReceipt",
    "eth_getTransactionByBlockHashAndIndex",
    "eth_getTransactionByBlockNumberAndIndex",
    "eth_getUncleByBlockHashAndIndex",
    "eth_getBlockTransactionCountByHash",
    "eth_getUncleCountByBlockHash",
    "net_version",
    "eth_chainId",
    "web3_clientVersion",
];

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
    inflight: Arc<RwLock<std::collections::HashMap<String, broadcast::Sender<Arc<JsonRpcResponse>>>>>,
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

    pub fn should_cache(method: &str) -> bool {
        !NEVER_CACHE_METHODS.contains(&method)
    }

    pub fn ttl_for_request(request: &JsonRpcRequest, default_ttl: Duration) -> Duration {
        let method = request.method.as_str();

        if IMMUTABLE_METHODS.contains(&method) {
            return Duration::from_secs(IMMUTABLE_TTL_SECS);
        }

        // eth_getBlockByNumber with a specific block number (not "latest"/"pending") is immutable
        if method == "eth_getBlockByNumber" {
            if let Some(block_param) = request.params.as_array().and_then(|a| a.first()) {
                if let Some(s) = block_param.as_str() {
                    if s.starts_with("0x") {
                        return Duration::from_secs(IMMUTABLE_TTL_SECS);
                    }
                }
            }
        }

        // eth_getLogs with a specific blockHash is immutable
        if method == "eth_getLogs" {
            if let Some(filter) = request.params.as_array().and_then(|a| a.first()) {
                if filter.get("blockHash").is_some() {
                    return Duration::from_secs(IMMUTABLE_TTL_SECS);
                }
            }
        }

        default_ttl
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
        self.cache
            .insert(key, CacheEntry { response, ttl })
            .await;
    }

    pub async fn subscribe_inflight(&self, key: &str) -> Option<broadcast::Receiver<Arc<JsonRpcResponse>>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_cache() {
        assert!(RpcCache::should_cache("eth_blockNumber"));
        assert!(RpcCache::should_cache("eth_getBlockByHash"));
        assert!(!RpcCache::should_cache("eth_sendRawTransaction"));
        assert!(!RpcCache::should_cache("eth_sendTransaction"));
        assert!(!RpcCache::should_cache("personal_sign"));
    }

    #[test]
    fn test_ttl_immutable_methods() {
        let default = Duration::from_millis(2000);

        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_getTransactionReceipt","params":["0xabc"],"id":1}"#,
        ).unwrap();
        assert_eq!(RpcCache::ttl_for_request(&req, default), Duration::from_secs(IMMUTABLE_TTL_SECS));

        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}"#,
        ).unwrap();
        assert_eq!(RpcCache::ttl_for_request(&req, default), Duration::from_secs(IMMUTABLE_TTL_SECS));
    }

    #[test]
    fn test_ttl_short_lived_methods() {
        let default = Duration::from_millis(2000);

        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}"#,
        ).unwrap();
        assert_eq!(RpcCache::ttl_for_request(&req, default), default);

        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_gasPrice","params":[],"id":1}"#,
        ).unwrap();
        assert_eq!(RpcCache::ttl_for_request(&req, default), default);
    }

    #[test]
    fn test_ttl_block_by_number_specific() {
        let default = Duration::from_millis(2000);

        // Specific hex block → immutable
        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["0x123",true],"id":1}"#,
        ).unwrap();
        assert_eq!(RpcCache::ttl_for_request(&req, default), Duration::from_secs(IMMUTABLE_TTL_SECS));

        // "latest" → short
        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["latest",true],"id":1}"#,
        ).unwrap();
        assert_eq!(RpcCache::ttl_for_request(&req, default), default);
    }

    #[test]
    fn test_ttl_get_logs_with_block_hash() {
        let default = Duration::from_millis(2000);

        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_getLogs","params":[{"blockHash":"0xabc"}],"id":1}"#,
        ).unwrap();
        assert_eq!(RpcCache::ttl_for_request(&req, default), Duration::from_secs(IMMUTABLE_TTL_SECS));

        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_getLogs","params":[{"fromBlock":"0x1","toBlock":"0x2"}],"id":1}"#,
        ).unwrap();
        assert_eq!(RpcCache::ttl_for_request(&req, default), default);
    }

    #[tokio::test]
    async fn test_cache_get_miss() {
        let cache = RpcCache::new(100, 2000);
        assert!(cache.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_cache_insert_and_get() {
        let cache = RpcCache::new(100, 2000);
        let resp = Arc::new(JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: Some(serde_json::json!("0x123")),
            error: None,
            id: serde_json::json!(1),
        });
        cache.insert("key1".to_string(), resp.clone(), Duration::from_secs(60)).await;
        let cached = cache.get("key1").await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().result, resp.result);
    }
}
