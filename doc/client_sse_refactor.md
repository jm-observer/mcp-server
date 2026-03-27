# MCP SSE 客户端重构方案

## 背景

当前 [`src/client/sse_client.rs`](/home/fengqi/rust/mcp-server/src/client/sse_client.rs) 采用 3 个独立后台任务：

1. SSE reader：连接 `/sse` 并读取服务端事件
2. writer：从 `outbound_rx` 取消息并 `POST` 到握手得到的 endpoint
3. heartbeat：定时发送 `ping`

现状存在几个明确问题：

- 三个任务共享 `endpoint`，通过 `Arc<Mutex<Option<String>>>` 协调，状态分散
- writer 与 heartbeat 都通过轮询等待 endpoint，存在无意义 sleep
- `JoinHandle` 被当成“保活器”，但实际上任务即使丢失 handle 也会继续运行
- 客户端销毁时没有显式取消后台任务
- 重连逻辑只存在于 reader 内，writer 和 heartbeat 只能被动读取共享状态

本方案的目标不是改变协议，而是在**保留 heartbeat** 的前提下，把并发模型收敛成一个更易维护的单任务状态机。

## 重构目标

1. `McpSseClient` 只保留 1 个后台任务句柄
2. 后台任务内部统一使用 `tokio::select!` 驱动 SSE 读取、请求发送、heartbeat
3. `Drop for McpSseClient` 时显式取消该后台任务
4. 去掉 `Arc<Mutex<Option<String>>>` 以及围绕它的轮询等待
5. 保持当前对外 API 形态基本不变：
   - `outbound_sender(&self) -> UnboundedSender<String>`
   - `inbound_receiver(&mut self) -> &mut UnboundedReceiver<String>`

## 非目标

- 本次不改变现有服务端协议
- 本次不新增显式 `shutdown()` API
- 本次不引入新依赖
- 本次不处理“多消费者共享 inbound_rx”这类 API 扩展

## 新结构建议

建议把 `McpSseClient` 收敛为：

```rust
pub struct McpSseClient {
    outbound_tx: UnboundedSender<String>,
    inbound_rx: UnboundedReceiver<String>,
    task_handle: JoinHandle<()>,
}
```

其中：

- `outbound_tx` 供外部发送 JSON-RPC 请求
- `inbound_rx` 供外部接收服务端 `event: message`
- `task_handle` 是唯一后台任务，用于：
  - 建立 / 重建 SSE 连接
  - 解析 `event: endpoint`
  - 转发业务消息
  - 发送 heartbeat
  - 响应 drop 取消

## 单任务事件循环设计

### 整体思路

后台任务内部维护本地状态，而不是跨任务共享：

- `current_endpoint: Option<String>`
- `sse_stream: Option<ByteStream>`
- `heartbeat_interval`
- `outbound_rx`
- `inbound_tx`

事件循环分两层：

1. 外层负责“连接 / 重连”
2. 内层在连接建立后使用 `tokio::select!` 同时处理三类事件

### 建议流程

#### 阶段 1：建立 SSE 连接

1. `GET {base}/sse`
2. 校验响应状态码
3. 从 SSE 流中读取事件，直到获得首个 `event: endpoint`
4. 把 endpoint 保存到 `current_endpoint`
5. 进入阶段 2

如果连接失败或握手失败：

- 记录日志
- sleep 固定退避时间，例如 3 秒
- 返回阶段 1

#### 阶段 2：连接存活期间的事件循环

在 endpoint 已知的前提下，进入：

```rust
tokio::select! {
    maybe_item = sse_stream.next() => { ... }
    maybe_msg = outbound_rx.recv() => { ... }
    _ = heartbeat_interval.tick() => { ... }
}
```

处理规则：

- `sse_stream.next()`
  - 解析 `event: message` 的 `data`
  - 转发到 `inbound_tx`
  - 如果 SSE 断开或读取报错，清空 `current_endpoint`，跳回阶段 1
- `outbound_rx.recv()`
  - 若通道关闭，则退出整个后台任务
  - 若收到消息，则 `POST {base}{current_endpoint}`
  - 发送失败只记日志，不直接退出任务
- `heartbeat_interval.tick()`
  - 构造 `ping` JSON-RPC 请求
  - `POST {base}{current_endpoint}`
  - 失败只记日志，不直接退出任务

## 为什么改成一个任务

### 1. 生命周期集中

旧实现中 reader、writer、heartbeat 的生命周期并不一致。  
改成单任务后，“连接状态”和“发送能力”天然绑定，重连和重置 endpoint 只在一个地方处理。

### 2. 去掉共享可变状态

旧实现依赖 `Arc<Mutex<Option<String>>>`。  
单任务方案把 endpoint 限定为任务内部局部状态，不再需要锁，也不会有 writer/heartbeat 的轮询等待。

### 3. drop 语义更清晰

旧实现保留 3 个 `JoinHandle`，但没有任何取消动作。  
新方案只需在 `Drop` 中对单个 `task_handle.abort()`，行为简单直接。

### 4. 重连逻辑更自然

旧实现里只有 reader 负责重连，其余两个任务只是共享旧状态。  
新方案中 SSE 断开会触发整个连接状态回到“未握手”，不会出现职责分裂。

## heartbeat 保留策略

本方案明确**保留 heartbeat**，但改成单任务内的定时分支，而不是独立任务。

建议保持如下策略：

- 默认 30 秒发送一次 `ping`
- heartbeat 使用与普通业务请求相同的 `POST {base}{endpoint}`
- heartbeat 失败仅记录日志
- 不因为单次 heartbeat 失败立即关闭客户端

这样做的原因：

- 可以为后续服务端“链路活性检测”提供统一的活跃信号
- 不引入额外连接
- 不打散客户端并发模型

## Drop 语义

建议实现：

```rust
impl Drop for McpSseClient {
    fn drop(&mut self) {
        self.task_handle.abort();
    }
}
```

这里要明确两点：

1. `drop JoinHandle` 本身不会取消任务，只会 detach
2. `abort()` 是“强制取消”，不是优雅关闭

对当前客户端来说，`abort()` 足够，因为：

- 没有必须刷盘的本地状态
- 网络请求中断是可接受的
- 调用方通过 drop 销毁客户端时，本身就在表达“立即停止后台行为”

如果后续需要优雅关闭，可以再单独设计 `shutdown().await`，但不属于本次范围。

## 失败场景约定

### outbound channel 被关闭

- 视为调用方不再需要客户端
- 后台任务退出

### inbound receiver 被丢弃

- `inbound_tx.send(...)` 失败
- 记录日志后退出后台任务

### SSE 连接断开

- 记录日志
- 清空 endpoint
- 固定退避后重连

### POST 失败

- 记录日志
- 保持任务继续运行
- 不主动清理 endpoint

## 与当前实现相比的直接收益

- 从 3 个 `JoinHandle` 降到 1 个
- 从跨任务共享状态改为任务内局部状态
- 去掉轮询等待 endpoint
- `Drop` 行为变为显式可控
- 为后续服务端链路活性检测留出清晰接入点

## 建议实施顺序

1. 重写 `src/client/sse_client.rs` 内部并发模型为单任务
2. 保持现有公开 API 不变，避免影响调用方
3. 增加 `Drop for McpSseClient`
4. 修正示例 `client_sse` 的初始化握手与日志输出
5. 在服务端活性检测方案落地后，再根据服务端策略调整 heartbeat 间隔

## 备注

本文件只固化“客户端内部并发模型重构方案”。  
服务端如何基于 heartbeat 做会话活性判定，见单独文档。
