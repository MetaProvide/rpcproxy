use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
    pub id: serde_json::Value,
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
