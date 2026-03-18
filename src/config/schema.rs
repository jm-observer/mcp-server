use schemars::schema_for;
use super::tool::ToolFile;

pub fn tool_config_schema() -> String {
    let schema = schema_for!(ToolFile);
    serde_json::to_string_pretty(&schema).unwrap()
}
