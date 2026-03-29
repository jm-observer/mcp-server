use crate::protocol::McpHandler;
use actix_web::{App, HttpResponse, HttpServer, web};
use log::{error, info};
use serde_json::{Value, json};
use std::sync::Arc;

/// Simple HTTP transport: receives JSON-RPC requests via POST /rpc
/// Returns the handler's response directly as JSON.
pub async fn run_http(handler: McpHandler, config: &crate::config::ServerConfig) -> std::io::Result<()> {
    let handler = Arc::new(handler);
    let bind_addr = (config.server.host.as_str(), config.server.http_port);
    info!("Starting HTTP transport server on {}:{}", bind_addr.0, bind_addr.1);

    HttpServer::new(move || {
        let handler_clone = handler.clone();
        App::new()
            .app_data(web::Data::new(handler_clone))
            .route("/rpc", web::post().to(handle_rpc))
    })
    .bind(bind_addr)?
    .run()
    .await
}

async fn handle_rpc(body: web::Json<Value>, state: web::Data<Arc<McpHandler>>) -> impl actix_web::Responder {
    info!("Receive JSON: {:?}", body);
    let request = body.into_inner();
    let request_str = match serde_json::to_string(&request) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to serialize request: {}", e);
            return HttpResponse::BadRequest().json(json!({"error": "Invalid JSON"}));
        }
    };
    match state.handle_request(&request_str).await {
        Some(response) => HttpResponse::Ok().content_type("application/json").body(response),
        None => HttpResponse::Accepted().json(json!({"ok": true})),
    }
}
