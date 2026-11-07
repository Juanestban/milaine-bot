use std::sync::Arc;

use poise::serenity_prelude::{CreateMessage, GuildId, Http, async_trait};
use songbird::Call;
use songbird::events::{Event, EventContext, EventHandler, TrackEvent};
use songbird::tracks::{PlayMode, TrackHandle};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::player::{Players, Track};
use crate::state::Error;
use crate::ui::{self, TrackHeading};
use crate::youtube::{Query, Stream, YouTube};

/// Starts tracks, prefetches the next one and advances each guild's queue when a track ends.
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

    /// Starts the guild's current track, resolving it unless `stream` (or a prefetched one) is
    /// available. Tracks that fail to start are skipped. Returns the track that started.
    pub async fn play_current(
        &self,
        guild_id: GuildId,
        call: &Arc<Mutex<Call>>,
        mut stream: Option<Stream>,
    ) -> Option<Track> {
        loop {
            let (track, prefetched) = {
                let mut players = self.players.lock().await;
                let player = players.get_mut(&guild_id)?;
                let track = player.current.clone()?;
                let prefetched = player.take_prefetched(&track.info.url);
                (track, prefetched)
            };
            let stream = match stream.take().or(prefetched) {
                Some(stream) => Some(stream),
                None => self.resolve_stream(&track).await,
            };

            let mut players = self.players.lock().await;
            let player = players.get_mut(&guild_id)?;
            if player.current.as_ref() != Some(&track) || player.handle.is_some() {
                return None; // Someone changed the queue while we were resolving.
            }
            match self.start(guild_id, call, &track, stream.as_ref()).await {
                Ok(handle) => {
                    player.started(handle, stream);
                    let next = player.upcoming.front().cloned();
                    drop(players);
                    if let Some(next) = next {
                        self.prefetch(guild_id, next);
                    }
                    return Some(track);
                }
                Err(error) => {
                    warn!(%error, title = %track.info.title, "could not start track, skipping");
                    player.next();
                }
            }
        }
    }

    /// Resolves `track`'s stream in the background so it starts instantly when its turn comes.
    pub fn prefetch(&self, guild_id: GuildId, track: Track) {
        let this = self.clone();
        tokio::spawn(async move {
            let url = track.info.url.clone();
            {
                let players = this.players.lock().await;
                match players.get(&guild_id) {
                    Some(p) if !p.is_prefetched(&url) => {}
                    _ => return,
                }
            }
            if let Some(stream) = this.resolve_stream(&track).await
                && let Some(player) = this.players.lock().await.get_mut(&guild_id)
            {
                player.store_prefetched(url, stream);
                debug!(%guild_id, title = %track.info.title, "prefetched next track");
            }
        });
    }

    async fn resolve_stream(&self, track: &Track) -> Option<Stream> {
        match self
            .youtube
            .resolve(&Query::Video(track.info.url.clone()))
            .await
        {
            Ok(resolved) => resolved.stream,
            Err(error) => {
                warn!(%error, title = %track.info.title, "re-resolve failed, using yt-dlp pipe");
                None
            }
        }
    }

    /// Plays `track` on `call`, replacing whatever was playing. The caller must store the handle
    /// (via `GuildPlayer::started`) while still holding the players lock.
    async fn start(
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
        info!(%guild_id, title = %track.info.title, direct = stream.is_some(), "now playing");
        Ok(handle)
    }

    /// Called when track `finished` stops on its own: plays the next one or goes idle.
    async fn on_track_finished(&self, guild_id: GuildId, call: Arc<Mutex<Call>>, finished: Uuid) {
        {
            let mut players = self.players.lock().await;
            let Some(player) = players.get_mut(&guild_id) else {
                return;
            };
            // Tracks replaced or stopped by a command are not "finished".
            if player.handle.as_ref().map(TrackHandle::uuid) != Some(finished) {
                return;
            }
            if player.next().is_none() {
                info!(%guild_id, "queue finished, idle");
                return;
            }
        }

        let Some(track) = self.play_current(guild_id, &call, None).await else {
            return;
        };
        let (channel, queue_len) = {
            let players = self.players.lock().await;
            match players.get(&guild_id) {
                Some(p) => (p.text_channel, p.upcoming.len()),
                None => return,
            }
        };
        let embed = ui::track_embed(TrackHeading::NowPlaying, &track, queue_len, track.lang);
        if let Err(error) = channel
            .send_message(&self.http, CreateMessage::new().embed(embed))
            .await
        {
            warn!(%error, "could not announce next track");
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
            .on_track_finished(self.guild_id, self.call.clone(), self.track_id)
            .await;
        None
    }
}
