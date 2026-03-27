# MCP SSE 客户端实现说明

> 状态说明：本文档描述的是一版“目标实现”，但截至当前仓库状态，`src/client/sse_client.rs` 与本项目真实的 SSE 服务端协议并不一致。后续维护时，必须优先以服务端现有行为为准，而不是直接按本文档中的旧设计继续开发。

## 目标

1. 通过 **Server‑Sent Events (SSE)** 与本地 `mcp-server` 建立持久连接，实时接收服务器推送的 JSON‑RPC 消息。
2. 维持心跳：定时向服务器发送 `ping`，防止连接因空闲被关闭。
3. 提供外部交互通道：
   - **发送通道** (`UnboundedSender<String>`)：外部代码把要发送的 JSON‑RPC 请求放入此通道。
   - **接收通道** (`UnboundedReceiver<String>`)：用于获取服务器返回的响应。
4. 演示调用 `tools/list` 接口，验证客户端能够正常工作。

## 当前问题固化

以下问题来自对当前仓库实现的 review，属于已经确认的偏差，不是待讨论项。

### 1. 会话建立流程写错

当前文档和 `src/client/sse_client.rs` 假设：

- 客户端自行生成 `session_id`
- 连接地址是 `GET /sse?sid=...`
- 请求发送地址是 `POST /request?sid=...`
- 心跳地址是 `POST /ping?sid=...`

但本项目服务端真实实现并不是这样：

- 客户端先连接 `GET /sse`
- 服务端在建立 SSE 连接时创建 session
- 服务端第一条 SSE 事件会返回 `event: endpoint`
- 该事件的 `data` 是实际的消息投递地址：`/message?sessionId=...`
- 后续 JSON-RPC 请求必须 `POST` 到这个服务端返回的 endpoint

这意味着“客户端本地生成 UUID 并拼接 `sid` 参数”的方案对当前服务端无效，`/request` 和 `/ping` 也不是当前服务端暴露的接口。

### 2. SSE 第一条消息不是业务响应，而是 endpoint 握手信息

当前实现把所有 `data:` 行都直接转发给 `inbound_rx`，这会导致调用方首先收到类似：

```text
/message?sessionId=...
```

它不是 JSON-RPC 响应，而是服务端返回的消息投递地址。  
如果客户端不先解析并保存这个 endpoint，而是把它当成普通业务消息透传，上层代码就无法正确发送后续请求。

### 3. 示例二进制缺少初始化握手

`client_sse` 示例当前是直接发送 `tools/list`，但一个完整的 MCP 会话应先执行：

1. `initialize`
2. `initialized`
3. 业务请求，例如 `tools/list`

虽然当前服务端实现暂时没有强制校验初始化状态，但示例如果省略这一步，会把错误的使用方式固化下来；一旦后续服务端补上会话状态管理，这个示例将直接失效。

### 4. 当前实现与项目约束不一致

当前落地代码还有两处与仓库约束冲突：

- `src/client/sse_client.rs` 使用了 `#[allow(dead_code)]`，不符合项目“禁止用 `#[allow(...)]` 压制警告”的约束
- `src/bin/client_sse.rs` 使用 `println!` 输出，不符合项目“应用日志统一使用 `log` + `custom-utils`”的约束

## 与服务端真实协议对齐后的正确认知

如果后续要继续实现或修复 SSE 客户端，应以如下流程为准：

1. 连接 `GET {base}/sse`
2. 读取首个 SSE 事件，解析 `event: endpoint` 对应的 `data`
3. 将该 `data` 解析为真实消息投递地址，例如 `/message?sessionId=...`
4. 后续所有 JSON-RPC 请求都 `POST` 到该 endpoint
5. SSE 中 `event: message` 的 `data` 才是需要转发给上层的 JSON-RPC 响应
6. 在示例中先完成 `initialize` / `initialized`，再发 `tools/list`

## 关键实现文件

| 文件路径 | 作用 |
| -------- | ---- |
| `src/client/mod.rs` | 公共入口，导出 `McpSseClient` |
| `src/client/sse_client.rs` | 核心实现：创建会话、启动三大后台任务（SSE 读取、请求发送、心跳）并提供 sender/receiver |
| `src/bin/client_sse.rs` | 示例二进制，演示如何使用 `McpSseClient` |
| `Cargo.toml` | 添加 `anyhow` 依赖、配置新二进制目标 `client-sse` |

## 旧设计记录

以下内容保留为本次实现时采用过的旧设计记录，仅用于追溯，不应继续视为当前项目的正确协议说明。

### 1. `McpSseClient` 结构体
```rust
#[derive(Debug)]
pub struct McpSseClient {
    base_url: String,               // 服务器根地址，例如 "http://127.0.0.1:8080"
    http: reqwest::Client,          // 复用的 reqwest 客户端（使用 rustls）
    session_id: String,             // 本次会话的 UUID（客户端自行生成）
    outbound_tx: UnboundedSender<String>, // 用户写入请求的通道
    inbound_tx: UnboundedSender<String>,  // SSE 读取后写入的通道
    inbound_rx: UnboundedReceiver<String>,
    // 以下三个 handle 仅用于保持任务存活
    _reader_handle: JoinHandle<()>,
    _writer_handle: JoinHandle<()>,
    _heartbeat_handle: JoinHandle<()>,
}
```

### 2. 创建与启动 (`McpSseClient::new`)
1. **构建 reqwest 客户端**（`use_rustls_tls()`）。
2. **生成会话 UUID**（`Uuid::new_v4()`），因为服务器目前不需要专门的创建会话 API。
3. **建立通道**：`outbound_tx / outbound_rx` 与 `inbound_tx / inbound_rx`。
4. **克隆必要变量**（`base`, `sid`, `http`）为三个后台任务准备。
5. **启动任务**：
   - **SSE Reader**：连接 `GET {base}/sse?sid={sid}`，读取 `data:` 行并发送到 `inbound_tx`。错误或断开后 3 秒后重连。
   - **Writer**：从 `outbound_rx` 读取消息，`POST {base}/request?sid={sid}`，Content-Type `application/json`。
   - **Heartbeat**：每 30 秒 `POST {base}/ping?sid={sid}`，仅记录错误。
6. 返回包含所有字段的实例。

### 3. 对外 API
```rust
impl McpSseClient {
    /// 获得发送通道的克隆句柄
    pub fn outbound_sender(&self) -> UnboundedSender<String> { self.outbound_tx.clone() }

    /// 获得接收通道的可变引用（用于 `await recv()`）
    pub fn inbound_receiver(&mut self) -> &mut UnboundedReceiver<String> { &mut self.inbound_rx }
}
```

## 示例二进制 (`client_sse`)
```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 假设服务器在 127.0.0.1:8080
    let mut client = McpSseClient::new("http://127.0.0.1:8080").await?;

    // 发送 `tools/list` 请求
    let tx = client.outbound_sender();
    let request = r#"{
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/list",
        "params": null
    }"#;
    tx.send(request.to_string())?;

    // 读取并打印响应
    let rx = client.inbound_receiver();
    while let Some(msg) = rx.recv().await {
        println!("Server response: {}", msg);
    }
    Ok(())
}
```
运行方式：`cargo run --bin client-sse`

## 配置说明
- **依赖**：在 `Cargo.toml` 中新增 `anyhow = "1.0"`（错误处理），`reqwest` 继续使用 `json`、`stream` 两个 feature（不需要 `sse`）。
- **二进制目标**：已在 `Cargo.toml` 添加 `[[bin]] name = "client-sse" path = "src/bin/client_sse.rs"`。
- **日志**：使用项目已有的 `log` + `custom-utils` 日志实现，所有错误和关键事件都有日志记录。

## 可选扩展
- 若服务器后续需要显式会话创建，可在 `new` 中先 `POST /session` 获取 `session_id`。
- 支持自定义心跳间隔（在构造函数加入参数）。
- 实现 `shutdown` 方法以便显式销毁会话。
- 通过回调向调用者报告发送/接收错误。

---

**本文件用于记录 MCP SSE 客户端的实现背景、已确认问题，以及旧设计与真实服务端协议之间的差异，便于后续维护与修复。**
