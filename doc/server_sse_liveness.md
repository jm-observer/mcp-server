# MCP SSE 服务端链路活性检测补全方案

## 背景

当前服务端 SSE 相关实现位于：

- [`src/main.rs`](/home/fengqi/rust/mcp-server/src/main.rs)
- [`src/transport/sse.rs`](/home/fengqi/rust/mcp-server/src/transport/sse.rs)

现状具备以下能力：

- `GET /sse` 时创建 session，并返回 `event: endpoint`
- `POST /message?sessionId=...` 时把 JSON-RPC 请求交给 handler
- handler 支持 `ping`

但当前缺少“链路活性”相关的完整设计：

- session 创建后没有最后活跃时间
- SSE 断开后没有明确清理策略
- 客户端即使长期失联，session 也可能一直保留
- `ping` 只作为普通方法处理，没有参与 session 活性维护

本文件的目标是把服务端需要补全的活性检测机制固化下来，并与客户端 heartbeat 对齐。

## 目标

1. 服务端能够判断某个 session 是否仍然活跃
2. 客户端长期失联时，服务端可以回收 session
3. 客户端定时 heartbeat 能更新活跃时间
4. SSE 连接断开时，不立即误删仍可能短时重连的 session
5. 不引入复杂分布式语义，只覆盖当前单进程内存态实现

## 非目标

- 不做跨进程 session 共享
- 不做持久化 session
- 不引入外部存储
- 不改变现有 `/sse` 与 `/message` 对外协议

## 核心设计

建议为每个 session 增加以下状态：

- `sender: UnboundedSender<String>`
- `last_seen_at: Instant`
- `sse_connected: bool`

其中：

- `last_seen_at` 表示最近一次确认客户端存活的时间
- `sse_connected` 表示当前是否存在活动中的 SSE 下行连接

## 活跃时间更新规则

以下事件发生时，应更新 `last_seen_at`：

1. 新建 SSE 连接成功
2. 收到该 session 的任意 `POST /message`
3. 收到该 session 的 `ping`
4. 可选：成功向该 session 下发消息时

其中最关键的是前 3 个，因为它们明确表示“客户端仍在主动和服务端交互”。

## session 生命周期建议

### 1. 创建阶段

当客户端 `GET /sse`：

- 创建 session
- 初始化 `last_seen_at = Instant::now()`
- 设置 `sse_connected = true`

### 2. 正常运行阶段

当客户端调用 `POST /message?sessionId=...`：

- 先校验 session 是否存在
- 更新 `last_seen_at`
- 再处理 JSON-RPC 请求

如果请求是 `ping`，不需要特殊路由，但仍应更新 `last_seen_at`。

### 3. SSE 断开阶段

当 `rx.recv().await` 结束或 SSE streaming 退出：

- 不建议立刻删除 session
- 应只把 `sse_connected = false`
- 保留 session 进入“等待重连”状态

原因：

- 网络闪断时，客户端可能很快重连
- 如果立即删除，客户端已有的 endpoint 会瞬间失效
- 当前客户端计划保留 heartbeat 和重连逻辑，服务端应允许一个短暂恢复窗口

### 4. 过期回收阶段

由后台清理任务周期扫描 session：

- 若 `now - last_seen_at > session_ttl`
- 且 `sse_connected == false`
- 则删除该 session

建议初始参数：

- `heartbeat_interval = 30s`
- `session_ttl = 90s`
- `cleanup_interval = 15s`

这个比例允许客户端在 2 到 3 个 heartbeat 周期内恢复。

## 推荐数据结构调整

当前 [`src/transport/sse.rs`](/home/fengqi/rust/mcp-server/src/transport/sse.rs) 里：

```rust
sessions: DashMap<String, mpsc::UnboundedSender<String>>
```

建议调整为：

```rust
struct SessionEntry {
    tx: mpsc::UnboundedSender<String>,
    last_seen_at: Instant,
    sse_connected: bool,
}

sessions: DashMap<String, SessionEntry>
```

然后给 `SessionManager` 增加明确的方法：

- `create_session()`
- `touch(session_id)`
- `mark_disconnected(session_id)`
- `send(session_id, message)`
- `remove_expired_sessions(now, ttl)`

这样可以避免由外层路由直接操作细节状态。

## 与客户端 heartbeat 的配合

客户端侧建议：

- 每 30 秒发送一次 `ping`
- 在单任务事件循环里统一调度 heartbeat

服务端侧建议：

- 收到 `ping` 与收到普通请求一样，统一 `touch(session_id)`
- `ping` 仍由现有 MCP handler 返回空结果即可

这里不需要为 heartbeat 单独设计新 HTTP 路由。  
保留当前协议最简单：heartbeat 只是普通 JSON-RPC 请求的一种。

## SSE 断开后的具体处理建议

服务端在 `sse_connect` 的 streaming 退出时，当前只会打印日志。  
建议补成：

1. 记录该 session 的 SSE 下行已断开
2. 不立即移除 session
3. 等待 cleanup task 根据 TTL 回收

这样可以兼容以下场景：

- 客户端 reader 断线后 3 秒内重连
- heartbeat 因为 endpoint 仍有效而继续触发活跃更新
- 客户端短暂网络抖动后恢复

## 是否允许旧 endpoint 在重连后继续使用

建议短期内允许。

原因：

- 当前 endpoint 只是 `/message?sessionId=...`
- endpoint 本质绑定的是 session，而不是某一条具体 SSE TCP 连接
- 只要 session 仍未过期，旧 endpoint 继续有效能降低客户端复杂度

如果未来要提升安全性或收敛状态，再考虑：

- 重连时轮换 session
- 或对 endpoint 增加版本戳

但这不属于当前范围。

## 建议新增的后台清理任务

服务端在启动 SSE 模式时，建议额外启动一个清理任务：

1. 固定周期运行，例如每 15 秒
2. 扫描全部 session
3. 删除：
   - `sse_connected == false`
   - 且 `last_seen_at` 超过 TTL
4. 记录清理日志

该任务的职责仅是资源回收，不参与正常请求处理。

## 失败场景约定

### 客户端只建立 SSE，不发送任何请求

- `last_seen_at` 会在创建时更新一次
- 若后续一直无 heartbeat 且连接断开，则到期回收

### 客户端 heartbeat 停止，但 SSE 仍保持连接

- 可视为“下行连接仍存活”
- 建议只要 `sse_connected == true`，就不要因为 heartbeat 缺失立即删除
- 但可以记录告警日志，提示客户端活跃信号异常

### 服务端向 session 推送消息失败

- 可视为连接可能已失效
- 建议记录日志，并将 `sse_connected = false`
- 后续交给 TTL 清理，而不是立刻删除

## 分阶段落地建议

### 第一阶段

- 为 session 增加 `last_seen_at`
- `POST /message` 时更新活跃时间
- 增加 cleanup task

### 第二阶段

- 为 session 增加 `sse_connected`
- 在 SSE streaming 结束时标记断开
- cleanup task 基于 `sse_connected + TTL` 判断删除

### 第三阶段

- 细化推送失败时的状态更新
- 视需要补充更丰富的活性日志与指标

## 与客户端重构方案的关系

客户端单任务模型保留 heartbeat 后，服务端就可以把 heartbeat 作为稳定的活跃输入之一。  
两边配套后，整体行为会更一致：

- 客户端负责周期性声明“我还活着”
- 服务端负责基于最近活跃时间和连接状态做回收

## 备注

本文件只描述服务端活性检测与 session 回收方案。  
客户端内部并发模型重构见 [`doc/client_sse_refactor.md`](/home/fengqi/rust/mcp-server/doc/client_sse_refactor.md)。
