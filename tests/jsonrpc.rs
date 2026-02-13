use rpcproxy::jsonrpc::{JsonRpcBody, JsonRpcRequest, JsonRpcResponse};

#[test]
fn parse_single_request() {
    let json = r#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}"#;
    let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
    assert_eq!(req.method, "eth_blockNumber");
    assert_eq!(req.jsonrpc, "2.0");
    assert!(req.is_valid());
}

#[test]
fn parse_batch_request() {
    let json = r#"[
        {"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1},
        {"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":2}
    ]"#;
    let body: JsonRpcBody = serde_json::from_str(json).unwrap();
    match body {
        JsonRpcBody::Batch(reqs) => assert_eq!(reqs.len(), 2),
        _ => panic!("expected batch"),
    }
}

#[test]
fn invalid_json_returns_parse_error() {
    let result = serde_json::from_str::<JsonRpcBody>("not json");
    assert!(result.is_err());
}

#[test]
fn invalid_request_missing_method() {
    let json = r#"{"jsonrpc":"2.0","method":"","params":[],"id":1}"#;
    let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
    assert!(!req.is_valid());
}

#[test]
fn cache_key_ignores_id() {
    let req1: JsonRpcRequest =
        serde_json::from_str(r#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}"#)
            .unwrap();
    let req2: JsonRpcRequest = serde_json::from_str(
        r#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":999}"#,
    )
    .unwrap();
    assert_eq!(req1.cache_key(), req2.cache_key());
}

#[test]
fn cache_key_different_params() {
    let req1: JsonRpcRequest = serde_json::from_str(
        r#"{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["0x1",true],"id":1}"#,
    )
    .unwrap();
    let req2: JsonRpcRequest = serde_json::from_str(
        r#"{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["0x2",true],"id":1}"#,
    )
    .unwrap();
    assert_ne!(req1.cache_key(), req2.cache_key());
}

#[test]
fn error_response_serialization() {
    let resp = JsonRpcResponse::parse_error();
    let json = serde_json::to_string(&resp).unwrap();
    assert!(json.contains("-32700"));
    assert!(json.contains("Parse error"));
}
