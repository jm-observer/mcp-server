# MCP Server

使用 Rust 实现的 [Model Context Protocol (MCP)](https://modelcontextprotocol.io/) Server，通过配置文件动态注册 tools，支持命令执行和 HTTP API 调用。

[English](README.md)

## 特性

- **配置驱动**：所有 tool 通过 `tools.d/` 目录下的 TOML 文件动态注册，无硬编码 tool
- **多种 Tool 类型**：支持本地命令执行（command）和 HTTP API 调用（http）
- **双传输协议**：支持 SSE（Server-Sent Events）和 stdio 两种传输方式
- **安全机制**：目录白名单、路径逃逸防护、输出截断、进程注入防护
- **参数模板**：使用 `${var}` 占位符实现灵活的参数化命令
- **辅助工具**：附带 `mcp-tool-generator`，可通过 LLM 自动生成 tool 配置文件

## 快速开始

### 构建

```bash
cargo build --release
```

### 运行

**SSE 模式（默认）：**

```bash
./target/release/mcp-server --config config.toml
# 服务监听在 http://127.0.0.1:3000
```

**Stdio 模式：**

```bash
./target/release/mcp-server --config config.toml --stdio
```

**输出 Tool 配置的 JSON Schema：**

```bash
./target/release/mcp-server --schema
```

### 与 Claude Desktop 集成

在 Claude Desktop 配置文件中添加：

```json
{
  "mcpServers": {
    "mcp-server": {
      "command": "/path/to/mcp-server",
      "args": ["--config", "/path/to/config.toml", "--stdio"]
    }
  }
}
```

## 配置

### Server 配置（config.toml）

```toml
[server]
host = "127.0.0.1"
port = 3000

[defaults]
timeout_secs = 60
allowed_dirs = ["/data/repos", "/home/user/workspace"]

[security]
allow_direct_command = true
```

| 字段 | 说明 |
|------|------|
| `server.host` | HTTP 监听地址 |
| `server.port` | HTTP 监听端口 |
| `defaults.timeout_secs` | 全局超时上限（秒） |
| `defaults.allowed_dirs` | 可操作的目录白名单 |
| `security.allow_direct_command` | 是否暴露任意命令执行 tool |

### Tool 配置（tools.d/*.toml）

每个 `.toml` 文件可包含一个 `[config]` 公共配置段和多个 `[[tools]]` 定义。

#### Command 类型

```toml
[config]
working_dir = "/data/repos"
timeout_secs = 60
env = { "GIT_SSL_NO_VERIFY" = "1" }

[[tools]]
name = "git_status"
description = "Show git status of a project"
command = "git"
args = ["status"]
sub_dir = "${project}"

[[tools.parameters]]
name = "project"
description = "Project folder name"
type = "string"
required = true
```

#### HTTP 类型

```toml
[config]
base_url = "https://httpbin.org"
timeout_secs = 10

[[tools]]
name = "http_get_ip"
description = "Get current IP address"
type = "http"
method = "GET"
path = "/ip"
```

### 配置合并规则

- **超时**：取 tool 级别、文件级别、全局 `defaults.timeout_secs` 中最小值，全局为上限
- **环境变量**：`[config].env` 与 `[[tools]].env` 字典合并，tool 级别优先
- **工作目录**：`allowed_dirs` 为安全边界，`working_dir` 为分组目录，`sub_dir` 为其下的相对子目录

### 参数化

在 `args` 和 `sub_dir` 中使用 `${var}` 占位符引用参数。参数通过 `[[tools.parameters]]` 定义：

| 字段 | 说明 |
|------|------|
| `name` | 参数名称 |
| `type` | 参数类型（string / number / boolean） |
| `description` | 参数描述 |
| `required` | 是否必填 |

未提供的可选参数会自动从 args 中剔除。

## MCP 协议支持

| 方法 | 说明 |
|------|------|
| `initialize` | 握手，返回服务端能力 |
| `initialized` | 通知（无响应） |
| `ping` | 健康检查 |
| `tools/list` | 列出所有已注册 tool 及其 JSON Schema |
| `tools/call` | 执行指定 tool |

## 传输协议

### SSE（Server-Sent Events）

- `GET /sse`：创建会话，通过 SSE 返回 endpoint URL
- `POST /message?sessionId=<id>`：接收 MCP 请求，通过 SSE 返回响应

### Stdio

使用 `--stdio` 启动，从 stdin 读取 JSON-RPC 请求，向 stdout 写入响应（每行一条）。日志输出到 stderr，不干扰协议通信。

## 安全机制

- **目录白名单**：所有 tool 的工作目录必须在 `allowed_dirs` 范围内
- **路径逃逸防护**：`sub_dir` 解析后必须仍在 `working_dir` 内，防止 `../../` 越界
- **输出截断**：stdout/stderr 限制 50KB，防止内存耗尽和 LLM 上下文溢出
- **注入防护**：使用进程原生传参，不经过 shell 拼接
- **直接命令开关**：通过 `allow_direct_command` 控制任意命令执行能力

## 辅助工具：mcp-tool-generator

自动生成 tool 配置文件的辅助工具。通过递归解析命令的 `--help` 输出，借助 LLM（vLLM）为每个子命令生成 TOML 配置。

### 使用

```bash
cargo run -p mcp-tool-generator -- \
  -s /path/to/mcp-server \
  -c config.toml \
  -u http://localhost:8000 \
  -m 3 \
  -o output.toml \
  cargo
```

### 参数

| 参数 | 说明 | 默认值 |
|------|------|--------|
| `<command_name>` | 要分析的命令名 | （必填） |
| `-s, --mcp-server` | mcp-server 可执行文件路径 | `mcp-server` |
| `-c, --server-config-path` | Server 配置文件路径 | `config.toml` |
| `-u, --vllm-url` | vLLM API 地址 | `http://localhost:8000` |
| `-m, --max-depth` | 最大递归深度（上限 5） | `3` |
| `-o, --output-path` | 输出文件路径 | stdout |

### 工作流程

1. 通过 stdio 连接 mcp-server，使用 `direct_command` tool 执行 help 命令
2. 递归解析子命令（通过 LLM 识别子命令列表）
3. 将所有 help 文本和 JSON Schema 发送给 vLLM
4. LLM 为每个子命令生成 tool 配置并标记安全性
5. 输出合并后的 `.toml` 文件（危险命令以注释标记）

## 项目结构

```
├── Cargo.toml                 # Workspace 定义
├── config.toml                # Server 配置
├── tools.d/                   # Tool 配置目录
│   └── test.toml
├── src/                       # MCP Server 源码
│   ├── main.rs                # 入口，SSE/stdio 启动
│   ├── lib.rs                 # 库导出
│   ├── config/                # 配置解析
│   │   ├── server.rs          # Server 配置结构
│   │   ├── tool.rs            # Tool 注册与加载
│   │   └── schema.rs          # JSON Schema 生成
│   ├── protocol/              # MCP 协议
│   │   ├── types.rs           # JSON-RPC 类型定义
│   │   └── handler.rs         # 请求分发处理
│   ├── executor/              # 执行引擎
│   │   ├── command.rs         # 命令执行器
│   │   └── http.rs            # HTTP 执行器
│   ├── security.rs            # 路径安全校验
│   └── transport/             # 传输层
│       ├── sse.rs             # SSE 传输
│       └── stdio.rs           # Stdio 传输
├── mcp-tool-generator/        # 辅助工具
│   └── src/
│       ├── main.rs            # CLI 入口
│       ├── config.rs          # 配置定义
│       ├── mcp_client.rs      # Stdio MCP 客户端
│       ├── llm_client.rs      # vLLM API 客户端
│       ├── crawler.rs         # Help 递归爬取
│       ├── prompt.rs          # LLM Prompt 构建
│       ├── toml_output.rs     # TOML 输出生成
│       └── types.rs           # 数据结构
└── doc/                       # 开发文档
```

## License

MIT
