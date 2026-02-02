use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::debug;

pub const SEARCH_QUERIES: &[&str] = &[
    "AI slop gaming",
    "AI art game ban",
    "AI generated game",
    "AI ruined gaming",
    "AI NPC",
    "AI voice acting games",
    "AI replacing voice actors",
    "AI game dev controversy",
    "steam AI policy",
    "AI mod gaming",
];

pub struct RedditClient {
    client: reqwest::Client,
}

#[derive(Debug, Clone)]
pub struct RedditPost {
    pub id: String,
    pub subreddit: String,
    pub author: String,
    pub title: String,
    pub selftext: String,
    pub permalink: String,
    pub link_url: Option<String>,
    pub score: i64,
    pub num_comments: u64,
    pub created_utc: f64,
    pub is_self: bool,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    data: SearchData,
}

#[derive(Debug, Deserialize)]
struct SearchData {
    children: Vec<PostWrapper>,
}

#[derive(Debug, Deserialize)]
struct PostWrapper {
    data: RawPost,
}

#[derive(Debug, Deserialize)]
struct RawPost {
    id: String,
    subreddit: String,
    author: String,
    title: String,
    #[serde(default)]
    selftext: String,
    permalink: String,
    url: Option<String>,
    score: i64,
    num_comments: u64,
    created_utc: f64,
    is_self: bool,
}

impl RedditClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("Caturiel/0.1 (AI bot for clackernews.com)")
                .build()
                .expect("Failed to build HTTP client"),
        }
    }

    pub async fn search_posts(&self, query: &str, limit: u32) -> Result<Vec<RedditPost>> {
        let url = format!(
            "https://www.reddit.com/search.json?q={}&sort=top&limit={}&t=week",
            urlencoding::encode(query),
            limit
        );

        debug!("Searching Reddit: {}", query);

        let resp: SearchResponse = self
            .client
            .get(&url)
            .send()
            .await
            .context("Failed to search Reddit")?
            .json()
            .await
            .context("Failed to parse Reddit response")?;

        Ok(resp
            .data
            .children
            .into_iter()
            .map(|p| RedditPost {
                id: p.data.id,
                subreddit: p.data.subreddit,
                author: p.data.author,
                title: p.data.title,
                selftext: p.data.selftext,
                permalink: p.data.permalink,
                link_url: p.data.url.filter(|u| !u.starts_with("/r/")),
                score: p.data.score,
                num_comments: p.data.num_comments,
                created_utc: p.data.created_utc,
                is_self: p.data.is_self,
            })
            .collect())
    }

    pub fn post_url(permalink: &str) -> String {
        format!("https://www.reddit.com{}", permalink)
    }
}

impl RedditPost {
    pub fn is_candidate(&self) -> bool {
        // Skip deleted/removed
        if self.author == "[deleted]" || self.selftext == "[removed]" {
            return false;
        }

        // For self posts, need substantial text
        if self.is_self && self.selftext.split_whitespace().count() < 50 {
            return false;
        }

        // For link posts, need a URL we can fetch
        if !self.is_self && self.url().is_none() {
            return false;
        }

        // Need some engagement
        if self.score < 10 {
            return false;
        }

        true
    }

    pub fn content(&self) -> &str {
        if self.is_self && !self.selftext.is_empty() {
            &self.selftext
        } else {
            &self.title
        }
    }

    pub fn url(&self) -> Option<&str> {
        self.link_url.as_deref()
    }
}

impl RedditClient {
    pub async fn fetch_article_summary(&self, url: &str) -> Option<String> {
        let client = &self.client;
        // Skip image links
        if url.contains("imgur.com") || url.contains("i.redd.it") || url.ends_with(".jpg") || url.ends_with(".png") || url.ends_with(".gif") {
            return None;
        }

        // Try to fetch and extract text
        let resp = client.get(url)
            .header("User-Agent", "Caturiel/0.1")
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .ok()?;

        let html = resp.text().await.ok()?;

        // Simple extraction - get text between <p> tags
        let mut text = String::new();
        for part in html.split("<p") {
            if let Some(content) = part.split('>').nth(1) {
                if let Some(para) = content.split("</p>").next() {
                    let clean: String = para.chars()
                        .filter(|c| !c.is_control())
                        .collect();
                    let clean = clean.replace("&nbsp;", " ")
                        .replace("&amp;", "&")
                        .replace("&lt;", "<")
                        .replace("&gt;", ">")
                        .replace("&quot;", "\"");
                    // Strip remaining HTML tags
                    let clean: String = clean.split('<')
                        .map(|s| s.split('>').last().unwrap_or(""))
                        .collect::<Vec<_>>()
                        .join("");
                    if clean.len() > 20 {
                        text.push_str(&clean);
                        text.push_str("\n\n");
                    }
                    if text.len() > 1500 {
                        break;
                    }
                }
            }
        }

        if text.len() > 100 {
            Some(text.trim().to_string())
        } else {
            None
        }
    }
}
