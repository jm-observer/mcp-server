use super::types::*;
use crate::config::{PromptRegistry, ServerConfig, ToolAction, ToolRegistry};
use log::{error, info};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone)]
pub struct McpHandler {
    registry: Arc<ToolRegistry>,
    server_config: Arc<ServerConfig>,
    prompt_registry: Arc<PromptRegistry>,
}

impl McpHandler {
    pub fn new(registry: Arc<ToolRegistry>, server_config: Arc<ServerConfig>) -> Self {
        Self {
            registry,
            server_config,
            prompt_registry: Arc::new(PromptRegistry::new()),
        }
    }

    pub fn with_prompts(
        registry: Arc<ToolRegistry>,
        server_config: Arc<ServerConfig>,
        prompt_registry: Arc<PromptRegistry>,
    ) -> Self {
        Self {
            registry,
            server_config,
            prompt_registry,
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
        info!("Processing request: {}", req.method);
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
            "resources/list" => Some(self.handle_resources_list(req.id.clone().unwrap_or(Value::Null))),
            "resources/read" => Some(self.handle_resources_read(req.id.clone().unwrap_or(Value::Null), req.params)),
            "resources/templates/list" => {
                Some(self.handle_resource_templates_list(req.id.clone().unwrap_or(Value::Null)))
            }
            "prompts/list" => Some(self.handle_prompts_list(req.id.clone().unwrap_or(Value::Null))),
            "prompts/get" => Some(self.handle_prompts_get(req.id.clone().unwrap_or(Value::Null), req.params)),
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
        let dirs = &self.server_config.defaults.directories;
        let instructions = if dirs.is_empty() {
            None
        } else {
            let text: String = dirs
                .iter()
                .map(|d| d.description.trim().to_string())
                .collect::<Vec<_>>()
                .join("\n");
            Some(text)
        };

        let resources_cap =
            if !self.server_config.resources.is_empty() || !self.server_config.defaults.directories.is_empty() {
                Some(ResourcesCapability { list_changed: None })
            } else {
                None
            };

        let prompts_cap = if !self.prompt_registry.is_empty() {
            Some(PromptsCapability { list_changed: None })
        } else {
            None
        };

        let result = InitializeResult {
            protocol_version: "2025-03-26".into(),
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability {}),
                resources: resources_cap,
                prompts: prompts_cap,
            },
            server_info: ServerInfo {
                name: "mcp-server".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            instructions,
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
        info!("Calling tool with params: {:?}", params);
        let call_params: ToolCallParams = match params {
            Some(p) => match serde_json::from_value(p) {
                Ok(p) => p,
                Err(e) => {
                    return JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        id: Some(id),
                        result: None,
                        error: Some(JsonRpcError::invalid_params(&e.to_string())),
                    };
                }
            },
            None => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: Some(id),
                    result: None,
                    error: Some(JsonRpcError::invalid_params("missing params")),
                };
            }
        };

        let tool = match self.registry.get(&call_params.name) {
            Some(t) => t,
            None => {
                error!("Tool not found: {}", call_params.name);
                return JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: Some(id),
                    result: None,
                    error: Some(JsonRpcError::invalid_params(&format!(
                        "Tool not found: {}",
                        call_params.name
                    ))),
                };
            }
        };

        // Validate required params
        let provided_args = call_params.arguments.unwrap_or_default();
        info!(
            "Tool matched: {}, action: {:?} {provided_args:?}",
            tool.def.name, tool.def.action
        );
        if let Some(defined_params) = &tool.def.parameters {
            for param in defined_params {
                if param.required && !provided_args.contains_key(&param.name) {
                    return JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        id: Some(id),
                        result: None,
                        error: Some(JsonRpcError::invalid_params(&format!(
                            "Missing required parameter: {}",
                            param.name
                        ))),
                    };
                }
            }
        }

        use crate::executor::command::CommandExecutor;
        use crate::executor::http::HttpExecutor;

        info!("call_params.name: {}", call_params.name);

        // 内置文件操作 tool 分发
        match call_params.name.as_str() {
            "list_allowed_dirs" => return self.handle_builtin_list_allowed_dirs(id),
            "list_dir" => return self.handle_builtin_list_dir(id, &provided_args).await,
            "read_file" => return self.handle_builtin_read_file(id, &provided_args).await,
            "write_file" => return self.handle_builtin_write_file(id, &provided_args).await,
            _ => {}
        }

        if call_params.name == "direct_command" {
            let executor = CommandExecutor;
            let cmd_str = provided_args
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

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

            match executor.execute_direct(cmd_str, &parsed_args).await {
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
                    let call_result = ToolCallResult {
                        content,
                        is_error: if res.exit_code != 0 { Some(true) } else { None },
                    };
                    JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        id: Some(id),
                        result: Some(serde_json::to_value(call_result).unwrap()),
                        error: None,
                    }
                }
                Err(e) => Self::make_tool_error(id, format!("Execution Error: {}", e)),
            }
        } else if matches!(tool.def.action, ToolAction::Command { .. }) {
            let executor = CommandExecutor;
            match executor.execute(tool, &provided_args).await {
                Ok(res) => JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: Some(id),
                    result: Some(serde_json::to_value(&res).unwrap()),
                    error: None,
                },
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
        } else if matches!(tool.def.action, ToolAction::Http { .. }) {
            let executor = HttpExecutor::new();
            match executor.execute(tool, &provided_args).await {
                Ok(res) => {
                    let content = vec![ContentBlock {
                        r#type: "text".into(),
                        text: res.body,
                    }];

                    let is_error = if res.status >= 400 { Some(true) } else { None };

                    let call_result = ToolCallResult { content, is_error };

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

    // ===== Resources Handlers =====

    fn handle_resources_list(&self, id: Value) -> JsonRpcResponse {
        let mut resources = Vec::new();

        // 从 config.resources 加载显式配置的资源
        for res in &self.server_config.resources {
            resources.push(ResourceInfo {
                uri: res.uri.clone(),
                name: res.name.clone(),
                description: res.description.clone(),
                mime_type: res.mime_type.clone(),
            });
        }

        // 从 directories 生成目录资源
        for dir in &self.server_config.defaults.directories {
            let uri = format!("file://{}", dir.path.to_string_lossy());
            resources.push(ResourceInfo {
                uri,
                name: dir
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| dir.path.to_string_lossy().to_string()),
                description: Some(dir.description.clone()),
                mime_type: None,
            });
        }

        let result = ResourcesListResult { resources };
        JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result: Some(serde_json::to_value(result).unwrap()),
            error: None,
        }
    }

    fn handle_resources_read(&self, id: Value, params: Option<Value>) -> JsonRpcResponse {
        let read_params: ResourceReadParams = match params {
            Some(p) => match serde_json::from_value(p) {
                Ok(p) => p,
                Err(e) => {
                    return JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        id: Some(id),
                        result: None,
                        error: Some(JsonRpcError::invalid_params(&e.to_string())),
                    };
                }
            },
            None => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: Some(id),
                    result: None,
                    error: Some(JsonRpcError::invalid_params("missing params")),
                };
            }
        };

        let uri = &read_params.uri;

        // 支持 file:// 协议
        if let Some(file_path) = uri.strip_prefix("file://") {
            let path = Path::new(file_path);
            if path.is_file() {
                match std::fs::read_to_string(path) {
                    Ok(text) => {
                        let mime = mime_guess::from_path(path).first().map(|m| m.to_string());
                        let result = ResourceReadResult {
                            contents: vec![ResourceContent {
                                uri: uri.clone(),
                                mime_type: mime,
                                text: Some(text),
                                blob: None,
                            }],
                        };
                        return JsonRpcResponse {
                            jsonrpc: "2.0".into(),
                            id: Some(id),
                            result: Some(serde_json::to_value(result).unwrap()),
                            error: None,
                        };
                    }
                    Err(e) => {
                        return JsonRpcResponse {
                            jsonrpc: "2.0".into(),
                            id: Some(id),
                            result: None,
                            error: Some(JsonRpcError::internal_error(&format!("Failed to read file: {}", e))),
                        };
                    }
                }
            } else if path.is_dir() {
                // 对目录返回文件列表
                match std::fs::read_dir(path) {
                    Ok(entries) => {
                        let listing: Vec<String> = entries
                            .filter_map(|e| e.ok())
                            .map(|e| {
                                let name = e.file_name().to_string_lossy().to_string();
                                let ft = e.file_type().ok();
                                let prefix = if ft.as_ref().is_some_and(|t| t.is_dir()) {
                                    "[dir]"
                                } else {
                                    "[file]"
                                };
                                format!("{} {}", prefix, name)
                            })
                            .collect();
                        let result = ResourceReadResult {
                            contents: vec![ResourceContent {
                                uri: uri.clone(),
                                mime_type: Some("text/plain".into()),
                                text: Some(listing.join("\n")),
                                blob: None,
                            }],
                        };
                        return JsonRpcResponse {
                            jsonrpc: "2.0".into(),
                            id: Some(id),
                            result: Some(serde_json::to_value(result).unwrap()),
                            error: None,
                        };
                    }
                    Err(e) => {
                        return JsonRpcResponse {
                            jsonrpc: "2.0".into(),
                            id: Some(id),
                            result: None,
                            error: Some(JsonRpcError::internal_error(&format!(
                                "Failed to read directory: {}",
                                e
                            ))),
                        };
                    }
                }
            } else {
                return JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: Some(id),
                    result: None,
                    error: Some(JsonRpcError::invalid_params(&format!("Resource not found: {}", uri))),
                };
            }
        }

        JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result: None,
            error: Some(JsonRpcError::invalid_params(&format!(
                "Unsupported URI scheme: {}",
                uri
            ))),
        }
    }

    fn handle_resource_templates_list(&self, id: Value) -> JsonRpcResponse {
        let mut templates = Vec::new();

        // 为每个 directory 生成一个 URI 模板
        for dir in &self.server_config.defaults.directories {
            templates.push(ResourceTemplate {
                uri_template: format!("file://{}{{/path}}", dir.path.to_string_lossy()),
                name: format!(
                    "Files in {}",
                    dir.path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| dir.path.to_string_lossy().to_string())
                ),
                description: Some(dir.description.clone()),
                mime_type: None,
            });
        }

        let result = ResourceTemplatesListResult {
            resource_templates: templates,
        };
        JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result: Some(serde_json::to_value(result).unwrap()),
            error: None,
        }
    }

    // ===== Prompts Handlers =====

    fn handle_prompts_list(&self, id: Value) -> JsonRpcResponse {
        let prompts: Vec<PromptInfo> = self
            .prompt_registry
            .list()
            .map(|p| PromptInfo {
                name: p.name.clone(),
                description: p.description.clone(),
                arguments: p.arguments.as_ref().map(|args| {
                    args.iter()
                        .map(|a| PromptArgument {
                            name: a.name.clone(),
                            description: a.description.clone(),
                            required: a.required,
                        })
                        .collect()
                }),
            })
            .collect();

        let result = PromptsListResult { prompts };
        JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result: Some(serde_json::to_value(result).unwrap()),
            error: None,
        }
    }

    fn handle_prompts_get(&self, id: Value, params: Option<Value>) -> JsonRpcResponse {
        let get_params: PromptGetParams = match params {
            Some(p) => match serde_json::from_value(p) {
                Ok(p) => p,
                Err(e) => {
                    return JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        id: Some(id),
                        result: None,
                        error: Some(JsonRpcError::invalid_params(&e.to_string())),
                    };
                }
            },
            None => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: Some(id),
                    result: None,
                    error: Some(JsonRpcError::invalid_params("missing params")),
                };
            }
        };

        let prompt_def = match self.prompt_registry.get(&get_params.name) {
            Some(p) => p,
            None => {
                return JsonRpcResponse {
                    jsonrpc: "2.0".into(),
                    id: Some(id),
                    result: None,
                    error: Some(JsonRpcError::invalid_params(&format!(
                        "Prompt not found: {}",
                        get_params.name
                    ))),
                };
            }
        };

        let arguments = get_params.arguments.unwrap_or_default();

        // 对消息模板进行 ${variable} 替换
        let messages: Vec<PromptMessage> = prompt_def
            .messages
            .iter()
            .map(|msg| {
                let mut content = msg.content.clone();
                for (key, value) in &arguments {
                    content = content.replace(&format!("${{{}}}", key), value);
                }
                PromptMessage {
                    role: msg.role.clone(),
                    content: PromptContent {
                        r#type: "text".into(),
                        text: content,
                    },
                }
            })
            .collect();

        let result = PromptGetResult {
            description: prompt_def.description.clone(),
            messages,
        };

        JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result: Some(serde_json::to_value(result).unwrap()),
            error: None,
        }
    }

    fn make_tool_result(id: Value, content: Vec<ContentBlock>, is_error: Option<bool>) -> JsonRpcResponse {
        let call_result = ToolCallResult { content, is_error };
        JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result: Some(serde_json::to_value(call_result).unwrap()),
            error: None,
        }
    }

    fn make_tool_error(id: Value, msg: String) -> JsonRpcResponse {
        Self::make_tool_result(
            id,
            vec![ContentBlock {
                r#type: "text".into(),
                text: msg,
            }],
            Some(true),
        )
    }

    fn handle_builtin_list_allowed_dirs(&self, id: Value) -> JsonRpcResponse {
        let dirs = &self.server_config.defaults.directories;
        let text = if dirs.is_empty() {
            "(no allowed directories configured)".to_string()
        } else {
            dirs.iter()
                .map(|d| format!("{} - {}", d.path.to_string_lossy(), d.description))
                .collect::<Vec<_>>()
                .join("\n")
        };

        Self::make_tool_result(
            id,
            vec![ContentBlock {
                r#type: "text".into(),
                text,
            }],
            None,
        )
    }

    async fn handle_builtin_list_dir(&self, id: Value, args: &HashMap<String, Value>) -> JsonRpcResponse {
        let path_str = match args.get("path").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return Self::make_tool_error(id, "Missing required parameter: path".into()),
        };

        let path = Path::new(path_str);

        let entries = match std::fs::read_dir(path) {
            Ok(e) => e,
            Err(e) => return Self::make_tool_error(id, format!("IO error: {}", e)),
        };

        let mut lines = Vec::new();
        for entry in entries {
            match entry {
                Ok(entry) => {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let file_type = match entry.file_type() {
                        Ok(ft) => {
                            if ft.is_dir() {
                                "dir"
                            } else if ft.is_file() {
                                "file"
                            } else {
                                "other"
                            }
                        }
                        Err(_) => "unknown",
                    };
                    lines.push(format!("[{}] {}", file_type, name));
                }
                Err(e) => {
                    lines.push(format!("[error] {}", e));
                }
            }
        }

        let text = if lines.is_empty() {
            "(empty directory)".to_string()
        } else {
            lines.join("\n")
        };

        Self::make_tool_result(
            id,
            vec![ContentBlock {
                r#type: "text".into(),
                text,
            }],
            None,
        )
    }

    async fn handle_builtin_read_file(&self, id: Value, args: &HashMap<String, Value>) -> JsonRpcResponse {
        let path_str = match args.get("path").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return Self::make_tool_error(id, "Missing required parameter: path".into()),
        };

        let path = Path::new(path_str);

        match std::fs::read_to_string(path) {
            Ok(content) => Self::make_tool_result(
                id,
                vec![ContentBlock {
                    r#type: "text".into(),
                    text: content,
                }],
                None,
            ),
            Err(e) => Self::make_tool_error(id, format!("IO error: {}", e)),
        }
    }

    async fn handle_builtin_write_file(&self, id: Value, args: &HashMap<String, Value>) -> JsonRpcResponse {
        let path_str = match args.get("path").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return Self::make_tool_error(id, "Missing required parameter: path".into()),
        };
        let content = match args.get("content").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return Self::make_tool_error(id, "Missing required parameter: content".into()),
        };

        let path = Path::new(path_str);

        // 检查父目录是否存在（不自动创建）
        if let Some(parent) = path.parent().filter(|p| !p.exists()) {
            return Self::make_tool_error(id, format!("Parent directory does not exist: {}", parent.display()));
        }

        match std::fs::write(path, content) {
            Ok(_) => Self::make_tool_result(
                id,
                vec![ContentBlock {
                    r#type: "text".into(),
                    text: "File written successfully.".into(),
                }],
                None,
            ),
            Err(e) => Self::make_tool_error(id, format!("IO error: {}", e)),
        }
    }
}
