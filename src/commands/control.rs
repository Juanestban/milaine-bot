use std::sync::Arc;

use poise::serenity_prelude::GuildId;
use songbird::Call;
use tokio::sync::Mutex;
use tracing::{info, warn};

use super::{controlled_call, reply, reply_embed, reply_private};
use crate::i18n::Msg;
use crate::player::GuildPlayer;
use crate::state::{Context, Error, lang};
use crate::ui::{self, TrackHeading};

/// Skip to the next song in the queue.
#[poise::command(
    slash_command,
    guild_only,
    description_localized("es-ES", "Salta a la siguiente canción de la cola"),
    description_localized("es-419", "Salta a la siguiente canción de la cola")
)]
pub async fn next(ctx: Context<'_>) -> Result<(), Error> {
    navigate(ctx, Step::Next).await
}

/// Go back to the previous song.
#[poise::command(
    slash_command,
    guild_only,
    description_localized("es-ES", "Vuelve a la canción anterior"),
    description_localized("es-419", "Vuelve a la canción anterior")
)]
pub async fn prev(ctx: Context<'_>) -> Result<(), Error> {
    navigate(ctx, Step::Prev).await
}

/// Restart the current song from the beginning.
#[poise::command(
    slash_command,
    guild_only,
    description_localized("es-ES", "Reinicia la canción actual desde el principio"),
    description_localized("es-419", "Reinicia la canción actual desde el principio")
)]
pub async fn replay(ctx: Context<'_>) -> Result<(), Error> {
    navigate(ctx, Step::Replay).await
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Next,
    Prev,
    Replay,
}

async fn navigate(ctx: Context<'_>, step: Step) -> Result<(), Error> {
    let Some((guild_id, call)) = controlled_call(ctx).await? else {
        return Ok(());
    };
    let playback = &ctx.data().playback;

    let stream = {
        let mut players = playback.players.lock().await;
        let Some(player) = players.get_mut(&guild_id) else {
            drop(players);
            return reply_private(ctx, Msg::NothingPlaying).await;
        };
        match step {
            Step::Next => {
                if player.current.is_none() {
                    drop(players);
                    return reply_private(ctx, Msg::NothingPlaying).await;
                }
                if player.next().is_none() {
                    drop(players);
                    return reply(ctx, Msg::SkippedQueueEmpty).await;
                }
                None
            }
            Step::Prev => {
                if player.prev().is_none() {
                    drop(players);
                    return reply_private(ctx, Msg::NoPrevious).await;
                }
                None
            }
            Step::Replay => {
                if player.current.is_none() {
                    drop(players);
                    return reply_private(ctx, Msg::NothingPlaying).await;
                }
                player.restart()
            }
        }
    };
    info!(%guild_id, ?step, "navigate");

    // Starting may need yt-dlp (unless prefetched), which can exceed 3 s.
    ctx.defer().await?;
    match playback.play_current(guild_id, &call, stream).await {
        Some(track) => {
            let queue_len = queue_len(ctx, guild_id).await;
            let embed = ui::track_embed(TrackHeading::NowPlaying, &track, queue_len, lang(ctx));
            reply_embed(ctx, embed).await
        }
        None => reply(ctx, Msg::ResolveFailed).await,
    }
}

/// Pause the music, or resume it if it is already paused.
#[poise::command(
    slash_command,
    guild_only,
    description_localized("es-ES", "Pausa la música, o la reanuda si ya está en pausa"),
    description_localized("es-419", "Pausa la música, o la reanuda si ya está en pausa")
)]
pub async fn pause(ctx: Context<'_>) -> Result<(), Error> {
    set_paused(ctx, None).await
}

/// Resume the paused music.
#[poise::command(
    slash_command,
    guild_only,
    description_localized("es-ES", "Reanuda la música en pausa"),
    description_localized("es-419", "Reanuda la música en pausa")
)]
pub async fn resume(ctx: Context<'_>) -> Result<(), Error> {
    set_paused(ctx, Some(false)).await
}

/// `None` toggles.
async fn set_paused(ctx: Context<'_>, want: Option<bool>) -> Result<(), Error> {
    let Some((guild_id, _)) = controlled_call(ctx).await? else {
        return Ok(());
    };
    let msg = {
        let mut players = ctx.data().playback.players.lock().await;
        match players.get_mut(&guild_id) {
            Some(GuildPlayer {
                handle: Some(handle),
                paused,
                ..
            }) => {
                let pause = want.unwrap_or(!*paused);
                if !pause && !*paused {
                    Msg::NotPaused
                } else {
                    if pause { handle.pause() } else { handle.play() }?;
                    *paused = pause;
                    if pause { Msg::Paused } else { Msg::Resumed }
                }
            }
            _ => Msg::NothingPlaying,
        }
    };
    match msg {
        Msg::Paused | Msg::Resumed => reply(ctx, msg).await,
        _ => reply_private(ctx, msg).await,
    }
}

/// Stop the music and clear the queue (the bot stays in the channel).
#[poise::command(
    slash_command,
    guild_only,
    description_localized(
        "es-ES",
        "Detiene la música y vacía la cola (el bot se queda en el canal)"
    ),
    description_localized(
        "es-419",
        "Detiene la música y vacía la cola (el bot se queda en el canal)"
    )
)]
pub async fn stop(ctx: Context<'_>) -> Result<(), Error> {
    let Some((guild_id, _)) = controlled_call(ctx).await? else {
        return Ok(());
    };
    let stopped = {
        let mut players = ctx.data().playback.players.lock().await;
        match players.get_mut(&guild_id) {
            Some(player) if player.current.is_some() || !player.upcoming.is_empty() => {
                player.stop();
                true
            }
            _ => false,
        }
    };
    if stopped {
        info!(%guild_id, "stopped");
        reply(ctx, Msg::Stopped).await
    } else {
        reply_private(ctx, Msg::NothingPlaying).await
    }
}

/// Leave the voice channel and clear the queue.
#[poise::command(
    slash_command,
    guild_only,
    description_localized("es-ES", "Sale del canal de voz y vacía la cola"),
    description_localized("es-419", "Sale del canal de voz y vacía la cola")
)]
pub async fn disconnect(ctx: Context<'_>) -> Result<(), Error> {
    let Some((guild_id, call)) = controlled_call(ctx).await? else {
        return Ok(());
    };
    leave(ctx, guild_id, call).await?;
    reply(ctx, Msg::Disconnected).await
}

async fn leave(ctx: Context<'_>, guild_id: GuildId, call: Arc<Mutex<Call>>) -> Result<(), Error> {
    if let Some(mut player) = ctx.data().playback.players.lock().await.remove(&guild_id) {
        player.stop();
    }
    if let Err(error) = call.lock().await.leave().await {
        warn!(%error, "could not leave voice channel");
    }
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or("songbird is not registered")?;
    let _ = manager.remove(guild_id).await;
    info!(%guild_id, "disconnected");
    Ok(())
}

/// Show the current song and the queue.
#[poise::command(
    slash_command,
    guild_only,
    description_localized("es-ES", "Muestra la canción actual y la cola"),
    description_localized("es-419", "Muestra la canción actual y la cola")
)]
pub async fn queue(
    ctx: Context<'_>,
    #[description = "Page number"]
    #[name_localized("es-ES", "pagina")]
    #[name_localized("es-419", "pagina")]
    #[description_localized("es-ES", "Número de página")]
    #[description_localized("es-419", "Número de página")]
    #[min = 1]
    page: Option<u32>,
) -> Result<(), Error> {
    let guild_id = ctx.guild_id().ok_or("/queue is guild only")?;
    let embed = {
        let players = ctx.data().playback.players.lock().await;
        let player = players.get(&guild_id);
        let upcoming: Vec<_> = player
            .map(|p| p.upcoming.iter().cloned().collect())
            .unwrap_or_default();
        ui::queue_embed(
            player.and_then(|p| p.current.as_ref()),
            &upcoming,
            page.unwrap_or(1) as usize,
            player.is_some_and(|p| p.paused),
            lang(ctx),
        )
    };
    reply_embed(ctx, embed).await
}

async fn queue_len(ctx: Context<'_>, guild_id: GuildId) -> usize {
    let players = ctx.data().playback.players.lock().await;
    players.get(&guild_id).map_or(0, |p| p.upcoming.len())
}
