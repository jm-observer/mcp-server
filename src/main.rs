use mcp_server::config::{ServerConfig, ToolRegistry, ToolFile};
use tracing::{info, error};
use std::fs;
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    // 1. 加载 ServerConfig
    let server_config = if Path::new("config.toml").exists() {
        let content = fs::read_to_string("config.toml")?;
        toml::from_str::<ServerConfig>(&content)?
    } else {
        ServerConfig::default()
    };
    info!("Loaded server config: {:?}", server_config);

    // 2. 加载 ToolRegistry
    let mut registry = ToolRegistry::new();
    let tools_dir = Path::new("tools.d");
    if tools_dir.exists() && tools_dir.is_dir() {
        for entry in fs::read_dir(tools_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().unwrap_or_default() == "toml" {
                info!("Loading tool file: {:?}", path);
                let content = fs::read_to_string(&path)?;
                match toml::from_str::<ToolFile>(&content) {
                    Ok(tool_file) => {
                        if let Err(e) = registry.register(tool_file, server_config.defaults.timeout_secs) {
                            error!("Failed to register tools from {:?}: {}", path, e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse {:?}: {}", path, e);
                    }
                }
            }
        }
    } else {
        info!("tools.d directory not found or is not a directory");
    }

    info!("Loaded {} tools", registry.len());
    info!("Tool names: {:?}", registry.tool_names());

    Ok(())
}
