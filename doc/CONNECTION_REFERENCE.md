# Claude Code SDK 连接参考文档

> 本文档固化了 `claude-code` crate 的连接方式、参数和消息协议，供后续开发跟踪功能参考。

---

## 1. 传输机制

### 1.1 协议
- **传输方式**: 子进程 stdin/stdout 双向通信
- **数据格式**: 换行分隔的 JSON (Newline-Delimited JSON)
- **核心实现**: `SubprocessCliTransport` (`src/transport/subprocess_cli.rs`)

### 1.2 CLI 发现顺序
1. `ClaudeAgentOptions::cli_path` 自定义路径
2. `CLAUDE_CODE_BUNDLED_CLI` 环境变量指向的捆绑二进制
3. 可执行文件目录下的 `_bundled/claude`
4. `which claude` (PATH 搜索)
5. 常见安装位置: `~/.npm-global/bin/claude`, `/usr/local/bin/claude`, `~/.local/bin/claude`, `~/node_modules/.bin/claude`, `~/.yarn/bin/claude`, `~/.claude/local/claude`

### 1.3 最低版本要求
- Claude Code CLI >= `2.0.0`
- 可通过 `CLAUDE_AGENT_SDK_SKIP_VERSION_CHECK` 环境变量跳过检查

---

## 2. 连接流程

### 2.1 两种使用模式

| 特性 | `query()` 一次性查询 | `ClaudeSdkClient` 持久会话 |
|------|---------------------|--------------------------|
| 会话 | 每次新建 | 复用同一会话 |
| 多轮对话 | 不支持 | 支持 |
| 连接管理 | 自动 | 手动 connect/disconnect |
| 中断操作 | 不支持 | 支持 |
| 控制请求 | 不支持 | 支持 |

### 2.2 ClaudeSdkClient 连接流程

```
ClaudeSdkClient::connect(prompt)
  ├─> (可选) disconnect 已有连接
  ├─> 验证 can_use_tool 约束
  ├─> 创建 SubprocessCliTransport (或使用自定义 TransportFactory)
  ├─> transport.connect() — 启动 CLI 子进程
  ├─> transport.into_split() — 拆分为 reader/writer
  ├─> Query::start() — 启动后台消息读取任务
  ├─> Query::initialize() — 发送 streaming mode 初始化握手
  └─> (可选) 发送初始 prompt 消息
```

### 2.3 Transport trait 接口

```rust
pub trait Transport: Send {
    async fn connect(&mut self) -> Result<()>;
    async fn write(&mut self, data: &str) -> Result<()>;
    async fn end_input(&mut self) -> Result<()>;
    async fn read_next_message(&mut self) -> Result<Option<Value>>;
    async fn close(&mut self) -> Result<()>;
    fn is_ready(&self) -> bool;
    fn into_split(self: Box<Self>) -> TransportSplitResult;
    // TransportSplitResult = (Box<TransportReader>, Box<TransportWriter>, Box<TransportCloseHandle>)
}
```

### 2.4 子进程启动参数

固定参数:
```
claude --output-format stream-json --verbose --input-format stream-json
```

环境变量:
```
CLAUDE_CODE_ENTRYPOINT=sdk-rust
CLAUDE_AGENT_SDK_VERSION=<SDK版本>
CLAUDE_CODE_ENABLE_SDK_FILE_CHECKPOINTING=true  (如果启用)
PWD=<工作目录>  (如果指定 cwd)
```

---

## 3. 配置参数 (ClaudeAgentOptions)

### 3.1 Agent 与模型配置

| 字段 | 类型 | CLI 参数 | 说明 |
|------|------|---------|------|
| `tools` | `Option<ToolsOption>` | `--tools` | 工具配置: 显式列表或 preset("claude_code") |
| `allowed_tools` | `Vec<String>` | `--allowedTools` | 允许的工具名列表 |
| `disallowed_tools` | `Vec<String>` | `--disallowedTools` | 禁用的工具名列表 |
| `system_prompt` | `Option<SystemPrompt>` | `--system-prompt` / `--append-system-prompt` | 系统提示词 |
| `model` | `Option<String>` | `--model` | 主模型 ("sonnet", "opus", "haiku" 等) |
| `fallback_model` | `Option<String>` | `--fallback-model` | 备用模型 |
| `betas` | `Vec<String>` | `--betas` | 启用的 beta 功能 |

### 3.2 会话管理

| 字段 | 类型 | CLI 参数 | 说明 |
|------|------|---------|------|
| `continue_conversation` | `bool` | `--continue` | 继续最近的会话 |
| `resume` | `Option<String>` | `--resume` | 恢复指定 session_id 的会话 |
| `fork_session` | `bool` | `--fork-session` | 恢复时 fork 到新 session |
| `max_turns` | `Option<i64>` | `--max-turns` | 最大对话轮次 |
| `max_budget_usd` | `Option<f64>` | `--max-budget-usd` | 预算限制 (美元) |

### 3.3 权限控制

| 字段 | 类型 | CLI 参数 | 说明 |
|------|------|---------|------|
| `permission_mode` | `Option<PermissionMode>` | `--permission-mode` | 权限模式 |
| `permission_prompt_tool_name` | `Option<String>` | `--permission-prompt-tool` | 权限提示 MCP 工具名 |
| `can_use_tool` | `Option<CanUseToolCallback>` | (自动设置 `--permission-prompt-tool stdio`) | 自定义权限回调 |

**PermissionMode 枚举:**
- `Default` → `"default"` — 标准交互审批
- `AcceptEdits` → `"acceptEdits"` — 自动接受文件编辑
- `Plan` → `"plan"` — 计划模式 (不执行)
- `BypassPermissions` → `"bypassPermissions"` — 跳过所有权限检查

### 3.4 MCP 服务器配置

| 字段 | 类型 | CLI 参数 | 说明 |
|------|------|---------|------|
| `mcp_servers` | `McpServersOption` | `--mcp-config` | MCP 服务器配置 |

**McpServerConfig 四种传输类型:**

```rust
enum McpServerConfig {
    Stdio(McpStdioServerConfig),  // 外部进程 (command + args + env)
    Sse(McpSSEServerConfig),      // Server-Sent Events (url + headers)
    Http(McpHttpServerConfig),    // HTTP (url + headers)
    Sdk(McpSdkServerConfig),      // 进程内 Rust MCP 服务器
}
```

### 3.5 沙箱配置

| 字段 | 类型 | 说明 |
|------|------|------|
| `sandbox.enabled` | `Option<bool>` | 启用沙箱 |
| `sandbox.auto_allow_bash_if_sandboxed` | `Option<bool>` | 沙箱内自动批准 bash |
| `sandbox.excluded_commands` | `Option<Vec<String>>` | 绕过沙箱的命令 |
| `sandbox.allow_unsandboxed_commands` | `Option<bool>` | 允许请求非沙箱执行 |
| `sandbox.network` | `Option<SandboxNetworkConfig>` | 网络限制 |

### 3.6 Thinking 配置

| 字段 | 类型 | CLI 参数 | 说明 |
|------|------|---------|------|
| `thinking` | `Option<ThinkingConfig>` | `--max-thinking-tokens` | 扩展思维配置 |
| `effort` | `Option<String>` | `--effort` | 思维深度 ("low"/"medium"/"high"/"max") |

```rust
enum ThinkingConfig {
    Adaptive,                      // 自适应, 默认 32000 tokens
    Enabled { budget_tokens: i64 }, // 指定 token 预算
    Disabled,                       // 禁用 (设置为 0)
}
```

### 3.7 输入/输出配置

| 字段 | 类型 | CLI 参数 | 说明 |
|------|------|---------|------|
| `output_format` | `Option<Value>` | `--json-schema` | 结构化输出 (JSON schema) |
| `include_partial_messages` | `bool` | `--include-partial-messages` | 包含流式 StreamEvent |
| `enable_file_checkpointing` | `bool` | (环境变量) | 启用文件变更跟踪 |

### 3.8 执行环境

| 字段 | 类型 | CLI 参数 | 说明 |
|------|------|---------|------|
| `cwd` | `Option<PathBuf>` | (子进程工作目录) | 工作目录 |
| `env` | `HashMap<String, String>` | (子进程环境变量) | 附加环境变量 |
| `add_dirs` | `Vec<PathBuf>` | `--add-dir` | 可访问的额外目录 |
| `cli_path` | `Option<PathBuf>` | (CLI 路径) | 自定义 CLI 路径 |
| `settings` | `Option<String>` | `--settings` | 配置文件路径或 JSON |
| `setting_sources` | `Option<Vec<SettingSource>>` | `--setting-sources` | 加载哪些配置源 |
| `user` | `Option<String>` | (Unix uid) | 运行用户 |
| `plugins` | `Vec<SdkPluginConfig>` | `--plugin-dir` | 本地插件 |
| `extra_args` | `HashMap<String, Option<String>>` | `--<key> [value]` | 额外 CLI 参数 |
| `max_buffer_size` | `Option<usize>` | - | JSON 缓冲区上限 (默认 1MB) |
| `stderr` | `Option<StderrCallback>` | - | stderr 回调 |
| `strict_settings_merge` | `bool` | - | settings 合并失败时是否报错 |

---

## 4. 消息协议

### 4.1 输入消息 (SDK → CLI via stdin)

#### 用户消息
```json
{
  "type": "user",
  "message": {"role": "user", "content": "..."},
  "session_id": "..."
}
```

#### 控制请求 (streaming 模式)
```json
{
  "type": "control_request",
  "request_id": "req_N",
  "request": {
    "subtype": "initialize|interrupt|set_permission_mode|set_model|rewind_files|get_mcp_status|reconnect_mcp_server|toggle_mcp_server|stop_task",
    ...
  }
}
```

**Initialize 请求** (连接后的第一个消息):
```json
{
  "type": "control_request",
  "request_id": "req_0",
  "request": {
    "subtype": "initialize",
    "protocolVersion": 1,
    "hooks": { ... },
    "sdkMcpServers": { ... }
  }
}
```

### 4.2 输出消息 (CLI → SDK via stdout)

#### Message 枚举

```rust
enum Message {
    User(UserMessage),         // type: "user"
    Assistant(AssistantMessage), // type: "assistant"
    System(SystemMessage),     // type: "system"
    Result(ResultMessage),     // type: "result" (最终消息)
    StreamEvent(StreamEvent),  // type: "stream_event" (需启用 include_partial_messages)
}
```

#### UserMessage
```json
{
  "type": "user",
  "message": {"role": "user", "content": "..."},
  "uuid": "...",
  "session_id": "..."
}
```

#### AssistantMessage
```json
{
  "type": "assistant",
  "message": {
    "role": "assistant",
    "content": [ /* ContentBlock 数组 */ ],
    "model": "..."
  },
  "error": null
}
```

#### SystemMessage (子类型)
- `"initialized"` — 初始化完成
- `"task_started"` — 子任务启动 (包含 task_id, description)
- `"task_progress"` — 子任务进度 (包含 usage: {total_tokens, tool_uses, duration_ms})
- `"task_notification"` — 子任务完成通知 (包含 status, summary)

#### ResultMessage (最终消息)
```json
{
  "type": "result",
  "subtype": "success|error",
  "duration_ms": 1234,
  "duration_api_ms": 567,
  "is_error": false,
  "num_turns": 3,
  "session_id": "...",
  "stop_reason": "end_turn",
  "total_cost_usd": 0.05,
  "usage": {
    "input_tokens": 1000,
    "output_tokens": 500,
    "cache_creation_input_tokens": 0,
    "cache_read_input_tokens": 200
  },
  "result": "...",
  "structured_output": null
}
```

#### 控制响应
```json
{
  "type": "control_response",
  "request_id": "req_0",
  "response": { ... }
}
```

### 4.3 ContentBlock 类型

```rust
enum ContentBlock {
    Text(TextBlock),           // {"type": "text", "text": "..."}
    Thinking(ThinkingBlock),   // {"type": "thinking", "thinking": "...", "signature": "..."}
    ToolUse(ToolUseBlock),     // {"type": "tool_use", "id": "...", "name": "...", "input": {...}}
    ToolResult(ToolResultBlock) // {"type": "tool_result", "tool_use_id": "...", "content": {...}}
}
```

---

## 5. 会话控制操作

通过 `ClaudeSdkClient` 可执行的控制操作:

| 方法 | 控制请求 subtype | 说明 |
|------|-----------------|------|
| `query(prompt, session_id)` | (user 消息) | 发送用户消息 |
| `receive_message()` | - | 接收单条消息 |
| `receive_response()` | - | 接收完整响应 (到 Result 为止) |
| `interrupt()` | `"interrupt"` | 中断当前操作 |
| `set_permission_mode(mode)` | `"set_permission_mode"` | 切换权限模式 |
| `set_model(model)` | `"set_model"` | 切换模型 |
| `rewind_files(user_message_id)` | `"rewind_files"` | 回滚文件到指定检查点 |
| `get_mcp_status()` | `"get_mcp_status"` | 查询 MCP 服务器状态 |
| `reconnect_mcp_server(name)` | `"reconnect_mcp_server"` | 重连 MCP 服务器 |
| `toggle_mcp_server(name, enabled)` | `"toggle_mcp_server"` | 启用/禁用 MCP 服务器 |
| `stop_task(task_id)` | `"stop_task"` | 停止子任务 |
| `disconnect()` | - | 断开连接, 关闭子进程 |

---

## 6. 后台架构

### 6.1 消息路由

```
CLI stdout ──> SubprocessReader ──> Query 后台读取任务
                                       │
                                       ├─> control_response → 匹配等待的 oneshot channel
                                       ├─> control_request  → 后台处理 (权限/hooks/MCP)
                                       └─> SDK message      → mpsc channel (buffer=100) → 调用方
```

### 6.2 Deferred Stdin Close
- 当存在 hooks 或 MCP 服务器时，stdin 关闭延迟到收到首个 `Result` 消息
- 超时: `CLAUDE_CODE_STREAM_CLOSE_TIMEOUT` 环境变量 (默认 60s)

### 6.3 JSON 缓冲管理
- 默认最大缓冲: 1MB (`DEFAULT_MAX_BUFFER_SIZE`)
- 可通过 `max_buffer_size` 配置
- 每个完整 JSON 对象解析后清空缓冲

---

## 7. 会话历史查询

### list_sessions
```rust
pub async fn list_sessions(
    directory: Option<&Path>,  // 项目目录
    limit: Option<usize>,      // 返回数量限制
    include_worktrees: bool,   // 是否包含 git worktrees
) -> Result<Vec<SDKSessionInfo>>
```

返回:
```rust
struct SDKSessionInfo {
    session_id: String,
    summary: String,
    last_modified: i64,    // epoch ms
    file_size: u64,
    custom_title: Option<String>,
    first_prompt: Option<String>,
    git_branch: Option<String>,
    cwd: Option<String>,
}
```

### get_session_messages
```rust
pub async fn get_session_messages(
    session_id: &str,
    directory: Option<&Path>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<SessionMessage>>
```

---

## 8. 错误类型

```rust
enum Error {
    ClaudeSDK(ClaudeSDKError),         // 验证/逻辑错误
    CLIConnection(CLIConnectionError), // 连接失败
    CLINotFound(CLINotFoundError),     // CLI 未找到
    Process(ProcessError),             // 进程执行错误 (含 exit_code, stderr)
    CLIJSONDecode(CLIJSONDecodeError), // JSON 解析错误
    MessageParse(MessageParseError),   // 消息类型解析错误
    Io(std::io::Error),                // I/O 错误
    Json(serde_json::Error),           // serde JSON 错误
    Other(String),                     // 其他错误
}
```

---

## 9. 关键依赖

| 依赖 | 用途 |
|------|------|
| `tokio` | 异步运行时 (io-util, macros, process, rt-multi-thread, sync, time) |
| `async-trait` | 异步 trait 支持 |
| `futures` | Stream 及工具 trait |
| `serde` / `serde_json` | JSON 序列化 |
| `thiserror` | 错误处理 |
| `semver` | 版本号解析 |
| `which` | 二进制路径查找 |
| `tracing` | 日志/追踪 |

---

## 10. 跟踪功能开发参考要点

基于以上分析，开发跟踪功能时需要关注:

1. **连接建立**: 使用 `ClaudeSdkClient` 持久会话模式，通过 `connect()` 建立连接
2. **消息监听**: 通过 `receive_message()` 循环接收消息，根据 `Message` 枚举分类处理
3. **关键追踪点**:
   - `AssistantMessage` — 跟踪 AI 响应和工具调用
   - `SystemMessage::task_started/task_progress/task_notification` — 跟踪子任务生命周期和资源消耗
   - `ResultMessage` — 跟踪最终结果, 含 duration_ms, total_cost_usd, usage 等统计
   - `StreamEvent` — 实时流式事件 (需设置 `include_partial_messages=true`)
4. **会话恢复**: 通过 `resume` 参数或 `list_sessions`/`get_session_messages` 查询历史
5. **自定义传输**: 可实现 `TransportFactory` trait 来自定义传输层 (如加入追踪中间件)
6. **Hooks**: 通过 `hooks` 配置注入 `PreToolUse`/`PostToolUse` 等事件回调来追踪工具使用
7. **权限回调**: 通过 `can_use_tool` 回调在工具执行前进行自定义检查和记录
