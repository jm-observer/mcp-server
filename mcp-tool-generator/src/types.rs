use mcp::config::tool::ToolDef;

#[derive(Debug, Clone)]
pub struct CommandHelp {
    pub full_command: Vec<String>,
    pub help_text: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct SubcommandInfo {
    pub command: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub tool_def: ToolDef,
    pub command: Vec<String>,
}
