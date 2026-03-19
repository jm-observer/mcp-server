use std::sync::Arc;
use log::debug;
use serde_json::Value;
use crate::config::{ServerConfig, ToolRegistry};
use super::types::*;

pub struct McpHandler {
    registry: Arc<ToolRegistry>,
    server_config: Arc<ServerConfig>,
}

impl McpHandler {
    pub fn new(registry: Arc<ToolRegistry>, server_config: Arc<ServerConfig>) -> Self {
        Self {
            registry,
            server_config,
        }
    }

    pub async fn handle_request(&self, request: &str) -> Option<String> {
        let req: JsonRpcRequest = match serde_json::from_str(request) {
            Ok(r) => r,
            Err(_) => {
                let err_res = JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: None,
                    result: None,
                    error: Some(JsonRpcError::parse_error()),
                };
                return Some(serde_json::to_string(&err_res).unwrap());
            }
        };

        if req.jsonrpc != "2.0" {
            let err_res = JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: req.id.clone(),
                result: None,
                error: Some(JsonRpcError::invalid_request()),
            };
            return Some(serde_json::to_string(&err_res).unwrap());
        }
        debug!("{request}");
        let is_notification = req.id.is_none();

        let response = match req.method.as_str() {
            "initialize" => Some(self.handle_initialize(req.id.clone().unwrap_or(Value::Null))),
            "initialized" => None, // notification
            "ping" => Some(self.handle_ping(req.id.clone().unwrap_or(Value::Null))),
            "tools/list" => Some(self.handle_tools_list(req.id.clone().unwrap_or(Value::Null))),
            "tools/call" => {
                if is_notification {
                    None
                } else {
                    Some(self.handle_tools_call(req.id.clone().unwrap(), req.params).await)
                }
            }
            _ => {
                if is_notification {
                    None
                } else {
                    Some(JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        id: req.id.clone(),
                        result: None,
                        error: Some(JsonRpcError::method_not_found()),
                    })
                }
            }
        };

        if is_notification {
            None
        } else {
            response.map(|r| serde_json::to_string(&r).unwrap())
        }
    }

    fn handle_initialize(&self, id: Value) -> JsonRpcResponse {
        let result = InitializeResult {
            protocol_version: "2025-03-26".into(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {}),
            },
            server_info: ServerInfo {
                name: "mcp-server".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
        };

        JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result: Some(serde_json::to_value(result).unwrap()),
            error: None,
        }
    }

    fn handle_ping(&self, id: Value) -> JsonRpcResponse {
        JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result: Some(serde_json::json!({})),
            error: None,
        }
    }

    fn handle_tools_list(&self, id: Value) -> JsonRpcResponse {
        let mut tools = vec![];
        for tool in self.registry.list_tools() {
            let mut properties = serde_json::Map::new();
            let mut required = vec![];

            if let Some(params) = &tool.def.parameters {
                for param in params {
                    let mut prop_schema = serde_json::Map::new();
                    prop_schema.insert("type".into(), Value::String(param.r#type.clone()));
                    prop_schema.insert("description".into(), Value::String(param.description.clone()));
                    properties.insert(param.name.clone(), Value::Object(prop_schema));
                    if param.required {
                        required.push(Value::String(param.name.clone()));
                    }
                }
            }

            let mut input_schema = serde_json::Map::new();
            input_schema.insert("type".into(), Value::String("object".into()));
            if !properties.is_empty() {
                input_schema.insert("properties".into(), Value::Object(properties));
            }
            if !required.is_empty() {
                input_schema.insert("required".into(), Value::Array(required));
            }

            tools.push(ToolInfo {
                name: tool.def.name.clone(),
                description: tool.def.description.clone(),
                input_schema: Value::Object(input_schema),
            });
        }

        let result = ToolsListResult { tools };

        JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result: Some(serde_json::to_value(result).unwrap()),
            error: None,
        }
    }

    async fn handle_tools_call(&self, id: Value, params: Option<Value>) -> JsonRpcResponse {
        let call_params: ToolCallParams = match params {
            Some(p) => match serde_json::from_value(p) {
                Ok(p) => p,
                Err(e) => return JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: Some(id),
                    result: None,
                    error: Some(JsonRpcError::invalid_params(&e.to_string())),
                },
            },
            None => return JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: Some(id),
                result: None,
                error: Some(JsonRpcError::invalid_params("missing params")),
            },
        };

        let tool = match self.registry.get(&call_params.name) {
            Some(t) => t,
            None => return JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: Some(id),
                result: None,
                error: Some(JsonRpcError::invalid_params(&format!("Tool not found: {}", call_params.name))),
            },
        };

        // Validate required params
        let provided_args = call_params.arguments.unwrap_or_default();
        if let Some(defined_params) = &tool.def.parameters {
            for param in defined_params {
                if param.required && !provided_args.contains_key(&param.name) {
                    return JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        id: Some(id),
                        result: None,
                        error: Some(JsonRpcError::invalid_params(&format!("Missing required parameter: {}", param.name))),
                    };
                }
            }
        }

        use crate::config::ToolType;
        use crate::executor::command::CommandExecutor;
        use crate::executor::http::HttpExecutor;
        
        if call_params.name == "direct_command" {
            let executor = CommandExecutor::new(self.server_config.defaults.allowed_dirs.clone());
            let cmd_str = provided_args.get("command").and_then(|v| v.as_str()).unwrap_or_default();
            
            let mut parsed_args = Vec::new();
            if let Some(Value::Array(arr)) = provided_args.get("args") {
                for v in arr {
                    if let Some(s) = v.as_str() {
                        parsed_args.push(s.to_string());
                    }
                }
            } else if let Some(Value::String(s)) = provided_args.get("args") {
                parsed_args = s.split_whitespace().map(|s| s.to_string()).collect();
            }
            
            let wd = provided_args.get("working_dir").and_then(|v| v.as_str()).map(std::path::Path::new);

            match executor.execute_direct(cmd_str, &parsed_args, wd).await {
                Ok(res) => {
                    let mut content = vec![];
                    if !res.stdout.is_empty() {
                        content.push(ContentBlock { r#type: "text".into(), text: res.stdout });
                    }
                    if !res.stderr.is_empty() {
                        content.push(ContentBlock { r#type: "text".into(), text: res.stderr });
                    }
                    if content.is_empty() {
                        content.push(ContentBlock { r#type: "text".into(), text: "(Empty Output)".into() });
                    }
                    let call_result = ToolCallResult {
                        content,
                        is_error: if res.exit_code != 0 { Some(true) } else { None },
                    };
                    JsonRpcResponse { jsonrpc: "2.0".into(), id: Some(id), result: Some(serde_json::to_value(call_result).unwrap()), error: None }
                }
                Err(e) => {
                    let call_result = ToolCallResult {
                        content: vec![ContentBlock { r#type: "text".into(), text: format!("Execution Error: {}", e) }],
                        is_error: Some(true),
                    };
                    JsonRpcResponse { jsonrpc: "2.0".into(), id: Some(id), result: Some(serde_json::to_value(call_result).unwrap()), error: None }
                }
            }
        } else if tool.def.r#type == Some(ToolType::Command) {
            let executor = CommandExecutor::new(self.server_config.defaults.allowed_dirs.clone());
            match executor.execute(tool, &provided_args).await {
                Ok(res) => {
                    let mut content = vec![];
                    if !res.stdout.is_empty() {
                        content.push(ContentBlock {
                            r#type: "text".into(),
                            text: res.stdout,
                        });
                    }
                    if !res.stderr.is_empty() {
                        content.push(ContentBlock {
                            r#type: "text".into(),
                            text: res.stderr,
                        });
                    }
                    if content.is_empty() {
                        content.push(ContentBlock {
                            r#type: "text".into(),
                            text: "(Empty Output)".into(),
                        });
                    }

                    let mut is_error = None;
                    if res.exit_code != 0 {
                        is_error = Some(true);
                    }

                    let call_result = ToolCallResult {
                        content,
                        is_error,
                    };

                    JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        id: Some(id),
                        result: Some(serde_json::to_value(call_result).unwrap()),
                        error: None,
                    }
                }
                Err(e) => {
                    // Convert execution error to ToolCallResult error message
                    let call_result = ToolCallResult {
                        content: vec![ContentBlock {
                            r#type: "text".into(),
                            text: format!("Execution Error: {}", e),
                        }],
                        is_error: Some(true),
                    };

                    JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        id: Some(id),
                        result: Some(serde_json::to_value(call_result).unwrap()),
                        error: None,
                    }
                }
            }
        } else if tool.def.r#type == Some(ToolType::Http) {
            let executor = HttpExecutor::new();
            match executor.execute(tool, &provided_args).await {
                Ok(res) => {
                    let content = vec![ContentBlock {
                        r#type: "text".into(),
                        text: res.body,
                    }];
                    
                    let is_error = if res.status >= 400 { Some(true) } else { None };
                    
                    let call_result = ToolCallResult {
                        content,
                        is_error,
                    };

                    JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        id: Some(id),
                        result: Some(serde_json::to_value(call_result).unwrap()),
                        error: None,
                    }
                }
                Err(e) => {
                    let call_result = ToolCallResult {
                        content: vec![ContentBlock {
                            r#type: "text".into(),
                            text: format!("Executor Error: {}", e),
                        }],
                        is_error: Some(true),
                    };

                    JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        id: Some(id),
                        result: Some(serde_json::to_value(call_result).unwrap()),
                        error: None,
                    }
                }
            }
        } else {
            let call_result = ToolCallResult {
                content: vec![ContentBlock {
                    r#type: "text".into(),
                    text: "Not implemented or unknown tool type".into(),
                }],
                is_error: Some(true),
            };

            JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: Some(id),
                result: Some(serde_json::to_value(call_result).unwrap()),
                error: None,
            }
        }
    }
}
