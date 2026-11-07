use std::path::Path;
use std::time::Duration;

use reqwest::Url;
use songbird::input::{Input, YoutubeDl};

use crate::state::Error;

/// What the user asked `/play` for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    /// A single YouTube video, normalized to `https://www.youtube.com/watch?v=<id>`.
    Video(String),
    /// Free text to search on YouTube.
    Search(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum QueryError {
    #[error("empty query")]
    Empty,
    #[error("only YouTube video links are supported")]
    UnsupportedLink,
}

/// Turns user input into a [`Query`]: YouTube links become [`Query::Video`] (playlist params are
/// dropped), other links are rejected, and anything else is a search.
pub fn parse_query(input: &str) -> Result<Query, QueryError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(QueryError::Empty);
    }

    let url = if input.starts_with("http://") || input.starts_with("https://") {
        Url::parse(input).ok()
    } else if looks_like_youtube_host(input) {
        Url::parse(&format!("https://{input}")).ok()
    } else {
        None
    };

    match url {
        Some(url) => video_id(&url)
            .map(|id| Query::Video(watch_url(&id)))
            .ok_or(QueryError::UnsupportedLink),
        None if input.starts_with("http://") || input.starts_with("https://") => {
            Err(QueryError::UnsupportedLink)
        }
        None => Ok(Query::Search(input.to_owned())),
    }
}

pub fn watch_url(id: &str) -> String {
    format!("https://www.youtube.com/watch?v={id}")
}

fn looks_like_youtube_host(input: &str) -> bool {
    [
        "youtu.be/",
        "youtube.com/",
        "www.youtube.com/",
        "m.youtube.com/",
        "music.youtube.com/",
    ]
    .iter()
    .any(|prefix| input.starts_with(prefix))
}

fn video_id(url: &Url) -> Option<String> {
    let host = url.host_str()?;
    let host = host.strip_prefix("www.").unwrap_or(host);
    let host = host.strip_prefix("m.").unwrap_or(host);
    let mut segments = url.path_segments()?;

    let id = match host {
        "youtu.be" => segments.next()?.to_owned(),
        "youtube.com" | "music.youtube.com" | "youtube-nocookie.com" => match segments.next()? {
            "watch" => url.query_pairs().find(|(k, _)| k == "v")?.1.into_owned(),
            "shorts" | "live" | "embed" => segments.next()?.to_owned(),
            _ => return None,
        },
        _ => return None,
    };

    is_video_id(&id).then_some(id)
}

fn is_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Metadata of a resolved YouTube video.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackInfo {
    pub title: String,
    pub url: String,
    pub duration: Option<Duration>,
    pub thumbnail: Option<String>,
}

/// Resolves queries and builds playable inputs through `yt-dlp`.
#[derive(Clone)]
pub struct YouTube {
    http: reqwest::Client,
    ytdlp_args: Vec<String>,
}

impl YouTube {
    pub fn new(http: reqwest::Client, cookies: Option<&Path>) -> Self {
        let ytdlp_args = cookies
            .map(|path| vec!["--cookies".to_owned(), path.display().to_string()])
            .unwrap_or_default();
        Self { http, ytdlp_args }
    }

    fn ytdl(&self, query: &Query) -> YoutubeDl<'static> {
        let ytdl = match query {
            Query::Video(url) => YoutubeDl::new(self.http.clone(), url.clone()),
            Query::Search(text) => YoutubeDl::new_search(self.http.clone(), text.clone()),
        };
        ytdl.user_args(self.ytdlp_args.clone())
    }

    /// Looks up the first matching video. Takes a few seconds (spawns yt-dlp).
    pub async fn resolve(&self, query: &Query) -> Result<TrackInfo, Error> {
        let meta = self
            .ytdl(query)
            .search(Some(1))
            .await?
            .next()
            .ok_or("yt-dlp returned no results")?;
        let url = meta.source_url.ok_or("yt-dlp returned no video url")?;
        Ok(TrackInfo {
            title: meta.title.unwrap_or_else(|| url.clone()),
            url,
            duration: meta.duration,
            thumbnail: meta.thumbnail,
        })
    }

    /// A lazy audio input; the stream URL is fetched when playback starts so it never expires
    /// while waiting in the queue.
    pub fn input(&self, url: &str) -> Input {
        self.ytdl(&Query::Video(url.to_owned())).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "dQw4w9WgXcQ";

    fn video() -> Result<Query, QueryError> {
        Ok(Query::Video(watch_url(ID)))
    }

    #[test]
    fn parses_watch_links() {
        assert_eq!(
            parse_query(&format!("https://www.youtube.com/watch?v={ID}")),
            video()
        );
        assert_eq!(
            parse_query(&format!("https://m.youtube.com/watch?v={ID}&t=42")),
            video()
        );
        assert_eq!(
            parse_query(&format!("https://music.youtube.com/watch?v={ID}")),
            video()
        );
        assert_eq!(
            parse_query(&format!("  youtube.com/watch?v={ID}  ")),
            video()
        );
    }

    #[test]
    fn parses_short_and_alternative_links() {
        assert_eq!(
            parse_query(&format!("https://youtu.be/{ID}?si=abc")),
            video()
        );
        assert_eq!(parse_query(&format!("youtu.be/{ID}")), video());
        assert_eq!(
            parse_query(&format!("https://www.youtube.com/shorts/{ID}")),
            video()
        );
        assert_eq!(
            parse_query(&format!("https://www.youtube.com/live/{ID}")),
            video()
        );
    }

    #[test]
    fn strips_playlist_from_video_links() {
        assert_eq!(
            parse_query(&format!(
                "https://www.youtube.com/watch?v={ID}&list=PL123&index=3"
            )),
            video()
        );
    }

    #[test]
    fn rejects_non_video_links() {
        let unsupported = Err(QueryError::UnsupportedLink);
        assert_eq!(
            parse_query("https://www.youtube.com/playlist?list=PL123"),
            unsupported
        );
        assert_eq!(
            parse_query("https://open.spotify.com/track/abc"),
            unsupported
        );
        assert_eq!(
            parse_query("https://www.youtube.com/watch?v=short"),
            unsupported
        );
        assert_eq!(parse_query("https://"), unsupported);
    }

    #[test]
    fn plain_text_is_a_search() {
        assert_eq!(
            parse_query("  daft punk one more time "),
            Ok(Query::Search("daft punk one more time".into()))
        );
        assert_eq!(parse_query("   "), Err(QueryError::Empty));
    }
}
