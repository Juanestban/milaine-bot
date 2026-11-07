mod control;
mod play;

use std::sync::Arc;

use poise::CreateReply;
use poise::serenity_prelude::{ChannelId, CreateEmbed, GuildId, Mentionable, UserId};
use songbird::Call;
use tokio::sync::Mutex;

use crate::i18n::Msg;
use crate::state::{Context, Data, Error, lang};

/// Every slash command the bot registers.
pub fn all() -> Vec<poise::Command<Data, Error>> {
    vec![
        play::play(),
        control::pause(),
        control::resume(),
        control::next(),
        control::prev(),
        control::replay(),
        control::stop(),
        control::disconnect(),
        control::queue(),
        ping(),
    ]
}

/// Replies with `msg` in the invoking user's language.
async fn reply(ctx: Context<'_>, msg: Msg<'_>) -> Result<(), Error> {
    ctx.say(msg.render(lang(ctx))).await?;
    Ok(())
}

/// Replies with an embed.
async fn reply_embed(ctx: Context<'_>, embed: CreateEmbed) -> Result<(), Error> {
    ctx.send(CreateReply::default().embed(embed)).await?;
    Ok(())
}

/// Replies with `msg` visible only to the invoking user (ignored after a deferred reply).
async fn reply_private(ctx: Context<'_>, msg: Msg<'_>) -> Result<(), Error> {
    ctx.send(
        CreateReply::default()
            .content(msg.render(lang(ctx)))
            .ephemeral(true),
    )
    .await?;
    Ok(())
}

/// The voice channel `user` is currently in, from the gateway cache.
fn voice_channel_of(ctx: Context<'_>, guild_id: GuildId, user: UserId) -> Option<ChannelId> {
    let guild = ctx.serenity_context().cache.guild(guild_id)?;
    guild.voice_states.get(&user)?.channel_id
}

/// The bot's voice call, if the invoking user is in the same voice channel.
/// Otherwise replies with why not and returns `None`.
async fn controlled_call(ctx: Context<'_>) -> Result<Option<(GuildId, Arc<Mutex<Call>>)>, Error> {
    let guild_id = ctx.guild_id().ok_or("music commands are guild only")?;
    let manager = songbird::get(ctx.serenity_context())
        .await
        .ok_or("songbird is not registered")?;

    let bot_channel = match manager.get(guild_id) {
        Some(call) => {
            let channel = call.lock().await.current_channel();
            channel.map(|c| (ChannelId::new(c.0.get()), call))
        }
        None => None,
    };
    let Some((bot_channel, call)) = bot_channel else {
        reply_private(ctx, Msg::NothingPlaying).await?;
        return Ok(None);
    };
    if voice_channel_of(ctx, guild_id, ctx.author().id) != Some(bot_channel) {
        let channel = bot_channel.mention().to_string();
        reply_private(ctx, Msg::NotWithBot { channel: &channel }).await?;
        return Ok(None);
    }
    Ok(Some((guild_id, call)))
}

/// Check that the bot is alive.
#[poise::command(
    slash_command,
    description_localized("es-ES", "Comprueba que el bot está vivo"),
    description_localized("es-419", "Comprueba que el bot está vivo")
)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    reply(ctx, Msg::Pong).await
}
