pub mod config;
pub mod crawler;
pub mod llm_client;
pub mod prompt;
pub mod toml_output;
pub mod types;

use crate::crawler::HelpCrawler;
use crate::llm_client::LlmClient;
use anyhow::Result;
use clap::Parser;
use std::fs;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<()> {
    custom_utils::logger::logger_stdout_debug();
    let args = config::GeneratorConfig::parse();

    log::info!("Starting tool generator for command: {}", args.command_name);
    // 2. Initialize LLM Client
    let llm = LlmClient::new(&args.vllm_url, &args.model);

    // 3. Recursive Crawling
    log::info!("Starting recursive help crawl for {}", args.command_name);
    let mut crawler = HelpCrawler::new(&llm);
    let help_tree = crawler.crawl(&args.command_name).await?;

    // 4. 判断是否有子命令，决定生成哪些 tool
    let flat_commands = if help_tree.children.is_empty() {
        // 无子命令：只生成 root 命令自身的 tool
        log::info!("No subcommands found, generating tool for command itself");
        vec![crate::types::FlatCommand {
            full_command: help_tree.full_command.clone(),
            help_text: help_tree.help_text.clone(),
        }]
    } else {
        // 有子命令：跳过 root，只对子命令生成 tool
        let all = HelpCrawler::flatten(&help_tree);
        log::info!("Found {} subcommands, skipping root command", all.len() - 1);
        all.into_iter().skip(1).collect()
    };
    log::info!("Will generate tools for {} commands", flat_commands.len());

    // 5. 创建输出目录: output_dir/command_name/
    let out_dir = Path::new(&args.output_dir).join(&args.command_name);
    fs::create_dir_all(&out_dir)?;
    log::info!("Output directory: {}", out_dir.display());

    // 6. Get JSON Schema for tool definitions
    let schema = mcp::config::tool_config_schema();

    // 7. 对每个命令生成 tool 定义并写入独立文件
    let mut generated_count = 0;
    for cmd in &flat_commands {
        log::info!("Generating tool definition for: {}", cmd.full_command.join(" "));
        let prompt = prompt::build_json_generation_prompt(cmd, &schema);
        match llm.chat(prompt).await {
            Ok(resp) => match prompt::parse_json_response(&resp, cmd.full_command.clone()) {
                Ok(tool_output) => {
                    let cmd_label = cmd.full_command.join(" ");
                    let toml_content = toml_output::generate_single_tool_toml(&cmd_label, &tool_output);
                    let file_name = format!("{}.toml", tool_output.tool_def.name);
                    let file_path = out_dir.join(&file_name);
                    fs::write(&file_path, &toml_content)?;
                    log::info!("Written: {}", file_path.display());
                    generated_count += 1;
                }
                Err(e) => log::error!("Failed to parse LLM response for {}: {}", cmd.full_command.join(" "), e),
            },
            Err(e) => log::error!("LLM call failed for {}: {}", cmd.full_command.join(" "), e),
        }
    }
    log::info!("Generated {} tool files in {}", generated_count, out_dir.display());
    Ok(())
}
