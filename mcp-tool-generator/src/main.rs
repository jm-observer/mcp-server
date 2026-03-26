pub mod config;
pub mod crawler;
pub mod llm_client;
pub mod prompt;
pub mod toml_output;
pub mod types;

use crate::crawler::{HelpCrawler};
use crate::llm_client::LlmClient;
use anyhow::Result;
use clap::Parser;
use tokio::fs;
use std::path::Path;
use std::sync::Arc;
use log::error;
use log::LevelFilter::Info;
use tokio::sync::Semaphore;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = custom_utils::logger::logger_feature("mcp-tool", "debug,hyper_util=info,reqwest=info", Info, false).build();
    let args = config::GeneratorConfig::parse();

    log::info!("Starting tool generator for command: {}", args.command_name);

    // 2. Initialize LLM Client
    let llm = LlmClient::new(&args.vllm_url, &args.model);
    // 3. Recursive Crawling
    log::info!("Starting recursive help crawl for {}", args.command_name);
    let mut crawler = HelpCrawler::new(&llm);
    let help_tree = crawler.crawl(&args.command_name).await?;
    log::info!("Will generate tools for {} commands", help_tree.len());
    // 5. 创建输出目录: output_dir/command_name/
    let out_dir = Path::new(&args.output_dir).join(&args.command_name);
    fs::create_dir_all(&out_dir).await?;
    log::info!("Output directory: {}", out_dir.display());

    // 6. Get JSON Schema for tool definitions
    let schema = mcp::config::tool_config_schema();
    // 7. 对每个命令生成 tool 定义并写入独立文件
    let mut generated_count = 0;
    let sem = Arc::new(Semaphore::new(5));
    let mut handles = Vec::with_capacity(help_tree.len());
    for cmd in help_tree {
        let sem = sem.clone();
        let out_dir = out_dir.clone();
        let llm = llm.clone();
        let prompt = prompt::build_json_generation_prompt(&cmd, &schema);
        let handle = tokio::spawn(async move {
            log::info!("Generating tool definition for: {}", cmd.full_command.join(" "));
            // 获取令牌（没有就等待）
            let _permit = sem.acquire_owned().await.unwrap();
            match llm.chat(prompt).await {
                Ok(resp) => match prompt::parse_json_response(&resp, cmd.full_command.clone()) {
                    Ok(tool_output) => {
                        let cmd_label = cmd.full_command.join(" ");
                        let toml_content = toml_output::generate_single_tool_toml(&cmd_label, &tool_output);
                        let file_name = format!("{}.toml", tool_output.tool_def.name);
                        let file_path = out_dir.join(&file_name);
                        if let Err(err) = fs::write(&file_path, &toml_content).await {
                            error!("Failed to write toml output for {}: {}", cmd.full_command.join(" "), err);
                        } else {
                            log::info!("Written: {}", file_path.display());
                        }
                    }
                    Err(e) => log::error!("Failed to parse LLM response for {}: {}", cmd.full_command.join(" "), e),
                },
                Err(e) => log::error!("LLM call failed for {}: {}", cmd.full_command.join(" "), e),
            }
        });
        handles.push(handle);
    }
    for h in handles {
        if let Err(err) =  h.await {
            error!("Failed to build_json {}", err);
        } else {
            generated_count += 1;
        }
    }

    log::info!("Generated {} tool files in {}", generated_count, out_dir.display());
    Ok(())
}
