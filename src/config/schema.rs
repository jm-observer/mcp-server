use schemars::schema_for;
use crate::config::ToolDef;
use super::tool::ToolFile;

pub fn tool_file_config_schema() -> String {
    let schema = schema_for!(ToolFile);
    serde_json::to_string_pretty(&schema).unwrap()
}

pub fn tool_config_schema() -> String {
    let schema = schema_for!(ToolDef);
    serde_json::to_string_pretty(&schema).unwrap()
}
