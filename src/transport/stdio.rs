use crate::protocol::McpHandler;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;
use log::error;

pub async fn run_stdio(handler: Arc<McpHandler>) -> std::io::Result<()> {
    let stdin = tokio::io::stdin();
    let reader = BufReader::new(stdin);
    let mut lines = reader.lines();

    // 建立一个响应发送通道，专用于将并发处理结果串行安全写入 stdout 
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();

    // 独立的 Writer 任务，防止并发下的换行数据错乱
    tokio::spawn(async move {
        let mut stdout = tokio::io::stdout();
        while let Some(response) = rx.recv().await {
            if let Err(e) = stdout.write_all(format!("{}\n", response).as_bytes()).await {
                error!("Failed to write to stdout: {}", e);
                break;
            }
            if let Err(e) = stdout.flush().await {
                error!("Failed to flush stdout: {}", e);
                break;
            }
        }
    });

    // Reader 循环（并发派发机制）
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        
        let handler_clone = handler.clone();
        let tx_clone = tx.clone();
        
        tokio::spawn(async move {
            if let Some(response) = handler_clone.handle_request(&line).await {
                let _ = tx_clone.send(response);
            }
        });
    }

    Ok(())
}
