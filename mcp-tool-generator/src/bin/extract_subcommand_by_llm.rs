use clap::Parser;
use mcp_tool::config::{CliArgs, GeneratorConfig};
use mcp_tool::llm_client::LlmClient;
use mcp_tool::prompt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    custom_utils::logger::logger_stdout_debug();
    let args = CliArgs::parse();

    let workspace = custom_utils::args::workspace(&args.workspace, "mcp")?;
    log::info!("Workspace directory: {}", workspace.display());

    let config_path = workspace.join("config-generator.toml");
    let config: GeneratorConfig = if config_path.exists() {
        let content = tokio::fs::read_to_string(&config_path).await?;
        toml::from_str(&content)?
    } else {
        GeneratorConfig {
            vllm_url: "http://localhost:12340/v1".to_string(),
            model: "openai/gpt-oss-120b".to_string(),
        }
    };

    let llm = LlmClient::new(&config.vllm_url, &config.model);

    let command = "cargo";
    let help_path = workspace.join("res").join("cargo_help");
    let help_text = tokio::fs::read_to_string(&help_path).await?;

    println!("==> Step 1: 解析子命令");
    println!("==> 请求模型: {}", config.model);
    println!("==> 测试命令: {}\n", command);

    let subcmd_prompt = prompt::build_subcommand_prompt(command, &help_text);
    let response = llm.chat(subcmd_prompt).await?;
    println!("==> LLM 返回:\n{}\n", response);

    let subcommands = prompt::parse_subcommands_response(&response);
    println!("==> 解析到 {} 个子命令:", subcommands.len());
    for sub in &subcommands {
        println!("    - {} : {}", sub.command, sub.description);
    }
    println!();

    Ok(())
}
