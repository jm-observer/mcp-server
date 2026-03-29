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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolsCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourcesCapability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<PromptsCapability>,
}

#[derive(Debug, Serialize)]
pub struct ToolsCapability {}

#[derive(Debug, Serialize)]
pub struct ResourcesCapability {
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct PromptsCapability {
    #[serde(rename = "listChanged", skip_serializing_if = "Option::is_none")]
    pub list_changed: Option<bool>,
}

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
    #[serde(rename = "inputSchema")]
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

// ===== Resources =====

/// resources/list 响应中的单个资源
#[derive(Debug, Serialize, Clone)]
pub struct ResourceInfo {
    pub uri: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// resources/list 响应
#[derive(Debug, Serialize)]
pub struct ResourcesListResult {
    pub resources: Vec<ResourceInfo>,
}

/// resources/read 响应中的资源内容
#[derive(Debug, Serialize)]
pub struct ResourceContent {
    pub uri: String,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
}

/// resources/read 响应
#[derive(Debug, Serialize)]
pub struct ResourceReadResult {
    pub contents: Vec<ResourceContent>,
}

/// resources/read 请求参数
#[derive(Debug, Deserialize)]
pub struct ResourceReadParams {
    pub uri: String,
}

/// resources/templates/list 响应中的资源模板
#[derive(Debug, Serialize, Clone)]
pub struct ResourceTemplate {
    #[serde(rename = "uriTemplate")]
    pub uri_template: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "mimeType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// resources/templates/list 响应
#[derive(Debug, Serialize)]
pub struct ResourceTemplatesListResult {
    #[serde(rename = "resourceTemplates")]
    pub resource_templates: Vec<ResourceTemplate>,
}

// ===== Prompts =====

/// prompts/list 响应中的单个 prompt
#[derive(Debug, Serialize, Clone)]
pub struct PromptInfo {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Vec<PromptArgument>>,
}

/// prompt 参数定义
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PromptArgument {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// prompts/list 响应
#[derive(Debug, Serialize)]
pub struct PromptsListResult {
    pub prompts: Vec<PromptInfo>,
}

/// prompts/get 请求参数
#[derive(Debug, Deserialize)]
pub struct PromptGetParams {
    pub name: String,
    #[serde(default)]
    pub arguments: Option<HashMap<String, String>>,
}

/// prompts/get 响应中的消息
#[derive(Debug, Serialize)]
pub struct PromptMessage {
    pub role: String,
    pub content: PromptContent,
}

/// prompt 消息内容
#[derive(Debug, Serialize)]
pub struct PromptContent {
    pub r#type: String,
    pub text: String,
}

/// prompts/get 响应
#[derive(Debug, Serialize)]
pub struct PromptGetResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub messages: Vec<PromptMessage>,
}
