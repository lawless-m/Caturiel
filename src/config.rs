use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

const CONFIG_PATH: &str = "/home/matt/.config/caturiel/config.toml";
const API_KEY_PATH: &str = "/home/matt/.config/caturiel/apikey";

#[derive(Debug, Clone)]
pub struct Config {
    pub clacker_api_key: String,
    pub ollama_url: String,
    pub ollama_model: Option<String>,
    pub ntfy_topic: String,
    pub db_path: String,
    pub poll_interval_mins: u64,
    pub log_level: String,
    pub confirm_posts: bool,
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    ollama_url: Option<String>,
    ollama_model: Option<String>,
    ntfy_topic: Option<String>,
    db_path: Option<String>,
    poll_interval_mins: Option<u64>,
    log_level: Option<String>,
    confirm_posts: Option<bool>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let api_key = Self::load_api_key()?;
        let file_config = Self::load_config_file()?;

        Ok(Self {
            clacker_api_key: api_key,
            ollama_url: file_config
                .ollama_url
                .unwrap_or_else(|| "http://10.99.0.3:11434".to_string()),
            ollama_model: file_config.ollama_model,
            ntfy_topic: file_config
                .ntfy_topic
                .unwrap_or_else(|| "caturiel".to_string()),
            db_path: file_config
                .db_path
                .unwrap_or_else(|| "/home/matt/.local/share/caturiel/caturiel.db".to_string()),
            poll_interval_mins: file_config.poll_interval_mins.unwrap_or(30),
            log_level: file_config
                .log_level
                .unwrap_or_else(|| "info".to_string()),
            confirm_posts: file_config.confirm_posts.unwrap_or(true),
        })
    }

    fn load_api_key() -> Result<String> {
        let key = std::fs::read_to_string(API_KEY_PATH)
            .with_context(|| format!("Failed to read API key from {}", API_KEY_PATH))?
            .trim()
            .to_string();

        if key.is_empty() {
            anyhow::bail!("API key file is empty: {}", API_KEY_PATH);
        }

        Ok(key)
    }

    fn load_config_file() -> Result<ConfigFile> {
        let config_path = Path::new(CONFIG_PATH);

        if !config_path.exists() {
            // Return defaults if no config file
            return Ok(ConfigFile {
                ollama_url: None,
                ollama_model: None,
                ntfy_topic: None,
                db_path: None,
                poll_interval_mins: None,
                log_level: None,
                confirm_posts: None,
            });
        }

        let content = std::fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read config from {}", CONFIG_PATH))?;

        toml::from_str(&content).with_context(|| format!("Failed to parse {}", CONFIG_PATH))
    }

    pub async fn validate(&self) -> Result<()> {
        // Check Ollama connectivity
        let client = reqwest::Client::new();
        client
            .get(format!("{}/api/tags", self.ollama_url))
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
            .context("Ollama not reachable")?;

        // Ensure DB directory exists
        if let Some(parent) = Path::new(&self.db_path).parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).context("Cannot create database directory")?;
            }
        }

        Ok(())
    }
}
