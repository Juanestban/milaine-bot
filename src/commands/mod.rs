mod play;

use poise::CreateReply;

use crate::i18n::Msg;
use crate::state::{Context, Data, Error, lang};

/// Every slash command the bot registers.
pub fn all() -> Vec<poise::Command<Data, Error>> {
    vec![ping(), play::play()]
}

/// Replies with `msg` in the invoking user's language.
async fn reply(ctx: Context<'_>, msg: Msg<'_>) -> Result<(), Error> {
    ctx.say(msg.render(lang(ctx))).await?;
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

/// Check that the bot is alive.
#[poise::command(
    slash_command,
    description_localized("es-ES", "Comprueba que el bot está vivo"),
    description_localized("es-419", "Comprueba que el bot está vivo")
)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    reply(ctx, Msg::Pong).await
}
