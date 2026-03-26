pub mod config;
pub mod crawler;
pub mod llm_client;
pub mod prompt;
pub mod toml_output;
pub mod types;

use crate::crawler::HelpCrawler;
use crate::llm_client::LlmClient;
use crate::types::CommandHelp;
use anyhow::{Result, anyhow, bail};
use clap::Parser;
use log::LevelFilter::Info;
use log::error;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Semaphore;

// Generate a tool file for a single command
async fn generate_tool_for_cmd(
    llm: LlmClient,
    cmd: &CommandHelp,
    schema: &str,
    out_dir: PathBuf,
    sem: Arc<Semaphore>,
) -> anyhow::Result<()> {
    let _permit = sem.acquire_owned().await.unwrap();
    // Build the prompt for JSON generation
    let prompt = prompt::build_json_generation_prompt(&cmd, schema);
    // Call the LLM
    let resp = llm.chat(prompt).await?;
    // Parse the JSON response into a ToolOutput
    let tool_output = prompt::parse_json_response(&resp, cmd.full_command.clone())?;
    // Generate TOML content
    let cmd_label = cmd.full_command.join(" ");
    let toml_content = toml_output::generate_single_tool_toml(&cmd_label, &tool_output);
    // Determine file name and path
    let file_name = format!("{}.toml", cmd.full_command.join("-"));
    let file_path = out_dir.join(&file_name);
    // Write the TOML file
    fs::write(&file_path, &toml_content).await?;
    log::info!("Written: {}", file_path.display());
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = custom_utils::logger::logger_feature("mcp-tool", "debug,hyper_util=info,reqwest=info", Info, false).build();
    let args = config::GeneratorConfig::parse();
    // 2. Initialize LLM Client
    let llm = LlmClient::new(&args.vllm_url, &args.model);
    // 3. Recursive Crawling
    let mut crawler = HelpCrawler::new(&llm);
    let help_tree = crawler.crawl(&args.command_name).await?;

    let workspace = if let Some(stripped) = args.workspace.strip_prefix("~/") {
        let home = std::env::var("HOME").expect("HOME environment variable not set");
        PathBuf::from(home).join(stripped)
    } else if args.workspace == "~" {
        PathBuf::from(std::env::var("HOME").expect("HOME environment variable not set"))
    } else {
        PathBuf::from(args.workspace)
    };
    log::info!(
        "Starting tool generator for command: {} workspace: {}",
        args.command_name,
        workspace.display()
    );

    // 5. 创建输出目录: output_dir/command_name/
    let out_dir = workspace.join("tools.d").join(&args.command_name);
    fs::create_dir_all(&out_dir).await?;
    log::info!("Output directory: {}", out_dir.display());

    // 6. Get JSON Schema for tool definitions
    let schema = mcp::config::tool_config_schema();
    // 7. 对每个命令生成 tool 定义并写入独立文件
    let sem = Arc::new(Semaphore::new(5));
    let mut handles = Vec::with_capacity(help_tree.len());
    for cmd in help_tree {
        let sem = sem.clone();
        let out_dir = out_dir.clone();
        let llm = llm.clone();
        let schema = schema.clone();
        let handle = tokio::spawn(async move {
            // Acquire semaphore permit
            if let Err(e) = generate_tool_for_cmd(llm, &cmd, &schema, out_dir, sem).await {
                bail!("Failed to generate tool for '{}': {}", cmd.full_command.join(" "), e);
            }
            Ok(())
        });
        handles.push(handle);
    }

    let mut generated_count = 0;
    let mut generated_fail_count = 0;
    for h in handles {
        if let Err(err) = h.await {
            error!("Failed to build_json {}", err);
            eprintln!("Failed to build_json {}", err);
            generated_fail_count += 1;
        } else {
            generated_count += 1;
        }
    }

    println!(
        "Generated {} tool files in {}, failed: {}",
        generated_count,
        out_dir.display(),
        generated_fail_count
    );
    Ok(())
}
