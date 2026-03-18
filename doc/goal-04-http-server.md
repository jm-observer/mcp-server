# Goal 04: SSE 传输与 HTTP Server 集成

## 目标

使用 Actix-web 实现 MCP SSE 传输协议，客户端通过 `GET /sse` 建立 SSE 长连接接收响应，通过 `POST /message` 发送 JSON-RPC 请求。与 Goal 08（stdio）共同构成 server 的两种传输方式。

## 前置依赖

- Goal 01（配置解析）
- Goal 02（MCP 协议层）
- Goal 03（Command 执行器）

## 涉及文件

```
src/
├── main.rs              # 启动模式切换（默认 SSE，--stdio 切换）
├── transport/
│   ├── mod.rs           # 传输层模块
│   └── sse.rs           # SSE 传输实现
└── lib.rs               # 导出 transport 模块
```

## 实现方式

### MCP SSE 传输协议

```
客户端                              服务端
  │                                   │
  │──── GET /sse ────────────────────>│  建立 SSE 连接
  │<─── event: endpoint ─────────────│  data: /message?sessionId=xxx
  │                                   │
  │──── POST /message?sessionId=xxx ─>│  发送 JSON-RPC 请求
  │<─── event: message ──────────────│  data: {JSON-RPC 响应}
  │                                   │
  │──── POST /message?sessionId=xxx ─>│  再次发送请求
  │<─── event: message ──────────────│  data: {JSON-RPC 响应}
  │                                   │
  │──── 关闭连接 ────────────────────>│  清理 session
```

### 会话管理

```rust
pub struct SessionManager {
    sessions: DashMap<String, mpsc::UnboundedSender<String>>,
}

impl SessionManager {
    /// 创建新 session，返回 session_id 和消息接收端
    pub fn create_session(&self) -> (String, mpsc::UnboundedReceiver<String>);

    /// 向指定 session 发送 SSE 事件
    pub fn send(&self, session_id: &str, message: String) -> Result<()>;

    /// 移除 session
    pub fn remove_session(&self, session_id: &str);
}
```

- `session_id` 使用 UUID 生成
- `DashMap` 或 `Arc<RwLock<HashMap>>` 管理并发访问
- SSE 连接断开时自动清理 session

### 共享状态

```rust
pub struct AppState {
    pub handler: McpHandler,
    pub sessions: SessionManager,
}
```

### SSE Endpoint — `GET /sse`

```rust
async fn sse_connect(state: web::Data<AppState>) -> impl Responder {
    let (session_id, mut rx) = state.sessions.create_session();
    let endpoint_url = format!("/message?sessionId={}", session_id);

    HttpResponse::Ok()
        .content_type("text/event-stream")
        .streaming(async_stream::stream! {
            // 第一条事件：告知客户端 POST 地址
            yield Ok::<_, actix_web::Error>(
                Bytes::from(format!("event: endpoint\ndata: {}\n\n", endpoint_url))
            );

            // 后续事件：转发 handler 的响应
            while let Some(message) = rx.recv().await {
                yield Ok(
                    Bytes::from(format!("event: message\ndata: {}\n\n", message))
                );
            }
        })
}
```

### Message Endpoint — `POST /message`

```rust
async fn handle_message(
    body: web::Json<Value>,
    query: web::Query<SessionQuery>,
    state: web::Data<AppState>,
) -> impl Responder {
    let session_id = &query.session_id;

    // 验证 session 存在
    if !state.sessions.contains(session_id) {
        return HttpResponse::NotFound().json(json!({"error": "session not found"}));
    }

    let request_str = serde_json::to_string(&body.into_inner()).unwrap();

    match state.handler.handle_request(&request_str).await {
        Some(response) => {
            // 通过 SSE 通道发送响应
            if let Err(_) = state.sessions.send(session_id, response) {
                return HttpResponse::Gone().json(json!({"error": "session closed"}));
            }
            HttpResponse::Accepted().finish()
        }
        None => {
            // notification，不需要响应
            HttpResponse::Accepted().finish()
        }
    }
}

#[derive(Deserialize)]
struct SessionQuery {
    #[serde(rename = "sessionId")]
    session_id: String,
}
```

### main.rs 启动模式切换

```rust
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();

    if args.contains(&"--schema".to_string()) {
        // Goal 07: 输出 JSON Schema
        return Ok(());
    }

    // 初始化日志、加载配置、构建 handler...

    if args.contains(&"--stdio".to_string()) {
        // stdio 模式（Goal 08）
        run_stdio(handler).await
    } else {
        // 默认 SSE 模式
        run_sse_server(handler, &config).await
    }
}

async fn run_sse_server(handler: McpHandler, config: &ServerConfig) -> std::io::Result<()> {
    let app_state = AppState {
        handler,
        sessions: SessionManager::new(),
    };

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .route("/sse", web::get().to(sse_connect))
            .route("/message", web::post().to(handle_message))
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
- `tokio::sync::mpsc`：session 消息通道
- `uuid`：session ID 生成
- `async-stream`（或手写 `Stream` impl）：SSE 流生成

## 测试

### 集成测试

使用 `actix-web::test` 模块或启动真实 server 测试：

1. **SSE 连接建立**
   - GET /sse → 200，Content-Type: text/event-stream
   - 第一条事件为 `event: endpoint`，data 包含 `/message?sessionId=xxx`

2. **完整交互流程**
   - 建立 SSE 连接，获取 endpoint URL
   - POST initialize 到 endpoint → 202
   - 从 SSE 流收到 `event: message`，data 包含 initialize 响应
   - POST tools/list → 从 SSE 流收到 tool 列表
   - POST tools/call → 从 SSE 流收到命令执行结果

3. **session 管理**
   - 无效 sessionId → 404
   - SSE 连接关闭后 POST → session 已清理

4. **错误处理**
   - 非 JSON body → 400
   - 无效 JSON-RPC → SSE 流返回 Parse Error
   - 未知 method → SSE 流返回 Method Not Found

5. **notification**
   - POST initialized → 202，SSE 流无对应事件

6. **多 session 并发**
   - 同时建立多个 SSE 连接
   - 各 session 互不干扰

### 手动测试

```bash
# 1. 建立 SSE 连接（保持打开）
curl -N http://127.0.0.1:3000/sse
# 输出: event: endpoint
#       data: /message?sessionId=<uuid>

# 2. 另一个终端，发送 initialize
curl -X POST "http://127.0.0.1:3000/message?sessionId=<uuid>" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}'
# SSE 终端输出: event: message
#              data: {"jsonrpc":"2.0","id":1,"result":{...}}

# 3. 发送 tools/list
curl -X POST "http://127.0.0.1:3000/message?sessionId=<uuid>" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
```

## 完成标准

- [ ] `cargo run` 默认启动 SSE 模式，监听配置的 host:port
- [ ] GET /sse 建立 SSE 连接，首事件返回 endpoint URL
- [ ] POST /message 接收请求，响应通过 SSE 流返回
- [ ] session 生命周期管理正确（创建、使用、清理）
- [ ] initialize → tools/list → tools/call 完整流程可用
- [ ] 多 session 并发互不干扰
- [ ] 错误情况返回正确的 JSON-RPC error（通过 SSE 流）
- [ ] 集成测试全部通过
- [ ] curl 手动测试通过
