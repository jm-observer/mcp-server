//! MCP client that communicates with the server via Server‑Sent Events (SSE).
//!
//! It creates a persistent SSE connection, forwards inbound messages to a channel
//! and provides an outbound channel for the user to send JSON‑RPC requests.
//! The client also sends a periodic `ping` request to keep the connection alive.

use anyhow::{Context, Result};
use bytes::Bytes;
use futures::{StreamExt, stream::BoxStream};
use log::{error, info};
use reqwest::Client as HttpClient;
use std::time::Duration;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::JoinHandle;
use tokio::time::Interval;

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const RECONNECT_DELAY: Duration = Duration::from_secs(3);
const PING_REQUEST: &str = r#"{"jsonrpc":"2.0","id":"ping","method":"ping","params":null}"#;

type SseByteStream = BoxStream<'static, std::result::Result<Bytes, reqwest::Error>>;

#[derive(Debug, Default)]
struct SseEventBuilder {
    event: Option<String>,
    data_lines: Vec<String>,
}

#[derive(Debug)]
struct SseEvent {
    event: Option<String>,
    data: String,
}

impl SseEventBuilder {
    fn push_line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            if self.event.is_none() && self.data_lines.is_empty() {
                return None;
            }

            let event = SseEvent {
                event: self.event.take(),
                data: self.data_lines.join("\n"),
            };
            self.data_lines.clear();
            return Some(event);
        }

        if let Some(event) = line.strip_prefix("event:") {
            self.event = Some(event.trim_start().to_string());
            return None;
        }

        if let Some(data) = line.strip_prefix("data:") {
            self.data_lines.push(data.trim_start().to_string());
        }

        None
    }
}

fn decode_chunk(
    chunk: &[u8],
    buffer: &mut String,
    builder: &mut SseEventBuilder,
) -> std::result::Result<Vec<SseEvent>, std::str::Utf8Error> {
    let chunk_str = std::str::from_utf8(chunk)?;
    buffer.push_str(chunk_str);

    let mut events = Vec::new();
    while let Some(nl) = buffer.find('\n') {
        let line = buffer[..nl].trim_end_matches('\r').to_string();
        buffer.drain(..=nl);

        if let Some(event) = builder.push_line(&line) {
            events.push(event);
        }
    }

    Ok(events)
}

async fn post_json(http: &HttpClient, url: &str, body: String, request_name: &str) {
    match http
        .post(url)
        .header("Content-Type", "application/json")
        .body(body)
        .send()
        .await
    {
        Ok(response) if !response.status().is_success() => {
            error!("{request_name} returned error status {}", response.status());
        }
        Ok(_) => {}
        Err(error) => {
            error!("{request_name} failed: {error}");
        }
    }
}

struct SseConnection {
    endpoint: String,
    stream: SseByteStream,
    buffer: String,
    builder: SseEventBuilder,
}

enum TaskControl {
    Reconnect,
    Stop,
}

#[derive(Debug)]
pub struct McpSseClient {
    outbound_tx: UnboundedSender<String>,
    inbound_rx: UnboundedReceiver<String>,
    task_handle: JoinHandle<()>,
}

impl McpSseClient {
    /// Construct a new client.
    pub async fn new(base_url: impl Into<String>) -> Result<Self> {
        let base = base_url.into().trim_end_matches('/').to_string();
        let http = HttpClient::builder()
            .user_agent("mcp-client/0.1")
            .use_rustls_tls()
            .build()
            .context("failed to build reqwest client")?;

        // Channels: outbound (user -> server) and inbound (server -> user)
        let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<String>();
        let (inbound_tx, inbound_rx) = mpsc::unbounded_channel::<String>();

        let task_handle = tokio::spawn(async move {
            Self::run_task(base, http, outbound_rx, inbound_tx).await;
        });

        Ok(Self {
            outbound_tx,
            inbound_rx,
            task_handle,
        })
    }

    async fn run_task(
        base: String,
        http: HttpClient,
        mut outbound_rx: UnboundedReceiver<String>,
        inbound_tx: UnboundedSender<String>,
    ) {
        let mut hb_interval = tokio::time::interval(HEARTBEAT_INTERVAL);
        hb_interval.tick().await;

        loop {
            let mut connection = Self::connect_with_retry(&http, &base).await;
            match Self::run_connected_loop(
                &http,
                &base,
                &mut connection,
                &mut outbound_rx,
                &inbound_tx,
                &mut hb_interval,
            )
            .await
            {
                TaskControl::Reconnect => tokio::time::sleep(RECONNECT_DELAY).await,
                TaskControl::Stop => return,
            }
        }
    }

    async fn connect_with_retry(http: &HttpClient, base: &str) -> SseConnection {
        loop {
            if let Some(connection) = Self::try_connect(http, base).await {
                return connection;
            }

            tokio::time::sleep(RECONNECT_DELAY).await;
        }
    }

    async fn try_connect(http: &HttpClient, base: &str) -> Option<SseConnection> {
        let sse_url = format!("{}/sse", base);
        info!("Connecting to SSE endpoint: {}", sse_url);

        let response = match http.get(&sse_url).header("Accept", "text/event-stream").send().await {
            Ok(response) if response.status().is_success() => response,
            Ok(response) => {
                error!("SSE endpoint returned non-200 status: {}", response.status());
                return None;
            }
            Err(error) => {
                error!("Failed to connect to SSE endpoint: {error}");
                return None;
            }
        };

        let connection = SseConnection {
            endpoint: String::new(),
            stream: response.bytes_stream().boxed(),
            buffer: String::new(),
            builder: SseEventBuilder::default(),
        };

        Self::wait_for_endpoint(connection).await
    }

    async fn wait_for_endpoint(mut connection: SseConnection) -> Option<SseConnection> {
        loop {
            match connection.stream.next().await {
                Some(Ok(chunk)) => {
                    let events = Self::decode_events(chunk, &mut connection.buffer, &mut connection.builder)?;
                    for event in events {
                        if event.event.as_deref() == Some("endpoint") {
                            info!("Received endpoint: {}", event.data);
                            connection.endpoint = event.data;
                            return Some(connection);
                        }
                    }
                }
                Some(Err(error)) => {
                    error!("Error reading SSE stream while waiting for endpoint: {error}");
                    return None;
                }
                None => {
                    error!("SSE stream ended before endpoint handshake completed");
                    return None;
                }
            }
        }
    }

    async fn run_connected_loop(
        http: &HttpClient,
        base: &str,
        connection: &mut SseConnection,
        outbound_rx: &mut UnboundedReceiver<String>,
        inbound_tx: &UnboundedSender<String>,
        hb_interval: &mut Interval,
    ) -> TaskControl {
        loop {
            tokio::select! {
                maybe_item = connection.stream.next() => {
                    if let Some(control) = Self::handle_sse_item(maybe_item, connection, inbound_tx) {
                        return control;
                    }
                }
                maybe_msg = outbound_rx.recv() => {
                    match maybe_msg {
                        Some(msg) => {
                            Self::send_request(http, base, &connection.endpoint, msg, "POST to endpoint").await;
                        }
                        None => {
                            info!("Outbound channel closed, stopping SSE client task");
                            return TaskControl::Stop;
                        }
                    }
                }
                _ = hb_interval.tick() => {
                    Self::send_request(
                        http,
                        base,
                        &connection.endpoint,
                        PING_REQUEST.to_string(),
                        "Heartbeat POST",
                    ).await;
                }
            }
        }
    }

    fn handle_sse_item(
        maybe_item: Option<std::result::Result<Bytes, reqwest::Error>>,
        connection: &mut SseConnection,
        inbound_tx: &UnboundedSender<String>,
    ) -> Option<TaskControl> {
        match maybe_item {
            Some(Ok(chunk)) => {
                let Some(events) = Self::decode_events(chunk, &mut connection.buffer, &mut connection.builder) else {
                    return Some(TaskControl::Reconnect);
                };

                for event in events {
                    if event.event.as_deref() != Some("message") {
                        continue;
                    }

                    if let Err(error) = inbound_tx.send(event.data) {
                        error!("Failed to forward SSE data: {error}");
                        return Some(TaskControl::Stop);
                    }
                }

                None
            }
            Some(Err(error)) => {
                error!("Error reading SSE stream: {error}");
                Some(TaskControl::Reconnect)
            }
            None => {
                info!("SSE stream ended, reconnecting");
                Some(TaskControl::Reconnect)
            }
        }
    }

    fn decode_events(chunk: Bytes, buffer: &mut String, builder: &mut SseEventBuilder) -> Option<Vec<SseEvent>> {
        match decode_chunk(&chunk, buffer, builder) {
            Ok(events) => Some(events),
            Err(error) => {
                error!("Invalid UTF-8 chunk from SSE: {error}");
                None
            }
        }
    }

    async fn send_request(http: &HttpClient, base: &str, endpoint: &str, body: String, request_name: &str) {
        let url = format!("{}{}", base, endpoint);
        post_json(http, &url, body, request_name).await;
    }

    /// Return a clone of the outbound sender, allowing the caller to send JSON‑RPC
    /// requests to the server.
    pub fn outbound_sender(&self) -> UnboundedSender<String> {
        self.outbound_tx.clone()
    }

    /// Get a mutable reference to the inbound receiver. The caller can `await` on
    /// `recv()` to obtain messages pushed by the SSE reader.
    pub fn inbound_receiver(&mut self) -> &mut UnboundedReceiver<String> {
        &mut self.inbound_rx
    }
}

impl Drop for McpSseClient {
    fn drop(&mut self) {
        self.task_handle.abort();
    }
}
