use std::sync::Arc;

use poise::serenity_prelude::{CreateMessage, GuildId, Http, async_trait};
use songbird::Call;
use songbird::events::{Event, EventContext, EventHandler, TrackEvent};
use songbird::tracks::{PlayMode, TrackHandle};
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

use crate::player::{Players, Track};
use crate::state::Error;
use crate::ui::{self, TrackHeading};
use crate::youtube::{Query, Stream, YouTube};

/// Starts tracks and advances each guild's queue when a track ends.
#[derive(Clone)]
pub struct Playback {
    pub players: Players,
    pub youtube: YouTube,
    http: Arc<Http>,
}

impl Playback {
    pub fn new(youtube: YouTube, http: Arc<Http>) -> Self {
        Self {
            players: Players::default(),
            youtube,
            http,
        }
    }

    /// Plays `track` on `call`, replacing whatever was playing.
    ///
    /// Callers must hold the players lock and store the returned handle in the guild's player
    /// before releasing it, so the end-of-track handler always sees the current handle.
    pub async fn start(
        &self,
        guild_id: GuildId,
        call: &Arc<Mutex<Call>>,
        track: &Track,
        stream: Option<&Stream>,
    ) -> Result<TrackHandle, Error> {
        let input = self.youtube.input(&track.info.url, stream)?;
        let handle = call.lock().await.play_only_input(input);
        let on_finish = AdvanceOnFinish {
            playback: self.clone(),
            guild_id,
            call: call.clone(),
            track_id: handle.uuid(),
            title: track.info.title.clone(),
        };
        // A track that fails fires `Error` instead of `End`; both move the queue on.
        handle.add_event(Event::Track(TrackEvent::End), on_finish.clone())?;
        handle.add_event(Event::Track(TrackEvent::Error), on_finish)?;
        info!(%guild_id, title = %track.info.title, "now playing");
        Ok(handle)
    }

    /// Called when track `finished` stops: plays the next queued track or goes idle.
    async fn advance(&self, guild_id: GuildId, call: Arc<Mutex<Call>>, finished: Uuid) {
        let mut finished = Some(finished);
        loop {
            let next = {
                let mut players = self.players.lock().await;
                let Some(player) = players.get_mut(&guild_id) else {
                    return;
                };
                // Ignore tracks that were replaced or stopped on purpose.
                if let Some(id) = finished.take()
                    && player.handle.as_ref().map(TrackHandle::uuid) != Some(id)
                {
                    return;
                }
                match player.advance() {
                    Some(track) => track.clone(),
                    None => {
                        info!(%guild_id, "queue finished, idle");
                        return;
                    }
                }
            };

            // Stream URLs expire, so queued tracks are resolved when their turn comes.
            let stream = match self
                .youtube
                .resolve(&Query::Video(next.info.url.clone()))
                .await
            {
                Ok(resolved) => resolved.stream,
                Err(error) => {
                    warn!(%error, title = %next.info.title, "re-resolve failed, using yt-dlp pipe");
                    None
                }
            };

            let mut players = self.players.lock().await;
            let Some(player) = players.get_mut(&guild_id) else {
                return;
            };
            if player.current.as_ref() != Some(&next) {
                return; // The queue changed while resolving.
            }
            match self.start(guild_id, &call, &next, stream.as_ref()).await {
                Ok(handle) => {
                    player.handle = Some(handle);
                    let embed =
                        ui::track_embed(TrackHeading::NowPlaying, &next, player.upcoming.len());
                    let channel = player.text_channel;
                    drop(players);
                    let message = CreateMessage::new().embed(embed);
                    if let Err(error) = channel.send_message(&self.http, message).await {
                        warn!(%error, "could not announce next track");
                    }
                    return;
                }
                Err(error) => warn!(%error, title = %next.info.title, "could not start next track"),
            }
        }
    }
}

#[derive(Clone)]
struct AdvanceOnFinish {
    playback: Playback,
    guild_id: GuildId,
    call: Arc<Mutex<Call>>,
    track_id: Uuid,
    title: String,
}

#[async_trait]
impl EventHandler for AdvanceOnFinish {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        if let EventContext::Track(tracks) = ctx {
            for (state, _) in *tracks {
                if let PlayMode::Errored(error) = &state.playing {
                    warn!(guild_id = %self.guild_id, title = %self.title, %error, "track failed");
                }
            }
        }
        self.playback
            .advance(self.guild_id, self.call.clone(), self.track_id)
            .await;
        None
    }
}
