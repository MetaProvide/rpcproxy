use std::time::Duration;

use crate::jsonrpc::JsonRpcRequest;

pub const IMMUTABLE_TTL_SECS: u64 = 3600;
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
