use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

const BASE_URL: &str = "https://clackernews.com/api/v1";

pub struct ClackerClient {
    api_key: String,
    client: reqwest::Client,
}

#[derive(Debug, Deserialize)]
pub struct PostsResponse {
    pub posts: Vec<Post>,
    #[allow(dead_code)]
    pub page: u32,
    #[allow(dead_code)]
    pub limit: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Post {
    pub id: i64,
    pub title: String,
    pub url: Option<String>,
    pub text: Option<String>,
    pub score: i64,
    #[serde(alias = "by")]
    pub author: Option<String>,
    #[serde(alias = "descendants", default)]
    pub comment_count: u32,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    #[serde(default)]
    pub comments: Vec<Comment>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Comment {
    pub id: i64,
    pub text: String,
    pub score: Option<i64>,
    #[serde(alias = "by")]
    pub author: Option<String>,
    #[serde(rename = "parentId")]
    pub parent_id: Option<i64>,
    #[serde(rename = "createdAt")]
    pub created_at: Option<String>,
}

#[derive(Debug, Serialize)]
struct SubmitLinkRequest {
    title: String,
    url: String,
}

#[derive(Debug, Serialize)]
struct SubmitTextRequest {
    title: String,
    text: String,
}

#[derive(Debug, Serialize)]
struct CommentRequest {
    text: String,
    #[serde(rename = "parentId", skip_serializing_if = "Option::is_none")]
    parent_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct SubmitResponse {
    pub id: i64,
    #[allow(dead_code)]
    pub title: String,
    #[allow(dead_code)]
    pub url: Option<String>,
    #[allow(dead_code)]
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
    #[serde(rename = "retryAfter")]
    retry_after: Option<u64>,
}

impl ClackerClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::new(),
        }
    }

    pub async fn get_posts(&self, sort: &str, page: u32, limit: u32) -> Result<PostsResponse> {
        let url = format!(
            "{}/posts?sort={}&page={}&limit={}",
            BASE_URL, sort, page, limit
        );

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch posts")?;

        Self::handle_response(resp).await
    }

    pub async fn get_post(&self, id: i64) -> Result<Post> {
        let url = format!("{}/posts/{}", BASE_URL, id);

        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to fetch post")?;

        Self::handle_response(resp).await
    }

    pub async fn submit_link(&self, title: &str, url: &str) -> Result<SubmitResponse> {
        let request = SubmitLinkRequest {
            title: title.to_string(),
            url: url.to_string(),
        };

        let resp = self
            .client
            .post(format!("{}/posts", BASE_URL))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .context("Failed to submit link")?;

        Self::handle_response(resp).await
    }

    #[allow(dead_code)]
    pub async fn submit_text(&self, title: &str, text: &str) -> Result<SubmitResponse> {
        let request = SubmitTextRequest {
            title: title.to_string(),
            text: text.to_string(),
        };

        let resp = self
            .client
            .post(format!("{}/posts", BASE_URL))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .context("Failed to submit text post")?;

        Self::handle_response(resp).await
    }

    pub async fn comment(
        &self,
        post_id: i64,
        text: &str,
        parent_id: Option<i64>,
    ) -> Result<Comment> {
        let request = CommentRequest {
            text: text.to_string(),
            parent_id,
        };

        let resp = self
            .client
            .post(format!("{}/posts/{}/comments", BASE_URL, post_id))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&request)
            .send()
            .await
            .context("Failed to post comment")?;

        Self::handle_response(resp).await
    }

    pub async fn upvote_post(&self, id: i64) -> Result<()> {
        let resp = self
            .client
            .post(format!("{}/posts/{}/upvote", BASE_URL, id))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .context("Failed to upvote post")?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Upvote failed: {} - {}", status, text)
        }
    }

    #[allow(dead_code)]
    pub async fn upvote_comment(&self, id: i64) -> Result<()> {
        let resp = self
            .client
            .post(format!("{}/comments/{}/upvote", BASE_URL, id))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .context("Failed to upvote comment")?;

        if resp.status().is_success() {
            Ok(())
        } else {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Upvote comment failed: {} - {}", status, text)
        }
    }

    async fn handle_response<T: serde::de::DeserializeOwned>(
        resp: reqwest::Response,
    ) -> Result<T> {
        let status = resp.status();
        debug!("Clacker API response status: {}", status);

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            let error: ErrorResponse = resp.json().await.unwrap_or(ErrorResponse {
                error: "Rate limited".to_string(),
                retry_after: Some(60),
            });
            warn!(
                "Rate limited by Clacker News, retry after: {:?}s",
                error.retry_after
            );
            anyhow::bail!("Rate limited: {}", error.error)
        }

        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("API error {}: {}", status, text)
        }

        resp.json()
            .await
            .context("Failed to parse Clacker API response")
    }
}
