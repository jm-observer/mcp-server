use crate::types::{FlatCommand, SubcommandInfo, ToolOutput};
use async_openai::types::chat::{
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs,
    ChatCompletionRequestUserMessageArgs,
};

pub fn build_subcommand_prompt(command: &str, help_text: &str) -> Vec<ChatCompletionRequestMessage> {
    let system = r#"
    You are a CLI help parser.

Your task is to extract subcommands and their descriptions from a CLI `--help` output text.

Rules:
1. Only extract content under the "Commands:" section.
2. Ignore "Options", "Usage", or any other sections.
3. Each command may include aliases (e.g., "build, b"):
   - Only keep the primary command (the first one before the comma).
   - Discard all aliases.
4. Ignore placeholder entries like "..." or lines without real commands.
5. Trim extra whitespace.
6. Do not invent commands. Only extract explicitly listed ones.
7. Output must be valid JSON only. No explanations.

Output format:
[
  {
    "command": "<primary command>",
    "description": "<description>"
  }
]

If no commands are found, return an empty array: []
    "#;

    let user = format!(
        "命令: {}\n\nHelp 输出:\n{}\n\n请列出所有子命令：",
        command, help_text
    );

    vec![
        ChatCompletionRequestSystemMessageArgs::default()
            .content(system)
            .build()
            .unwrap()
            .into(),
        ChatCompletionRequestUserMessageArgs::default()
            .content(user)
            .build()
            .unwrap()
            .into(),
    ]
}

pub fn parse_subcommands_response(response: &str) -> Vec<SubcommandInfo> {
    // Try to parse as JSON first (new format)
    if let Ok(parsed) = serde_json::from_str::<Vec<SubcommandInfo>>(response.trim()) {
        return parsed;
    }

    // Try to extract JSON from markdown code block
    let trimmed = response.trim();
    if let Some(start) = trimmed.find("```json") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            let json_str = after[..end].trim();
            if let Ok(parsed) = serde_json::from_str::<Vec<SubcommandInfo>>(json_str) {
                return parsed;
            }
        }
    }

    // Fallback: line-by-line parsing (no description available)
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
             cmds.push(SubcommandInfo {
                 command: cleaned.to_string(),
                 description: String::new(),
             });
        }
    }
    cmds
}

pub fn build_toml_generation_prompt(command: &FlatCommand, json_schema: &str) -> Vec<ChatCompletionRequestMessage> {
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
        ChatCompletionRequestSystemMessageArgs::default()
            .content(system)
            .build()
            .unwrap()
            .into(),
        ChatCompletionRequestUserMessageArgs::default()
            .content(user)
            .build()
            .unwrap()
            .into(),
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
