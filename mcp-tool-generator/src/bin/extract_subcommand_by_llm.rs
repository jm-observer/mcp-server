use mcp_tool_generator::llm_client::LlmClient;
use mcp_tool_generator::prompt;

/// 测试完整流程：
/// 1. 用模拟的 help 输出调用 build_subcommand_prompt -> LLM -> parse_subcommands_response
/// 2. 对每个子命令模拟 help 输出，调用 build_toml_generation_prompt -> LLM -> parse_llm_response
/// 3. 合并输出最终 TOML 文件
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    custom_utils::logger::logger_stdout_debug();

    let base_url = "http://127.0.0.1:8082/v1";
    // let base_url = "http://192.168.0.68:12340/v1";
    let model = "Intel/Qwen3.5-122B-A10B-int4-AutoRound";

    let llm = LlmClient::new(base_url, model);

    // ========== Step 1: 解析子命令 ==========
    let command = "cargo";

    let help_text = tokio::fs::read_to_string("./res/cargo_help").await?;

    println!("==> Step 1: 解析子命令");
    println!("==> 请求模型: {}", model);
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
    //
    // // ========== Step 2: 为每个子命令生成 TOML ==========
    // // 这里用主命令的 help 文本模拟（实际流程中会递归获取每个子命令的 --help）
    // let schema = mcp_server::config::tool_config_schema();
    //
    // let mut tool_outputs = Vec::new();
    //
    // // 为主命令生成
    // let main_cmd = FlatCommand {
    //     full_command: vec![command.to_string()],
    //     help_text: help_text.to_string(),
    // };
    //
    // println!("==> Step 2: 为命令生成 TOML 配置");
    // println!("==> 生成: {}", command);
    // let toml_prompt = prompt::build_toml_generation_prompt(&main_cmd, &schema);
    // match llm.chat(toml_prompt).await {
    //     Ok(resp) => {
    //         println!("==> LLM 返回:\n{}\n", resp);
    //         match prompt::parse_llm_response(&resp, main_cmd.full_command.clone()) {
    //             Ok(out) => tool_outputs.push(out),
    //             Err(e) => eprintln!("==> 解析失败: {}", e),
    //         }
    //     }
    //     Err(e) => eprintln!("==> LLM 调用失败: {}", e),
    // }
    //
    // // 为前 3 个子命令生成（避免测试时间过长）
    // for sub in subcommands.iter().take(3) {
    //     let full_cmd = vec![command.to_string(), sub.command.clone()];
    //     let cmd_str = full_cmd.join(" ");
    //     println!("==> 生成: {}", cmd_str);
    //
    //     // 模拟子命令的 help 文本（实际流程中通过 MCP 执行 `cargo <sub> --help` 获取）
    //     let sub_help = format!(
    //         "{}\n\nUsage: {} [OPTIONS]\n\nOptions:\n  -h, --help    Print help\n  -V, --version Print version",
    //         sub.description, cmd_str
    //     );
    //
    //     let flat = FlatCommand {
    //         full_command: full_cmd.clone(),
    //         help_text: sub_help,
    //     };
    //
    //     let toml_prompt = prompt::build_toml_generation_prompt(&flat, &schema);
    //     match llm.chat(toml_prompt).await {
    //         Ok(resp) => {
    //             println!("==> LLM 返回:\n{}\n", resp);
    //             match prompt::parse_llm_response(&resp, full_cmd) {
    //                 Ok(out) => tool_outputs.push(out),
    //                 Err(e) => eprintln!("==> 解析失败: {}", e),
    //             }
    //         }
    //         Err(e) => eprintln!("==> LLM 调用失败: {}", e),
    //     }
    // }
    //
    // // ========== Step 3: 合并输出最终 TOML ==========
    // println!("\n==> Step 3: 生成最终 TOML 文件\n");
    // let final_toml = toml_output::generate_toml_file(command, &tool_outputs);
    // println!("{}", final_toml);

    Ok(())
}
