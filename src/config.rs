use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use poise::serenity_prelude::GuildId;

/// Runtime configuration, loaded from environment variables (and `.env` in local dev).
#[derive(Debug, Clone)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "idle timeouts are consumed by M5")
)]
pub struct Config {
    pub discord_token: String,
    pub youtube_api_key: Option<String>,
    pub idle_timeout: Duration,
    pub alone_timeout: Duration,
    pub dev_guild_id: Option<GuildId>,
    pub ytdlp_cookies: Option<PathBuf>,
    /// yt-dlp executable; a pip-installed launcher starts ~1 s faster than the standalone exe.
    pub ytdlp_path: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let non_empty = |key: &str| get(key).filter(|v| !v.trim().is_empty());

        let discord_token = non_empty("DISCORD_TOKEN")
            .context("missing DISCORD_TOKEN env var (see .env.example)")?;

        let secs = |key: &str, default: u64| -> Result<Duration> {
            match non_empty(key) {
                Some(v) => {
                    Ok(Duration::from_secs(v.parse().with_context(|| {
                        format!("{key} must be a number of seconds")
                    })?))
                }
                None => Ok(Duration::from_secs(default)),
            }
        };

        let dev_guild_id = non_empty("DEV_GUILD_ID")
            .map(|v| v.parse::<u64>().map(GuildId::new))
            .transpose()
            .context("DEV_GUILD_ID must be a numeric guild id")?;

        Ok(Self {
            discord_token,
            youtube_api_key: non_empty("YOUTUBE_API_KEY"),
            idle_timeout: secs("IDLE_TIMEOUT_SECS", 600)?,
            alone_timeout: secs("ALONE_TIMEOUT_SECS", 120)?,
            dev_guild_id,
            ytdlp_cookies: non_empty("YTDLP_COOKIES").map(PathBuf::from),
            ytdlp_path: non_empty("YTDLP_PATH").unwrap_or_else(|| "yt-dlp".to_owned()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn load(vars: &[(&str, &str)]) -> Result<Config> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(|k| map.get(k).cloned())
    }

    #[test]
    fn requires_token() {
        assert!(load(&[]).is_err());
        assert!(load(&[("DISCORD_TOKEN", "  ")]).is_err());
    }

    #[test]
    fn applies_defaults() {
        let c = load(&[("DISCORD_TOKEN", "t")]).unwrap();
        assert_eq!(c.idle_timeout, Duration::from_secs(600));
        assert_eq!(c.alone_timeout, Duration::from_secs(120));
        assert!(c.youtube_api_key.is_none());
        assert!(c.dev_guild_id.is_none());
    }

    #[test]
    fn parses_overrides() {
        let c = load(&[
            ("DISCORD_TOKEN", "t"),
            ("IDLE_TIMEOUT_SECS", "30"),
            ("DEV_GUILD_ID", "1234"),
        ])
        .unwrap();
        assert_eq!(c.idle_timeout, Duration::from_secs(30));
        assert_eq!(c.dev_guild_id, Some(GuildId::new(1234)));
    }

    #[test]
    fn rejects_bad_numbers() {
        assert!(load(&[("DISCORD_TOKEN", "t"), ("IDLE_TIMEOUT_SECS", "ten")]).is_err());
    }
}
