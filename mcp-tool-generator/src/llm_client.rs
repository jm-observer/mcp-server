use anyhow::{anyhow, Result};
use log::debug;
use reqwest::Client;
use serde::{Deserialize, Serialize};

pub struct LlmClient {
    client: Client,
    base_url: String,
    model: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    max_tokens: usize,
}

#[derive(Deserialize, Debug)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize, Debug)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Deserialize, Debug)]
struct ChatMessageResponse {
    content: Option<String>,
}

impl LlmClient {
    pub fn new(base_url: &str, model: &str) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .unwrap(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
        }
    }

    pub async fn chat(&self, messages: Vec<ChatMessage>) -> Result<String> {
        let url = format!("{}/v1/chat/completions", self.base_url);
        debug!("Sending chat request to {}: {:?}", url, self.model);
        for msg in &messages {
            debug!("{}-{}", msg.role, msg.content);
        }
        let req = ChatRequest {
            model: self.model.clone(),
            messages,
            temperature: 0.1,
            max_tokens: 2048,
        };



        let resp = self.client.post(&url).json(&req).send().await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await?;
            return Err(anyhow!("LLM API error: {} - {}", status, text));
        }

        let mut chat_resp: ChatResponse = resp.json().await?;
        debug!("chat resp: {chat_resp:?}");
        if chat_resp.choices.is_empty() {
            return Err(anyhow!("Empty choices in LLM response"));
        }

        let content = chat_resp.choices.remove(0).message.content.unwrap_or_default();
        Ok(content)
    }
}
