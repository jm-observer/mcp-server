use clap::Parser;
use serde::Deserialize;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about = "MCP Tool Generator - creates tool definitions by inspecting commands", long_about = None)]
pub struct CliArgs {
    #[arg(short = 'w', long)]
    pub workspace: Option<String>,

    #[arg()]
    pub command_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeneratorConfig {
    #[serde(default = "default_vllm_url")]
    pub vllm_url: String,

    #[serde(default = "default_model")]
    pub model: String,
}

fn default_vllm_url() -> String {
    "http://localhost:12340/v1".to_string()
}

fn default_model() -> String {
    "openai/gpt-oss-120b".to_string()
}
