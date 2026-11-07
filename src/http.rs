use std::time::Duration;

/// Neutral User-Agent for every outgoing HTTP request.
///
/// Never put personal data (e.g. an email address) in this or any other header.
pub const USER_AGENT: &str = concat!("milaine-bot/", env!("CARGO_PKG_VERSION"));

/// Builds the shared HTTP client used for YouTube lookups and passed to songbird.
pub fn client() -> reqwest::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(Duration::from_secs(10))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_has_no_personal_data() {
        assert!(USER_AGENT.starts_with("milaine-bot/"));
        assert!(!USER_AGENT.contains('@'));
    }
}
