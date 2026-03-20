use bytes::Bytes;
use futures::StreamExt;
use reqwest::Client;
use serde_json::json;
use std::time::Duration;

type ByteStream =
    std::pin::Pin<Box<dyn futures::Stream<Item = Result<Bytes, reqwest::Error>> + Send>>;

const SSE_URL: &str = "http://192.168.0.8:3000/sse";
const TIMEOUT_SECS: u64 = 300;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::builder()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
        .build()?;

    println!("========================================");
    println!("       MCP Client 客户端");
    println!("========================================\n");

    // 1. 建立 SSE 连接
    let (post_url, mut stream) = establish_sse_connection(&client, SSE_URL).await?;
    println!("握手成功，会话端点：{}\n", post_url);

    // 2. 获取工具列表
    list_available_tools(&client, &post_url, &mut stream).await?;

    // 3. 调用具体工具
    call_tool(&client, &post_url, &mut stream).await?;

    Ok(())
}

/// 建立 SSE 连接并返回消息端点 URL 和事件流
async fn establish_sse_connection(
    client: &Client,
    sse_url: &str,
) -> Result<(String, ByteStream), Box<dyn std::error::Error>> {
    println!("正在连接服务器 {}...", sse_url);

    let response = client.get(sse_url).send().await?;

    // 检查状态码
    if response.status().is_client_error() || response.status().is_server_error() {
        eprintln!(
            "错误：HTTP {} - {}",
            response.status(),
            response.status().canonical_reason().unwrap_or("未知错误")
        );
        return Err(format!("SSE 连接失败：HTTP {}", response.status()).into());
    }

    // 创建字节流
    let stream = response.bytes_stream();
    let mut message_endpoint = String::new();

    // 从流中读取第一条消息，获取 endpoint
    let mut stream = Box::pin(stream);
    while let Some(item) = stream.as_mut().next().await {
        let chunk = item?;
        let text = String::from_utf8_lossy(&chunk);

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
        return Err("未能获取消息端点".into());
    }

    let post_url = format!("http://127.0.0.1:3000{}", message_endpoint);
    Ok((post_url, stream))
}

/// 获取并显示可用工具列表
async fn list_available_tools(
    client: &Client,
    post_url: &str,
    stream: &mut ByteStream,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("----------------------------------------");
    println!("正在获取可用工具列表...");

    let request_id = 1;
    let list_request = json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "tools/list",
        "params": {}
    });

    client.post(post_url).json(&list_request).send().await?;
    println!("请求已发送，等待结果...\n");

    // 等待并显示结果
    let mut stream = Box::pin(stream);
    while let Some(item) = stream.as_mut().next().await {
        let chunk = item?;
        let text = String::from_utf8_lossy(&chunk);

        for line in text.lines() {
            if line.starts_with("data: ") {
                let data = &line["data: ".len()..];
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    // 检查是否为工具列表响应
                    if let Some(result) = json.get("result") {
                        println!("========================================");
                        println!("         可用工具列表");
                        println!("========================================\n");

                        if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
                            println!("共找到 {} 个可用工具:\n", tools.len());

                            for (index, tool) in tools.iter().enumerate() {
                                let name = tool
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("unknown");
                                let description = tool
                                    .get("description")
                                    .and_then(|d| d.as_str())
                                    .unwrap_or("无描述");

                                println!("{}. {}", index + 1, name);
                                println!("   描述：{}", description);

                                // 显示参数信息
                                if let Some(schema) =
                                    tool.get("inputSchema").and_then(|s| s.as_object())
                                {
                                    if let Some(properties) =
                                        schema.get("properties").and_then(|p| p.as_object())
                                    {
                                        if !properties.is_empty() {
                                            println!("   参数:");
                                            for (param_name, param_def) in properties {
                                                let param_type = param_def
                                                    .get("type")
                                                    .and_then(|t| t.as_str())
                                                    .unwrap_or("any");
                                                let param_desc = param_def
                                                    .get("description")
                                                    .and_then(|d| d.as_str())
                                                    .unwrap_or("");
                                                println!(
                                                    "      - {}: {} ({})",
                                                    param_name, param_type, param_desc
                                                );
                                            }
                                        }
                                    }
                                }
                                println!();
                            }
                        } else {
                            println!("工具列表响应：{:#}", result);
                        }
                        println!();
                        return Ok(());
                    }
                }
            }
        }
    }

    Err("未收到工具列表响应".into())
}

/// 调用指定工具
async fn call_tool(
    client: &Client,
    post_url: &str,
    stream: &mut ByteStream,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("----------------------------------------");
    println!("正在调用工具...");

    let request_id = 2;
    let tool_request = json!({
        "jsonrpc": "2.0",
        "id": request_id,
        "method": "tools/call",
        "params": {
            "name": "cargo_build",
            "arguments": {
                "cwd": "D:\\git\\mcp-server",
                "bin": "deserialize_tool_toml",
                "package": "mcp-tool-generator",
                "release": true
            }
        }
    });

    println!("正在发送工具调用请求...");
    let _post_response = client.post(post_url).json(&tool_request).send().await?;

    println!("请求已发送，正在等待结果...\n");

    // 等待执行结果
    let mut stream = Box::pin(stream);
    while let Some(item) = stream.as_mut().next().await {
        let chunk = item?;
        let text = String::from_utf8_lossy(&chunk);

        for line in text.lines() {
            if line.starts_with("data: ") {
                let data = &line["data: ".len()..];
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    println!("========================================");
                    println!("           执行结果");
                    println!("========================================\n");

                    // 检查是否有错误
                    if let Some(error) = json.get("error") {
                        eprintln!("执行失败:\n{}", serde_json::to_string_pretty(error)?);
                    } else if let Some(result) = json.get("result") {
                        println!("执行成功:\n{}", serde_json::to_string_pretty(result)?);
                    } else {
                        println!("响应内容:\n{}", serde_json::to_string_pretty(&json)?);
                    }

                    println!("\n========================================");
                    return Ok(());
                }
            }
        }
    }

    Err("未收到工具执行结果".into())
}
