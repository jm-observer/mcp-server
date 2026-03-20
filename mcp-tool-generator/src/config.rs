use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about = "MCP Tool Generator - creates tool definitions by inspecting commands", long_about = None)]
pub struct GeneratorConfig {
    #[arg(short = 's', long = "mcp-server", default_value = "mcp-server")]
    pub mcp_server_path: String,

    #[arg(short = 'c', long, default_value = "config.toml")]
    pub server_config_path: String,

    #[arg(short = 'u', long, default_value = "http://localhost:8000")]
    pub vllm_url: String,

    #[arg(short = 'm', long, default_value = "qwen3-coder")]
    pub model: String,

    #[arg(short = 'd', long, default_value_t = 2)]
    pub depth: usize,

    #[arg()]
    pub command_name: String,

    #[arg(short, long, default_value = "tools.d")]
    pub output_dir: String,
}

impl GeneratorConfig {
    pub fn validate(&mut self) {
        if self.depth > 5 {
            self.depth = 5;
        }
    }
}
