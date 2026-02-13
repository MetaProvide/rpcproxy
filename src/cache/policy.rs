use std::time::Duration;

use crate::jsonrpc::JsonRpcRequest;

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

#[cfg(test)]
mod tests {
    use super::*;

    const IMMUTABLE_TTL: Duration = Duration::from_secs(IMMUTABLE_TTL_SECS);

    #[test]
    fn test_should_cache() {
        assert!(should_cache("eth_blockNumber"));
        assert!(should_cache("eth_getBlockByHash"));
        assert!(!should_cache("eth_sendRawTransaction"));
        assert!(!should_cache("eth_sendTransaction"));
        assert!(!should_cache("personal_sign"));
    }

    #[test]
    fn test_ttl_immutable_methods() {
        let default = Duration::from_millis(2000);

        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_getTransactionReceipt","params":["0xabc"],"id":1}"#,
        ).unwrap();
        assert_eq!(ttl_for_request(&req, default), IMMUTABLE_TTL);

        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}"#,
        ).unwrap();
        assert_eq!(ttl_for_request(&req, default), IMMUTABLE_TTL);
    }

    #[test]
    fn test_ttl_short_lived_methods() {
        let default = Duration::from_millis(2000);

        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}"#,
        ).unwrap();
        assert_eq!(ttl_for_request(&req, default), default);

        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_gasPrice","params":[],"id":1}"#,
        ).unwrap();
        assert_eq!(ttl_for_request(&req, default), default);
    }

    #[test]
    fn test_ttl_block_by_number_specific() {
        let default = Duration::from_millis(2000);

        // Specific hex block → immutable
        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["0x123",true],"id":1}"#,
        ).unwrap();
        assert_eq!(ttl_for_request(&req, default), IMMUTABLE_TTL);

        // "latest" → short
        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["latest",true],"id":1}"#,
        ).unwrap();
        assert_eq!(ttl_for_request(&req, default), default);
    }

    #[test]
    fn test_ttl_get_logs_with_block_hash() {
        let default = Duration::from_millis(2000);

        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_getLogs","params":[{"blockHash":"0xabc"}],"id":1}"#,
        ).unwrap();
        assert_eq!(ttl_for_request(&req, default), IMMUTABLE_TTL);

        let req: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_getLogs","params":[{"fromBlock":"0x1","toBlock":"0x2"}],"id":1}"#,
        ).unwrap();
        assert_eq!(ttl_for_request(&req, default), default);
    }
}
