# Goal 08: stdio 传输

## 目标

实现 MCP stdio 传输模式，与 SSE（Goal 04）共同构成 server 的两种核心传输方式。通过 `--stdio` 命令行参数启动，用于本地客户端直连和辅助工具子进程调用。

## 前置依赖

- Goal 02（MCP 协议层、McpHandler）
- Goal 03（Command 执行器）

## 涉及文件

```
src/
├── main.rs              # --stdio 分支启动 stdio 循环
├── transport/
│   ├── mod.rs           # 传输层模块声明
│   └── stdio.rs         # stdio 传输实现
└── lib.rs               # 导出 transport 模块
```

## 实现方式

### 启动模式

```
mcp-server              → SSE 模式（默认，Goal 04）
mcp-server --stdio      → stdio 模式（本 Goal）
mcp-server --schema     → 输出 JSON Schema（Goal 07）
```

三种模式互斥，main.rs 中统一判断。

### stdio 传输（stdio.rs）

MCP stdio 协议规范：
- 每条消息为一行完整的 JSON（以 `\n` 分隔）
- stdin 读取请求，stdout 写入响应
- stderr 用于日志输出（不干扰协议通信）

```rust
pub async fn run_stdio(handler: McpHandler) -> Result<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handler.handle_request(&line).await {
            let mut out = stdout.lock();
            writeln!(out, "{}", response)?;
            out.flush()?;
        }
        // notification（如 initialized）不返回响应，跳过输出
    }
    Ok(())
}
```

### 日志输出

stdio 模式下日志必须走 stderr，不能写 stdout（会破坏协议）：
- `env_logger` 默认输出到 stderr，无需特殊处理
- 确认 `log` crate 的所有输出不会混入 stdout

### lib.rs 导出

```rust
pub mod config;
pub mod protocol;
pub mod executor;
pub mod security;
pub mod transport;   // 包含 sse 和 stdio
```

辅助工具使用 stdio 的两种方式：
1. 以子进程方式启动 `mcp-server --stdio`，通过 stdin/stdout 通信
2. 直接依赖 lib，在进程内调用 `McpHandler`（更轻量）

### 与 SSE 的关系

两种传输方式共享同一个 `McpHandler`，差异仅在 I/O 层：

```
                ┌─── sse.rs ──── GET /sse + POST /message
McpHandler ─────┤
                └─── stdio.rs ── stdin / stdout
```

## 测试

### 单元测试

1. **消息解析**
   - 单行 JSON-RPC 正确解析并处理
   - 空行跳过
   - 非法 JSON 返回 Parse Error

### 集成测试

1. **子进程通信**
   - 启动 `mcp-server --stdio` 子进程
   - 通过 stdin 发送 initialize 请求
   - 从 stdout 读取响应，验证 protocol_version 和 capabilities
   - 发送 tools/list，验证返回 tool 列表
   - 发送 tools/call（command tool），验证返回执行结果

2. **多轮对话**
   - 连续发送多条请求，验证每条都正确响应
   - notification 不产生输出

3. **进程退出**
   - 关闭 stdin → server 进程正常退出

## 完成标准

- [ ] `mcp-server --stdio` 启动 stdio 模式
- [ ] 无参数启动为 SSE 模式（不影响 Goal 04 功能）
- [ ] stdin/stdout 通信符合 MCP stdio 协议规范
- [ ] 日志输出到 stderr，不干扰协议
- [ ] 子进程通信集成测试通过
- [ ] lib.rs 导出 transport 模块供外部 crate 使用
