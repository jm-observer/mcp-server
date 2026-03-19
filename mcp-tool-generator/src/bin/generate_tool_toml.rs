use mcp_tool_generator::llm_client::LlmClient;
use mcp_tool_generator::prompt;
use mcp_tool_generator::toml_output;
use mcp_tool_generator::types::FlatCommand;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    custom_utils::logger::logger_stdout_debug();
    let help_text = tokio::fs::read_to_string("./res/cargo_build_help").await?;
    let full_command: Vec<String> = "cargo build".split_whitespace().map(String::from).collect();
    let cmd_str = full_command.join(" ");
    println!("==> 命令: {}", cmd_str);

    let llm = LlmClient::new("http://127.0.0.1:8082", "Intel/Qwen3.5-122B-A10B-int4-AutoRound");
    let schema = mcp_server::config::tool_config_schema();
    println!("==> 命令: {} {schema}", cmd_str);
    let flat = FlatCommand {
        full_command: full_command.clone(),
        help_text,
    };

    println!("==> 正在调用 LLM 生成 TOML 配置...\n");
    let toml_prompt = prompt::build_toml_generation_prompt(&flat, &schema);
    let resp = llm.chat(toml_prompt).await?;

    println!("==> LLM 原始返回:\n{}\n", resp);

    let tool_output = prompt::parse_llm_response(&resp, full_command)?;

    let final_toml = toml_output::generate_toml_file(&cmd_str, &[tool_output]);
    tokio::fs::write("./tools.d/cargo_build.toml", &final_toml).await?;
    Ok(())
}
