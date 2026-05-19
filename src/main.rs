use actix_web::{App, HttpResponse, HttpServer, Responder, web};
use bytes::Bytes;
use custom_utils::updater::{CliAction, DeployCommand, LinuxService};
use log::{LevelFilter::Debug, error, info};
use mcp::config::{PromptFile, PromptRegistry, ServerConfig, ToolFile, ToolRegistry};
use mcp::protocol::McpHandler;

use mcp::transport::sse::SessionManager;
use serde::Deserialize;
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::sync::Arc;

/// 宿主拥有顶层 CLI，把部署子命令作为透传变体嵌入；
/// `LinuxService::parse_deploy()` 未匹配即本项目的服务命令。
enum AppCmd {
    /// 本项目命令：运行 MCP 服务（SSE + HTTP，或 --stdio）。
    Serve,
    /// 透传给 `LinuxService::dispatch` 的部署子命令（库不读 argv/不碰 stdout）。
    Deploy(DeployCommand),
}

/// 本项目自有 usage；与库的 deploy 用法段拼接后由本项目打印。
fn project_usage() -> String {
    "Usage: mcp [OPTIONS]\n\
     \n\
     Run the MCP server: SSE + HTTP RPC, or stdio mode for Claude Desktop.\n\
     \n\
     Options:\n  \
     -w, --workspace <DIR>  Dir with config.toml and tools.d (alias: --cwd; default: ~/.config/mcp)\n      \
     --stdio            Run in stdio mode (for Claude Desktop / CLI integration)\n      \
     --schema           Print tool configuration JSON Schema and exit"
        .to_string()
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

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // 统一 Linux 部署栈：配置一次，派生 install / update / workspace / watchdog。
    let svc = LinuxService::new("mcp", "jm-observer", "mcp-server", env!("CARGO_PKG_VERSION"))
        .description("MCP - Model Context Protocol server")
        .extra_bins(["mcp-tool"]);

    // 透传式 CLI：库轻量解析 argv；未匹配则交还本项目自理。
    let cmd = match svc.parse_deploy() {
        Some(c) => AppCmd::Deploy(c),
        None => AppCmd::Serve,
    };

    if let AppCmd::Deploy(c) = cmd {
        // 库不碰 stdout：文本结果由本项目打印；--help 拼接本项目自有 usage。
        match svc.dispatch(c).await.map_err(std::io::Error::other)? {
            CliAction::DryRun(t) | CliAction::Version(t) => println!("{t}"),
            CliAction::Help(t) => println!("{}\n\n{}", project_usage(), t),
            CliAction::Handled => {}
            CliAction::Run { .. } => unreachable!(),
        }
        return Ok(());
    }

    let _ = custom_utils::logger::logger_feature("mcp", Debug, Debug, false).build();

    if std::env::args().any(|a| a == "--schema") {
        println!("{}", mcp::config::tool_config_schema());
        return Ok(());
    }

    // workspace 统一走 svc：默认 ~/.config/mcp（与 args::workspace 一致），
    // -w/--workspace/--cwd 覆盖（同时作用于 install 与运行时，与单元 WorkingDirectory 一致）。
    let cwd =
        custom_utils::args::arg_value("--workspace", "-w").or_else(|| custom_utils::args::arg_value("--cwd", "-w"));
    let workspace_path = match &cwd {
        Some(w) => svc.clone().workspace_arg(w).workspace(),
        None => svc.workspace(),
    }
    .map_err(|e| std::io::Error::new(std::io::ErrorKind::NotFound, e.to_string()))?;
    info!("Workspace directory: {}", workspace_path.display());
    // The config file is expected to be 'config.toml' inside this workspace
    let config_path = workspace_path.join("config.toml");
    let server_config = if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        toml::from_str::<ServerConfig>(&content).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse {}: {}", config_path.display(), e),
            )
        })?
    } else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Config file not found: {}", config_path.display()),
        ));
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

    // updater 下恒可用；prod + Linux 才真正发心跳，否则 no-op。
    let _wd = svc.spawn_watchdog();

    if std::env::args().any(|a| a == "--stdio") {
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
