use actix_web::{App, HttpResponse, HttpServer, Responder, web};
use bytes::Bytes;
use clap::Parser;
use log::{LevelFilter::Debug, error, info};
use mcp::config::{PromptFile, PromptRegistry, ServerConfig, ToolFile, ToolRegistry};
use mcp::protocol::McpHandler;

use mcp::transport::sse::SessionManager;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Parser, Debug)]
#[command(
    name = "mcp",
    version,
    about = "MCP - A Model Context Protocol server implementation"
)]
struct Cli {
    #[arg(long = "schema", help = "Print tool configuration schema (JSON)")]
    schema: bool,

    #[arg(
        short = 'w',
        long = "cwd",
        default_value = "~/.config/mcp",
        help = "Working directory containing config.toml and tools.d"
    )]
    cwd: String,

    #[arg(long = "stdio", help = "Run in stdio mode (for CLI integration)")]
    stdio: bool,
}

#[derive(Clone)]
pub struct AppState {
    pub handler: Arc<McpHandler>,
    pub sessions: Arc<SessionManager>,
}

#[derive(Deserialize)]
struct SessionQuery {
    #[serde(rename = "sessionId")]
    session_id: String,
}

async fn sse_connect(req: actix_web::HttpRequest, state: web::Data<AppState>) -> impl Responder {
    let (session_id, mut rx) = state.sessions.create_session();
    let endpoint_url = format!("/message?sessionId={}", session_id);
    info!(
        "New SSE connection created, session_id: {}, from: {} {}",
        session_id,
        req.method(),
        req.connection_info().realip_remote_addr().unwrap_or("unknown")
    );

    HttpResponse::Ok()
        .content_type("text/event-stream")
        .insert_header(("Cache-Control", "no-cache"))
        .insert_header(("Connection", "keep-alive"))
        .streaming(async_stream::stream! {
            yield Ok::<_, actix_web::Error>(
                Bytes::from(format!("event: endpoint\ndata: {}\n\n", endpoint_url))
            );

            while let Some(message) = rx.recv().await {
                info!("Forwarding message to session {}: {}", session_id, message);
                yield Ok(
                    Bytes::from(format!("event: message\ndata: {}\n\n", message))
                );
            }
            // Mark session as disconnected for cleanup tracking
            let sessions = state.sessions.clone();
            sessions.mark_disconnected(&session_id);
            info!("SSE session {} closed", session_id);
        })
}

async fn handle_message(
    body: web::Json<Value>,
    query: web::Query<SessionQuery>,
    state: web::Data<AppState>,
) -> impl Responder {
    let session_id = &query.session_id;
    let request = body.into_inner();
    info!("Received message for session {}: {}", session_id, request);

    if !state.sessions.contains(session_id) {
        error!("Session not found: {}", session_id);
        return HttpResponse::NotFound().json(json!({"error": "session not found"}));
    }
    // Update session activity
    state.sessions.touch(session_id);

    let request_str = serde_json::to_string(&request).unwrap();

    match state.handler.handle_request(&request_str).await {
        Some(response) => {
            info!("Handler produced response for session {}: {}", session_id, response);
            if let Err(e) = state.sessions.send(session_id, response) {
                error!("Failed to send response to session {}: {}", session_id, e);
                return HttpResponse::Gone().json(json!({"error": "session closed"}));
            }
            HttpResponse::Accepted()
                .content_type("application/json")
                .json(json!({"ok": true}))
        }
        None => {
            info!("Handler produced no response for session {}", session_id);
            HttpResponse::Accepted()
                .content_type("application/json")
                .json(json!({"ok": true}))
        }
    }
}

async fn run_sse_server(handler: McpHandler, config: &ServerConfig) -> std::io::Result<()> {
    let app_state = AppState {
        handler: Arc::new(handler),
        sessions: Arc::new(SessionManager::new()),
    };

    let bind_addr = (config.server.host.as_str(), config.server.sse_port);
    info!("Starting SSE server on {}:{}", bind_addr.0, bind_addr.1);

    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(app_state.clone()))
            .route("/sse", web::get().to(sse_connect))
            .route("/message", web::post().to(handle_message))
    })
    .bind(bind_addr)?
    .run()
    .await
}

/// 递归加载目录下所有 .toml 工具文件（支持 tools.d/cargo/cargo_build.toml 结构）
fn load_tool_files(dir: &Path, registry: &mut ToolRegistry, default_timeout: u64) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            load_tool_files(&path, registry, default_timeout)?;
        } else if path.extension().unwrap_or_default() == "toml" {
            info!("Loading tool file: {:?}", path);
            let content = fs::read_to_string(&path)?;
            match toml::from_str::<ToolFile>(&content) {
                Ok(tool_file) => {
                    if let Err(e) = registry.register(tool_file, default_timeout) {
                        error!("Failed to register tools from {:?}: {}", path, e);
                    }
                }
                Err(e) => {
                    error!("Failed to parse {:?}: {}", path, e);
                }
            }
        }
    }
    Ok(())
}

/// 递归加载目录下所有 .toml prompt 文件
fn load_prompt_files(dir: &Path, registry: &mut PromptRegistry) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            load_prompt_files(&path, registry)?;
        } else if path.extension().unwrap_or_default() == "toml" {
            info!("Loading prompt file: {:?}", path);
            let content = fs::read_to_string(&path)?;
            match toml::from_str::<PromptFile>(&content) {
                Ok(prompt_file) => {
                    if let Err(e) = registry.register(prompt_file) {
                        error!("Failed to register prompts from {:?}: {}", path, e);
                    }
                }
                Err(e) => {
                    error!("Failed to parse prompt file {:?}: {}", path, e);
                }
            }
        }
    }
    Ok(())
}

// cargo build --workspace --features prod
// cargo run --bin mcp-client -- '{"id":6,"jsonrpc":"2.0","method":"tools/call","params":{"arguments":{"command_name":"git log","workspace":"/home/fengqi/.config/mcp"},"name":"mcp-tool"}}'
#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Cli::parse();

    let _ = custom_utils::logger::logger_feature("mcp", Debug, Debug, false).build();

    if args.schema {
        println!("{}", mcp::config::tool_config_schema());
        return Ok(());
    }

    let raw_path = args.cwd.clone();
    let workspace_path = if let Some(stripped) = raw_path.strip_prefix("~/") {
        let home = std::env::var("HOME").expect("HOME environment variable not set");
        PathBuf::from(home).join(stripped)
    } else if raw_path == "~" {
        PathBuf::from(std::env::var("HOME").expect("HOME environment variable not set"))
    } else {
        PathBuf::from(raw_path)
    };
    info!("Workspace directory: {}", workspace_path.display());
    // The config file is expected to be 'config.toml' inside this workspace
    let config_path = workspace_path.join("config.toml");
    let server_config = if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        toml::from_str::<ServerConfig>(&content).expect("Failed to parse config")
    } else {
        panic!("Config file not found: {:?}", config_path);
    };
    info!("Loaded server config");

    let mut registry = ToolRegistry::new();

    registry.register_builtin_file_tools();
    info!("Builtin file tools registered (list_dir, read_file, write_file)");

    if server_config.security.allow_direct_command {
        registry.register_builtin_direct_command();
        info!("Builtin direct_command tool registered");
    }

    // Define tools directory relative to the workspace
    let tools_dir = workspace_path.join("tools.d");
    if tools_dir.exists() && tools_dir.is_dir() {
        load_tool_files(&tools_dir, &mut registry, server_config.defaults.timeout_secs)?;
    } else {
        info!("tools.d directory not found or is not a directory");
    }

    // 加载 prompts.d 目录
    let mut prompt_registry = PromptRegistry::new();
    let prompts_dir = workspace_path.join("prompts.d");
    if prompts_dir.exists() && prompts_dir.is_dir() {
        load_prompt_files(&prompts_dir, &mut prompt_registry)?;
        info!("Prompts loaded from prompts.d");
    } else {
        info!("prompts.d directory not found or is not a directory");
    }

    let handler = McpHandler::with_prompts(
        Arc::new(registry),
        Arc::new(server_config.clone()),
        Arc::new(prompt_registry),
    );

    if args.stdio {
        mcp::transport::stdio::run_stdio(Arc::new(handler)).await
    } else {
        let http_handler = handler.clone();
        let http_server_config = server_config.clone();
        // Run both HTTP and SSE servers concurrently
        let http_fut = mcp::transport::http::run_http(http_handler, &http_server_config);
        let sse_fut = run_sse_server(handler, &server_config);
        // Await both futures; propagate any error
        tokio::try_join!(http_fut, sse_fut)?;
        Ok(())
    }
}
