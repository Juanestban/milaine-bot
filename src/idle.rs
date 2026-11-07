use std::sync::Arc;
use std::time::{Duration, Instant};

use poise::serenity_prelude::{self as serenity, ChannelId, CreateMessage, GuildId};
use songbird::Songbird;
use tracing::{info, warn};

use crate::i18n::Msg;
use crate::playback::Playback;

const CHECK_EVERY: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Stay,
    /// Nothing played for the idle timeout.
    LeaveIdle,
    /// No humans in the voice channel for the alone timeout.
    LeaveAlone,
}

#[derive(Debug, Clone, Copy)]
pub struct Timeouts {
    pub idle: Duration,
    pub alone: Duration,
}

/// Tracks for how long a guild has been idle (nothing playing; paused does not count) and alone.
#[derive(Debug, Clone, Default)]
pub struct IdleClock {
    idle_since: Option<Instant>,
    alone_since: Option<Instant>,
}

impl IdleClock {
    pub fn observe(&mut self, now: Instant, idle: bool, alone: bool, t: Timeouts) -> Verdict {
        let since = |slot: &mut Option<Instant>, active: bool| {
            if active {
                Some(now - *slot.get_or_insert(now))
            } else {
                *slot = None;
                None
            }
        };
        let alone_for = since(&mut self.alone_since, alone);
        let idle_for = since(&mut self.idle_since, idle);
        if alone_for.is_some_and(|d| d >= t.alone) {
            Verdict::LeaveAlone
        } else if idle_for.is_some_and(|d| d >= t.idle) {
            Verdict::LeaveIdle
        } else {
            Verdict::Stay
        }
    }
}

/// Periodically leaves voice channels that are idle or empty, and forgets players whose
/// connection is gone (e.g. the bot was kicked from the channel).
pub fn spawn(ctx: serenity::Context, songbird: Arc<Songbird>, playback: Playback, t: Timeouts) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(CHECK_EVERY);
        loop {
            ticker.tick().await;
            check_all(&ctx, &songbird, &playback, t).await;
        }
    });
}

async fn check_all(ctx: &serenity::Context, songbird: &Songbird, playback: &Playback, t: Timeouts) {
    let guilds: Vec<GuildId> = playback.players.lock().await.keys().copied().collect();
    let now = Instant::now();
    for guild_id in guilds {
        let bot_channel = match songbird.get(guild_id) {
            Some(call) => call.lock().await.current_channel(),
            None => None,
        };
        let Some(bot_channel) = bot_channel.map(|c| ChannelId::new(c.0.get())) else {
            // Disconnected outside our commands: drop the stale state.
            if let Some(mut player) = playback.players.lock().await.remove(&guild_id) {
                player.stop();
                info!(%guild_id, "voice connection gone, cleared player");
            }
            continue;
        };
        let alone = humans_in(ctx, guild_id, bot_channel) == 0;

        let verdict = {
            let mut players = playback.players.lock().await;
            let Some(player) = players.get_mut(&guild_id) else {
                continue;
            };
            player.channel_id = bot_channel; // May have been moved by a moderator.
            let idle = player.current.is_none();
            player.idle.observe(now, idle, alone, t)
        };
        let msg = match verdict {
            Verdict::Stay => continue,
            Verdict::LeaveIdle => Msg::LeftIdle {
                minutes: t.idle.as_secs().div_ceil(60),
            },
            Verdict::LeaveAlone => Msg::LeftAlone,
        };

        let Some(mut player) = playback.players.lock().await.remove(&guild_id) else {
            continue;
        };
        player.stop();
        if let Err(error) = songbird.remove(guild_id).await {
            warn!(%guild_id, %error, "could not leave voice channel");
        }
        info!(%guild_id, ?verdict, "left voice channel");
        let message = CreateMessage::new().content(msg.render(player.lang));
        if let Err(error) = player.text_channel.send_message(&ctx.http, message).await {
            warn!(%error, "could not announce leaving");
        }
    }
}

/// Non-bot users currently in `channel`, from the gateway cache.
fn humans_in(ctx: &serenity::Context, guild_id: GuildId, channel: ChannelId) -> usize {
    let Some(guild) = ctx.cache.guild(guild_id) else {
        return 0;
    };
    guild
        .voice_states
        .values()
        .filter(|vs| vs.channel_id == Some(channel))
        .filter(|vs| {
            let is_bot = vs
                .member
                .as_ref()
                .map(|m| m.user.bot)
                .or_else(|| guild.members.get(&vs.user_id).map(|m| m.user.bot))
                .unwrap_or(false);
            !is_bot && vs.user_id != ctx.cache.current_user().id
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Timeouts = Timeouts {
        idle: Duration::from_secs(600),
        alone: Duration::from_secs(120),
    };

    #[test]
    fn leaves_after_idle_timeout() {
        let mut clock = IdleClock::default();
        let t0 = Instant::now();
        assert_eq!(clock.observe(t0, true, false, T), Verdict::Stay);
        assert_eq!(
            clock.observe(t0 + Duration::from_secs(599), true, false, T),
            Verdict::Stay
        );
        assert_eq!(
            clock.observe(t0 + Duration::from_secs(600), true, false, T),
            Verdict::LeaveIdle
        );
    }

    #[test]
    fn playing_again_resets_the_idle_timer() {
        let mut clock = IdleClock::default();
        let t0 = Instant::now();
        clock.observe(t0, true, false, T);
        clock.observe(t0 + Duration::from_secs(500), false, false, T); // a song played
        assert_eq!(
            clock.observe(t0 + Duration::from_secs(700), true, false, T),
            Verdict::Stay,
            "idle restarted at 700 s"
        );
        assert_eq!(
            clock.observe(t0 + Duration::from_secs(1300), true, false, T),
            Verdict::LeaveIdle
        );
    }

    #[test]
    fn leaves_when_alone_even_while_playing() {
        let mut clock = IdleClock::default();
        let t0 = Instant::now();
        assert_eq!(clock.observe(t0, false, true, T), Verdict::Stay);
        assert_eq!(
            clock.observe(t0 + Duration::from_secs(120), false, true, T),
            Verdict::LeaveAlone
        );
    }

    #[test]
    fn someone_rejoining_resets_the_alone_timer() {
        let mut clock = IdleClock::default();
        let t0 = Instant::now();
        clock.observe(t0, false, true, T);
        clock.observe(t0 + Duration::from_secs(100), false, false, T);
        assert_eq!(
            clock.observe(t0 + Duration::from_secs(150), false, true, T),
            Verdict::Stay
        );
    }
}
