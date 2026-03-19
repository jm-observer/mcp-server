use crate::config::{RegisteredTool, ToolAction};
use reqwest::{Client, Method, header::{HeaderMap, HeaderName, HeaderValue}};
use serde_json::Value;
use std::collections::HashMap;
use thiserror::Error;
use urlencoding::encode;

#[derive(Error, Debug)]
pub enum HttpError {
    #[error("Template resolution error: {0}")]
    TemplateResolution(String),
    #[error("Missing URL or path")]
    MissingUrl,
    #[error("Reqwest error: {0}")]
    Reqwest(#[from] reqwest::Error),
    #[error("Invalid HTTP method: {0}")]
    InvalidMethod(String),
    #[error("Invalid header: {0}")]
    InvalidHeader(String),
}

pub struct HttpExecutor {
    client: Client,
}

pub struct HttpResult {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl Default for HttpExecutor {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpExecutor {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    pub fn resolve_template_url_encoded(template: &str, args: &HashMap<String, Value>) -> Result<String, HttpError> {
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
                        Value::String(s) => result.push_str(&encode(s)),
                        Value::Number(n) => result.push_str(&encode(&n.to_string())),
                        Value::Bool(b) => result.push_str(&encode(&b.to_string())),
                        _ => return Err(HttpError::TemplateResolution(format!("Variable {} is not simple", var_name))),
                    }
                }
            } else {
                result.push(c);
            }
        }
        
        Ok(result)
    }

    pub fn resolve_template(template: &str, args: &HashMap<String, Value>) -> Result<String, HttpError> {
        // basic replace for body without URL encoding
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
                        _ => return Err(HttpError::TemplateResolution(format!("Variable {} is not a simple value", var_name))),
                    }
                }
            } else {
                result.push(c);
            }
        }
        Ok(result)
    }

    pub async fn execute(
        &self,
        tool: &RegisteredTool,
        arguments: &HashMap<String, Value>,
    ) -> Result<HttpResult, HttpError> {
        let (path_opt, method_opt, content_type_opt, body_opt) = match &tool.def.action {
            ToolAction::Http { method, path, body, content_type } => (path, method, content_type, body),
            _ => return Err(HttpError::MissingUrl),
        };

        let base_url = tool.base_url.as_deref().unwrap_or("");
        let t_path = path_opt.as_deref().unwrap_or("");
        let path_resolved = Self::resolve_template_url_encoded(t_path, arguments)?;

        let url = format!("{}{}", base_url, path_resolved);
        if url.is_empty() {
            return Err(HttpError::MissingUrl);
        }

        let method_str = method_opt.as_deref().unwrap_or("GET");
        let method = Method::from_bytes(method_str.as_bytes())
            .map_err(|_| HttpError::InvalidMethod(method_str.to_string()))?;

        let mut req_builder = self.client.request(method, &url)
            .timeout(std::time::Duration::from_secs(tool.effective_timeout));

        let mut headers = HeaderMap::new();
        // apply env headers from tool definition
        for (k, v) in &tool.env {
            let h_name = HeaderName::from_bytes(k.as_bytes())
                .map_err(|_| HttpError::InvalidHeader(k.clone()))?;
            let h_val = HeaderValue::from_str(v)
                .map_err(|_| HttpError::InvalidHeader(v.clone()))?;
            headers.insert(h_name, h_val);
        }

        if let Some(ct) = content_type_opt {
            headers.insert(reqwest::header::CONTENT_TYPE, HeaderValue::from_str(ct).unwrap());
        }

        req_builder = req_builder.headers(headers);

        if let Some(body_tpl) = body_opt {
            let body_resolved = Self::resolve_template(body_tpl, arguments)?;
            req_builder = req_builder.body(body_resolved);
        }

        let response = req_builder.send().await?;

        let status = response.status().as_u16();
        
        let mut res_headers = HashMap::new();
        for (k, v) in response.headers() {
            if let Ok(val) = v.to_str() {
                res_headers.insert(k.as_str().to_string(), val.to_string());
            }
        }

        let body = response.text().await?;

        Ok(HttpResult {
            status,
            headers: res_headers,
            body,
        })
    }
}
