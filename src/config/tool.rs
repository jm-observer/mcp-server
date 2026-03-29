use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ToolFile {
    pub config: Option<ToolFileConfig>,
    #[serde(default)]
    pub tools: Vec<ToolDef>,
}

#[derive(Debug, Serialize, Deserialize, Clone, JsonSchema, Default)]
pub struct ToolFileConfig {
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
    /// 是否从参数 "cwd" 中获取工作目录。
    /// 设为 true 时，tool 调用必须传入名为 "cwd" 的参数作为工作目录（绝对路径）。
    /// 替代原有的 working_dir + sub_dir 两层设计。
    #[serde(default)]
    pub cwd: bool,
    #[serde(default)]
    pub parameters: Option<Vec<ParameterDef>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, JsonSchema)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ToolAction {
    Command {
        command: Option<String>,
        subcommands: Option<Vec<String>>,
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
            subcommands: None,
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
    #[serde(default)]
    pub arg: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct RegisteredTool {
    pub def: ToolDef,
    pub base_url: Option<String>,
    pub effective_timeout: u64,
    pub env: HashMap<String, String>,
}

#[derive(Debug, Default)]
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
        Self { tools: HashMap::new() }
    }

    pub fn register(&mut self, file: ToolFile, global_timeout: u64) -> Result<(), ToolError> {
        let file_config = file.config.unwrap_or_default();

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
            let base_url = file_config.base_url.clone();

            let registered = RegisteredTool {
                def: def.clone(),
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

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
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
                subcommands: None,
            },
            env: None,
            timeout_secs: None,
            cwd: false,
            parameters: Some(vec![
                ParameterDef {
                    name: "command".to_string(),
                    description: "要执行的命令".to_string(),
                    r#type: "string".to_string(),
                    required: true,
                    arg: None,
                },
                ParameterDef {
                    name: "args".to_string(),
                    description: "命令参数".to_string(),
                    r#type: "array".to_string(),
                    required: false,
                    arg: None,
                },
                ParameterDef {
                    name: "working_dir".to_string(),
                    description: "工作目录".to_string(),
                    r#type: "string".to_string(),
                    required: false,
                    arg: None,
                },
            ]),
        };

        let registered = RegisteredTool {
            def,
            base_url: None,
            effective_timeout: 60,
            env: HashMap::new(),
        };

        self.tools.insert(registered.def.name.clone(), registered);
    }

    /// 注册内置文件操作 tool：list_dir、read_file、write_file
    pub fn register_builtin_file_tools(&mut self) {
        // list_dir
        let list_dir = RegisteredTool {
            def: ToolDef {
                name: "list_dir".to_string(),
                description: "List files and subdirectories in the specified directory. Path must be an absolute path."
                    .to_string(),
                action: ToolAction::Command {
                    command: None,
                    subcommands: None,
                },
                env: None,
                timeout_secs: None,
                cwd: false,
                parameters: Some(vec![ParameterDef {
                    name: "path".to_string(),
                    description: "Absolute path of the target directory".to_string(),
                    r#type: "string".to_string(),
                    required: true,
                    arg: None,
                }]),
            },
            base_url: None,
            effective_timeout: 60,
            env: HashMap::new(),
        };
        self.tools.insert("list_dir".to_string(), list_dir);

        // read_file
        let read_file = RegisteredTool {
            def: ToolDef {
                name: "read_file".to_string(),
                description: "Read the content of a file. Path must be an absolute path.".to_string(),
                action: ToolAction::Command {
                    command: None,
                    subcommands: None,
                },
                env: None,
                timeout_secs: None,
                cwd: false,
                parameters: Some(vec![ParameterDef {
                    name: "path".to_string(),
                    description: "Absolute path of the file to read".to_string(),
                    r#type: "string".to_string(),
                    required: true,
                    arg: None,
                }]),
            },
            base_url: None,
            effective_timeout: 60,
            env: HashMap::new(),
        };
        self.tools.insert("read_file".to_string(), read_file);

        // write_file
        let write_file = RegisteredTool {
            def: ToolDef {
                name: "write_file".to_string(),
                description: "Write content to a file (overwrites if exists, creates if not). Path must be an absolute path. Parent directory must exist.".to_string(),
                action: ToolAction::Command { command: None, subcommands: None, },
                env: None,
                timeout_secs: None,
                cwd: false,
                parameters: Some(vec![
                    ParameterDef {
                        name: "path".to_string(),
                        description: "Absolute path of the file to write".to_string(),
                        r#type: "string".to_string(),
                        required: true,
                        arg: None,
                    },
                    ParameterDef {
                        name: "content".to_string(),
                        description: "Content to write to the file".to_string(),
                        r#type: "string".to_string(),
                        required: true,
                        arg: None,
                    },
                ]),
            },
            base_url: None,
            effective_timeout: 60,
            env: HashMap::new(),
        };
        self.tools.insert("write_file".to_string(), write_file);

        // list_allowed_dirs
        let list_allowed_dirs = RegisteredTool {
            def: ToolDef {
                name: "list_allowed_dirs".to_string(),
                description: "List all configured directories and their usage descriptions. This helps the client discover which directories are available for file operations.".to_string(),
                action: ToolAction::Command { command: None, subcommands: None, },
                env: None,
                timeout_secs: None,
                cwd: false,
                parameters: None,
            },
            base_url: None,
            effective_timeout: 60,
            env: HashMap::new(),
        };
        self.tools.insert("list_allowed_dirs".to_string(), list_allowed_dirs);
    }
}
