use std::sync::atomic::{AtomicI64, Ordering};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    request_id: AtomicI64,
}

#[derive(Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<i64>,
    pub method: String,
    pub params: Value,
}

#[derive(Deserialize, Debug)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<i64>,
    pub result: Option<Value>,
    pub error: Option<Value>,
}

pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
}

impl McpClient {
    pub async fn connect(server_path: &str, config_path: &str) -> Result<Self> {
        // Because of rust compilation tests, might be using mcp-server.exe
        let mut child = Command::new(server_path)
            .arg("--stdio")
            .arg("--config")
            .arg(config_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| anyhow!("Failed to spawn mcp-server {}: {:?}", server_path, e))?;

        let stdin = child.stdin.take().ok_or_else(|| anyhow!("Failed to open stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("Failed to open stdout"))?;

        Ok(Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            request_id: AtomicI64::new(1),
        })
    }

    async fn send_request(&mut self, method: &str, params: Value) -> Result<JsonRpcResponse> {
        let id = self.request_id.fetch_add(1, Ordering::SeqCst);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: method.to_string(),
            params,
        };

        let req_str = serde_json::to_string(&req)? + "\n";
        self.stdin.write_all(req_str.as_bytes()).await?;
        self.stdin.flush().await?;

        let mut line = String::new();
        self.stdout.read_line(&mut line).await?;
        if line.trim().is_empty() {
            return Err(anyhow!("Server closed connection or empty response"));
        }

        let resp: JsonRpcResponse = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => return Err(anyhow!("Failed to parse response: {} - Original: {}", e, line)),
        };
        
        if let Some(err) = resp.error {
            return Err(anyhow!("JSON-RPC Error: {:?}", err));
        }

        Ok(resp)
    }

    pub async fn initialize(&mut self) -> Result<()> {
        let params = serde_json::json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {
                "name": "mcp-tool-generator",
                "version": "1.0.0"
            }
        });

        self.send_request("initialize", params).await?;

        // Send initialized notification
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: "initialized".to_string(),
            params: serde_json::json!({}),
        };
        let req_str = serde_json::to_string(&req)? + "\n";
        self.stdin.write_all(req_str.as_bytes()).await?;
        self.stdin.flush().await?;

        Ok(())
    }

    pub async fn execute_command(
        &mut self,
        command: &str,
        args: &[String],
        working_dir: Option<&str>,
    ) -> Result<CommandOutput> {
        let mut params_obj = serde_json::Map::new();
        params_obj.insert("command".to_string(), Value::String(command.to_string()));
        let args_val = args.iter().map(|s| Value::String(s.clone())).collect::<Vec<_>>();
        params_obj.insert("args".to_string(), Value::Array(args_val));
        
        if let Some(wd) = working_dir {
            params_obj.insert("working_dir".to_string(), Value::String(wd.to_string()));
        }

        let run_params = serde_json::json!({
            "name": "direct_command",
            "arguments": params_obj
        });

        let resp = self.send_request("tools/call", run_params).await?;
        if let Some(result) = resp.result {
             Ok(CommandOutput {
                 stdout: serde_json::to_string(&result)?,
                 stderr: "".to_string(),
             })
        } else {
             Err(anyhow!("No result in tools/call response"))
        }

    }

    pub fn get_tool_schema(&self) -> String {
        mcp_server::config::tool_config_schema()
    }

    pub async fn close(&mut self) -> Result<()> {
        self.child.kill().await?;
        Ok(())
    }
}
