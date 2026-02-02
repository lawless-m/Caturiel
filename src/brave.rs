use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::debug;

const API_URL: &str = "https://api.search.brave.com/res/v1/web/search";

pub struct BraveClient {
    client: reqwest::Client,
    api_key: String,
}

#[derive(Debug, Deserialize)]
pub struct BraveSearchResponse {
    pub web: Option<WebResults>,
}

#[derive(Debug, Deserialize)]
pub struct WebResults {
    pub results: Vec<SearchResult>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub description: String,
    #[serde(default)]
    pub age: Option<String>,
}

impl BraveClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("Caturiel/0.1 (AI bot for clackernews.com)")
                .build()
                .expect("Failed to create HTTP client"),
            api_key,
        }
    }

    pub async fn search(&self, query: &str, count: u8) -> Result<Vec<SearchResult>> {
        debug!("Brave search: {}", query);

        let response = self
            .client
            .get(API_URL)
            .header("X-Subscription-Token", &self.api_key)
            .query(&[("q", query), ("count", &count.to_string())])
            .send()
            .await
            .context("Brave search request failed")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Brave search failed: {} - {}", status, body);
        }

        let search_response: BraveSearchResponse = response
            .json()
            .await
            .context("Failed to parse Brave response")?;

        Ok(search_response
            .web
            .map(|w| w.results)
            .unwrap_or_default())
    }

    /// Fetch article content from a URL
    pub async fn fetch_article(&self, url: &str) -> Result<String> {
        debug!("Fetching article: {}", url);

        let response = self
            .client
            .get(url)
            .send()
            .await
            .context("Failed to fetch article")?;

        if !response.status().is_success() {
            anyhow::bail!("Article fetch failed: {}", response.status());
        }

        let html = response.text().await.context("Failed to read article")?;
        Ok(Self::extract_text(&html))
    }

    /// Basic HTML to text extraction
    fn extract_text(html: &str) -> String {
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
        words.into_iter().take(1500).collect::<Vec<_>>().join(" ")
    }
}

impl SearchResult {
    /// Check if this looks like a good article to post
    pub fn is_candidate(&self) -> bool {
        let dominated_by = [
            "twitter.com", "x.com", "facebook.com", "instagram.com",
            "youtube.com", "reddit.com", "linkedin.com",
        ];

        // Skip social media (we want articles)
        !dominated_by.iter().any(|d| self.url.contains(d))
    }
}
