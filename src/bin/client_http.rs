//! Simple HTTP client for MCP.
//! Reads a JSON request from STDIN, POSTs it to the MCP server (default http://127.0.0.1:3000),
//! and prints the server's response.

use anyhow::Context;
use log::{debug, error};
use reqwest::Client;
use serde_json::Value;
use std::env::args;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logger – keep consistent with other binary tools
    custom_utils::logger::logger_stdout_debug();
    let mut args = args();
    args.next();
    let default_payload = r#"{"id":4,"jsonrpc":"2.0","method":"tools/call","params":{"arguments":{"days":1,"url":"https://github.com/loongclaw-ai/loongclaw"},"name":"github-commit-info"}}"#;
    // let default_payload = r#"{"id":3,"jsonrpc":"2.0","method":"tools/call","params":{"arguments":{"command_name":"cargo build"},"name":"mcp-tool"}}"#;
    let stdin_buf = args.next().unwrap_or(default_payload.to_string());
    debug!("{}", stdin_buf);

    // Trim whitespace and ensure we got something
    let json_str = stdin_buf.trim();
    if json_str.is_empty() {
        error!("[client_http] No input JSON provided on stdin.");
        return Ok(());
    }

    // Parse the JSON to make sure it's valid
    let payload: Value = serde_json::from_str(json_str).context("provided stdin is not valid JSON")?;

    // Default MCP server address – can be overridden by env var if needed later
    let url = std::env::var("MCP_URL").unwrap_or_else(|_| "http://192.168.0.68:3001/rpc".to_string());

    // Build the HTTP client (use rustls, 300 s timeout – same as other examples)
    let client = Client::builder()
        .timeout(Duration::from_secs(300))
        .use_rustls_tls()
        .build()
        .context("failed to build reqwest client")?;

    // Send POST request with JSON body
    let response = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .context("HTTP request failed")?;

    // Handle response
    if response.status().is_success() {
        let body = response.text().await.context("failed to read response body")?;
        debug!("{}", body);
    } else {
        error!("server returned error status {}", response.status());
        // Print any error body for debugging purposes
        let err_body = response.text().await.unwrap_or_default();
        error!("Error body: {}", err_body);
    }

    Ok(())
}
