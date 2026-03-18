# Goal 02: MCP 协议层实现

## 目标

实现 MCP 协议的 JSON-RPC 类型定义和请求分发处理，支持 `initialize`、`initialized`、`tools/list`、`tools/call`、`ping` 方法。

## 前置依赖

- Goal 01（配置解析、ToolRegistry）

## 涉及文件

```
src/
├── lib.rs               # 新增导出 protocol 模块
└── protocol/
    ├── mod.rs            # 模块声明
    ├── types.rs          # JSON-RPC 请求/响应类型、MCP 特定类型
    └── handler.rs        # 方法分发与各 method 处理逻辑
```

## 实现方式

### JSON-RPC 基础类型（types.rs）

```rust
/// JSON-RPC 2.0 请求
pub struct JsonRpcRequest {
    pub jsonrpc: String,           // 固定 "2.0"
    pub id: Option<Value>,         // 请求 ID，notification 时为 None
    pub method: String,
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 响应
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
}

pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}
```

### MCP 类型（types.rs）

```rust
/// initialize 响应
pub struct InitializeResult {
    pub protocol_version: String,  // "2025-03-26"
    pub capabilities: ServerCapabilities,
    pub server_info: ServerInfo,
}

pub struct ServerCapabilities {
    pub tools: Option<ToolsCapability>,
}

pub struct ToolsCapability {}      // 声明支持 tools 即可

pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// tools/list 响应
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,       // JSON Schema 描述参数
}

/// tools/call 请求参数
pub struct ToolCallParams {
    pub name: String,
    pub arguments: Option<HashMap<String, Value>>,
}

/// tools/call 响应
pub struct ToolCallResult {
    pub content: Vec<ContentBlock>,
    pub is_error: Option<bool>,
}

pub struct ContentBlock {
    pub r#type: String,            // "text"
    pub text: String,
}
```

### Handler（handler.rs）

```rust
pub struct McpHandler {
    registry: Arc<ToolRegistry>,
    server_config: Arc<ServerConfig>,
}

impl McpHandler {
    /// 主分发入口：接收 JSON-RPC 请求字符串，返回响应字符串
    pub async fn handle_request(&self, request: &str) -> Option<String>;

    /// 各 method 处理
    fn handle_initialize(&self, id: Value) -> JsonRpcResponse;
    fn handle_ping(&self, id: Value) -> JsonRpcResponse;
    fn handle_tools_list(&self, id: Value) -> JsonRpcResponse;
    async fn handle_tools_call(&self, id: Value, params: Value) -> JsonRpcResponse;
}
```

处理逻辑：

- **initialize**：返回 `InitializeResult`，声明 `protocol_version` 和 `capabilities.tools`
- **initialized**：notification（无 id），不返回响应
- **ping**：返回空 result `{}`
- **tools/list**：遍历 ToolRegistry，将每个 tool 转为 `ToolInfo`（含 `input_schema`）
- **tools/call**：本阶段仅做参数解析和 tool 查找。待执行支持接入后（Goal 03/05），如遇执行崩溃或超时，**应该通过正常的 `ToolCallResult` (结合 `is_error=true` 及文本报错原因)** 传递结果给大模型，避免只使用死板的 JSON-RPC 级底层报错（如 `-32603`），这样使得 LLM 可以借由错误详情实现“自我修正”。目前只返回 "not implemented"

### input_schema 生成

将 `ParameterDef` 转为 JSON Schema：

```json
{
  "type": "object",
  "properties": {
    "repo_url": {
      "type": "string",
      "description": "Git repository URL"
    }
  },
  "required": ["repo_url"]
}
```

### lib.rs 导出

```rust
pub mod config;
pub mod protocol;
```

## 测试

### 单元测试

1. **JSON-RPC 解析**
   - 正常请求解析（含 id、method、params）
   - notification 解析（无 id）
   - 格式错误返回 Parse Error（-32700）

2. **initialize**
   - 返回正确的 protocol_version
   - 返回 capabilities 含 tools
   - 返回 server_info

3. **ping**
   - 返回空 result

4. **tools/list**
   - 空注册表返回空数组
   - 返回所有已注册 tool 的 name、description、input_schema
   - input_schema 正确反映参数定义（type、description、required）

5. **tools/call**
   - tool name 不存在返回错误
   - 缺少 required 参数返回错误
   - 本阶段正常调用返回 "not implemented"

6. **initialized**
   - notification 不返回响应（handle_request 返回 None）

### 错误码

- `-32700`：Parse Error
- `-32601`：Method Not Found
- `-32602`：Invalid Params
- `-32603`：Internal Error

## 完成标准

- [ ] JSON-RPC 请求解析与响应序列化正确
- [ ] initialize 返回符合 MCP 协议的响应
- [ ] tools/list 正确列出所有配置的 tool 及其 input_schema
- [ ] tools/call 能定位 tool 并校验参数（executor 调用待后续目标）
- [ ] ping / initialized 处理正确
- [ ] 错误码符合 JSON-RPC 2.0 规范
- [ ] 所有测试通过
