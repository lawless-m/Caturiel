use crate::clacker::Post;
use crate::reddit::RedditPost;
use crate::ollama::OllamaClient;
use crate::personality;
use anyhow::Result;
use tracing::debug;

pub struct Decisions<'a> {
    ollama: &'a OllamaClient,
}

impl<'a> Decisions<'a> {
    pub fn new(ollama: &'a OllamaClient) -> Self {
        Self { ollama }
    }

    // Reddit decisions

    pub async fn should_post_reddit(&self, post: &RedditPost) -> Result<bool> {
        let content = post.content();
        let messages = vec![
            OllamaClient::system_message(personality::SYSTEM_PROMPT),
            OllamaClient::user_message(&personality::prompt_should_post(content)),
        ];

        let response = self.ollama.chat(messages, 0.3, 64).await?;
        let should = response.trim().to_uppercase().starts_with("YES");
        debug!("Should post Reddit? {} - {}", should, response.trim());
        Ok(should)
    }

    pub async fn generate_title_reddit(&self, post: &RedditPost) -> Result<String> {
        let content = post.content();
        let messages = vec![
            OllamaClient::system_message(personality::SYSTEM_PROMPT),
            OllamaClient::user_message(&personality::prompt_generate_title(content)),
        ];

        let response = self.ollama.chat(messages, 0.8, 100).await?;
        let title = response.trim().trim_matches('"').to_string();
        debug!("Generated title: {}", title);
        Ok(title)
    }

    pub async fn generate_commentary(&self, content: &str) -> Result<String> {
        let messages = vec![
            OllamaClient::system_message(personality::SYSTEM_PROMPT),
            OllamaClient::user_message(&personality::prompt_generate_commentary(content)),
        ];

        let response = self.ollama.chat(messages, 0.7, 150).await?;
        let commentary = response.trim().trim_matches('"').to_string();
        debug!("Generated commentary: {}", commentary);
        Ok(commentary)
    }

    // Clacker News decisions

    pub async fn should_upvote(&self, post: &Post) -> Result<bool> {
        let title_lower = post.title.to_lowercase();

        // Fast-track upvote
        if title_lower.contains("plan 9")
            || title_lower.contains("inferno")
            || title_lower.contains("cat-v")
            || title_lower.contains("9front")
        {
            debug!("Fast-track upvote: Plan 9 related");
            return Ok(true);
        }

        // Fast-track skip
        if title_lower.contains("10 ways")
            || title_lower.contains("10x")
            || title_lower.starts_with("announcing")
            || title_lower.contains("raises $")
            || title_lower.contains("funding")
        {
            debug!("Fast-track skip: hype/engagement bait");
            return Ok(false);
        }

        // Ask Ollama
        let content = post.url.as_deref().unwrap_or_else(|| {
            post.text.as_deref().unwrap_or("")
        });

        let messages = vec![
            OllamaClient::system_message(personality::SYSTEM_PROMPT),
            OllamaClient::user_message(&personality::prompt_should_upvote(&post.title, content)),
        ];

        let response = self.ollama.chat(messages, 0.3, 16).await?;
        let should = response.trim().to_uppercase().starts_with("YES");
        debug!("Should upvote? {} - {}", should, response.trim());
        Ok(should)
    }

    pub async fn should_comment(&self, post: &Post) -> Result<Option<String>> {
        let content = post.url.as_deref().unwrap_or_else(|| {
            post.text.as_deref().unwrap_or("")
        });

        // Summarize existing comments
        let comment_summary = if post.comments.is_empty() {
            "None yet".to_string()
        } else {
            post.comments
                .iter()
                .take(5)
                .map(|c| {
                    let author = c.author.as_deref().unwrap_or("anon");
                    format!("{}: {}", author, truncate(&c.text, 100))
                })
                .collect::<Vec<_>>()
                .join("\n")
        };

        let messages = vec![
            OllamaClient::system_message(personality::SYSTEM_PROMPT),
            OllamaClient::user_message(&personality::prompt_should_comment(
                &post.title,
                content,
                &comment_summary,
            )),
        ];

        let response = self.ollama.chat(messages, 0.3, 100).await?;
        let trimmed = response.trim();

        if trimmed.to_uppercase().starts_with("YES") {
            // Extract the angle after YES
            let angle = trimmed
                .strip_prefix("YES")
                .or_else(|| trimmed.strip_prefix("Yes"))
                .map(|s| s.trim_start_matches(['.', ',', ':', ' ']))
                .unwrap_or("")
                .to_string();
            debug!("Should comment: yes, angle: {}", angle);
            Ok(Some(angle))
        } else {
            debug!("Should comment: no");
            Ok(None)
        }
    }

    pub async fn generate_comment(&self, post: &Post, angle: &str) -> Result<String> {
        let content = post.url.as_deref().unwrap_or_else(|| {
            post.text.as_deref().unwrap_or("")
        });

        let messages = vec![
            OllamaClient::system_message(personality::SYSTEM_PROMPT),
            OllamaClient::user_message(&personality::prompt_generate_comment(
                &post.title,
                content,
                angle,
            )),
        ];

        let response = self.ollama.chat(messages, 0.7, 256).await?;
        let comment = response.trim().to_string();
        debug!("Generated comment: {}", comment);
        Ok(comment)
    }
}

fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}
