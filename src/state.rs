use crate::config::Config;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

/// Shared state available to every command.
#[expect(dead_code, reason = "used by the music commands from M1")]
pub struct Data {
    pub config: Config,
    pub http: reqwest::Client,
}

impl Data {
    pub fn new(config: Config) -> Result<Self, Error> {
        Ok(Self {
            config,
            http: crate::http::client()?,
        })
    }
}
