use crate::config::{RegisteredTool, ToolAction};
use log::info;
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;

use std::process::Stdio;
use std::time::Duration;
use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

#[derive(Error, Debug)]
pub enum CommandError {
    #[error("Template resolution error: {0}")]
    TemplateResolution(String),
    #[error("Tool missing command executable")]
    MissingCommand,
    #[error("Tool missing arg: {0}")]
    MissingArg(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Command execution timeout")]
    Timeout,
}

pub struct CommandExecutor;

#[derive(Serialize)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

impl CommandExecutor {
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
                        _ => {
                            return Err(CommandError::TemplateResolution(format!(
                                "Variable {} is not a simple value",
                                var_name
                            )));
                        }
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

    pub fn resolve_args(templates: &[String], args: &HashMap<String, Value>) -> Result<Vec<String>, CommandError> {
        let mut resolved = Vec::new();

        for tpl in templates {
            let res = Self::resolve_template(tpl, args)?;
            resolved.push(res);
        }

        Ok(resolved)
    }

    fn resolve_parameter_args(
        tool: &RegisteredTool,
        arguments: &HashMap<String, Value>,
    ) -> Result<Vec<String>, CommandError> {
        let mut resolved_args = Vec::new();

        // Add subcommands first
        if let Some(t_args) = match &tool.def.action {
            ToolAction::Command { subcommands: args, .. } => args.clone(),
            _ => None,
        } {
            resolved_args.extend(t_args);
        }
        if let Some(params) = &tool.def.parameters {
            for param in params {
                if param.arg.is_none() && param.required {
                    continue;
                }
                let Some(value) = arguments.get(&param.name) else {
                    if param.required {
                        return Err(CommandError::MissingArg(param.name.clone()));
                    }
                    continue;
                };
                // Boolean flag handling
                if param.r#type == "boolean" {
                    if let Value::Bool(true) = value
                        && let Some(arg) = param.arg.as_ref().and_then(|x| x.first()).cloned()
                    {
                        resolved_args.push(arg);
                    }
                    continue;
                }
                // For non‑boolean, push the first flag then the value
                if let Some(first) = param.arg.as_ref().and_then(|v| v.first()) {
                    resolved_args.push(first.clone());
                }
                match value {
                    Value::String(s) => resolved_args.push(s.clone()),
                    Value::Number(n) => resolved_args.push(n.to_string()),
                    Value::Bool(b) => resolved_args.push(b.to_string()),
                    _ => {
                        return Err(CommandError::TemplateResolution(format!(
                            "Parameter '{}' has unsupported value type",
                            param.name
                        )));
                    }
                }
            }

            // Finally handle required positional parameters (no arg, required)
            for param in params {
                if param.arg.is_none() && param.required {
                    let Some(value) = arguments.get(&param.name) else {
                        return Err(CommandError::MissingArg(param.name.clone()));
                    };
                    match value {
                        Value::String(s) => resolved_args.push(s.clone()),
                        Value::Number(n) => resolved_args.push(n.to_string()),
                        Value::Bool(b) => resolved_args.push(b.to_string()),
                        _ => {
                            return Err(CommandError::TemplateResolution(format!(
                                "Parameter '{}' has unsupported value type",
                                param.name
                            )));
                        }
                    }
                }
            }
        }

        Ok(resolved_args)
    }

    pub async fn execute(
        &self,
        tool: &RegisteredTool,
        arguments: &HashMap<String, Value>,
    ) -> Result<CommandResult, CommandError> {
        let cmd_opt = match &tool.def.action {
            ToolAction::Command { command, .. } => command,
            _ => return Err(CommandError::MissingCommand),
        };

        let cmd_exec = cmd_opt.clone().ok_or(CommandError::MissingCommand)?;

        let resolved_args = Self::resolve_parameter_args(tool, arguments)?;

        info!("{cmd_exec} {resolved_args:?}");

        let mut child_cmd = Command::new(cmd_exec);
        // Inherit the current process environment variables, then apply tool-specific ones
        for (key, value) in std::env::vars() {
            child_cmd.env(key, value);
        }
        child_cmd
            .args(resolved_args)
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

            let (status, out_bytes, err_bytes) = tokio::join!(child.wait(), out_reader, err_reader);

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

    pub async fn execute_direct(&self, command: &str, args: &[String]) -> Result<CommandResult, CommandError> {
        let mut child_cmd = Command::new(command);
        child_cmd
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());

        let mut child = child_cmd.spawn()?;

        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();

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

            let (status, out_bytes, err_bytes) = tokio::join!(child.wait(), out_reader, err_reader);

            let exit_code = status.map(|s| s.code().unwrap_or(1)).unwrap_or(1);

            CommandResult {
                stdout: String::from_utf8_lossy(&out_bytes).into_owned(),
                stderr: String::from_utf8_lossy(&err_bytes).into_owned(),
                exit_code,
            }
        };

        let timeout_duration = Duration::from_secs(60); // Use large timeout
        match timeout(timeout_duration, process_future).await {
            Ok(res) => Ok(res),
            Err(_) => {
                let _ = child.kill().await;
                Err(CommandError::Timeout)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::CommandExecutor;
    use crate::config::{ParameterDef, RegisteredTool, ToolAction, ToolDef};
    use serde_json::{Value, json};
    use std::collections::HashMap;

    fn sample_tool(parameters: Vec<ParameterDef>) -> RegisteredTool {
        RegisteredTool {
            def: ToolDef {
                name: "cargo_build".to_string(),
                description: "Build cargo project".to_string(),
                action: ToolAction::Command {
                    command: Some("cargo".to_string()),
                    subcommands: Some(vec!["build".to_string()]),
                },
                env: None,
                timeout_secs: None,
                cwd: false,
                parameters: Some(parameters),
            },
            base_url: None,
            effective_timeout: 60,
            env: HashMap::new(),
        }
    }

    fn string_param(name: &str, flags: &[&str]) -> ParameterDef {
        ParameterDef {
            name: name.to_string(),
            description: name.to_string(),
            r#type: "string".to_string(),
            required: false,
            arg: Some(flags.iter().map(|item| (*item).to_string()).collect()),
        }
    }

    fn boolean_param(name: &str, arg: &[&str]) -> ParameterDef {
        ParameterDef {
            name: name.to_string(),
            description: name.to_string(),
            r#type: "boolean".to_string(),
            required: false,
            arg: Some(arg.iter().map(|item| (*item).to_string()).collect()),
        }
    }

    fn metadata_param(name: &str) -> ParameterDef {
        ParameterDef {
            name: name.to_string(),
            description: name.to_string(),
            r#type: "string".to_string(),
            required: false,
            arg: None,
        }
    }

    fn args_map(entries: &[(&str, Value)]) -> HashMap<String, Value> {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_string(), value.clone()))
            .collect()
    }

    #[test]
    fn appends_dynamic_args_in_parameter_declaration_order() {
        let tool = sample_tool(vec![string_param("package", &["-p"]), string_param("bin", &["--bin"])]);
        let arguments = args_map(&[("bin", json!("demo-bin")), ("package", json!("demo-pkg"))]);

        let resolved = CommandExecutor::resolve_parameter_args(&tool, &arguments).unwrap();

        assert_eq!(resolved, vec!["build", "-p", "demo-pkg", "--bin", "demo-bin"]);
    }

    #[test]
    fn skips_optional_flag_value_pair_when_string_arg_missing() {
        let tool = sample_tool(vec![string_param("bin", &["--bin"])]);
        let arguments = args_map(&[]);

        let resolved = CommandExecutor::resolve_parameter_args(&tool, &arguments).unwrap();

        assert_eq!(resolved, vec!["build"]);
    }

    #[test]
    fn only_appends_boolean_args_when_true() {
        let tool = sample_tool(vec![boolean_param("release", &["--release"])]);

        let enabled = CommandExecutor::resolve_parameter_args(&tool, &args_map(&[("release", json!(true))])).unwrap();
        let disabled = CommandExecutor::resolve_parameter_args(&tool, &args_map(&[("release", json!(false))])).unwrap();

        assert_eq!(enabled, vec!["build", "--release"]);
        assert_eq!(disabled, vec!["build"]);
    }

    #[test]
    fn metadata_parameters_do_not_affect_cli_args() {
        let tool = sample_tool(vec![metadata_param("project"), string_param("package", &["-p"])]);
        let arguments = args_map(&[("project", json!("mcp-server")), ("package", json!("demo-pkg"))]);

        let resolved = CommandExecutor::resolve_parameter_args(&tool, &arguments).unwrap();

        assert_eq!(resolved, vec!["build", "mcp-server", "-p", "demo-pkg"]);
    }

    #[test]
    fn rejects_complex_values_used_in_arg() {
        let tool = sample_tool(vec![string_param("items", &["--items"])]);
        let arguments = args_map(&[("items", json!(["a", "b"]))]);
        let err = CommandExecutor::resolve_parameter_args(&tool, &arguments).unwrap_err();
        assert!(matches!(
            err,
            crate::executor::command::CommandError::TemplateResolution(_)
        ));
    }

    #[test]
    fn resolves_params_from_mcp_tool_toml() {
        // Load the TOML definition from the repository resources
        let toml_str = include_str!("../../res/mcp-tool.toml");
        let tool_file: crate::config::ToolFile = toml::from_str(toml_str).expect("parse TOML");
        let mut registry = crate::config::ToolRegistry::new();
        registry.register(tool_file, 60).expect("register tool");
        let tool = registry.get("mcp-tool").expect("tool exists");

        // Build arguments map from a JSON object
        let args_json = json!({
            "command_name": "git log",
            "workspace": "/tmp",
            "vllm_url": "http://example.com",
            "model": "gpt-4",
            "help": true,
            "version": false
        });
        let mut args = std::collections::HashMap::new();
        if let serde_json::Value::Object(map) = args_json {
            for (k, v) in map {
                args.insert(k, v);
            }
        }

        let resolved = CommandExecutor::resolve_parameter_args(tool, &args).unwrap();
        assert_eq!(
            resolved,
            vec!["-w", "/tmp", "-u", "http://example.com", "-m", "gpt-4", "-h", "git log"]
        );
    }
}
