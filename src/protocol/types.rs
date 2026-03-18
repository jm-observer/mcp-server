use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// JSON-RPC 2.0 请求
#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 响应
#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn parse_error() -> Self {
        Self {
            code: -32700,
            message: "Parse error".into(),
            data: None,
        }
    }
    pub fn invalid_request() -> Self {
        Self {
            code: -32600,
            message: "Invalid Request".into(),
            data: None,
        }
    }
    pub fn method_not_found() -> Self {
        Self {
            code: -32601,
            message: "Method not found".into(),
            data: None,
        }
    }
    pub fn invalid_params(msg: &str) -> Self {
        Self {
            code: -32602,
            message: format!("Invalid params: {}", msg),
            data: None,
        }
    }
    pub fn internal_error(msg: &str) -> Self {
        Self {
            code: -32603,
            message: format!("Internal error: {}", msg),
            data: None,
        }
    }
}

/// initialize 响应
#[derive(Debug, Serialize)]
pub struct InitializeResult {
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
}

#[derive(Debug, Serialize)]
pub struct ServerCapabilities {
    pub tools: Option<ToolsCapability>,
}

#[derive(Debug, Serialize)]
pub struct ToolsCapability {}

#[derive(Debug, Serialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// tools/list 响应
#[derive(Debug, Serialize)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Serialize)]
pub struct ToolsListResult {
    pub tools: Vec<ToolInfo>,
}

/// tools/call 请求参数
#[derive(Debug, Deserialize)]
pub struct ToolCallParams {
    pub name: String,
    pub arguments: Option<HashMap<String, Value>>,
}

/// tools/call 响应
#[derive(Debug, Serialize)]
pub struct ToolCallResult {
    pub content: Vec<ContentBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct ContentBlock {
    pub r#type: String,
    pub text: String,
}
