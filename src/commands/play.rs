use poise::serenity_prelude::{ChannelId, GuildId, Mentionable, UserId};
use songbird::events::{Event, TrackEvent};
use tracing::{info, warn};

use super::{reply, reply_private};
use crate::events::LogTrackError;
use crate::i18n::Msg;
use crate::player::{Enqueued, GuildPlayer, MAX_QUEUE, Track};
use crate::state::{Context, Error};
use crate::youtube::{self, QueryError};

/// Play a song from YouTube, or add it to the queue.
#[poise::command(
    slash_command,
    guild_only,
    description_localized("es-ES", "Reproduce una canción de YouTube o agrégala a la cola"),
    description_localized("es-419", "Reproduce una canción de YouTube o agrégala a la cola")
)]
pub async fn play(
    ctx: Context<'_>,
    #[description = "Song name or YouTube link"]
    #[name_localized("es-ES", "cancion")]
    #[name_localized("es-419", "cancion")]
    #[description_localized("es-ES", "Nombre de la canción o link de YouTube")]
    #[description_localized("es-419", "Nombre de la canción o link de YouTube")]
    query: String,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("/play is guild only")?;

    let query = match youtube::parse_query(&query) {
        Ok(q) => q,
        Err(QueryError::Empty) => return reply_private(ctx, Msg::EmptyQuery).await,
        Err(QueryError::UnsupportedLink) => return reply_private(ctx, Msg::UnsupportedLink).await,
    };

    let Some(user_channel) = voice_channel_of(ctx, guild_id, ctx.author().id) else {
        return reply_private(ctx, Msg::NotInVoice).await;
    };

    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or("songbird is not registered")?;

    let bot_channel = match manager.get(guild_id) {
        Some(call) => call.lock().await.current_channel(),
        None => None,
    };
    match bot_channel {
        Some(current) if current.0.get() != user_channel.get() => {
            let channel = ChannelId::new(current.0.get()).mention().to_string();
            return reply_private(ctx, Msg::BusyElsewhere { channel: &channel }).await;
        }
        Some(_) => {}
        // Not connected: drop any stale state left by a previous session.
        None => {
            ctx.data().players.lock().await.remove(&guild_id);
        }
    }

    // Resolving spawns yt-dlp and can take longer than Discord's 3 s reply window.
    ctx.defer().await?;

    let info = match ctx.data().youtube.resolve(&query).await {
        Ok(info) => info,
        Err(error) => {
            warn!(?query, %error, "could not resolve track");
            return reply(ctx, Msg::ResolveFailed).await;
        }
    };

    let call = manager.join(guild_id, user_channel).await?;
    if let Err(error) = call.lock().await.deafen(true).await {
        warn!(%error, "could not self-deafen");
    }

    let track = Track {
        info,
        requested_by: ctx.author().id,
    };

    let mut players = ctx.data().players.lock().await;
    let player = players
        .entry(guild_id)
        .or_insert_with(|| GuildPlayer::new(user_channel));
    player.channel_id = user_channel;

    let (title, url, duration) = (
        track.info.title.clone(),
        track.info.url.clone(),
        track.info.duration,
    );

    match player.enqueue(track) {
        Ok(Enqueued::PlayNow) => {
            let input = match ctx.data().youtube.input(&url) {
                Ok(input) => input,
                Err(error) => {
                    warn!(%error, "could not start stream");
                    player.current = None;
                    return reply(ctx, Msg::ResolveFailed).await;
                }
            };
            let handle = call.lock().await.play_only_input(input);
            handle.add_event(
                Event::Track(TrackEvent::Error),
                LogTrackError {
                    guild_id,
                    title: title.clone(),
                },
            )?;
            player.handle = Some(handle);
            info!(%guild_id, %title, "now playing");
            reply(
                ctx,
                Msg::NowPlaying {
                    title: &title,
                    url: &url,
                    duration,
                },
            )
            .await
        }
        Ok(Enqueued::Queued(position)) => {
            info!(%guild_id, %title, position, "queued");
            reply(
                ctx,
                Msg::Queued {
                    title: &title,
                    url: &url,
                    position,
                },
            )
            .await
        }
        Err(_) => reply(ctx, Msg::QueueFull { max: MAX_QUEUE }).await,
    }
}

/// The voice channel `user` is currently in, from the gateway cache.
fn voice_channel_of(ctx: Context<'_>, guild_id: GuildId, user: UserId) -> Option<ChannelId> {
    let guild = ctx.serenity_context().cache.guild(guild_id)?;
    guild.voice_states.get(&user)?.channel_id
}
