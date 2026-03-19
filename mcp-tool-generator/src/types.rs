#[derive(Debug, Clone)]
pub struct CommandHelp {
    pub full_command: Vec<String>,
    pub help_text: String,
    pub children: Vec<CommandHelp>,
}

#[derive(Debug, Clone)]
pub struct FlatCommand {
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
    pub toml_block: String,
    pub is_dangerous: bool,
    pub command: Vec<String>,
}
