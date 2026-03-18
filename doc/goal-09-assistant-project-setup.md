# Goal 09: 辅助工具 - 项目搭建与 MCP 连接

## 目标

创建独立的辅助工具项目，实现 CLI 入口和与 mcp-server 的 stdio 连接，能够通过 MCP 协议调用 server 的 direct_command 执行命令。

## 前置依赖

- Goal 06（mcp-server direct_command 功能）
- Goal 07（JSON Schema 输出）
- Goal 08（mcp-server stdio 传输）

## 项目结构

```
mcp-tool-generator/           # 独立项目，与 mcp-server 同级或为 workspace member
├── Cargo.toml
└── src/
    ├── main.rs               # CLI 入口
    ├── mcp_client.rs         # MCP 客户端，管理与 server 的 stdio 通信
    └── config.rs             # 辅助工具自身的配置
```

## 实现方式

### 依赖

- `tokio`：异步运行时
- `serde` + `serde_json`：序列化
- `log` + `env_logger`：日志
- `mcp-server`（lib）：复用 MCP 协议类型定义（可选，或自行定义客户端侧类型）

### CLI 入口（main.rs）

```
用法: mcp-tool-generator <command_name> [options]

参数:
  <command_name>        要生成配置的命令名（如 cargo, git, docker）

选项:
  --mcp-server <path>   mcp-server 可执行文件路径（默认 PATH 中查找）
  --config <path>       mcp-server 的 config.toml 路径
  --vllm-url <url>      vLLM API 地址（默认 http://localhost:8000）
  --max-depth <n>       递归解析最大深度（默认 3）
  --output <path>       输出 .toml 文件路径（默认 stdout）
```

### MCP 客户端（mcp_client.rs）

管理与 mcp-server 子进程的通信：

```rust
pub struct McpClient {
    child: tokio::process::Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    request_id: AtomicI64,
}

impl McpClient {
    /// 启动 mcp-server --stdio 子进程并建立连接
    pub async fn connect(server_path: &str, config_path: &str) -> Result<Self>;

    /// 发送 initialize 并等待响应
    pub async fn initialize(&mut self) -> Result<()>;

    /// 调用 direct_command 执行任意命令
    pub async fn execute_command(
        &mut self,
        command: &str,
        args: &[String],
        working_dir: Option<&str>,
    ) -> Result<CommandOutput>;

    /// 获取 tool 配置 JSON Schema（调用 tools/list 或直接用库函数）
    pub fn get_tool_schema(&self) -> String;

    /// 关闭连接
    pub async fn close(&mut self) -> Result<()>;
}

pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}
```

### 连接流程

1. 启动 `mcp-server --stdio --config <path>` 子进程
2. 发送 `initialize` 请求，确认 server 就绪
3. 发送 `initialized` notification
4. 后续通过 `tools/call` 调用 `direct_command` 执行命令

### 配置（config.rs）

```rust
pub struct GeneratorConfig {
    pub mcp_server_path: String,
    pub server_config_path: String,
    pub vllm_url: String,
    pub max_depth: usize,          // 最大递归深度，硬上限 5
    pub command_name: String,
    pub output_path: Option<String>,
}
```

- `max_depth` 用户可配但受硬上限约束（如最大 5 层）

## 测试

### 单元测试

1. **CLI 参数解析**
   - 正常参数解析
   - 缺少必填参数报错
   - 默认值正确

2. **JSON-RPC 消息构建**
   - initialize 请求格式正确
   - tools/call direct_command 请求格式正确
   - request id 递增

### 集成测试

1. **MCP 连接**
   - 启动 mcp-server 子进程（需 mcp-server 已编译）
   - 完成 initialize 握手
   - 调用 direct_command 执行 `echo hello` → 返回 "hello"
   - 正常关闭

2. **命令执行**
   - 通过 MCP 执行 `git --help` → 返回 help 文本
   - 命令不存在 → 返回错误

## 完成标准

- [ ] 独立项目创建，`cargo build` 通过
- [ ] CLI 参数解析正确
- [ ] 能启动 mcp-server 子进程并完成 initialize 握手
- [ ] 能通过 direct_command 执行命令并获取输出
- [ ] 子进程正常关闭，无孤儿进程
- [ ] 集成测试通过
