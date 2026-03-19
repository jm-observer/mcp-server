use serde::{Deserialize, Serialize};
use schemars::JsonSchema;
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ToolFile {
    pub config: Option<ToolFileConfig>,
    #[serde(default)]
    pub tools: Vec<ToolDef>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct ToolFileConfig {
    pub working_dir: Option<String>,
    pub timeout_secs: Option<u64>,
    pub env: Option<HashMap<String, String>>,
    pub base_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    #[serde(flatten, default)]
    pub action: ToolAction,
    pub env: Option<HashMap<String, String>>,
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub parameters: Option<Vec<ParameterDef>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolAction {
    Command {
        command: Option<String>,
        args: Option<Vec<String>>,
        sub_dir: Option<String>,
    },
    Http {
        method: Option<String>,
        path: Option<String>,
        body: Option<String>,
        content_type: Option<String>,
    },
}

impl Default for ToolAction {
    fn default() -> Self {
        ToolAction::Command {
            command: None,
            args: None,
            sub_dir: None,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct ParameterDef {
    pub name: String,
    pub description: String,
    pub r#type: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct RegisteredTool {
    pub def: ToolDef,
    pub working_dir: Option<PathBuf>,
    pub base_url: Option<String>,
    pub effective_timeout: u64,
    pub env: HashMap<String, String>,
}

pub struct ToolRegistry {
    tools: HashMap<String, RegisteredTool>,
}

#[derive(Error, Debug)]
pub enum ToolError {
    #[error("duplicate tool name: {0}")]
    DuplicateName(String),
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, file: ToolFile, global_timeout: u64) -> Result<(), ToolError> {
        let file_config = file.config.unwrap_or_else(|| ToolFileConfig {
            working_dir: None,
            timeout_secs: None,
            env: None,
            base_url: None,
        });

        for def in file.tools {
            if self.tools.contains_key(&def.name) {
                return Err(ToolError::DuplicateName(def.name.clone()));
            }

            // timeout 合并逻辑：取 tool 级别、file config 级别和 global 级别中较小的
            let mut timeout = global_timeout;
            if let Some(cfg_timeout) = file_config.timeout_secs {
                timeout = timeout.min(cfg_timeout);
            }
            if let Some(tool_timeout) = def.timeout_secs {
                timeout = timeout.min(tool_timeout);
            }

            // env 合并逻辑：tool 级别覆盖 config 级别
            let mut env = HashMap::new();
            if let Some(cfg_env) = &file_config.env {
                for (k, v) in cfg_env {
                    env.insert(k.clone(), v.clone());
                }
            }
            if let Some(tool_env) = &def.env {
                for (k, v) in tool_env {
                    env.insert(k.clone(), v.clone());
                }
            }
            
            // working_dir/base_url 从 config 处继承
            let working_dir = file_config.working_dir.as_ref().map(PathBuf::from);
            let base_url = file_config.base_url.clone();
            
            let registered = RegisteredTool {
                def: def.clone(),
                working_dir,
                base_url,
                effective_timeout: timeout,
                env,
            };

            self.tools.insert(registered.def.name.clone(), registered);
        }

        Ok(())
    }
    
    pub fn get(&self, name: &str) -> Option<&RegisteredTool> {
        self.tools.get(name)
    }
    
    pub fn list_tools(&self) -> impl Iterator<Item = &RegisteredTool> {
        self.tools.values()
    }
    
    pub fn len(&self) -> usize {
        self.tools.len()
    }
    
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }

    pub fn register_builtin_direct_command(&mut self) {
        let def = ToolDef {
            name: "direct_command".to_string(),
            description: "Execute an arbitrary shell command".to_string(),
            action: ToolAction::Command {
                command: None,
                args: None,
                sub_dir: None,
            },
            env: None,
            timeout_secs: None,
            parameters: Some(vec![
                ParameterDef {
                    name: "command".to_string(),
                    description: "要执行的命令".to_string(),
                    r#type: "string".to_string(),
                    required: true,
                },
                ParameterDef {
                    name: "args".to_string(),
                    description: "命令参数".to_string(),
                    r#type: "array".to_string(),
                    required: false,
                },
                ParameterDef {
                    name: "working_dir".to_string(),
                    description: "工作目录".to_string(),
                    r#type: "string".to_string(),
                    required: false,
                },
            ]),
        };

        let registered = RegisteredTool {
            def,
            working_dir: None,
            base_url: None,
            effective_timeout: 60,
            env: HashMap::new(),
        };

        self.tools.insert(registered.def.name.clone(), registered);
    }
}
