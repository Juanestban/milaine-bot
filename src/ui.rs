use std::time::Duration;

use poise::serenity_prelude::{Colour, CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter};

use crate::i18n::{Lang, Msg, fmt_clock};
use crate::player::Track;
use crate::youtube::TrackInfo;

/// Accent colour of every embed (pink, matching the bot's theme).
pub const COLOUR: Colour = Colour::new(0xF472B6);

/// Discord's limit for embed titles.
const TITLE_MAX_CHARS: usize = 256;
/// Titles inside the queue list are shorter so ten of them fit comfortably.
const LIST_TITLE_MAX_CHARS: usize = 70;
pub const QUEUE_PAGE_SIZE: usize = 10;

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
pub fn track_embed(
    heading: TrackHeading,
    track: &Track,
    queue_len: usize,
    lang: Lang,
) -> CreateEmbed {
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

/// Current track plus one page of the upcoming queue. `page` is 1-based and clamped.
pub fn queue_embed(
    current: Option<&Track>,
    upcoming: &[Track],
    page: usize,
    paused: bool,
    lang: Lang,
) -> CreateEmbed {
    let pages = upcoming.len().div_ceil(QUEUE_PAGE_SIZE).max(1);
    let page = page.clamp(1, pages);

    let mut description = String::new();
    match current {
        Some(track) => {
            let label = if paused {
                Msg::PausedLabel
            } else {
                Msg::NowPlayingLabel
            };
            description.push_str(&format!(
                "{} {}\n\n",
                label.render(lang),
                list_entry(&track.info)
            ));
        }
        None => description.push_str(&format!("{}\n\n", Msg::NothingPlaying.render(lang))),
    }
    if upcoming.is_empty() {
        description.push_str(&Msg::QueueEmpty.render(lang));
    } else {
        let start = (page - 1) * QUEUE_PAGE_SIZE;
        for (i, track) in upcoming
            .iter()
            .enumerate()
            .skip(start)
            .take(QUEUE_PAGE_SIZE)
        {
            description.push_str(&format!("`{}.` {}\n", i + 1, list_entry(&track.info)));
        }
    }

    let total: Duration = upcoming.iter().filter_map(|t| t.info.duration).sum();
    let footer = Msg::QueueFooter {
        page,
        pages,
        count: upcoming.len(),
        total: &fmt_clock(total),
    };
    CreateEmbed::new()
        .colour(COLOUR)
        .title(Msg::QueueHeading.render(lang))
        .description(description)
        .footer(CreateEmbedFooter::new(footer.render(lang)))
}

/// `Song title · [03:53]`, truncating the title so the duration always fits.
pub fn track_title(info: &TrackInfo) -> String {
    with_clock(&info.title, info.duration, TITLE_MAX_CHARS)
}

/// `[Song title](url) · [03:53]` for lists (brackets inside link text would break markdown).
fn list_entry(info: &TrackInfo) -> String {
    let title = truncate(&escape_link_text(&info.title), LIST_TITLE_MAX_CHARS);
    format!("[{title}]({}) · [{}]", info.url, clock(info.duration))
}

fn with_clock(title: &str, duration: Option<Duration>, max_chars: usize) -> String {
    let suffix = format!(" · [{}]", clock(duration));
    let room = max_chars - suffix.chars().count();
    format!("{}{suffix}", truncate(title, room))
}

fn clock(duration: Option<Duration>) -> String {
    duration.map_or_else(|| "LIVE".to_owned(), fmt_clock)
}

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_owned();
    }
    let mut t: String = s.chars().take(max_chars - 1).collect();
    t.push('…');
    t
}

/// Square brackets would break `[text](url)` markdown links.
fn escape_link_text(s: &str) -> String {
    s.replace('[', "(").replace(']', ")")
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn list_entries_escape_brackets() {
        assert_eq!(
            list_entry(&info("Song [Official Video]", Some(61))),
            "[Song (Official Video)](https://www.youtube.com/watch?v=FGBhQbmPwH8) · [01:01]"
        );
    }
}
