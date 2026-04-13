use crate::protocol::McpHandler;
use log::{error, info};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

#[allow(clippy::collapsible_if)]
pub async fn run_stdio(handler: Arc<McpHandler>) -> std::io::Result<()> {
    // Initialize stdin and a buffered reader
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    info!("Starting stdio server...");

    // Channel for sending responses back to the writer task
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // Writer task – ensures responses are written sequentially with proper LSP framing.
    tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(response_body) = rx.recv().await {
            // LSP expects a "Content-Length" header followed by two CRLFs and the JSON body.
            let bytes = response_body.as_bytes();
            let header = format!("Content-Length: {}\r\n\r\n", bytes.len());
            if let Err(e) = stdout.write_all(header.as_bytes()).await {
                error!("Failed to write header to stdout: {}", e);
                break;
            }
            if let Err(e) = stdout.write_all(bytes).await {
                error!("Failed to write response body to stdout: {}", e);
                break;
            }
            if let Err(e) = stdout.flush().await {
                error!("Failed to flush stdout: {}", e);
                break;
            }
        }
    });

    // Main read loop – parse incoming messages according to the LSP over stdio protocol.
    loop {
        // Parse headers to find the Content-Length. If not present, treat the next line as a raw JSON request.
        let mut content_length: Option<usize> = None;
        // Read the first line (could be a header or a raw JSON request).
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            // EOF reached – exit.
            break;
        }
        // Trim CR and LF characters.
        let trimmed = line.trim_end_matches(&['\r', '\n'][..]);
        if trimmed.is_empty() {
            // Skip empty lines.
            continue;
        }

        // If the line looks like a JSON object, handle it directly.
        if trimmed.starts_with('{') {
            let request_str = trimmed.to_string();
            let handler_clone = handler.clone();
            let tx_clone = tx.clone();
            tokio::spawn(async move {
                if let Some(response) = handler_clone.handle_request(&request_str).await {
                    let _ = tx_clone.send(response);
                }
            });
            continue;
        }

        // Otherwise treat the line as the first header line.
        // Helper to parse Content-Length from a header line.
        let parse_content_length = |line: &str| -> Option<usize> {
            let colon = line.find(':')?;
            let (key, value) = line.split_at(colon);
            if !key.eq_ignore_ascii_case("Content-Length") {
                return None;
            }
            value[1..].trim().parse::<usize>().ok()
        };
        // Try to extract Content-Length from the first header line (if any).
        if let Some(cl) = parse_content_length(trimmed) {
            content_length = Some(cl);
        }

        // Read subsequent header lines until an empty line indicates the end of headers.
        loop {
            let mut header = String::new();
            let n = reader.read_line(&mut header).await?;
            if n == 0 {
                // EOF while reading headers – exit.
                break;
            }
            let header_trim = header.trim_end_matches(&['\r', '\n'][..]);
            if header_trim.is_empty() {
                // End of headers.
                break;
            }
            if content_length.is_none() {
                if let Some(cl) = parse_content_length(header_trim) {
                    content_length = Some(cl);
                }
            }
        }

        // If we didn't find a Content-Length, skip this message.
        let len = match content_length {
            Some(l) => l,
            None => continue,
        };

        // Read the exact number of bytes for the body.
        let mut body = vec![0u8; len];
        if let Err(e) = reader.read_exact(&mut body).await {
            error!("Failed to read request body: {}", e);
            break;
        }
        let request_str = match String::from_utf8(body) {
            Ok(s) => s,
            Err(e) => {
                error!("Invalid UTF-8 in request body: {}", e);
                continue;
            }
        };

        let handler_clone = handler.clone();
        let tx_clone = tx.clone();
        tokio::spawn(async move {
            if let Some(response) = handler_clone.handle_request(&request_str).await {
                let _ = tx_clone.send(response);
            }
        });
    }

    Ok(())
}
