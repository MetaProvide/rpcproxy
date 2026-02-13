use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    pub id: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcResponse {
    pub fn error(id: serde_json::Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
            id,
        }
    }

    pub fn parse_error() -> Self {
        Self::error(serde_json::Value::Null, -32700, "Parse error")
    }

    pub fn invalid_request(id: serde_json::Value) -> Self {
        Self::error(id, -32600, "Invalid request")
    }

    pub fn internal_error(id: serde_json::Value) -> Self {
        Self::error(id, -32603, "Internal error")
    }
}

impl JsonRpcRequest {
    pub fn cache_key(&self) -> String {
        let mut params = self.params.clone();
        normalize_value(&mut params);
        format!("{}:{}", self.method, serde_json::to_string(&params).unwrap_or_default())
    }

    pub fn is_valid(&self) -> bool {
        self.jsonrpc == "2.0" && !self.method.is_empty()
    }
}

fn normalize_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for v in map.values_mut() {
                normalize_value(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                normalize_value(v);
            }
        }
        _ => {}
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcBody {
    Single(JsonRpcRequest),
    Batch(Vec<JsonRpcRequest>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_single_request() {
        let json = r#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.method, "eth_blockNumber");
        assert_eq!(req.jsonrpc, "2.0");
        assert!(req.is_valid());
    }

    #[test]
    fn test_parse_batch_request() {
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
    fn test_invalid_json_returns_parse_error() {
        let result = serde_json::from_str::<JsonRpcBody>("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_request_missing_method() {
        let json = r#"{"jsonrpc":"2.0","method":"","params":[],"id":1}"#;
        let req: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert!(!req.is_valid());
    }

    #[test]
    fn test_cache_key_ignores_id() {
        let req1: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}"#,
        ).unwrap();
        let req2: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":999}"#,
        ).unwrap();
        assert_eq!(req1.cache_key(), req2.cache_key());
    }

    #[test]
    fn test_cache_key_different_params() {
        let req1: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["0x1",true],"id":1}"#,
        ).unwrap();
        let req2: JsonRpcRequest = serde_json::from_str(
            r#"{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["0x2",true],"id":1}"#,
        ).unwrap();
        assert_ne!(req1.cache_key(), req2.cache_key());
    }

    #[test]
    fn test_error_response_serialization() {
        let resp = JsonRpcResponse::parse_error();
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("-32700"));
        assert!(json.contains("Parse error"));
    }
}
