use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about = "MCP Tool Generator - creates tool definitions by inspecting commands", long_about = None)]
pub struct GeneratorConfig {
    // removed mcp_server_path field (now fixed to "mcp-server")
    #[arg(short = 'c', long, default_value = "config.toml")]
    pub server_config_path: String,

    #[arg(short = 'u', long, default_value = "http://localhost:12340")]
    pub vllm_url: String,

    #[arg(short = 'm', long, default_value = "openai/gpt-oss-120b")]
    pub model: String,

    // depth is fixed to 2, removed user input
    // pub depth: usize,  // no longer needed
    #[arg()]
    pub command_name: String,

    #[arg(short, long, default_value = "tools.d")]
    pub output_dir: String,
}

impl GeneratorConfig {
    pub fn validate(&mut self) {
        // No validation needed now
    }
}
