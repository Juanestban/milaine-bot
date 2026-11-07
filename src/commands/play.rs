use std::sync::Arc;
use std::time::Instant;

use poise::serenity_prelude::{
    AutocompleteChoice, ChannelId, CreateAutocompleteResponse, GuildId, Mentionable,
};
use songbird::{Call, Songbird};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use super::{reply, reply_embed, reply_private, voice_channel_of};
use crate::i18n::Msg;
use crate::player::{Enqueued, GuildPlayer, MAX_QUEUE, Track};
use crate::state::{Context, Error, lang};
use crate::ui::{self, TrackHeading};
use crate::youtube::{self, QueryError, Resolved};

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
    #[autocomplete = "autocomplete_query"]
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
    let playback = &ctx.data().playback;

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
            playback.players.lock().await.remove(&guild_id);
        }
    }

    // Resolving spawns yt-dlp and can take longer than Discord's 3 s reply window.
    ctx.defer().await?;

    // Join the voice channel while yt-dlp resolves, so both waits overlap.
    let started = Instant::now();
    let (resolved, call) = tokio::join!(
        playback.youtube.resolve(&query),
        join_voice(&manager, guild_id, user_channel)
    );
    let call = call?;
    let Resolved { info, stream } = match resolved {
        Ok(resolved) => resolved,
        Err(error) => {
            warn!(?query, %error, "could not resolve track");
            return reply(ctx, Msg::ResolveFailed).await;
        }
    };
    debug!(
        elapsed_ms = started.elapsed().as_millis(),
        direct = stream.is_some(),
        "resolved"
    );

    let track = Track {
        info,
        requested_by: ctx.author().id,
        requester_avatar: ctx.author().face(),
        lang: lang(ctx),
    };

    let enqueued = {
        let mut players = playback.players.lock().await;
        let player = players
            .entry(guild_id)
            .or_insert_with(|| GuildPlayer::new(user_channel, ctx.channel_id()));
        player.channel_id = user_channel;
        player.text_channel = ctx.channel_id();
        player
            .enqueue(track.clone())
            .map(|e| (e, player.upcoming.len()))
    };

    match enqueued {
        Ok((Enqueued::PlayNow, queue_len)) => {
            if playback
                .play_current(guild_id, &call, stream)
                .await
                .is_none()
            {
                return reply(ctx, Msg::ResolveFailed).await;
            }
            let embed = ui::track_embed(TrackHeading::NowPlaying, &track, queue_len, lang(ctx));
            reply_embed(ctx, embed).await
        }
        Ok((Enqueued::Queued(position), queue_len)) => {
            info!(%guild_id, title = %track.info.title, position, "queued");
            if position == 1 {
                // It plays next: resolve it now so the switch is instant.
                playback.prefetch(guild_id, track.clone());
            }
            let embed = ui::track_embed(TrackHeading::AddedToQueue, &track, queue_len, lang(ctx));
            reply_embed(ctx, embed).await
        }
        Err(_) => reply(ctx, Msg::QueueFull { max: MAX_QUEUE }).await,
    }
}

/// Joins (or stays in) `channel`, self-deafened since the bot never listens.
async fn join_voice(
    manager: &Songbird,
    guild_id: GuildId,
    channel: ChannelId,
) -> Result<Arc<Mutex<Call>>, Error> {
    let call = manager.join(guild_id, channel).await?;
    if let Err(error) = call.lock().await.deafen(true).await {
        warn!(%error, "could not self-deafen");
    }
    Ok(call)
}

/// Live YouTube results while the user types the `/play` query.
async fn autocomplete_query(ctx: Context<'_>, partial: &str) -> CreateAutocompleteResponse {
    let choices = ctx
        .data()
        .search
        .autocomplete(ctx.author().id, partial)
        .await
        .into_iter()
        .map(|c| AutocompleteChoice::new(c.name, c.value))
        .collect();
    CreateAutocompleteResponse::new().set_choices(choices)
}
