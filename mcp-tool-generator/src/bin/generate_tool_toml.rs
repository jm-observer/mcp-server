use clap::Parser;
use mcp_tool::config::{CliArgs, GeneratorConfig};
use mcp_tool::llm_client::LlmClient;
use mcp_tool::prompt;
use mcp_tool::toml_output;
use mcp_tool::types::CommandHelp;

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

    let help_path = workspace.join("res").join("cargo_build_help");
    let help_text = tokio::fs::read_to_string(&help_path).await?;
    let full_command: Vec<String> = "cargo build".split_whitespace().map(String::from).collect();
    let cmd_str = full_command.join(" ");
    println!("==> 命令: {}", cmd_str);

    let llm = LlmClient::new(&config.vllm_url, &config.model);
    let schema = mcp::config::tool_config_schema();
    println!("==> Schema: {schema}");
    let flat = CommandHelp {
        full_command: full_command.clone(),
        help_text,
    };

    println!("==> 正在调用 LLM 生成 tool 定义（JSON）...\n");
    let json_prompt = prompt::build_json_generation_prompt(&flat, &schema);
    let resp = llm.chat(json_prompt).await?;

    println!("==> LLM 原始返回:\n{}\n", resp);

    let tool_output = prompt::parse_json_response(&resp, full_command)?;

    println!("==> 解析得到 ToolDef: {:?}\n", tool_output.tool_def);

    let final_toml = toml_output::generate_toml_file(&cmd_str, &[tool_output]);
    println!("==> 生成的 TOML:\n{}", final_toml);

    let out_path = workspace.join("tools.d").join("cargo_build.toml");
    tokio::fs::create_dir_all(out_path.parent().unwrap()).await?;
    tokio::fs::write(&out_path, &final_toml).await?;
    println!("==> 已保存到 {}", out_path.display());
    Ok(())
}
