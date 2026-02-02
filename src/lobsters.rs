use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::debug;

const BASE_URL: &str = "https://lobste.rs";

pub const SEARCH_TAGS: &[&str] = &["ai", "ml", "practices"];

pub struct LobstersClient {
    client: reqwest::Client,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LobstersPost {
    pub short_id: String,
    pub title: String,
    pub url: String,
    pub description: String,
    pub description_plain: String,
    pub score: i64,
    pub comment_count: u32,
    pub created_at: String,
    pub short_id_url: String,
    pub comments_url: String,
    pub submitter_user: String,
    pub tags: Vec<String>,
}

impl LobstersClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("Caturiel/0.1 (AI bot for clackernews.com)")
                .build()
                .expect("Failed to create HTTP client"),
        }
    }

    /// Fetch posts by tag
    pub async fn get_tagged(&self, tag: &str, page: u32) -> Result<Vec<LobstersPost>> {
        let url = format!("{}/t/{}/page/{}.json", BASE_URL, tag, page);
        debug!("Fetching Lobsters tag: {}", url);

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch Lobsters posts")?;

        if !resp.status().is_success() {
            anyhow::bail!("Lobsters API error: {}", resp.status());
        }

        let posts: Vec<LobstersPost> = resp.json().await.context("Failed to parse Lobsters JSON")?;
        Ok(posts)
    }

    /// Fetch newest posts
    pub async fn get_newest(&self) -> Result<Vec<LobstersPost>> {
        let url = format!("{}/newest.json", BASE_URL);
        debug!("Fetching Lobsters newest: {}", url);

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch Lobsters posts")?;

        if !resp.status().is_success() {
            anyhow::bail!("Lobsters API error: {}", resp.status());
        }

        let posts: Vec<LobstersPost> = resp.json().await.context("Failed to parse Lobsters JSON")?;
        Ok(posts)
    }

    /// Search by checking newest/tagged posts for keywords
    pub async fn search_ai_content(&self) -> Result<Vec<LobstersPost>> {
        let mut results = Vec::new();

        // Check AI/ML tags
        for tag in SEARCH_TAGS {
            match self.get_tagged(tag, 1).await {
                Ok(posts) => {
                    for post in posts {
                        if Self::is_ai_related(&post) {
                            results.push(post);
                        }
                    }
                }
                Err(e) => {
                    debug!("Failed to fetch tag {}: {}", tag, e);
                }
            }
        }

        // Also check newest for AI-related content
        if let Ok(posts) = self.get_newest().await {
            for post in posts {
                if Self::is_ai_related(&post) && !results.iter().any(|p| p.short_id == post.short_id) {
                    results.push(post);
                }
            }
        }

        Ok(results)
    }

    fn is_ai_related(post: &LobstersPost) -> bool {
        let title_lower = post.title.to_lowercase();
        let desc_lower = post.description_plain.to_lowercase();

        let keywords = [
            "ai", "artificial intelligence", "llm", "chatgpt", "gpt-4", "gpt4",
            "claude", "copilot", "machine learning", "neural network",
            "generative ai", "ai art", "ai slop", "ai generated",
            "large language model", "openai", "anthropic", "midjourney",
            "stable diffusion", "ai replace", "ai job", "ai voice",
        ];

        keywords.iter().any(|kw| title_lower.contains(kw) || desc_lower.contains(kw))
            || post.tags.iter().any(|t| t == "ai" || t == "ml")
    }
}

impl LobstersPost {
    /// Check if this post is a good candidate for Clacker News
    pub fn is_candidate(&self) -> bool {
        // Has some discussion
        self.comment_count >= 3
            // Not too old (score indicates engagement)
            && self.score >= 5
            // Has an external URL (not just a lobsters discussion)
            && !self.url.is_empty()
            && !self.url.starts_with("https://lobste.rs")
    }

    /// Get the discussion URL on Lobsters
    pub fn discussion_url(&self) -> &str {
        &self.short_id_url
    }

    /// Get the article URL
    pub fn article_url(&self) -> &str {
        &self.url
    }
}

impl LobstersClient {
    /// Fetch and extract text content from an article URL
    pub async fn fetch_article(&self, url: &str) -> Result<String> {
        debug!("Fetching article: {}", url);

        let resp = self
            .client
            .get(url)
            .send()
            .await
            .context("Failed to fetch article")?;

        if !resp.status().is_success() {
            anyhow::bail!("Article fetch error: {}", resp.status());
        }

        let html = resp.text().await.context("Failed to read article body")?;

        // Basic HTML to text extraction - strip tags
        Ok(Self::extract_text(&html))
    }

    /// Basic HTML to text extraction
    fn extract_text(html: &str) -> String {
        // Remove script and style tags with content
        let mut text = html.to_string();

        // Remove script blocks
        while let Some(start) = text.find("<script") {
            if let Some(end) = text[start..].find("</script>") {
                text = format!("{}{}", &text[..start], &text[start + end + 9..]);
            } else {
                break;
            }
        }

        // Remove style blocks
        while let Some(start) = text.find("<style") {
            if let Some(end) = text[start..].find("</style>") {
                text = format!("{}{}", &text[..start], &text[start + end + 8..]);
            } else {
                break;
            }
        }

        // Remove all HTML tags
        let mut result = String::new();
        let mut in_tag = false;
        for c in text.chars() {
            match c {
                '<' => in_tag = true,
                '>' => in_tag = false,
                _ if !in_tag => result.push(c),
                _ => {}
            }
        }

        // Collapse whitespace and limit length
        let words: Vec<&str> = result.split_whitespace().collect();
        let truncated: String = words.into_iter().take(1500).collect::<Vec<_>>().join(" ");

        truncated
    }
}
