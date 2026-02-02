use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::Connection;

pub struct State {
    conn: Connection,
}

impl State {
    pub fn new(path: &str) -> Result<Self> {
        let conn = Connection::open(path).context("Failed to open database")?;
        let state = Self { conn };
        state.init_schema()?;
        Ok(state)
    }

    fn init_schema(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            -- Clacker News posts we've seen
            CREATE TABLE IF NOT EXISTS clacker_posts (
                id INTEGER PRIMARY KEY,
                title TEXT NOT NULL,
                url TEXT,
                seen_at TEXT NOT NULL,
                upvoted INTEGER DEFAULT 0,
                commented INTEGER DEFAULT 0
            );

            -- Clacker News comments we've made
            CREATE TABLE IF NOT EXISTS clacker_comments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                post_id INTEGER NOT NULL,
                comment_text TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (post_id) REFERENCES clacker_posts(id)
            );

            -- Source posts we've seen/posted (Reddit, Lobsters, etc)
            CREATE TABLE IF NOT EXISTS bluesky_posts (
                uri TEXT PRIMARY KEY,
                author_handle TEXT NOT NULL,
                post_text TEXT NOT NULL,
                seen_at TEXT NOT NULL,
                posted_to_clacker INTEGER DEFAULT 0,
                clacker_post_id INTEGER
            );

            -- Simple key-value for misc state
            CREATE TABLE IF NOT EXISTS meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            -- Indexes
            CREATE INDEX IF NOT EXISTS idx_clacker_seen_at ON clacker_posts(seen_at);
            CREATE INDEX IF NOT EXISTS idx_bluesky_seen_at ON bluesky_posts(seen_at);
            CREATE INDEX IF NOT EXISTS idx_bluesky_posted ON bluesky_posts(posted_to_clacker);
            "#,
        )?;
        Ok(())
    }

    // Clacker News

    pub fn has_seen_clacker_post(&self, id: i64) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM clacker_posts WHERE id = ?",
            [id],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn mark_clacker_post_seen(&self, id: i64, title: &str, url: Option<&str>) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO clacker_posts (id, title, url, seen_at) VALUES (?, ?, ?, ?)",
            (id, title, url, Utc::now().to_rfc3339()),
        )?;
        Ok(())
    }

    pub fn mark_upvoted(&self, id: i64) -> Result<()> {
        self.conn
            .execute("UPDATE clacker_posts SET upvoted = 1 WHERE id = ?", [id])?;
        Ok(())
    }

    pub fn mark_commented(&self, id: i64, comment: &str) -> Result<()> {
        self.conn
            .execute("UPDATE clacker_posts SET commented = 1 WHERE id = ?", [id])?;
        self.conn.execute(
            "INSERT INTO clacker_comments (post_id, comment_text, created_at) VALUES (?, ?, ?)",
            (id, comment, Utc::now().to_rfc3339()),
        )?;
        Ok(())
    }

    pub fn has_upvoted(&self, id: i64) -> Result<bool> {
        let upvoted: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(upvoted, 0) FROM clacker_posts WHERE id = ?",
                [id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(upvoted > 0)
    }

    pub fn has_commented(&self, id: i64) -> Result<bool> {
        let commented: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(commented, 0) FROM clacker_posts WHERE id = ?",
                [id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        Ok(commented > 0)
    }

    // Bluesky

    pub fn has_seen_bluesky_post(&self, uri: &str) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM bluesky_posts WHERE uri = ?",
            [uri],
            |row| row.get(0),
        )?;
        Ok(count > 0)
    }

    pub fn mark_bluesky_post_seen(&self, uri: &str, author: &str, text: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO bluesky_posts (uri, author_handle, post_text, seen_at) VALUES (?, ?, ?, ?)",
            (uri, author, text, Utc::now().to_rfc3339()),
        )?;
        Ok(())
    }

    pub fn mark_posted_to_clacker(&self, uri: &str, clacker_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE bluesky_posts SET posted_to_clacker = 1, clacker_post_id = ? WHERE uri = ?",
            (clacker_id, uri),
        )?;
        Ok(())
    }

    // Stats for rate limiting

    pub fn posts_this_cycle(&self) -> Result<i64> {
        // Posts in the last hour (covers a 30-min cycle with margin)
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM bluesky_posts WHERE posted_to_clacker = 1 AND seen_at >= datetime('now', '-1 hour')",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn comments_this_cycle(&self) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM clacker_comments WHERE created_at >= datetime('now', '-1 hour')",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn upvotes_this_cycle(&self) -> Result<i64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM clacker_posts WHERE upvoted = 1 AND seen_at >= datetime('now', '-1 hour')",
            [],
            |row| row.get(0),
        )?;
        Ok(count)
    }
}
