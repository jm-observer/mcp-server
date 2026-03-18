pub mod config;
pub mod mcp_client;
pub mod llm_client;
pub mod crawler;
pub mod types;
pub mod prompt;
pub mod toml_output;

use clap::Parser;
use std::path::Path;
use anyhow::Result;
use crate::llm_client::LlmClient;
use crate::crawler::HelpCrawler;
use std::fs;

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

    // 1. Connect to MCP Server
    let mut client = mcp_client::McpClient::connect(&mcp_path, &args.server_config_path).await?;
    log::info!("Connected to MCP server child process");

    client.initialize().await?;
    log::info!("MCP Handshake complete");

    // 2. Initialize LLM Client
    let llm = LlmClient::new(&args.vllm_url, "qwen3-coder"); // Replace with actual model from config if needed

    // 3. Recursive Crawling
    log::info!("Starting recursive help crawl for {} (max_depth: {})", args.command_name, args.max_depth);
    let mut crawler = HelpCrawler::new(&mut client, &llm, args.max_depth);
    let help_tree = crawler.crawl(&args.command_name).await?;
    let flat_commands = HelpCrawler::flatten(&help_tree);
    log::info!("Crawled {} command variants", flat_commands.len());

    // 4. Get JSON Schema for tool definitions
    let schema = client.get_tool_schema();

    // 5. Generate TOML for each command using LLM
    let mut tool_outputs = Vec::new();
    for cmd in flat_commands {
        log::info!("Generating TOML for: {}", cmd.full_command.join(" "));
        let prompt = prompt::build_toml_generation_prompt(&cmd, &schema);
        match llm.chat(prompt).await {
            Ok(resp) => {
                match prompt::parse_llm_response(&resp, cmd.full_command.clone()) {
                    Ok(out) => tool_outputs.push(out),
                    Err(e) => log::error!("Failed to parse LLM response for {}: {}", cmd.full_command.join(" "), e),
                }
            }
            Err(e) => log::error!("LLM call failed for {}: {}", cmd.full_command.join(" "), e),
        }
    }

    // 6. Merge and Output TOML
    let final_toml = toml_output::generate_toml_file(&args.command_name, &tool_outputs);
    
    if let Some(path) = &args.output_path {
        fs::write(path, final_toml)?;
        log::info!("Tool configuration saved to: {}", path);
    } else {
        println!("{}", final_toml);
    }

    client.close().await?;
    Ok(())
}
