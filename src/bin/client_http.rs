//! Simple HTTP client for MCP.
//! Reads a JSON request from STDIN, POSTs it to the MCP server (default http://127.0.0.1:3000),
//! and prints the server's response.

use anyhow::Context;
use log::error;
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;
use tokio::io::{self, AsyncReadExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logger – keep consistent with other binary tools
    custom_utils::logger::logger_stdout_debug();

    // Read complete stdin (blocking until EOF)
    let mut stdin_buf = String::new();
    io::stdin()
        .read_to_string(&mut stdin_buf)
        .await
        .context("failed to read stdin")?;

    // Trim whitespace and ensure we got something
    let json_str = stdin_buf.trim();
    if json_str.is_empty() {
        eprintln!("[client_http] No input JSON provided on stdin.");
        return Ok(());
    }

    // Parse the JSON to make sure it's valid
    let payload: Value = serde_json::from_str(json_str).context("provided stdin is not valid JSON")?;

    // Default MCP server address – can be overridden by env var if needed later
    let url = std::env::var("MCP_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".to_string());

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
        println!("{}", body);
    } else {
        error!("server returned error status {}", response.status());
        // Print any error body for debugging purposes
        let err_body = response.text().await.unwrap_or_default();
        eprintln!("Error body: {}", err_body);
    }

    Ok(())
}
