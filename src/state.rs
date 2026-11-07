use crate::config::Config;
use crate::i18n::Lang;
use crate::player::Players;
use crate::search::Search;
use crate::youtube::YouTube;

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

/// Shared state available to every command.
pub struct Data {
    pub youtube: YouTube,
    pub search: Search,
    pub players: Players,
}

impl Data {
    pub fn new(config: Config) -> Result<Self, Error> {
        let http = crate::http::client()?;
        Ok(Self {
            youtube: YouTube::new(http.clone(), config.ytdlp_cookies.as_deref()),
            search: Search::new(http, config.youtube_api_key),
            players: Players::default(),
        })
    }
}

/// Reply language for the user who invoked the command.
pub fn lang(ctx: Context<'_>) -> Lang {
    Lang::from_locale(ctx.locale())
}
