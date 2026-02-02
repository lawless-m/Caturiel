use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

pub struct OllamaClient {
    base_url: String,
    model: String,
    client: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    stream: bool,
    options: ChatOptions,
}

#[derive(Debug, Serialize)]
struct ChatOptions {
    temperature: f32,
    num_predict: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    message: ChatMessage,
    #[allow(dead_code)]
    total_duration: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct TagsResponse {
    models: Vec<Model>,
}

#[derive(Debug, Deserialize)]
struct Model {
    name: String,
}

impl OllamaClient {
    pub async fn new(base_url: &str, model: Option<&str>) -> Result<Self> {
        let client = reqwest::Client::new();

        let model = match model {
            Some(m) => m.to_string(),
            None => Self::auto_detect_model(&client, base_url).await?,
        };

        info!("Using Ollama model: {}", model);

        Ok(Self {
            base_url: base_url.to_string(),
            model,
            client,
        })
    }

    async fn auto_detect_model(client: &reqwest::Client, base_url: &str) -> Result<String> {
        let resp: TagsResponse = client
            .get(format!("{}/api/tags", base_url))
            .send()
            .await
            .context("Failed to connect to Ollama")?
            .json()
            .await
            .context("Failed to parse Ollama models response")?;

        let preferences = [
            "qwen2.5:32b",
            "qwen2.5:14b",
            "qwen2.5:7b",
            "qwen2:72b",
            "qwen2:7b",
        ];

        for pref in preferences {
            if resp.models.iter().any(|m| m.name == pref) {
                return Ok(pref.to_string());
            }
        }

        // Fall back to any qwen model
        resp.models
            .iter()
            .find(|m| m.name.starts_with("qwen"))
            .map(|m| m.name.clone())
            .ok_or_else(|| anyhow::anyhow!("No Qwen model found in Ollama"))
    }

    pub async fn chat(
        &self,
        messages: Vec<ChatMessage>,
        temperature: f32,
        max_tokens: u32,
    ) -> Result<String> {
        let request = ChatRequest {
            model: self.model.clone(),
            messages,
            stream: false,
            options: ChatOptions {
                temperature,
                num_predict: max_tokens,
            },
        };

        debug!("Ollama request: {:?}", request);

        let resp: ChatResponse = self
            .client
            .post(format!("{}/api/chat", self.base_url))
            .json(&request)
            .send()
            .await
            .context("Failed to send request to Ollama")?
            .json()
            .await
            .context("Failed to parse Ollama response")?;

        debug!("Ollama response: {:?}", resp.message.content);

        Ok(resp.message.content)
    }

    pub fn system_message(content: &str) -> ChatMessage {
        ChatMessage {
            role: "system".to_string(),
            content: content.to_string(),
        }
    }

    pub fn user_message(content: &str) -> ChatMessage {
        ChatMessage {
            role: "user".to_string(),
            content: content.to_string(),
        }
    }
}
