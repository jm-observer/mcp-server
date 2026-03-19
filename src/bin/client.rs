use futures::StreamExt;
use reqwest::Client;
use serde_json::json;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(300))
        .build()?;

    let sse_url = "http://127.0.0.1:3000/sse";
    println!("正在连接服务器 {}...", sse_url);

    // 1. 发起 GET 请求建立 SSE 连接并获取端点
    let response = client.get(sse_url).send().await?;

    // 如果是 404，可能是因为地址或方法不对
    if response.status() == 404 {
        eprintln!("错误: 404 未找到。请确保服务器已启动且监听端口正确。");
        return Ok(());
    }

    let mut stream = response.bytes_stream();
    let mut message_endpoint = String::new();

    // 我们需要从流中读取第一条消息，它包含了 endpoint
    while let Some(item) = stream.next().await {
        let chunk = item?;
        let text = String::from_utf8_lossy(&chunk);

        // 解析内容中的 endpoint
        // 格式通常是 event: endpoint\ndata: /message?sessionId=xxx
        for line in text.lines() {
            if line.starts_with("data: ") {
                message_endpoint = line["data: ".len()..].trim().to_string();
                break;
            }
        }

        if !message_endpoint.is_empty() {
            break;
        }
    }

    if message_endpoint.is_empty() {
        eprintln!("未能从 SSE 握手中获取有效的端点。");
        return Ok(());
    }

    let post_url = format!("http://127.0.0.1:3000{}", message_endpoint);
    println!("握手成功，会话端点: {}", post_url);

    // 2. 发送 POST 请求执行工具调用
    let tool_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {
            "name": "cargo_build",
            "arguments": {
                "project": "mcp-server",
                "bin": "deserialize_tool_toml",
                "package": "mcp-tool-generator",
                "release": true
            }
        }
    });

    println!("正在发送工具调用请求...");
    let post_response = client.post(&post_url).json(&tool_request).send().await?;

    println!("请求已发送，正在等待结果（检查 SSE 流）...");

    // 真正的结果应该在之前的 stream 变量里，我们继续读取它
    while let Some(item) = stream.next().await {
        let chunk = item?;
        let text = String::from_utf8_lossy(&chunk);

        for line in text.lines() {
            if line.starts_with("data: ") {
                let data = &line["data: ".len()..];
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    println!("收到执行结果: {}", serde_json::to_string_pretty(&json)?);
                    return Ok(());
                }
            }
        }
    }

    Ok(())
}
