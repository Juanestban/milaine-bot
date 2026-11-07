use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};

use poise::serenity_prelude::{ChannelId, GuildId, UserId};
use songbird::tracks::TrackHandle;

use crate::i18n::Lang;
use crate::youtube::{Stream, TrackInfo};

pub const MAX_QUEUE: usize = 100;
pub const MAX_HISTORY: usize = 50;
/// Resolved stream URLs live for hours; reuse them only while comfortably fresh.
const STREAM_FRESH_FOR: Duration = Duration::from_secs(30 * 60);

/// A song in a guild's queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub info: TrackInfo,
    pub requested_by: UserId,
    pub requester_avatar: String,
    /// Language of the requester, used for announcements about this track.
    pub lang: Lang,
}

/// Result of adding a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Enqueued {
    /// Nothing was playing: the track became current and must be started now.
    PlayNow,
    /// Added to the queue at this 1-based position.
    Queued(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the queue is full ({MAX_QUEUE} songs)")]
pub struct QueueFull;

/// A stream resolved ahead of time for a specific video.
#[derive(Debug, Clone)]
struct CachedStream {
    url: String,
    stream: Stream,
    at: Instant,
}

/// Playback state of one guild.
pub struct GuildPlayer {
    pub channel_id: ChannelId,
    /// Text channel where "now playing" announcements go.
    pub text_channel: ChannelId,
    pub current: Option<Track>,
    pub handle: Option<TrackHandle>,
    pub upcoming: VecDeque<Track>,
    /// Previously played tracks, most recent last.
    pub history: VecDeque<Track>,
    pub paused: bool,
    current_stream: Option<CachedStream>,
    prefetched: Option<CachedStream>,
}

impl GuildPlayer {
    pub fn new(channel_id: ChannelId, text_channel: ChannelId) -> Self {
        Self {
            channel_id,
            text_channel,
            current: None,
            handle: None,
            upcoming: VecDeque::new(),
            history: VecDeque::new(),
            paused: false,
            current_stream: None,
            prefetched: None,
        }
    }

    pub fn enqueue(&mut self, track: Track) -> Result<Enqueued, QueueFull> {
        if self.current.is_none() {
            self.current = Some(track);
            return Ok(Enqueued::PlayNow);
        }
        if self.upcoming.len() >= MAX_QUEUE {
            return Err(QueueFull);
        }
        self.upcoming.push_back(track);
        Ok(Enqueued::Queued(self.upcoming.len()))
    }

    /// Moves to the next queued track (or none when the queue is empty).
    pub fn next(&mut self) -> Option<&Track> {
        self.stop_handle();
        if let Some(track) = self.current.take() {
            self.remember(track);
        }
        self.current = self.upcoming.pop_front();
        self.current.as_ref()
    }

    /// Goes back to the previous track; the current one returns to the front of the queue.
    /// Returns `None` (and changes nothing) when there is no history.
    pub fn prev(&mut self) -> Option<&Track> {
        let previous = self.history.pop_back()?;
        self.stop_handle();
        if let Some(track) = self.current.take() {
            self.upcoming.push_front(track);
        }
        self.current = Some(previous);
        self.current.as_ref()
    }

    /// Stops the current track so it can be started again from 0:00.
    /// Returns the stream it was using, if still fresh.
    pub fn restart(&mut self) -> Option<Stream> {
        let stream = self.current_stream.take().and_then(CachedStream::fresh);
        self.stop_handle();
        stream
    }

    /// Stops playback and clears the queue (history is kept for `/prev`).
    pub fn stop(&mut self) {
        self.stop_handle();
        if let Some(track) = self.current.take() {
            self.remember(track);
        }
        self.upcoming.clear();
        self.prefetched = None;
    }

    /// Records the handle and stream of the track that just started.
    pub fn started(&mut self, handle: TrackHandle, stream: Option<Stream>) {
        self.handle = Some(handle);
        self.paused = false;
        let url = self.current.as_ref().map(|t| t.info.url.clone());
        self.current_stream = url
            .zip(stream)
            .map(|(url, stream)| CachedStream::new(url, stream));
    }

    pub fn store_prefetched(&mut self, url: String, stream: Stream) {
        self.prefetched = Some(CachedStream::new(url, stream));
    }

    /// A fresh prefetched stream for `url`, if any.
    pub fn take_prefetched(&mut self, url: &str) -> Option<Stream> {
        match self.prefetched.take() {
            Some(c) if c.url == url => c.fresh(),
            other => {
                self.prefetched = other;
                None
            }
        }
    }

    pub fn is_prefetched(&self, url: &str) -> bool {
        self.prefetched.as_ref().is_some_and(|c| c.url == url)
    }

    fn stop_handle(&mut self) {
        if let Some(handle) = self.handle.take() {
            // Already-finished tracks return an error here; that's fine.
            let _ = handle.stop();
        }
        self.paused = false;
        self.current_stream = None;
    }

    fn remember(&mut self, track: Track) {
        self.history.push_back(track);
        if self.history.len() > MAX_HISTORY {
            self.history.pop_front();
        }
    }
}

impl CachedStream {
    fn new(url: String, stream: Stream) -> Self {
        Self {
            url,
            stream,
            at: Instant::now(),
        }
    }

    fn fresh(self) -> Option<Stream> {
        (self.at.elapsed() < STREAM_FRESH_FOR).then_some(self.stream)
    }
}

/// Players of every guild the bot is in a voice channel of.
pub type Players = Arc<tokio::sync::Mutex<HashMap<GuildId, GuildPlayer>>>;

#[cfg(test)]
mod tests {
    use super::*;

    fn track(n: u64) -> Track {
        Track {
            info: TrackInfo {
                title: format!("song {n}"),
                url: format!("https://www.youtube.com/watch?v={n:011}"),
                duration: None,
                thumbnail: None,
            },
            requested_by: UserId::new(1),
            requester_avatar: String::new(),
            lang: Lang::En,
        }
    }

    fn stream(n: u64) -> Stream {
        Stream {
            url: format!("https://googlevideo/{n}"),
            headers: Vec::new(),
        }
    }

    fn player() -> GuildPlayer {
        GuildPlayer::new(ChannelId::new(1), ChannelId::new(2))
    }

    fn titles(tracks: &VecDeque<Track>) -> Vec<&str> {
        tracks.iter().map(|t| t.info.title.as_str()).collect()
    }

    #[test]
    fn first_track_plays_now_then_queues() {
        let mut p = player();
        assert_eq!(p.enqueue(track(1)), Ok(Enqueued::PlayNow));
        assert_eq!(p.current, Some(track(1)));
        assert_eq!(p.enqueue(track(2)), Ok(Enqueued::Queued(1)));
        assert_eq!(p.enqueue(track(3)), Ok(Enqueued::Queued(2)));
        assert_eq!(p.upcoming.len(), 2);
    }

    #[test]
    fn rejects_when_full() {
        let mut p = player();
        p.enqueue(track(0)).unwrap();
        for n in 1..=MAX_QUEUE as u64 {
            p.enqueue(track(n)).unwrap();
        }
        assert_eq!(p.enqueue(track(999)), Err(QueueFull));
    }

    #[test]
    fn next_moves_through_queue_then_goes_idle() {
        let mut p = player();
        p.enqueue(track(1)).unwrap();
        p.enqueue(track(2)).unwrap();
        assert_eq!(p.next(), Some(&track(2)));
        assert!(p.upcoming.is_empty());
        assert_eq!(p.next(), None);
        assert_eq!(p.current, None);
        assert_eq!(titles(&p.history), ["song 1", "song 2"]);
        // After going idle, the next /play must start immediately.
        assert_eq!(p.enqueue(track(3)), Ok(Enqueued::PlayNow));
    }

    #[test]
    fn prev_goes_back_and_requeues_current() {
        let mut p = player();
        assert_eq!(p.prev(), None, "no history yet");
        p.enqueue(track(1)).unwrap();
        p.enqueue(track(2)).unwrap();
        p.enqueue(track(3)).unwrap();
        p.next(); // now 2
        assert_eq!(p.prev(), Some(&track(1)));
        assert_eq!(titles(&p.upcoming), ["song 2", "song 3"]);
        assert!(p.history.is_empty());
        assert_eq!(p.next(), Some(&track(2)), "forward again after going back");
    }

    #[test]
    fn prev_works_after_queue_finished() {
        let mut p = player();
        p.enqueue(track(1)).unwrap();
        p.next();
        assert_eq!(p.current, None);
        assert_eq!(p.prev(), Some(&track(1)));
    }

    #[test]
    fn stop_clears_queue_but_keeps_history() {
        let mut p = player();
        p.enqueue(track(1)).unwrap();
        p.enqueue(track(2)).unwrap();
        p.stop();
        assert_eq!(p.current, None);
        assert!(p.upcoming.is_empty());
        assert_eq!(titles(&p.history), ["song 1"]);
    }

    #[test]
    fn history_is_bounded() {
        let mut p = player();
        for n in 0..(MAX_HISTORY as u64 + 10) {
            p.enqueue(track(n)).unwrap();
            p.next();
        }
        assert_eq!(p.history.len(), MAX_HISTORY);
        assert_eq!(p.history.front(), Some(&track(10)));
    }

    #[test]
    fn prefetched_stream_only_matches_its_url() {
        let mut p = player();
        p.store_prefetched(track(2).info.url, stream(2));
        assert!(p.is_prefetched(&track(2).info.url));
        assert_eq!(p.take_prefetched(&track(3).info.url), None);
        assert_eq!(p.take_prefetched(&track(2).info.url), Some(stream(2)));
        assert_eq!(p.take_prefetched(&track(2).info.url), None, "taken once");
    }

    #[test]
    fn restart_reuses_current_stream() {
        let mut p = player();
        p.enqueue(track(1)).unwrap();
        p.current_stream = Some(CachedStream::new(track(1).info.url, stream(1)));
        assert_eq!(p.restart(), Some(stream(1)));
        assert_eq!(p.current, Some(track(1)), "still the current track");
    }
}
