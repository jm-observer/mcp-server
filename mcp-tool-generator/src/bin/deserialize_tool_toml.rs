use mcp_server::config::tool::ToolFile;

/// 反序列化 tools.d/cargo_build.toml 并打印结构化内容
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "./tools.d/cargo_build.toml".to_string());
    println!("==> 读取文件: {}", path);

    let content = tokio::fs::read_to_string(&path).await?;
    let tool_file: ToolFile = toml::from_str(&content)?;

    println!("==> 反序列化成功!\n");

    if let Some(config) = &tool_file.config {
        println!("[config]");
        if let Some(wd) = &config.working_dir {
            println!("  working_dir = {:?}", wd);
        }
        if let Some(timeout) = config.timeout_secs {
            println!("  timeout_secs = {}", timeout);
        }
        if let Some(env) = &config.env {
            println!("  env = {:?}", env);
        }
        if let Some(base_url) = &config.base_url {
            println!("  base_url = {:?}", base_url);
        }
        println!();
    }

    println!("==> 共 {} 个 tool 定义:\n", tool_file.tools.len());

    for (i, tool) in tool_file.tools.iter().enumerate() {
        println!("--- Tool #{} ---", i + 1);
        println!("  name: {}", tool.name);
        println!("  description: {}", tool.description);
        println!("  action: {:?}", tool.action);
        if let Some(timeout) = tool.timeout_secs {
            println!("  timeout_secs: {}", timeout);
        }
        if let Some(env) = &tool.env {
            println!("  env: {:?}", env);
        }
        if let Some(params) = &tool.parameters {
            println!("  parameters ({}):", params.len());
            for p in params {
                println!("    - {} ({}): {} [required={}]", p.name, p.r#type, p.description, p.required);
            }
        }
        println!();
    }

    Ok(())
}
