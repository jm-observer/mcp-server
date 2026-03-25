// Simple stdio client for MCP
// Builds a JSON‑RPC request (or uses a raw request) and sends it to an
// MCP server started in a separate Tokio task via stdio. The server is
// executed as "./target/debug/mcp --stdio". The client writes the request
// to the server's stdin and reads a single‑line JSON response from the
// server's stdout.

use clap::Parser;
use serde_json::{Value, json};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// Command line arguments for the MCP client
#[derive(Parser, Debug)]
#[command(author, version, about = "MCP stdio client", long_about = None)]
struct Args {
    /// If provided, this raw JSON string is sent directly as the request
    #[arg(short, long, value_name = "JSON")]
    request: Option<String>,
    /// JSON‑RPC method name to invoke (used when --request is not set)
    method: Option<String>,
    /// Optional JSON string for parameters (defaults to empty object)
    #[arg(short, long, default_value = "{}")]
    params: String,
}

async fn run_client(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    // Determine the request JSON
    let request_str = if let Some(raw) = args.request {
        raw
    } else {
        // Build request from method and params
        let method = args.method.unwrap_or_default();
        let params: Value = serde_json::from_str(&args.params).unwrap_or_else(|_| json!({}));
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        })
        .to_string()
    };

    // Spawn the MCP server in stdio mode
    let mut child = Command::new("./target/debug/mcp")
        .arg("--stdio")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn mcp server");

    // Write the request to the server's stdin
    let mut child_stdin = child.stdin.take().expect("failed to open child stdin");
    child_stdin.write_all(request_str.as_bytes()).await?;
    child_stdin.write_all(b"\n").await?;
    child_stdin.flush().await?;
    // 注意：不能立刻 drop(child_stdin)！
    // 服务端 run_stdio 用 tokio::spawn 处理请求，如果这时关闭 stdin，
    // 主循环读到 EOF 退出 → runtime 关闭 → spawn 的任务被取消 → 响应丢失。
    // 所以必须先读到响应，再关闭 stdin。

    // Read a single line response from the server's stdout
    let child_stdout = child.stdout.take().expect("failed to open child stdout");
    let mut reader = BufReader::new(child_stdout);
    let mut response = String::new();

    // 读取 stderr 以便调试
    if let Some(stderr) = child.stderr.take() {
        let mut err_reader = BufReader::new(stderr);
        tokio::spawn(async move {
            loop {
                let mut line = String::new();
                match err_reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) =>
                        println!("{line}"),
                    Err(_) => break,

                }
            }
        });
    }

    // 带超时读取，防止服务端异常时永远阻塞
    match tokio::time::timeout(std::time::Duration::from_secs(300), reader.read_line(&mut response)).await {
        Ok(Ok(_)) => {
            if response.trim().is_empty() {
                println!("Error: server returned empty response");
            } else {
                println!("{}", response.trim_end());
            }
        }
        Ok(Err(e)) => println!("Error reading response: {}", e),
        Err(_) => println!("Error: timeout waiting for server response"),
    }
    // 读完响应后关闭 stdin，让服务端退出
    drop(child_stdin);

    let _ = child.wait().await;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse CLI arguments
    let args = Args::parse();
    // Run client logic in a separate async task
    run_client_task(args).await
}

// Helper function to spawn the client task asynchronously
async fn run_client_task(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let handle = tokio::spawn(async move {
        if let Err(err) = run_client(args).await {
            println!("Client error: {err}");
        }
    });
    handle.await.map_err(|e| -> Box<dyn std::error::Error> { Box::new(e) })?;
    Ok(())
}
