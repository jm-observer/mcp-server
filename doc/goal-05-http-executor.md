# Goal 05: HTTP 类型 Tool 执行

## 目标

实现 HTTP 类型 tool 的执行逻辑，支持配置化的 HTTP 请求（GET/POST 等），并接入 `tools/call` 处理流程。

## 前置依赖

- Goal 01（配置解析，HTTP tool 定义）
- Goal 02（MCP 协议层）
- Goal 04（HTTP server 集成）

## 涉及文件

```
src/
├── executor/
│   ├── mod.rs           # 新增 HTTP executor 分发
│   └── http.rs          # HTTP 执行器实现
└── protocol/
    └── handler.rs       # handle_tools_call 增加 HTTP 类型分支
tools.d/
└── http_test.toml       # httpbin.org 测试 tool 配置
```

## 实现方式

### HTTP 执行器（http.rs）

```rust
pub struct HttpExecutor {
    client: reqwest::Client,
}

pub struct HttpResult {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl HttpExecutor {
    pub async fn execute(
        &self,
        tool: &RegisteredTool,
        arguments: &HashMap<String, Value>,
    ) -> Result<HttpResult>;
}
```

执行流程：
1. 构建 URL：`base_url`（来自 [config]）+ `path`（来自 tool 定义）
2. 解析参数模板：`path`、`body` 中的 `${var}` 占位符（**注意：用于 `path` 的变量值在替换为真实数据前，必须进行安全的 URL Encode，防止非法字符破坏网络请求**）
3. 构建请求：
   - method: GET / POST / PUT / DELETE 等
   - 根据全局与该 Tool 单独配置的 `headers` 项注入鉴权或业务自定义 HTTP 请求头
   - 如有 body 和 content_type，设置请求体和 Content-Type header
4. 发送请求：受 `effective_timeout` 控制（reqwest 的 timeout 设置）
5. 收集结果：status code、response body

### 参数化支持

HTTP tool 的参数可用于：
- `path` 中的路径参数：`/users/${user_id}`
- `body` 中的请求体参数：`{"name": "${name}"}`

复用 Goal 03 中的 `resolve_template` 函数。

### 接入 handler

修改 `handle_tools_call`：

```rust
match tool_type {
    ToolType::Command => command_executor.execute(tool, args).await,
    ToolType::Http => http_executor.execute(tool, args).await,
}
```

将 `HttpResult` 转为 MCP `ToolCallResult`：
- status code + body 组合为 text content
- 非 2xx 状态码时设置 `is_error: true`

### 依赖

新增：
- `reqwest`：HTTP client（启用 `json` feature）

### 示例配置（http_test.toml）

```toml
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
# 还可以在此处加入： headers = { "Authorization" = "Bearer xxx" } 以满足鉴权 API 需要
body = '{"message": "hello"}'
content_type = "application/json"

[[tools]]
name = "http_get_ip"
description = "Get current IP address"
type = "http"
method = "GET"
path = "/ip"
```

## 测试

### 单元测试

1. **URL 构建**
   - base_url + path 正确拼接
   - path 中的 `${var}` 正确替换

2. **请求体构建**
   - body 中的 `${var}` 正确替换
   - content_type 正确设置

3. **结果转换**
   - 2xx → is_error: false
   - 4xx/5xx → is_error: true
   - response body 正确包含在 content 中

### 集成测试

使用 httpbin.org（或 mock server）：

1. **GET 请求**
   - 调用 http_get_test → 返回 200 及响应内容

2. **POST 请求**
   - 调用 http_post_test → 返回 200，body 中包含发送的 JSON

3. **超时**
   - 设置极短超时 → 超时错误

4. **端到端**
   - 通过 MCP tools/call 调用 HTTP tool → 返回正确结果

## 完成标准

- [ ] HTTP GET/POST 请求正确发送与接收
- [ ] 参数模板在 path 和 body 中正确替换
- [ ] base_url + path 拼接正确
- [ ] 超时控制生效
- [ ] 非 2xx 状态码正确标记为 error
- [ ] tools/call 对 HTTP 类型 tool 端到端可用
- [ ] 所有测试通过
