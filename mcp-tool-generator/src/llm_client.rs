use anyhow::{anyhow, bail, Result};
use async_openai::{
    Client,
    config::OpenAIConfig,
    types::chat::{
        ChatCompletionRequestMessage, CreateChatCompletionRequestArgs,
    },
};
use log::{debug, info};

pub struct LlmClient {
    client: Client<OpenAIConfig>,
    model: String,
}

impl LlmClient {
    pub fn new(base_url: &str, model: &str) -> Self {
        let base = base_url.trim_end_matches('/');
        let api_base = if base.ends_with("/v1") {
            base.to_string()
        } else {
            format!("{}/v1", base)
        };
        let config = OpenAIConfig::new().with_api_base(api_base);
        Self {
            client: Client::with_config(config),
            model: model.to_string(),
        }
    }

    pub async fn chat(&self, messages: Vec<ChatCompletionRequestMessage>) -> Result<String> {
        debug!("Sending chat request with model: {}", self.model);
        for msg in &messages {
            debug!("{:?}", msg);
        }

        let request = CreateChatCompletionRequestArgs::default()
            .model(&self.model)
            .messages(messages)
            .temperature(0.1)
            .max_tokens(10000u32)
            .build()?;

        let response = self.client.chat().create(request).await
            .map_err(|e| anyhow!("LLM API error: {}", e))?;

        debug!("chat resp: {:?}", response);
        let content = response
            .choices
            .first().ok_or_else(|| anyhow!("Empty choices in LLM response"))?;
        if let Some(reason) = content.finish_reason {
            info!("chat end{reason:?}");
        }

        let content = response
            .choices
            .first()
            .and_then(|c| {
                debug!("choice response: {:?}", c);
                c.message.content.as_deref()
            })
            .ok_or_else(|| anyhow!("Empty choices in LLM response"))?
            .to_string();

        Ok(content)
    }
}
