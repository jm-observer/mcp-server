use crate::config::RegisteredTool;
use crate::security::{validate_sub_dir, validate_working_dir, SecurityError};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("Security error: {0}")]
    Security(#[from] SecurityError),
    #[error("Template resolution error: {0}")]
    TemplateResolution(String),
    #[error("Tool missing command executable")]
    MissingCommand,
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Command execution timeout")]
    Timeout,
}

pub struct CommandExecutor {
    allowed_dirs: Vec<PathBuf>,
}

pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl CommandExecutor {
    pub fn new(allowed_dirs: Vec<PathBuf>) -> Self {
        Self { allowed_dirs }
    }

    pub fn resolve_template(template: &str, args: &HashMap<String, Value>) -> Result<String, CommandError> {
        let mut result = String::new();
        let mut chars = template.chars().peekable();
        
        while let Some(c) = chars.next() {
            if c == '$' && chars.peek() == Some(&'{') {
                chars.next(); // consume '{'
                let mut var_name = String::new();
                for inner_c in chars.by_ref() {
                    if inner_c == '}' {
                        break;
                    }
                    var_name.push(inner_c);
                }
                
                if let Some(val) = args.get(&var_name) {
                    match val {
                        Value::String(s) => result.push_str(s),
                        Value::Number(n) => result.push_str(&n.to_string()),
                        Value::Bool(b) => result.push_str(&b.to_string()),
                        _ => return Err(CommandError::TemplateResolution(format!("Variable {} is not a simple value", var_name))),
                    }
                } else {
                    // inner empty string replacement as specified:
                    // 对于内嵌变量则使用空字符串处理替换。
                }
            } else {
                result.push(c);
            }
        }
        
        Ok(result)
    }

    pub fn resolve_args(
        templates: &[String],
        args: &HashMap<String, Value>,
    ) -> Result<Vec<String>, CommandError> {
        let mut resolved = Vec::new();
        
        for tpl in templates {
            if tpl.starts_with("${") && tpl.ends_with('}') && tpl.chars().filter(|&c| c == '{').count() == 1 {
                let var_name = &tpl[2..tpl.len() - 1];
                if !args.contains_key(var_name) {
                    // 如果在 args 数组中遇到了独立占据整个元素的 "${optional_var}" 而传入的参数里未填选项时，应当从该 args 数组里剔除这一整项
                    continue;
                }
            }
            let res = Self::resolve_template(tpl, args)?;
            resolved.push(res);
        }
        
        Ok(resolved)
    }

    pub async fn execute(
        &self,
        tool: &RegisteredTool,
        arguments: &HashMap<String, Value>,
    ) -> Result<CommandResult, CommandError> {
        let t_cmd = tool.def.command.as_ref().ok_or(CommandError::MissingCommand)?;
        
        let mut work_dir = tool.working_dir.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
        
        if let Some(sub_tpl) = &tool.def.sub_dir {
            let sub_res = Self::resolve_template(sub_tpl, arguments)?;
            work_dir = validate_sub_dir(&work_dir, &sub_res)?;
        }
        validate_working_dir(&work_dir, &self.allowed_dirs)?;

        let cmd_exec = Self::resolve_template(t_cmd, arguments)?;
        
        let mut resolved_args = Vec::new();
        if let Some(t_args) = &tool.def.args {
            resolved_args = Self::resolve_args(t_args, arguments)?;
        }

        let mut child_cmd = Command::new(cmd_exec);
        child_cmd.args(resolved_args)
                 .current_dir(work_dir)
                 .envs(&tool.env)
                 .stdout(Stdio::piped())
                 .stderr(Stdio::piped())
                 .stdin(Stdio::null());

        let mut child = child_cmd.spawn()?;

        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();

        let timeout_duration = Duration::from_secs(tool.effective_timeout);

        let process_future = async {
            let limit = 50 * 1024; // 50KB

            let mut out_buf = Vec::new();
            let mut buf = [0u8; 4096];
            let mut out_len = 0;

            let mut err_buf = Vec::new();
            let mut err_buf_arr = [0u8; 4096];
            let mut err_len = 0;

            let out_reader = async {
                loop {
                    match stdout.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            let remain = limit - out_len;
                            if remain == 0 {
                                continue;
                            }
                            let to_take = n.min(remain);
                            out_buf.extend_from_slice(&buf[..to_take]);
                            out_len += to_take;
                        }
                        Err(_) => break,
                    }
                }
                out_buf
            };

            let err_reader = async {
                loop {
                    match stderr.read(&mut err_buf_arr).await {
                        Ok(0) => break,
                        Ok(n) => {
                            let remain = limit - err_len;
                            if remain == 0 {
                                continue;
                            }
                            let to_take = n.min(remain);
                            err_buf.extend_from_slice(&err_buf_arr[..to_take]);
                            err_len += to_take;
                        }
                        Err(_) => break,
                    }
                }
                err_buf
            };

            let (status, out_bytes, err_bytes) = tokio::join!(
                child.wait(),
                out_reader,
                err_reader
            );

            let exit_code = status.map(|s| s.code().unwrap_or(1)).unwrap_or(1);

            CommandResult {
                stdout: String::from_utf8_lossy(&out_bytes).into_owned(),
                stderr: String::from_utf8_lossy(&err_bytes).into_owned(),
                exit_code,
            }
        };

        match timeout(timeout_duration, process_future).await {
            Ok(res) => Ok(res),
            Err(_) => {
                let _ = child.kill().await;
                Err(CommandError::Timeout)
            }
        }
    }
}
