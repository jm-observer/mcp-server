use mcp_tool::llm_client::LlmClient;
use mcp_tool::prompt;
use mcp_tool::toml_output;
use mcp_tool::types::CommandHelp;

/// 读取 help 文件，通过 build_json_generation_prompt -> LLM -> parse_json_response 链路
/// 生成 ToolDef，再经 toml_output::generate_toml_file 序列化为 TOML 并写入文件。
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    custom_utils::logger::logger_stdout_debug();

    let base_url = std::env::var("LLM_BASE_URL").unwrap_or_else(|_| "http://192.168.0.68:12340/v1".to_string());
    let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "openai/gpt-oss-120b".to_string());
    let help_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "./res/cargo_build_help".to_string());
    let output_path = std::env::args()
        .nth(2)
        .unwrap_or_else(|| "./tools.d/cargo_build_llm.toml".to_string());

    println!("==> 读取 help 文件: {}", help_path);
    let help_text = tokio::fs::read_to_string(&help_path).await?;

    let llm = LlmClient::new(&base_url, &model);

    // 从 help 文件名推断命令名，例如 "cargo_build_help" -> "cargo build"
    let cmd_str = std::path::Path::new(&help_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("cargo_build")
        .trim_end_matches("_help")
        .replace('_', " ");
    let full_command: Vec<String> = cmd_str.split_whitespace().map(String::from).collect();

    println!("==> 命令: {}", cmd_str);

    let schema = mcp::config::tool_config_schema();

    let flat = CommandHelp {
        full_command: full_command.clone(),
        help_text,
    };

    println!("==> 正在调用 LLM ({})...\n", model);
    let json_prompt = prompt::build_json_generation_prompt(&flat, &schema);
    let response = llm.chat(json_prompt).await?;
    println!("==> LLM 原始返回:\n{}\n", response);

    let tool_output = prompt::parse_json_response(&response, full_command)?;
    println!("==> 解析得到 ToolDef: {:?}\n", tool_output.tool_def);

    let final_toml = toml_output::generate_toml_file(&cmd_str, &[tool_output]);
    println!("==> 生成的 TOML:\n{}\n", final_toml);

    tokio::fs::write(&output_path, &final_toml).await?;
    println!("==> 已写入: {}", output_path);

    Ok(())
}
