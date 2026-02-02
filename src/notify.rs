use anyhow::Result;
use tracing::{info, warn};

pub async fn notify(topic: &str, message: &str) -> Result<()> {
    let url = format!("https://ntfy.sh/{}", topic);
    let client = reqwest::Client::new();

    match client
        .post(&url)
        .header("Content-Type", "text/plain")
        .body(message.to_string())
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            info!("Notification sent: {}", message);
            Ok(())
        }
        Ok(resp) => {
            warn!("Notification failed with status {}: {}", resp.status(), message);
            Ok(()) // Don't fail the whole operation for notification issues
        }
        Err(e) => {
            warn!("Notification error: {}", e);
            Ok(()) // Don't fail the whole operation for notification issues
        }
    }
}
