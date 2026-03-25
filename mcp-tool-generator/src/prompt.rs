use crate::types::{FlatCommand, SubcommandInfo, ToolOutput};
use async_openai::types::chat::{
    ChatCompletionRequestMessage, ChatCompletionRequestSystemMessageArgs, ChatCompletionRequestUserMessageArgs,
};
use mcp::config::tool::ToolDef;

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
7. Output must be a valid JSON array of objects. No explanations.

Output format:
[
  {
    "command": "<primary command>",
    "description": "<description>"
  }
]

If no commands are found, return an empty array: []
    "#;

    let user = format!("命令: {}\n\nHelp 输出:\n{}\n\n请列出所有子命令：", command, help_text);

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

pub fn build_json_generation_prompt(command: &FlatCommand, json_schema: &str) -> Vec<ChatCompletionRequestMessage> {
    let system = format!(
        r#"You are an MCP tool configuration generator.
Given a CLI command’s --help output, generate a single tool definition that strictly conforms to the provided JSON Schema.

# Output Requirements

Output exactly one JSON object.

Do NOT output TOML, explanations, markdown, or any extra text.

The output must strictly conform to the JSON Schema.

The object represents one tool only (NOT a full ToolFile).

# Extraction Rules

Extract all CLI parameters from the help text and place them into the parameters array.

Fixed parts of the command must be placed in:

action.command

action.args

Optional arguments MUST be represented via parameters[*].arg.

Do NOT hardcode optional flags or values into action.args.

Do NOT invent parameters that are not explicitly present in the help text.

If information is missing or unclear, use the minimal valid value required by the schema.

# Working Directory (cwd)

If the command typically needs to run in a specific project/working directory (e.g., build tools, package managers, version control), set `"cwd": true`.

When `cwd` is true, the framework automatically injects a required "cwd" parameter (absolute path) for the caller to specify the working directory. You do NOT need to add a "cwd" parameter to the parameters array yourself.

If the command is location-independent (e.g., system utilities, help, version), set `"cwd": false` or omit it.

# Strictness Rules

The output must be valid JSON and fully compliant with the schema.

Do NOT include comments, trailing commas, or additional fields not defined in the schema.

# JSON Schema
JSON Schema:
{}
"#,
        json_schema
    );

    let user = format!(
        "命令: {}\n\nHelp 输出:\n{}\n\n请生成该命令对应的单个 tool 定义（JSON）。",
        command.full_command.join(" "),
        command.help_text
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

/// 从 LLM 响应文本中提取 JSON 字符串。
///
/// 优先级：
/// 1. ```json ... ``` 代码块
/// 2. 整段响应直接作为 JSON
/// 3. 第一个 `{` 到最后一个 `}` 的截取
fn extract_json(response: &str) -> anyhow::Result<&str> {
    let trimmed = response.trim();

    // 1. 提取 ```json ... ``` 代码块
    if let Some(start) = trimmed.find("```json") {
        let after = &trimmed[start + 7..];
        if let Some(end) = after.find("```") {
            let json_str = after[..end].trim();
            if !json_str.is_empty() {
                return Ok(json_str);
            }
        }
    }

    // 2. 整段响应直接作为 JSON
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Ok(trimmed);
    }

    // 3. 第一个 `{` 到最后一个 `}` 的截取
    if let Some(start) = trimmed.find('{') {
        if let Some(end) = trimmed.rfind('}') {
            if end > start {
                return Ok(&trimmed[start..=end]);
            }
        }
    }

    anyhow::bail!("无法从 LLM 响应中提取 JSON 对象:\n{}", trimmed)
}

/// 从 LLM 响应中提取 JSON 并反序列化为 ToolDef。
pub fn parse_json_response(response: &str, command: Vec<String>) -> anyhow::Result<ToolOutput> {
    let json_str = extract_json(response)?;
    let tool_def: ToolDef = serde_json::from_str(json_str)
        .map_err(|e| anyhow::anyhow!("JSON 反序列化为 ToolDef 失败: {}\nJSON 内容:\n{}", e, json_str))?;
    Ok(ToolOutput { tool_def, command })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_from_code_block() {
        let response = r#"这是一些解释文字

```json
{"name": "test", "description": "a test tool", "type": "command", "command": "echo", "args": ["hello"]}
```

以上是生成结果。"#;
        let json_str = extract_json(response).unwrap();
        let tool_def: ToolDef = serde_json::from_str(json_str).unwrap();
        assert_eq!(tool_def.name, "test");
    }

    #[test]
    fn test_extract_json_bare() {
        let response = r#"{"name": "test", "description": "desc", "type": "command", "command": "ls"}"#;
        let json_str = extract_json(response).unwrap();
        let tool_def: ToolDef = serde_json::from_str(json_str).unwrap();
        assert_eq!(tool_def.name, "test");
    }

    #[test]
    fn test_extract_json_with_surrounding_text() {
        let response = r#"Here is the tool definition:
{"name": "cargo_build", "description": "Build a cargo project", "type": "command", "command": "cargo", "args": ["build"]}
Hope this helps!"#;
        let json_str = extract_json(response).unwrap();
        let tool_def: ToolDef = serde_json::from_str(json_str).unwrap();
        assert_eq!(tool_def.name, "cargo_build");
    }

    #[test]
    fn test_extract_json_no_json() {
        let response = "This response has no JSON at all.";
        assert!(extract_json(response).is_err());
    }

    #[test]
    fn test_parse_json_response_missing_required_field() {
        let response = r#"{"description": "no name field", "type": "command"}"#;
        assert!(parse_json_response(response, vec!["test".into()]).is_err());
    }

    #[test]
    fn test_parse_json_response_with_parameters() {
        let response = r#"{
            "name": "cargo_build",
            "description": "Build a cargo project",
            "type": "command",
            "command": "cargo",
            "args": ["build"],
            "parameters": [
                {
                    "name": "package",
                    "description": "Package to build",
                    "type": "string",
                    "required": false,
                    "arg": ["-p", "${package}"]
                }
            ]
        }"#;
        let output = parse_json_response(response, vec!["cargo".into(), "build".into()]).unwrap();
        let params = output.tool_def.parameters.unwrap();
        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "package");
        assert_eq!(
            params[0].arg.as_ref().unwrap(),
            &vec!["-p".to_string(), "${package}".to_string()]
        );
    }
}
