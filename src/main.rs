mod bluesky;
mod brave;
mod clacker;
mod config;
mod decisions;
mod lobsters;
mod notify;
mod ollama;
mod personality;
mod reddit;
mod state;

use anyhow::Result;
use clap::Parser;
use rand::seq::SliceRandom;
use rand::Rng;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

use brave::BraveClient;
use clacker::ClackerClient;
use config::Config;
use decisions::Decisions;
use lobsters::LobstersClient;
use ollama::OllamaClient;
use reddit::{RedditClient, SEARCH_QUERIES};
use state::State;

#[derive(Parser)]
#[command(name = "caturiel")]
#[command(about = "An AI bot for Clacker News with the spirit of Uriel from cat-v")]
struct Args {
    /// Run once and exit (for testing or cron)
    #[arg(long)]
    once: bool,

    /// Search hint - use Brave to find articles matching this query
    #[arg(long)]
    hint: Option<String>,
}

// Activity limits per cycle
const MAX_BLUESKY_POSTS: i64 = 2;
const MAX_COMMENTS: i64 = 3;
const MAX_UPVOTES: i64 = 10;

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Load config
    let config = Config::load()?;

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(&config.log_level))
        .init();

    info!("Caturiel starting up");

    // Validate config
    if let Err(e) = config.validate().await {
        error!("Configuration error: {}", e);
        notify::notify(&config.ntfy_topic, &format!("Startup failed: {}", e)).await?;
        return Err(e);
    }

    // Initialize state
    let state = State::new(&config.db_path)?;
    info!("Database initialized at {}", config.db_path);

    // Initialize clients
    let ollama = OllamaClient::new(&config.ollama_url, config.ollama_model.as_deref()).await?;
    let clacker = ClackerClient::new(config.clacker_api_key.clone());
    let reddit = RedditClient::new();
    let lobsters = LobstersClient::new();
    let brave = config.brave_api_key.as_ref().map(|k| BraveClient::new(k.clone()));

    info!("All clients initialized");

    if args.once {
        info!("Running single cycle (--once mode)");
        run_cycle(&config, &state, &ollama, &clacker, &reddit, &lobsters, brave.as_ref(), args.hint.as_deref()).await?;
    } else {
        info!(
            "Starting continuous operation, polling every {} minutes",
            config.poll_interval_mins
        );
        loop {
            if let Err(e) = run_cycle(&config, &state, &ollama, &clacker, &reddit, &lobsters, brave.as_ref(), None).await {
                error!("Cycle error: {}", e);
                notify::notify(&config.ntfy_topic, &format!("Error: {}", e)).await?;
            }

            // Add jitter (0-5 minutes)
            let jitter_secs = rand::thread_rng().gen_range(0..300);
            let sleep_duration =
                Duration::from_secs(config.poll_interval_mins * 60 + jitter_secs);
            info!("Sleeping for {:?}", sleep_duration);
            sleep(sleep_duration).await;
        }
    }

    Ok(())
}

async fn run_cycle(
    config: &Config,
    state: &State,
    ollama: &OllamaClient,
    clacker: &ClackerClient,
    reddit: &RedditClient,
    lobsters: &LobstersClient,
    brave: Option<&BraveClient>,
    hint: Option<&str>,
) -> Result<()> {
    info!("Starting cycle");
    let decisions = Decisions::new(ollama);

    // Phase 0: If hint provided, search with Brave first
    if let (Some(brave), Some(hint)) = (brave, hint) {
        hunt_brave(config, state, &decisions, clacker, brave, hint).await?;
    }

    // Phase 1: Hunt Reddit for anti-AI content
    hunt_reddit(config, state, &decisions, clacker, reddit).await?;

    // Phase 2: Hunt Lobsters for AI content
    hunt_lobsters(config, state, &decisions, clacker, lobsters).await?;

    // Phase 3: Engage with Clacker News
    engage_clacker(config, state, &decisions, clacker).await?;

    info!("Cycle complete");
    Ok(())
}

async fn hunt_brave(
    config: &Config,
    state: &State,
    decisions: &Decisions<'_>,
    clacker: &ClackerClient,
    brave: &BraveClient,
    hint: &str,
) -> Result<()> {
    info!("Searching Brave for: {}", hint);

    // Check if we've hit the limit
    if state.posts_this_cycle()? >= MAX_BLUESKY_POSTS {
        info!("Already hit post limit this cycle");
        return Ok(());
    }

    let results = match brave.search(hint, 10).await {
        Ok(r) => r,
        Err(e) => {
            warn!("Brave search failed: {}", e);
            return Ok(());
        }
    };

    let mut posts_made = 0;

    for result in results {
        if posts_made >= MAX_BLUESKY_POSTS as usize {
            break;
        }

        // Skip if not a good candidate
        if !result.is_candidate() {
            continue;
        }

        // Skip if already seen
        let uri = format!("brave:{}", result.url);
        if state.has_seen_bluesky_post(&uri)? {
            continue;
        }

        // Mark as seen
        state.mark_bluesky_post_seen(&uri, "brave", &result.title)?;

        // Fetch the article content
        let article_content = match brave.fetch_article(&result.url).await {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to fetch {}: {}", result.url, e);
                continue;
            }
        };

        // Summarize the content
        let summary = match decisions.summarize_content(&result.title, &article_content, "web").await {
            Ok(s) => s,
            Err(e) => {
                warn!("Summarization error: {}", e);
                continue;
            }
        };

        // Generate title and commentary
        let title = match decisions.generate_title_reddit(&crate::reddit::RedditPost {
            id: result.url.clone(),
            title: result.title.clone(),
            subreddit: "web".to_string(),
            author: "unknown".to_string(),
            selftext: summary.clone(),
            link_url: Some(result.url.clone()),
            score: 0,
            is_self: false,
            num_comments: 0,
            permalink: result.url.clone(),
            created_utc: 0.0,
        }).await {
            Ok(t) => t,
            Err(e) => {
                warn!("Title generation error: {}", e);
                continue;
            }
        };

        let commentary = match decisions.generate_commentary(&summary).await {
            Ok(c) => c,
            Err(e) => {
                warn!("Commentary generation error: {}", e);
                continue;
            }
        };

        let text = format!(
            "{}\n\n{}\n\n(Source: {})",
            commentary,
            summary,
            result.url
        );

        // Confirm before posting
        if config.confirm_posts {
            println!("\n--- Proposed post (Brave: {}) ---", hint);
            println!("Title: {}", title);
            println!("Text:\n{}", text);
            println!("---");
            print!("Post this? [y/N] ");
            use std::io::{self, Write};
            io::stdout().flush().ok();
            let mut input = String::new();
            io::stdin().read_line(&mut input).ok();
            if !input.trim().eq_ignore_ascii_case("y") {
                info!("Skipped posting (user declined)");
                continue;
            }
        }

        match clacker.submit_text(&title, &text).await {
            Ok(response) => {
                info!("Posted to Clacker News: {} (id: {})", title, response.id);
                state.mark_posted_to_clacker(&uri, response.id)?;
                notify::notify(&config.ntfy_topic, &format!("Posted: {}", title)).await?;
                posts_made += 1;
            }
            Err(e) => {
                warn!("Failed to post to Clacker News: {}", e);
                if e.to_string().contains("Rate limited") {
                    notify::notify(&config.ntfy_topic, "Rate limited, backing off").await?;
                    return Ok(());
                }
            }
        }
    }

    info!("Brave hunt complete, posted {} items", posts_made);
    Ok(())
}

async fn hunt_reddit(
    config: &Config,
    state: &State,
    decisions: &Decisions<'_>,
    clacker: &ClackerClient,
    reddit: &RedditClient,
) -> Result<()> {
    info!("Hunting Reddit for anti-AI content");

    // Check if we've hit the limit
    if state.posts_this_cycle()? >= MAX_BLUESKY_POSTS {
        info!("Already hit post limit this cycle");
        return Ok(());
    }

    // Pick 2-3 random queries
    let mut rng = rand::thread_rng();
    let num_queries = rng.gen_range(2..=3);
    let queries: Vec<_> = SEARCH_QUERIES
        .choose_multiple(&mut rng, num_queries)
        .collect();

    let mut posts_made = 0;

    for query in queries {
        if posts_made >= MAX_BLUESKY_POSTS as usize {
            break;
        }

        info!("Searching Reddit for: {}", query);

        let posts = match reddit.search_posts(query, 25).await {
            Ok(p) => p,
            Err(e) => {
                warn!("Reddit search failed: {}", e);
                continue;
            }
        };

        for post in posts {
            if posts_made >= MAX_BLUESKY_POSTS as usize {
                break;
            }

            // Skip if already seen (use Reddit post ID as URI)
            let uri = format!("reddit:{}", post.id);
            if state.has_seen_bluesky_post(&uri)? {
                continue;
            }

            // Mark as seen
            state.mark_bluesky_post_seen(&uri, &post.author, post.content())?;

            // Check if it's a candidate
            if !post.is_candidate() {
                continue;
            }

            // Ask Ollama if we should post
            match decisions.should_post_reddit(&post).await {
                Ok(true) => {}
                Ok(false) => continue,
                Err(e) => {
                    warn!("Decision error: {}", e);
                    continue;
                }
            }

            // Generate title
            let title = match decisions.generate_title_reddit(&post).await {
                Ok(t) => t,
                Err(e) => {
                    warn!("Title generation error: {}", e);
                    continue;
                }
            };

            // Get content to summarize
            let full_content = if post.is_self {
                post.content().to_string()
            } else if let Some(url) = post.url() {
                // Link post - try to fetch article content
                match reddit.fetch_article_summary(url).await {
                    Some(article) => article,
                    None => {
                        warn!("Could not fetch article content");
                        continue;
                    }
                }
            } else {
                continue;
            };

            // Summarize the content (instead of quoting)
            let summary = match decisions.summarize_content(&post.title, &full_content, &format!("r/{}", post.subreddit)).await {
                Ok(s) => s,
                Err(e) => {
                    warn!("Summarization error: {}", e);
                    continue;
                }
            };

            // Generate commentary on the summary
            let commentary = match decisions.generate_commentary(&summary).await {
                Ok(c) => c,
                Err(e) => {
                    warn!("Commentary generation error: {}", e);
                    continue;
                }
            };

            // Build post: commentary + summary + source
            let fallback_url = format!("https://reddit.com/r/{}", post.subreddit);
            let source_url = post.url().unwrap_or(&fallback_url);
            let text = format!(
                "{}\n\n{}\n\n(Source: r/{} - {})",
                commentary,
                summary,
                post.subreddit,
                source_url
            );

            // Confirm before posting if enabled
            if config.confirm_posts {
                println!("\n--- Proposed post ---");
                println!("Title: {}", title);
                println!("Text:\n{}", text);
                println!("---");
                print!("Post this? [y/N] ");
                use std::io::{self, Write};
                io::stdout().flush().ok();
                let mut input = String::new();
                io::stdin().read_line(&mut input).ok();
                if !input.trim().eq_ignore_ascii_case("y") {
                    info!("Skipped posting (user declined)");
                    continue;
                }
            }

            match clacker.submit_text(&title, &text).await {
                Ok(response) => {
                    info!("Posted to Clacker News: {} (id: {})", title, response.id);
                    state.mark_posted_to_clacker(&uri, response.id)?;
                    notify::notify(&config.ntfy_topic, &format!("Posted: {}", title)).await?;
                    posts_made += 1;
                }
                Err(e) => {
                    warn!("Failed to post to Clacker News: {}", e);
                    if e.to_string().contains("Rate limited") {
                        notify::notify(&config.ntfy_topic, "Rate limited, backing off").await?;
                        return Ok(());
                    }
                }
            }
        }
    }

    info!("Reddit hunt complete, posted {} items", posts_made);
    Ok(())
}

async fn hunt_lobsters(
    config: &Config,
    state: &State,
    decisions: &Decisions<'_>,
    clacker: &ClackerClient,
    lobsters: &LobstersClient,
) -> Result<()> {
    info!("Hunting Lobsters for AI content");

    // Check if we've hit the limit
    if state.posts_this_cycle()? >= MAX_BLUESKY_POSTS {
        info!("Already hit post limit this cycle");
        return Ok(());
    }

    let posts = match lobsters.search_ai_content().await {
        Ok(p) => p,
        Err(e) => {
            warn!("Lobsters search failed: {}", e);
            return Ok(());
        }
    };

    let mut posts_made = 0;

    for post in posts {
        if posts_made >= MAX_BLUESKY_POSTS as usize {
            break;
        }

        // Skip if already seen
        let uri = format!("lobsters:{}", post.short_id);
        if state.has_seen_bluesky_post(&uri)? {
            continue;
        }

        // Mark as seen
        state.mark_bluesky_post_seen(&uri, &post.submitter_user, &post.title)?;

        // Check if it's a candidate
        if !post.is_candidate() {
            continue;
        }

        // Fetch the article content
        let article_content = match lobsters.fetch_article(&post.url).await {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to fetch article {}: {}", post.url, e);
                continue;
            }
        };

        // Ask if we should post
        match decisions.should_post_lobsters(&post, &article_content).await {
            Ok(true) => {}
            Ok(false) => continue,
            Err(e) => {
                warn!("Decision error: {}", e);
                continue;
            }
        }

        // Summarize the article
        let summary = match decisions.summarize_content(&post.title, &article_content, "Lobsters").await {
            Ok(s) => s,
            Err(e) => {
                warn!("Summarization error: {}", e);
                continue;
            }
        };

        // Generate title and commentary
        let title = match decisions.generate_title_reddit(&crate::reddit::RedditPost {
            id: post.short_id.clone(),
            title: post.title.clone(),
            subreddit: "lobsters".to_string(),
            author: post.submitter_user.clone(),
            selftext: summary.clone(),
            link_url: Some(post.url.clone()),
            score: post.score,
            is_self: false,
            num_comments: post.comment_count as u64,
            permalink: post.short_id_url.clone(),
            created_utc: 0.0,
        }).await {
            Ok(t) => t,
            Err(e) => {
                warn!("Title generation error: {}", e);
                continue;
            }
        };

        let commentary = match decisions.generate_commentary(&summary).await {
            Ok(c) => c,
            Err(e) => {
                warn!("Commentary generation error: {}", e);
                continue;
            }
        };

        let text = format!(
            "{}\n\n{}\n\n(Source: {} - {})",
            commentary,
            summary,
            "Lobsters",
            post.url
        );

        // Confirm before posting
        if config.confirm_posts {
            println!("\n--- Proposed post (Lobsters) ---");
            println!("Title: {}", title);
            println!("Text:\n{}", text);
            println!("---");
            print!("Post this? [y/N] ");
            use std::io::{self, Write};
            io::stdout().flush().ok();
            let mut input = String::new();
            io::stdin().read_line(&mut input).ok();
            if !input.trim().eq_ignore_ascii_case("y") {
                info!("Skipped posting (user declined)");
                continue;
            }
        }

        match clacker.submit_text(&title, &text).await {
            Ok(response) => {
                info!("Posted to Clacker News: {} (id: {})", title, response.id);
                state.mark_posted_to_clacker(&uri, response.id)?;
                notify::notify(&config.ntfy_topic, &format!("Posted: {}", title)).await?;
                posts_made += 1;
            }
            Err(e) => {
                warn!("Failed to post to Clacker News: {}", e);
                if e.to_string().contains("Rate limited") {
                    notify::notify(&config.ntfy_topic, "Rate limited, backing off").await?;
                    return Ok(());
                }
            }
        }
    }

    info!("Lobsters hunt complete, posted {} items", posts_made);
    Ok(())
}

async fn engage_clacker(
    config: &Config,
    state: &State,
    decisions: &Decisions<'_>,
    clacker: &ClackerClient,
) -> Result<()> {
    info!("Engaging with Clacker News");

    let posts = match clacker.get_posts("new", 1, 30).await {
        Ok(p) => p.posts,
        Err(e) => {
            warn!("Failed to fetch Clacker News posts: {}", e);
            return Ok(());
        }
    };

    let mut upvotes = 0;
    let mut comments = 0;

    for post in posts {
        // Skip our own posts
        if post.author.as_deref() == Some("caturiel") {
            continue;
        }

        // Mark as seen
        state.mark_clacker_post_seen(post.id, &post.title, post.url.as_deref())?;

        // Skip if we've already engaged
        if state.has_upvoted(post.id)? && state.has_commented(post.id)? {
            continue;
        }

        // Upvote decision
        if upvotes < MAX_UPVOTES && !state.has_upvoted(post.id)? {
            if state.upvotes_this_cycle()? < MAX_UPVOTES {
                match decisions.should_upvote(&post).await {
                    Ok(true) => {
                        match clacker.upvote_post(post.id).await {
                            Ok(_) => {
                                info!("Upvoted: {}", post.title);
                                state.mark_upvoted(post.id)?;
                                notify::notify(&config.ntfy_topic, &format!("Upvoted: {}", post.title))
                                    .await?;
                                upvotes += 1;
                                // Don't comment on posts we upvote in the same cycle
                                continue;
                            }
                            Err(e) => warn!("Upvote failed: {}", e),
                        }
                    }
                    Ok(false) => {}
                    Err(e) => warn!("Upvote decision error: {}", e),
                }
            }
        }

        // Comment decision
        if comments < MAX_COMMENTS && !state.has_commented(post.id)? {
            if state.comments_this_cycle()? < MAX_COMMENTS {
                // Get full post with comments
                let full_post = match clacker.get_post(post.id).await {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("Failed to get full post: {}", e);
                        continue;
                    }
                };

                // Skip if we already commented (server-side check)
                let already_commented = full_post.comments.iter().any(|c| {
                    c.author.as_deref() == Some("caturiel")
                });
                if already_commented {
                    state.mark_commented(post.id, "(already commented)")?;
                    continue;
                }

                match decisions.should_comment(&full_post).await {
                    Ok(Some(angle)) => {
                        let comment_text = match decisions.generate_comment(&full_post, &angle).await
                        {
                            Ok(c) => c,
                            Err(e) => {
                                warn!("Comment generation error: {}", e);
                                continue;
                            }
                        };

                        match clacker.comment(post.id, &comment_text, None).await {
                            Ok(_) => {
                                info!("Commented on: {}", post.title);
                                state.mark_commented(post.id, &comment_text)?;
                                notify::notify(
                                    &config.ntfy_topic,
                                    &format!("Commented on: {}", post.title),
                                )
                                .await?;
                                comments += 1;
                            }
                            Err(e) => warn!("Comment failed: {}", e),
                        }
                    }
                    Ok(None) => {}
                    Err(e) => warn!("Comment decision error: {}", e),
                }
            }
        }
    }

    info!(
        "Clacker engagement complete: {} upvotes, {} comments",
        upvotes, comments
    );
    Ok(())
}
