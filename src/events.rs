use poise::serenity_prelude::{GuildId, async_trait};
use songbird::events::{Event, EventContext, EventHandler};
use songbird::tracks::PlayMode;
use tracing::warn;

/// Logs why a track failed to play (otherwise songbird drops the error silently).
pub struct LogTrackError {
    pub guild_id: GuildId,
    pub title: String,
}

#[async_trait]
impl EventHandler for LogTrackError {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(tracks) = ctx {
            for (state, _) in *tracks {
                if let PlayMode::Errored(error) = &state.playing {
                    warn!(guild_id = %self.guild_id, title = %self.title, %error, "track failed");
                }
            }
        }
        None
    }
}
