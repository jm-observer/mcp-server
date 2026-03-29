use serde::Deserialize;
use std::collections::HashMap;
use thiserror::Error;

/// prompts.d/*.toml 文件结构
#[derive(Debug, Deserialize)]
pub struct PromptFile {
    #[serde(default)]
    pub prompts: Vec<PromptDef>,
}

/// 单个 prompt 定义
#[derive(Debug, Deserialize, Clone)]
pub struct PromptDef {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub arguments: Option<Vec<PromptArgumentDef>>,
    /// prompt 消息模板列表，每条包含 role 和 content
    pub messages: Vec<PromptMessageDef>,
}

/// prompt 参数定义
#[derive(Debug, Deserialize, Clone)]
pub struct PromptArgumentDef {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// prompt 消息模板
#[derive(Debug, Deserialize, Clone)]
pub struct PromptMessageDef {
    pub role: String,
    pub content: String,
}

#[derive(Error, Debug)]
pub enum PromptError {
    #[error("duplicate prompt name: {0}")]
    DuplicateName(String),
    #[error("prompt not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Default)]
pub struct PromptRegistry {
    prompts: HashMap<String, PromptDef>,
}

impl PromptRegistry {
    pub fn new() -> Self {
        Self {
            prompts: HashMap::new(),
        }
    }

    pub fn register(&mut self, file: PromptFile) -> Result<(), PromptError> {
        for def in file.prompts {
            if self.prompts.contains_key(&def.name) {
                return Err(PromptError::DuplicateName(def.name.clone()));
            }
            self.prompts.insert(def.name.clone(), def);
        }
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&PromptDef> {
        self.prompts.get(name)
    }

    pub fn list(&self) -> impl Iterator<Item = &PromptDef> {
        self.prompts.values()
    }

    pub fn is_empty(&self) -> bool {
        self.prompts.is_empty()
    }
}
