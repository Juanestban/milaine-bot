use std::collections::{HashMap, VecDeque};

use poise::serenity_prelude::{ChannelId, GuildId, UserId};
use songbird::tracks::TrackHandle;

use crate::youtube::TrackInfo;

pub const MAX_QUEUE: usize = 100;

/// A song in a guild's queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub info: TrackInfo,
    pub requested_by: UserId,
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
    pub current: Option<Track>,
    pub handle: Option<TrackHandle>,
    pub upcoming: VecDeque<Track>,
}

impl GuildPlayer {
    pub fn new(channel_id: ChannelId) -> Self {
        Self {
            channel_id,
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
}

/// Players of every guild the bot is in a voice channel of.
pub type Players = tokio::sync::Mutex<HashMap<GuildId, GuildPlayer>>;

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
        }
    }

    #[test]
    fn first_track_plays_now_then_queues() {
        let mut p = GuildPlayer::new(ChannelId::new(1));
        assert_eq!(p.enqueue(track(1)), Ok(Enqueued::PlayNow));
        assert_eq!(p.current, Some(track(1)));
        assert_eq!(p.enqueue(track(2)), Ok(Enqueued::Queued(1)));
        assert_eq!(p.enqueue(track(3)), Ok(Enqueued::Queued(2)));
        assert_eq!(p.upcoming.len(), 2);
    }

    #[test]
    fn rejects_when_full() {
        let mut p = GuildPlayer::new(ChannelId::new(1));
        p.enqueue(track(0)).unwrap();
        for n in 1..=MAX_QUEUE as u64 {
            p.enqueue(track(n)).unwrap();
        }
        assert_eq!(p.enqueue(track(999)), Err(QueueFull));
    }
}
