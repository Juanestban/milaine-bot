use poise::serenity_prelude::{Colour, CreateEmbed, CreateEmbedAuthor};

use crate::i18n::{Msg, fmt_clock};
use crate::player::Track;
use crate::youtube::TrackInfo;

/// Accent colour of every embed (pink, matching the bot's theme).
pub const COLOUR: Colour = Colour::new(0xF472B6);

/// Discord's limit for embed titles.
const TITLE_MAX_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackHeading {
    NowPlaying,
    AddedToQueue,
}

/// ```text
/// (requester avatar) Added to Queue 🎶
/// Song title · [03:53]          <- links to the video
/// Queue Length: 1
/// ```
pub fn track_embed(heading: TrackHeading, track: &Track, queue_len: usize) -> CreateEmbed {
    let lang = track.lang;
    let heading = match heading {
        TrackHeading::NowPlaying => Msg::NowPlayingHeading,
        TrackHeading::AddedToQueue => Msg::AddedToQueueHeading,
    };
    let mut author = CreateEmbedAuthor::new(heading.render(lang));
    if !track.requester_avatar.is_empty() {
        author = author.icon_url(&track.requester_avatar);
    }
    CreateEmbed::new()
        .colour(COLOUR)
        .author(author)
        .title(track_title(&track.info))
        .url(&track.info.url)
        .description(Msg::QueueLength { len: queue_len }.render(lang))
}

/// `Song title · [03:53]`, truncating the title so the duration always fits.
pub fn track_title(info: &TrackInfo) -> String {
    let clock = info.duration.map_or_else(|| "LIVE".to_owned(), fmt_clock);
    let suffix = format!(" · [{clock}]");
    let room = TITLE_MAX_CHARS - suffix.chars().count();
    let title = if info.title.chars().count() > room {
        let mut t: String = info.title.chars().take(room - 1).collect();
        t.push('…');
        t
    } else {
        info.title.clone()
    };
    format!("{title}{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn info(title: &str, secs: Option<u64>) -> TrackInfo {
        TrackInfo {
            title: title.into(),
            url: "https://www.youtube.com/watch?v=FGBhQbmPwH8".into(),
            duration: secs.map(Duration::from_secs),
            thumbnail: None,
        }
    }

    #[test]
    fn title_has_middle_dot_and_bracketed_clock() {
        assert_eq!(
            track_title(&info("Lobo-hombre en París", Some(233))),
            "Lobo-hombre en París · [03:53]"
        );
        assert_eq!(track_title(&info("Stream", None)), "Stream · [LIVE]");
    }

    #[test]
    fn long_titles_keep_the_duration() {
        let t = track_title(&info(&"x".repeat(300), Some(3723)));
        assert_eq!(t.chars().count(), TITLE_MAX_CHARS);
        assert!(t.ends_with("… · [01:02:03]"));
    }
}
