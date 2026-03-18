use crate::types::{FlatCommand, ToolOutput};
use crate::llm_client::ChatMessage;

pub fn build_subcommand_prompt(command: &str, help_text: &str) -> Vec<ChatMessage> {
    let system = r#"你是一个命令行工具分析器。根据命令的 --help 输出，识别出所有可用的子命令。

规则：
1. 只返回子命令名称列表，每行一个
2. 不包含选项/标志（如 --verbose, -h）
3. 不包含 "help" 子命令本身
4. 如果没有子命令，返回空（仅输出 "NONE"）
5. 只返回命令名，不要描述"#;

    let user = format!("命令: {}\n\nHelp 输出:\n{}\n\n请列出所有子命令：", command, help_text);

    vec![
        ChatMessage {
            role: "system".to_string(),
            content: system.to_string(),
        },
        ChatMessage {
            role: "user".to_string(),
            content: user,
        },
    ]
}

pub fn parse_subcommands_response(response: &str) -> Vec<String> {
    let mut cmds = Vec::new();
    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.eq_ignore_ascii_case("NONE") {
            continue;
        }
        if trimmed.contains(' ') || trimmed.starts_with('-') || trimmed.eq_ignore_ascii_case("help") {
            continue;
        }
        let cleaned = trimmed.trim_start_matches("- ").trim_start_matches("* ").trim();
        if !cleaned.is_empty() && !cleaned.contains(' ') {
             cmds.push(cleaned.to_string());
        }
    }
    cmds
}

pub fn build_toml_generation_prompt(command: &FlatCommand, json_schema: &str) -> Vec<ChatMessage> {
    let system = format!(r#"你是一个 MCP tool 配置生成器。根据命令的 --help 输出，生成符合给定 JSON Schema 的 tool 配置。

规则：
1. 为命令生成一个 [[tools]] 配置块（TOML 格式）
2. 从 help 文本中提取参数，定义为 [[tools.parameters]]
3. 使用 ${{var}} 占位符引用参数
4. 判断命令安全性：
   - safe: 只读操作、查询、显示信息
   - dangerous: 删除、修改、写入、发送、部署等有副作用的操作
5. 对 dangerous 命令，在 TOML 前加 # DANGEROUS 注释

JSON Schema:
<schema>
{}
</schema>"#, json_schema);

    let user = format!("命令: {}\n\nHelp 输出:\n{}\n\n请生成该命令的 tool 配置（TOML 格式）。", command.full_command.join(" "), command.help_text);

    vec![
        ChatMessage {
            role: "system".to_string(),
            content: system,
        },
        ChatMessage {
            role: "user".to_string(),
            content: user,
        },
    ]
}

pub fn parse_llm_response(response: &str, command: Vec<String>) -> anyhow::Result<ToolOutput> {
    // 提取 TOML 块
    let toml_block = if let Some(start) = response.find("```toml\n") {
        let after_start = &response[start + 8..];
        if let Some(end) = after_start.find("```") {
            after_start[..end].to_string()
        } else {
            after_start.to_string()
        }
    } else if let Some(start) = response.find(r#"[[tools]]"#) {
        response[start..].to_string()
    } else {
        response.to_string()
    };

    let is_dangerous = response.contains("# DANGEROUS") || response.contains("dangerous") || response.contains("DANGEROUS");

    Ok(ToolOutput {
        toml_block: toml_block.trim().to_string(),
        is_dangerous,
        command,
    })
}
