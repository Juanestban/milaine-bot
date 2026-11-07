mod commands;
mod config;
mod http;
mod i18n;
mod playback;
mod player;
mod search;
mod state;
mod ui;
mod youtube;

use poise::serenity_prelude as serenity;
use songbird::SerenityInit;
use tracing::{error, info};

use crate::config::Config;
use crate::state::{Data, Error};

async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    match error {
        poise::FrameworkError::Setup { error, .. } => panic!("failed to start bot: {error:?}"),
        poise::FrameworkError::Command { error, ctx, .. } => {
            error!(command = %ctx.command().name, ?error, "command failed");
        }
        other => {
            if let Err(e) = poise::builtins::on_error(other).await {
                error!(?e, "error while handling error");
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,milaine_bot=debug".into()),
        )
        .init();

    let config = Config::from_env()?;
    let token = config.discord_token.clone();
    let dev_guild = config.dev_guild_id;

    let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: commands::all(),
            on_error: |error| Box::pin(on_error(error)),
            ..Default::default()
        })
        .setup(move |ctx, ready, framework| {
            Box::pin(async move {
                info!(user = %ready.user.name, "connected to Discord");
                let commands = &framework.options().commands;
                match dev_guild {
                    // Guild registration is instant; global registration can take up to an hour.
                    Some(guild) => {
                        poise::builtins::register_in_guild(ctx, commands, guild).await?;
                        // Drop global copies so the dev guild doesn't show every command twice.
                        serenity::Command::set_global_commands(ctx, vec![]).await?;
                        info!(%guild, "registered commands in dev guild, cleared global ones");
                    }
                    None => {
                        poise::builtins::register_globally(ctx, commands).await?;
                        info!("registered commands globally");
                    }
                }
                Data::new(config, ctx.http.clone())
            })
        })
        .build();

    let intents =
        serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::GUILD_VOICE_STATES;

    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .register_songbird()
        .await?;

    client.start().await?;
    Ok(())
}
