// Simple stdio client for MCP
// Builds a JSON‑RPC request (or uses a raw request) and sends it to an
// MCP server started in a separate Tokio task via stdio. The server is
// executed as "./target/debug/mcp --stdio". The client writes the request
// to the server's stdin and reads a single‑line JSON response from the
// server's stdout.

use clap::Parser;
use log::{debug, warn};

use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// Command line arguments for the MCP client
#[derive(Parser, Debug)]
#[command(author, version, about = "MCP stdio client", long_about = None)]
struct Args {
    /// JSON‑RPC request to send
    #[arg(value_name = "JSON")]
    request: String,
}

async fn run_client(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let request_str = args.request;

    warn!(
        "Please make sure compile mcp add feature 'prod'. If you receive normal messages from stderr, please add the 'prod' feature to the mcp server compilation."
    );
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
                    Ok(_) => debug!("from stderr: {line}"),
                    Err(_) => break,
                }
            }
        });
    }

    // 带超时读取，防止服务端异常时永远阻塞
    match tokio::time::timeout(std::time::Duration::from_secs(300), reader.read_line(&mut response)).await {
        Ok(Ok(_)) => {
            if response.trim().is_empty() {
                debug!("Error: server returned empty response");
            } else {
                debug!("response {}", response.trim_end());
            }
        }
        Ok(Err(e)) => debug!("Error reading response: {}", e),
        Err(_) => debug!("Error: timeout waiting for server response"),
    }
    // 读完响应后关闭 stdin，让服务端退出
    drop(child_stdin);

    debug!("end");

    // let _ = child.wait().await;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse CLI arguments
    let args = Args::parse();
    custom_utils::logger::logger_stdout_debug();
    // Run client logic in a separate async task
    if let Err(err) = run_client(args).await {
        debug!("Client error: {err}");
    }
    Ok(())
}
