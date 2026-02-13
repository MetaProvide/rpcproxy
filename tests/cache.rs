use std::sync::Arc;
use std::time::Duration;

use rpcproxy::cache::policy::{self, IMMUTABLE_TTL_SECS};
use rpcproxy::cache::RpcCache;
use rpcproxy::jsonrpc::{JsonRpcRequest, JsonRpcResponse};

const IMMUTABLE_TTL: Duration = Duration::from_secs(IMMUTABLE_TTL_SECS);

// ---------------------------------------------------------------------------
// Policy
// ---------------------------------------------------------------------------

#[test]
fn policy_should_cache() {
    assert!(policy::should_cache("eth_blockNumber"));
    assert!(policy::should_cache("eth_getBlockByHash"));
    assert!(!policy::should_cache("eth_sendRawTransaction"));
    assert!(!policy::should_cache("eth_sendTransaction"));
    assert!(!policy::should_cache("personal_sign"));
}

#[test]
fn policy_ttl_immutable_methods() {
    let default = Duration::from_millis(2000);

    let req: JsonRpcRequest = serde_json::from_str(
        r#"{"jsonrpc":"2.0","method":"eth_getTransactionReceipt","params":["0xabc"],"id":1}"#,
    )
    .unwrap();
    assert_eq!(policy::ttl_for_request(&req, default), IMMUTABLE_TTL);

    let req: JsonRpcRequest =
        serde_json::from_str(r#"{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}"#)
            .unwrap();
    assert_eq!(policy::ttl_for_request(&req, default), IMMUTABLE_TTL);
}

#[test]
fn policy_ttl_short_lived_methods() {
    let default = Duration::from_millis(2000);

    let req: JsonRpcRequest =
        serde_json::from_str(r#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}"#)
            .unwrap();
    assert_eq!(policy::ttl_for_request(&req, default), default);

    let req: JsonRpcRequest =
        serde_json::from_str(r#"{"jsonrpc":"2.0","method":"eth_gasPrice","params":[],"id":1}"#)
            .unwrap();
    assert_eq!(policy::ttl_for_request(&req, default), default);
}

#[test]
fn policy_ttl_block_by_number_specific() {
    let default = Duration::from_millis(2000);

    let req: JsonRpcRequest = serde_json::from_str(
        r#"{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["0x123",true],"id":1}"#,
    )
    .unwrap();
    assert_eq!(policy::ttl_for_request(&req, default), IMMUTABLE_TTL);

    let req: JsonRpcRequest = serde_json::from_str(
        r#"{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["latest",true],"id":1}"#,
    )
    .unwrap();
    assert_eq!(policy::ttl_for_request(&req, default), default);
}

#[test]
fn policy_ttl_get_logs_with_block_hash() {
    let default = Duration::from_millis(2000);

    let req: JsonRpcRequest = serde_json::from_str(
        r#"{"jsonrpc":"2.0","method":"eth_getLogs","params":[{"blockHash":"0xabc"}],"id":1}"#,
    )
    .unwrap();
    assert_eq!(policy::ttl_for_request(&req, default), IMMUTABLE_TTL);

    let req: JsonRpcRequest = serde_json::from_str(
        r#"{"jsonrpc":"2.0","method":"eth_getLogs","params":[{"fromBlock":"0x1","toBlock":"0x2"}],"id":1}"#,
    ).unwrap();
    assert_eq!(policy::ttl_for_request(&req, default), default);
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[tokio::test]
async fn store_get_miss() {
    let cache = RpcCache::new(100, 2000);
    assert!(cache.get("nonexistent").await.is_none());
}

#[tokio::test]
async fn store_insert_and_get() {
    let cache = RpcCache::new(100, 2000);
    let resp = Arc::new(JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        result: Some(serde_json::json!("0x123")),
        error: None,
        id: serde_json::json!(1),
    });
    cache
        .insert("key1".to_string(), resp.clone(), Duration::from_secs(60))
        .await;
    let cached = cache.get("key1").await;
    assert!(cached.is_some());
    assert_eq!(cached.unwrap().result, resp.result);
}
