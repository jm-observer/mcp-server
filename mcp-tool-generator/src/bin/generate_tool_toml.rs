use mcp_tool::llm_client::LlmClient;
use mcp_tool::prompt;
use mcp_tool::toml_output;
use mcp_tool::types::CommandHelp;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    custom_utils::logger::logger_stdout_debug();
    let help_text = tokio::fs::read_to_string("./res/cargo_build_help").await?;
    let full_command: Vec<String> = "cargo build".split_whitespace().map(String::from).collect();
    let cmd_str = full_command.join(" ");
    println!("==> 命令: {}", cmd_str);

    let llm = LlmClient::new("http://127.0.0.1:12340/v1", "openai/gpt-oss-120b");
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

    tokio::fs::write("./tools.d/cargo_build.toml", &final_toml).await?;
    println!("==> 已保存到 ./tools.d/cargo_build.toml");
    Ok(())
}
