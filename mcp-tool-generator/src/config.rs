use clap::Parser;

#[derive(Parser, Debug, Clone)]
#[command(author, version, about = "MCP Tool Generator - creates tool definitions by inspecting commands", long_about = None)]
pub struct GeneratorConfig {
    #[arg(short = 'w', long, default_value = ".")]
    pub workspace: String,

    #[arg(short = 'u', long, default_value = "http://localhost:12340")]
    pub vllm_url: String,

    #[arg(short = 'm', long, default_value = "openai/gpt-oss-120b")]
    pub model: String,

    #[arg()]
    pub command_name: String,

    #[arg(short, long, default_value = "./tools.d")]
    pub output_dir: String,
}
