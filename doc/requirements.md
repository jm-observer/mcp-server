# MCP Server 需求文档

## 项目概述

使用 Rust 实现的 MCP (Model Context Protocol) Server，通过配置文件动态注册 tools，支持命令执行、HTTP API 调用等多种 tool 类型。

## 传输层

- **HTTP**：Streamable HTTP，基于 Actix-web 框架
- **stdio**：支持本地 stdio 传输，用于辅助工具等本地场景
- 传输方式通过配置文件指定

## MCP 协议

支持以下 MCP 协议方法：

- `initialize` / `initialized`
- `tools/list`
- `tools/call`
- `ping`

## Tools

### 设计原则

- **无内置 tool**，所有 tool（包括文件操作、目录操作、git 命令等）均通过配置文件动态注册
- 扫描 `tools.d/` 目录下所有 `.toml` 文件加载 tool 定义
- tool name 全局唯一，重复则报错

### Tool 类型

通过配置中的 `type` 字段区分：

- **`command`**（默认）：执行本地 shell 命令，当前实现
- **`http`**：调用 HTTP API，预置 httpbin.org 测试 tool
- **`mqtt`**：待设计

### 直接命令执行

- 通过 `config.toml` 中的配置开关控制
- 开启后 server 暴露一个可执行任意命令的 tool
- 用于辅助工具（如 tool 配置生成器）场景

## 配置

### Server 配置（config.toml）

```toml
[server]
host = "127.0.0.1"
port = 3000
transport = ["http", "stdio"]

[defaults]
timeout_secs = 30

allowed_dirs = [
  "/data/repos",
  "/home/user/workspace"
]

[security]
allow_direct_command = true
```

- `host` / `port`：HTTP 监听地址
- `transport`：启用的传输方式
- `timeout_secs`：全局默认超时上限
- `allowed_dirs`：可操作的目录白名单，所有 tool 的工作目录必须在白名单内
- `allow_direct_command`：是否允许直接执行任意命令

### Tool 配置（tools.d/*.toml）

每个 `.toml` 文件可包含一个 `[config]` 公共配置和多个 `[[tools]]` 定义。

#### Command 类型示例

```toml
# tools.d/git.toml

[config]
working_dir = "/data/repos"
timeout_secs = 60
env = { "GIT_SSL_NO_VERIFY" = "1", "LANG" = "en_US.UTF-8" }

[[tools]]
name = "git_clone"
description = "Clone a git repository"
command = "git"
args = ["clone", "${repo_url}"]

[[tools.parameters]]
name = "repo_url"
description = "Git repository URL"
type = "string"
required = true

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

#### HTTP 类型示例

```toml
# tools.d/http_test.toml

[config]
base_url = "https://httpbin.org"
timeout_secs = 10

[[tools]]
name = "http_get_test"
description = "Test HTTP GET request"
type = "http"
method = "GET"
path = "/get"

[[tools]]
name = "http_post_test"
description = "Test HTTP POST request"
type = "http"
method = "POST"
path = "/post"
body = '{"message": "hello"}'
content_type = "application/json"

[[tools]]
name = "http_get_ip"
description = "Get current IP address"
type = "http"
method = "GET"
path = "/ip"
```

### 参数化

- 使用 `${var}` 占位符在 `args` 和 `sub_dir` 中引用参数
- 参数通过 `[[tools.parameters]]` 定义 name、type、description、required
- server 收到 `tools/call` 请求后，用请求参数填充占位符，拼接出完整命令执行

### 配置合并规则

- **目录**：`allowed_dirs`（全局白名单）最优先，是硬性安全边界；`[config].working_dir` 是分组工作目录；`sub_dir` 是其下的相对子目录
- **超时**：取 tool 级别和全局 `defaults.timeout_secs` 中**较小的那个**，全局是上限
- **环境变量**：`[config].env` 与 `[[tools]].env` 会进行字典合并，遇到相同 key 时该 tool 自身环境优先。

## Command 类型 Tool 执行流程

1. 收到 MCP `tools/call` 请求
2. 根据 tool name 找到配置中的命令模板
3. 用请求参数填充 `${var}` 占位符
4. 拼接完整命令和参数（遇到空的可选参数应当将其自动降维剔除丢弃）
5. 确定执行目录：`working_dir` / `sub_dir`（使用词法层级合并来校验在白名单且防逃逸，勿借助可能产生 IO 缺失报错的 canonicalize）
6. 执行命令（加载前期的继承环境变量，附带由于超时或输出失控容量爆炸导致的中断限额防御机制）
7. 返回结果：安全截获的内容 `stdout`、`stderr`、`exit_code`（或可供重试的异常提示）分开返回

## 安全

- `allowed_dirs` 白名单：所有 tool 的 `working_dir` 必须在白名单内
- `sub_dir` 防路径逃逸：解析后的实际路径必须仍在 `working_dir` 范围内，防范 `../../` 越界攻击（强制采用规范化字符层面解析，从而免受 Windows `\\\\?\\` 或目录本身临时不存在所发酵成的验证漏洞阻断影响）
- **输出截断防爆**：限制进程日志返回过海的字节体量（比如 50KB 上帝截断），避免耗尽宿主机内存以及对 LLM 模型产生数据上下文溢出污染
- **执行过程防注入**：一加采用执行进程原生的传参映射，坚决避讳任何拼接向具有解析隐患的 `sh -c` 内发包而带来的 Shell RCE 注入死角
- `allow_direct_command` 开关：控制是否暴露任意命令执行能力
- 认证：暂不实现

## 配置格式输出

- server 提供 tool 配置的 **JSON Schema**
- 供外部辅助工具（tool 配置生成器）使用，确保生成的 `.toml` 配置合法

## 暂不实现

- 热加载
- 认证与权限控制
- MQTT 类型 tool（待设计）

---

## 辅助工具（独立项目）

用于自动生成 tool 配置文件的辅助工具，与 mcp-server 为独立项目。

### 功能流程

1. 输入一个命令名（如 `cargo`）
2. 执行 `命令 --help`，解析出子命令列表
3. 递归执行每个子命令的 `--help`（设最大递归深度限制）
4. 把所有 help 文本 + mcp-server 提供的 tool config JSON Schema 发给 vLLM
5. LLM 为每个子命令生成 tool 配置
6. LLM 判断每个命令的安全性（safe / dangerous）
7. 输出单个 `.toml` 文件：safe 的正常输出，dangerous 的用注释标记

### 技术要点

- 通过 **stdio** 连接 mcp-server，由 server 执行 `--help` 命令
- 调用 vLLM 的 **OpenAI 兼容 API**（`/v1/chat/completions`）
- 递归深度可配置，有固定最大深度限制
