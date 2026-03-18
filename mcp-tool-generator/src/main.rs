pub mod config;
pub mod mcp_client;

use clap::Parser;
use std::path::Path;
use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let mut args = config::GeneratorConfig::parse();
    args.validate();

    log::info!("Starting tool generator for command: {}", args.command_name);

    let mcp_path = if args.mcp_server_path == "mcp-server" {
        // Fallback for tests if not in PATH
        if cfg!(windows) {
            if Path::new("..\\target\\debug\\mcp-server.exe").exists() {
                "..\\target\\debug\\mcp-server.exe".to_string()
            } else {
                args.mcp_server_path.clone()
            }
        } else {
            if Path::new("../target/debug/mcp-server").exists() {
                "../target/debug/mcp-server".to_string()
            } else {
                args.mcp_server_path.clone()
            }
        }
    } else {
         args.mcp_server_path.clone()
    };

    let mut client = mcp_client::McpClient::connect(&mcp_path, &args.server_config_path).await?;
    log::info!("Connected to MCP server child process");

    client.initialize().await?;
    log::info!("MCP Handshake complete");

    let out = client.execute_command(&args.command_name, &["--help".to_string()], None).await?;
    log::info!("Output:\n{}", out.stdout);

    let schema = client.get_tool_schema();
    log::info!("Tool schema length: {}", schema.len());

    client.close().await?;
    Ok(())
}
