use crate::config::Config;
use crate::i18n::Lang;
use std::sync::Arc;

use poise::serenity_prelude::Http;

use crate::playback::Playback;
use crate::search::Search;
use crate::youtube::YouTube;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

/// Shared state available to every command.
pub struct Data {
    pub playback: Playback,
    pub search: Search,
}

impl Data {
    pub fn new(config: Config, discord: Arc<Http>) -> Result<Self, Error> {
        let http = crate::http::client()?;
        Ok(Self {
            playback: Playback::new(
                YouTube::new(config.ytdlp_path, config.ytdlp_cookies.as_deref()),
                discord,
            ),
            search: Search::new(http, config.youtube_api_key),
        })
    }
}

/// Reply language for the user who invoked the command.
pub fn lang(ctx: Context<'_>) -> Lang {
    Lang::from_locale(ctx.locale())
}
