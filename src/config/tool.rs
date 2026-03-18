use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Deserialize)]
pub struct ToolFile {
    pub config: Option<ToolFileConfig>,
    #[serde(default)]
    pub tools: Vec<ToolDef>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ToolFileConfig {
    pub working_dir: Option<String>,
    pub timeout_secs: Option<u64>,
    pub env: Option<HashMap<String, String>>,
    pub base_url: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ToolDef {
    pub name: String,
    pub description: String,
    pub r#type: Option<ToolType>,
    
    // command 字段
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<HashMap<String, String>>,
    pub sub_dir: Option<String>,
    
    // http 字段
    pub method: Option<String>,
    pub path: Option<String>,
    pub body: Option<String>,
    pub content_type: Option<String>,
    
    // 通用
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub parameters: Option<Vec<ParameterDef>>,
}

#[derive(Debug, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolType {
    Command,
    Http,
}

#[derive(Debug, Deserialize, Clone)]
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
            
            let mut final_def = def.clone();
            if final_def.r#type.is_none() {
                final_def.r#type = Some(ToolType::Command);
            }

            let registered = RegisteredTool {
                def: final_def,
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
}
