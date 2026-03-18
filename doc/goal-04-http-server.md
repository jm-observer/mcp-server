# Goal 04: HTTP Server 集成

## 目标

使用 Actix-web 搭建 HTTP server，将 MCP 协议层接入 HTTP endpoint，实现端到端可用的 MCP server。

## 前置依赖

- Goal 01（配置解析）
- Goal 02（MCP 协议层）
- Goal 03（Command 执行器）

## 涉及文件

```
src/
├── main.rs              # Actix-web server 启动
├── server.rs            # HTTP route 定义与请求处理
└── lib.rs               # 导出 server 模块（可选，视辅助工具需求）
```

## 实现方式

### 共享状态

```rust
pub struct AppState {
    pub handler: McpHandler,
}
```

`AppState` 通过 `web::Data<AppState>`（即 `Arc`）在所有请求间共享。

### HTTP Endpoint

MCP Streamable HTTP（非 SSE 模式）的规范：

- **路由**：单一路径 `/mcp`（可配置）
- **方法**：`POST`
- **请求**：`Content-Type: application/json`，body 为 JSON-RPC 请求
- **响应**：`Content-Type: application/json`，body 为 JSON-RPC 响应

```rust
async fn handle_mcp(
    body: web::Json<Value>,
    state: web::Data<AppState>,
) -> impl Responder {
    let request_str = serde_json::to_string(&body.into_inner()).unwrap();
    match state.handler.handle_request(&request_str).await {
        Some(response) => HttpResponse::Ok()
            .content_type("application/json")
            .body(response),
        None => HttpResponse::Accepted().finish(),  // notification，无需响应
    }
}
```

### main.rs 启动流程

```rust
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // 1. 初始化 tracing
    // 2. 加载 ServerConfig
    // 3. 加载 ToolRegistry
    // 4. 构建 McpHandler
    // 5. 构建 AppState
    // 6. 启动 HttpServer
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .route("/mcp", web::post().to(handle_mcp))
    })
    .bind((config.server.host.as_str(), config.server.port))?
    .run()
    .await
}
```

### 配置文件路径

启动时按以下优先级查找配置：
1. 命令行参数 `--config <path>`（可选，后续增加）
2. 当前目录下的 `config.toml`
3. 找不到则报错退出

`tools.d/` 目录相对于 `config.toml` 所在目录。

### 依赖

新增：
- `actix-web`：HTTP server 框架

## 测试

### 集成测试

使用 `actix-web::test` 模块进行测试，不需要实际启动 server：

1. **initialize 请求**
   - POST JSON-RPC initialize 请求 → 200，返回正确的 capabilities

2. **ping 请求**
   - POST ping → 200，返回空 result

3. **tools/list 请求**
   - POST tools/list → 200，返回配置中的 tool 列表

4. **tools/call 请求（command）**
   - POST tools/call → 200，返回命令执行结果
   - 使用安全的测试命令（如 echo）

5. **错误处理**
   - 非 JSON body → 400
   - 无效 JSON-RPC → 返回 Parse Error
   - 未知 method → 返回 Method Not Found

6. **notification**
   - POST initialized → 202（无响应 body）

### 手动测试

提供 curl 示例：

```bash
# initialize
curl -X POST http://127.0.0.1:3000/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}'

# tools/list
curl -X POST http://127.0.0.1:3000/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'

# ping
curl -X POST http://127.0.0.1:3000/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":3,"method":"ping"}'
```

## 完成标准

- [ ] `cargo run` 启动 HTTP server，监听配置的 host:port
- [ ] POST /mcp 接收 JSON-RPC 请求并返回正确响应
- [ ] initialize → tools/list → tools/call 完整流程可用
- [ ] notification（initialized）返回 202
- [ ] 错误情况返回正确的 JSON-RPC error
- [ ] actix-web 集成测试全部通过
- [ ] curl 手动测试通过
