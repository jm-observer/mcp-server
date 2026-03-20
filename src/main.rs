use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use mcp_server::config::{ServerConfig, ToolFile, ToolRegistry};
use mcp_server::protocol::McpHandler;
use mcp_server::transport::sse::SessionManager;
use serde::Deserialize;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use log::{error, info, LevelFilter::{Debug, Info}};
use bytes::Bytes;

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

async fn sse_connect(state: web::Data<AppState>) -> impl Responder {
    let (session_id, mut rx) = state.sessions.create_session();
    let endpoint_url = format!("/message?sessionId={}", session_id);
    info!("New SSE connection created, session_id: {}", session_id);

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

    let request_str = serde_json::to_string(&request).unwrap();

    match state.handler.handle_request(&request_str).await {
        Some(response) => {
            info!("Handler produced response for session {}: {}", session_id, response);
            if let Err(e) = state.sessions.send(session_id, response) {
                error!("Failed to send response to session {}: {}", session_id, e);
                return HttpResponse::Gone().json(json!({"error": "session closed"}));
            }
            HttpResponse::Accepted().finish()
        }
        None => {
            info!("Handler produced no response for session {}", session_id);
            HttpResponse::Accepted().finish()
        },
    }
}

async fn run_sse_server(handler: McpHandler, config: &ServerConfig) -> std::io::Result<()> {
    let app_state = AppState {
        handler: Arc::new(handler),
        sessions: Arc::new(SessionManager::new()),
    };

    let bind_addr = (config.server.host.as_str(), config.server.port);
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

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let _ = custom_utils::logger::logger_feature("mcp-server", Debug, Info, false).build();
    let args: Vec<String> = std::env::args().collect();

    if args.contains(&"--schema".to_string()) {
        println!("{}", mcp_server::config::tool_config_schema());
        return Ok(());
    }

    let mut config_path = Path::new("config.toml").to_path_buf();
    for i in 0..args.len() {
        if args[i] == "--config" && i + 1 < args.len() {
            config_path = Path::new(&args[i + 1]).to_path_buf();
        }
    }

    let server_config = if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        toml::from_str::<ServerConfig>(&content).expect("Failed to parse config")
    } else {
        panic!("Config file not found: {:?}", config_path);
    };
    info!("Loaded server config");

    let mut registry = ToolRegistry::new();
    
    // Register builtin tools
    registry.register_builtin_file_tools();
    info!("Builtin file tools registered (list_dir, read_file, write_file)");

    if server_config.security.allow_direct_command {
        registry.register_builtin_direct_command();
        info!("Builtin direct_command tool registered");
    }

    let config_dir = config_path.parent().unwrap_or(Path::new(""));
    let tools_dir = if config_dir.as_os_str().is_empty() {
        Path::new("tools.d").to_path_buf()
    } else {
        config_dir.join("tools.d")
    };
    if tools_dir.exists() && tools_dir.is_dir() {
        load_tool_files(&tools_dir, &mut registry, server_config.defaults.timeout_secs)?;
    } else {
        info!("tools.d directory not found or is not a directory");
    }

    let handler = McpHandler::new(Arc::new(registry), Arc::new(server_config.clone()));

    if args.contains(&"--stdio".to_string()) {
        mcp_server::transport::stdio::run_stdio(Arc::new(handler)).await
    } else {
        run_sse_server(handler, &server_config).await
    }
}
