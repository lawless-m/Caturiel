use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::debug;

const BASE_URL: &str = "https://public.api.bsky.app";

pub const SEARCH_QUERIES: &[&str] = &[
    "AI slop",
    "LLM garbage",
    "chatgpt ruined",
    "AI generated trash",
    "sick of AI",
    "AI art is",
    "ban AI",
    "AI will never",
    "replace humans AI",
    "AI apocalypse",
];

pub struct BlueskyClient {
    client: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct BlueskyPost {
    pub uri: String,
    pub author_handle: String,
    pub author_display_name: Option<String>,
    pub text: String,
    pub created_at: String,
    pub like_count: u64,
    pub repost_count: u64,
    pub reply_count: u64,
    pub is_reply: bool,
    pub langs: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    posts: Vec<RawPost>,
    #[allow(dead_code)]
    cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawPost {
    uri: String,
    author: RawAuthor,
    record: RawRecord,
    #[serde(rename = "likeCount", default)]
    like_count: u64,
    #[serde(rename = "repostCount", default)]
    repost_count: u64,
    #[serde(rename = "replyCount", default)]
    reply_count: u64,
}

#[derive(Debug, Deserialize)]
struct RawAuthor {
    handle: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawRecord {
    text: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(default)]
    langs: Vec<String>,
    reply: Option<serde_json::Value>,
}

impl BlueskyClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("Caturiel/0.1 (AI bot for clackernews.com)")
                .build()
                .expect("Failed to build HTTP client"),
        }
    }

    pub async fn search_posts(&self, query: &str, limit: u32) -> Result<Vec<BlueskyPost>> {
        let url = format!(
            "{}/xrpc/app.bsky.feed.searchPosts?q={}&limit={}&sort=latest",
            BASE_URL,
            urlencoding::encode(query),
            limit
        );

        debug!("Searching Bluesky: {}", query);

        let resp: SearchResponse = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to search Bluesky")?
            .json()
            .await
            .context("Failed to parse Bluesky response")?;

        Ok(resp
            .posts
            .into_iter()
            .map(|p| BlueskyPost {
                uri: p.uri,
                author_handle: p.author.handle,
                author_display_name: p.author.display_name,
                text: p.record.text,
                created_at: p.record.created_at,
                like_count: p.like_count,
                repost_count: p.repost_count,
                reply_count: p.reply_count,
                is_reply: p.record.reply.is_some(),
                langs: p.record.langs,
            })
            .collect())
    }

    pub fn post_web_url(handle: &str, uri: &str) -> String {
        // URI format: at://did:plc:xxx/app.bsky.feed.post/yyy
        // We need the last segment (yyy) as the post ID
        let post_id = uri.rsplit('/').next().unwrap_or("");
        format!("https://bsky.app/profile/{}/post/{}", handle, post_id)
    }
}

impl BlueskyPost {
    pub fn is_candidate(&self) -> bool {
        // Skip replies
        if self.is_reply {
            return false;
        }

        // Skip non-English (for now)
        if !self.langs.is_empty() && !self.langs.iter().any(|l| l.starts_with("en")) {
            return false;
        }

        // Skip very short posts
        if self.text.split_whitespace().count() < 10 {
            return false;
        }

        // Skip low-engagement (might be spam/noise)
        if self.like_count < 5 {
            return false;
        }

        true
    }
}
