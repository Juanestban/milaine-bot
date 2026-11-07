use crate::i18n::Lang;
use crate::state::{Context, Data, Error};

/// Every slash command the bot registers.
pub fn all() -> Vec<poise::Command<Data, Error>> {
    vec![ping()]
}

/// Check that the bot is alive.
#[poise::command(
    slash_command,
    description_localized("es-ES", "Comprueba que el bot está vivo"),
    description_localized("es-419", "Comprueba que el bot está vivo")
)]
pub async fn ping(ctx: Context<'_>) -> Result<(), Error> {
    let reply = match Lang::from_locale(ctx.locale()) {
        Lang::En => "Pong! 🎵",
        Lang::Es => "¡Pong! 🎵",
    };
    ctx.say(reply).await?;
    Ok(())
}
