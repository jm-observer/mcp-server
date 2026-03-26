use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    #[serde(default)]
    pub server: ServerSection,
    #[serde(default)]
    pub defaults: DefaultsSection,
    #[serde(default)]
    pub security: SecuritySection,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerSection {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

#[allow(clippy::derivable_impls)]
impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server: Default::default(),
            defaults: Default::default(),
            security: Default::default(),
        }
    }
}

impl Default for ServerSection {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    3000
}

#[derive(Debug, Deserialize, Clone)]
pub struct DefaultsSection {
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    #[serde(default)]
    pub directories: Vec<DirectoryConfig>,
}

impl Default for DefaultsSection {
    fn default() -> Self {
        Self {
            timeout_secs: default_timeout(),
            directories: vec![],
        }
    }
}

fn default_timeout() -> u64 {
    60
}

#[derive(Debug, Deserialize, Clone)]
pub struct SecuritySection {
    #[serde(default)]
    pub allow_direct_command: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DirectoryConfig {
    pub path: PathBuf,
    pub description: String,
}

#[allow(clippy::derivable_impls)]
impl Default for SecuritySection {
    fn default() -> Self {
        Self {
            allow_direct_command: false,
        }
    }
}
