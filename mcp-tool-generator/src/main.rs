pub mod config;
pub mod crawler;
pub mod llm_client;
pub mod prompt;
pub mod toml_output;
pub mod types;

use crate::config::{CliArgs, GeneratorConfig};
use crate::crawler::HelpCrawler;
use crate::llm_client::LlmClient;
use crate::types::CommandHelp;
use anyhow::{Result, bail};
use clap::Parser;
use log::LevelFilter::Info;
use log::error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Semaphore;

async fn generate_tool_for_cmd(
    llm: LlmClient,
    cmd: &CommandHelp,
    schema: &str,
    out_dir: PathBuf,
    sem: Arc<Semaphore>,
) -> anyhow::Result<()> {
    let _permit = sem.acquire_owned().await.unwrap();
    let prompt = prompt::build_json_generation_prompt(cmd, schema);
    let resp = llm.chat(prompt).await?;
    let tool_output = prompt::parse_json_response(&resp, cmd.full_command.clone())?;
    let cmd_label = cmd.full_command.join(" ");
    let toml_content = toml_output::generate_single_tool_toml(&cmd_label, &tool_output);
    let file_name = format!("{}.toml", cmd.full_command.join("-"));
    let file_path = out_dir.join(&file_name);
    fs::write(&file_path, &toml_content).await?;
    log::info!("Written: {}", file_path.display());
    Ok(())
}

async fn generate_tools(llm: &LlmClient, help_tree: &[CommandHelp], out_dir: &Path) -> anyhow::Result<()> {
    let schema = mcp::config::tool_config_schema();
    let sem = Arc::new(Semaphore::new(5));
    let mut handles = Vec::with_capacity(help_tree.len());
    for cmd in help_tree.iter() {
        let sem = sem.clone();
        let out_dir = out_dir.to_path_buf();
        let llm = llm.clone();
        let schema = schema.clone();
        let cmd = cmd.clone();
        let handle = tokio::spawn(async move {
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
            error!("Failed to generate tool: {}", err);
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

#[tokio::main]
async fn main() -> Result<()> {
    let _ = custom_utils::logger::logger_feature("mcp-tool", "debug,hyper_util=info,reqwest=info", Info, false).build();
    let args = CliArgs::parse();

    let workspace = custom_utils::args::workspace(&args.workspace, "mcp")?;
    log::info!("Workspace directory: {}", workspace.display());

    let config_path = workspace.join("config-generator.toml");
    let config: GeneratorConfig = if config_path.exists() {
        let content = fs::read_to_string(&config_path).await?;
        toml::from_str(&content)?
    } else {
        GeneratorConfig {
            vllm_url: "http://localhost:12340/v1".to_string(),
            model: "openai/gpt-oss-120b".to_string(),
        }
    };

    let command_name = args
        .command_name
        .ok_or_else(|| anyhow::anyhow!("command_name is required as positional argument"))?;

    let llm = LlmClient::new(&config.vllm_url, &config.model);
    let mut crawler = HelpCrawler::new(&llm);
    let help_tree = crawler.crawl(&command_name).await?;

    log::info!(
        "Starting tool generator for command: {} workspace: {}",
        command_name,
        workspace.display()
    );

    let top_command = command_name.split_whitespace().next().unwrap_or(&command_name);
    let out_dir = workspace.join("tools.d").join(top_command);
    fs::create_dir_all(&out_dir).await?;
    log::info!("Output directory: {}", out_dir.display());

    generate_tools(&llm, &help_tree, &out_dir).await?;
    Ok(())
}
