use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use poise::serenity_prelude::{ChannelId, GuildId, UserId};
use songbird::tracks::TrackHandle;

use crate::i18n::Lang;
use crate::youtube::TrackInfo;

pub const MAX_QUEUE: usize = 100;

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

/// Playback state of one guild.
pub struct GuildPlayer {
    pub channel_id: ChannelId,
    /// Text channel where "now playing" announcements go.
    pub text_channel: ChannelId,
    pub current: Option<Track>,
    pub handle: Option<TrackHandle>,
    pub upcoming: VecDeque<Track>,
}

impl GuildPlayer {
    pub fn new(channel_id: ChannelId, text_channel: ChannelId) -> Self {
        Self {
            channel_id,
            text_channel,
            current: None,
            handle: None,
            upcoming: VecDeque::new(),
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

    /// Moves the next queued track into `current` (or clears it when the queue is empty).
    pub fn advance(&mut self) -> Option<&Track> {
        self.handle = None;
        self.current = self.upcoming.pop_front();
        self.current.as_ref()
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

    fn player() -> GuildPlayer {
        GuildPlayer::new(ChannelId::new(1), ChannelId::new(2))
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
    fn advance_moves_through_queue_then_goes_idle() {
        let mut p = player();
        p.enqueue(track(1)).unwrap();
        p.enqueue(track(2)).unwrap();
        assert_eq!(p.advance(), Some(&track(2)));
        assert!(p.upcoming.is_empty());
        assert_eq!(p.advance(), None);
        assert_eq!(p.current, None);
        // After going idle, the next /play must start immediately.
        assert_eq!(p.enqueue(track(3)), Ok(Enqueued::PlayNow));
    }
}
