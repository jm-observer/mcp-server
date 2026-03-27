//! An example client that uses the SSE based MCP client.
//!
//! It demonstrates a full MCP session: initialize, initialized, then tools/list.

use log::{debug, info};
use mcp::client::McpSseClient; // the crate name is "mcp"
use serde_json::json;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    custom_utils::logger::logger_stdout("info,client_sse=debug,mcp=debug");
    // Create the client (assume the server is listening on 127.0.0.1:8080)
    let mut client = McpSseClient::new("http://127.0.0.1:3000").await?;

    debug!("connected");
    // Get a handle to send requests to the server
    let tx = client.outbound_sender();

    // 1. Initialize request
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {
                "name": "client-sse",
                "version": env!("CARGO_PKG_VERSION"),
            }
        }
    });
    tx.send(init_req.to_string())?;
    let rx = client.inbound_receiver();
    let msg = rx.recv().await;
    info!("Initialize response: {:?}", msg);

    // 2. Initialized notification (no response expected)
    let init_notif = json!({
        "jsonrpc": "2.0",
        "method": "initialized",
        "params": {}
    });
    tx.send(init_notif.to_string())?;

    info!("calling tools/list");
    // 3. Tools list request
    let tools_req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    tx.send(tools_req.to_string())?;

    // Receive and log responses (in a simple loop)
    while let Some(msg) = rx.recv().await {
        info!("Server response: {}", msg);
    }
    Ok(())
}
