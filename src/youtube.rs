use std::collections::HashMap;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use reqwest::Url;
use serde::Deserialize;
use songbird::input::{ChildContainer, Input, RawAdapter};
use symphonia::core::io::ReadOnlySource;

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

/// Direct audio stream found by yt-dlp. Expires after a few hours, so it is only used to
/// start playback right away; queued tracks are resolved again when their turn comes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stream {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub info: TrackInfo,
    pub stream: Option<Stream>,
}

/// Resolves queries and builds playable inputs through `yt-dlp` and `ffmpeg`.
#[derive(Clone)]
pub struct YouTube {
    ytdlp: String,
    ytdlp_args: Vec<String>,
}

impl YouTube {
    pub fn new(ytdlp: String, cookies: Option<&Path>) -> Self {
        let ytdlp_args = cookies
            .map(|path| vec!["--cookies".to_owned(), path.display().to_string()])
            .unwrap_or_default();
        Self { ytdlp, ytdlp_args }
    }

    /// Finds the first matching video and its audio stream with a single yt-dlp run (~4-5 s).
    pub async fn resolve(&self, query: &Query) -> Result<Resolved, Error> {
        let target = match query {
            Query::Video(url) => url.clone(),
            Query::Search(text) => format!("ytsearch1:{text}"),
        };
        let output = tokio::process::Command::new(&self.ytdlp)
            .args(&self.ytdlp_args)
            .args(["--dump-json", "--format", AUDIO_FORMAT, "--no-playlist"])
            .args(["--no-warnings", &target])
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| format!("could not start {}: {e}", self.ytdlp))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("yt-dlp failed: {}", stderr.trim()).into());
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let line = stdout.lines().find(|l| !l.trim().is_empty());
        parse_ytdlp_json(line.ok_or("yt-dlp returned no results")?)
    }

    /// Starts decoding a track to raw 48 kHz stereo PCM.
    ///
    /// With a resolved `stream`, ffmpeg reads it directly using yt-dlp's headers (first audio in
    /// under a second). Without one, `yt-dlp -o - | ffmpeg` downloads it (slower, always works).
    /// Note: songbird's own HTTP fetch of the stream URL gets `403 Forbidden`; ffmpeg doesn't.
    pub fn input(&self, track_url: &str, stream: Option<&Stream>) -> Result<Input, Error> {
        let children = match stream {
            Some(stream) => vec![spawn_ffmpeg(&direct_input_args(stream), Stdio::null())?],
            None => self.spawn_piped(track_url)?,
        };
        // ChildContainer reads the last process (ffmpeg) and kills all of them on drop.
        let pcm = ReadOnlySource::new(ChildContainer::from(children));
        Ok(RawAdapter::new(pcm, SAMPLE_RATE, CHANNELS).into())
    }

    fn spawn_piped(&self, track_url: &str) -> Result<Vec<Child>, Error> {
        let mut ytdlp = Command::new(&self.ytdlp)
            .args(&self.ytdlp_args)
            .args(["--format", AUDIO_FORMAT, "--output", "-", "--no-playlist"])
            .args(["--quiet", "--no-warnings", track_url])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("could not start {}: {e}", self.ytdlp))?;
        let audio = ytdlp.stdout.take().ok_or("yt-dlp has no stdout")?;
        let input = ["-i".to_owned(), "pipe:0".to_owned()];
        match spawn_ffmpeg(&input, Stdio::from(audio)) {
            Ok(ffmpeg) => Ok(vec![ytdlp, ffmpeg]),
            Err(e) => {
                let _ = ytdlp.kill();
                Err(e)
            }
        }
    }
}

const FFMPEG: &str = "ffmpeg";
const AUDIO_FORMAT: &str = "bestaudio[acodec=opus]/bestaudio/best";
const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u32 = 2;

/// Runs ffmpeg on `input_args`, writing interleaved little-endian f32 PCM (what `RawAdapter`
/// reads) to stdout.
fn spawn_ffmpeg(input_args: &[String], stdin: Stdio) -> Result<Child, Error> {
    Command::new(FFMPEG)
        .args(["-hide_banner", "-loglevel", "error"])
        .args(input_args)
        .args([
            "-vn",
            "-f",
            "f32le",
            "-acodec",
            "pcm_f32le",
            "-ar",
            "48000",
            "-ac",
            "2",
        ])
        .arg("pipe:1")
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start {FFMPEG}: {e}").into())
}

fn direct_input_args(stream: &Stream) -> Vec<String> {
    let mut args: Vec<String> = [
        "-reconnect",
        "1",
        "-reconnect_streamed",
        "1",
        "-reconnect_delay_max",
        "5",
    ]
    .map(String::from)
    .into();
    let headers: String = stream
        .headers
        .iter()
        .map(|(k, v)| format!("{k}: {v}\r\n"))
        .collect();
    if !headers.is_empty() {
        args.extend(["-headers".to_owned(), headers]);
    }
    args.extend(["-i".to_owned(), stream.url.clone()]);
    args
}

#[derive(Deserialize)]
struct YtDlpJson {
    title: Option<String>,
    webpage_url: Option<String>,
    duration: Option<f64>,
    thumbnail: Option<String>,
    url: Option<String>,
    #[serde(default)]
    http_headers: HashMap<String, String>,
}

fn parse_ytdlp_json(line: &str) -> Result<Resolved, Error> {
    let json: YtDlpJson = serde_json::from_str(line)?;
    let url = json.webpage_url.ok_or("yt-dlp returned no video url")?;
    let mut headers: Vec<_> = json.http_headers.into_iter().collect();
    headers.sort();
    Ok(Resolved {
        info: TrackInfo {
            title: json.title.unwrap_or_else(|| url.clone()),
            duration: json
                .duration
                .filter(|d| d.is_finite() && *d >= 0.0)
                .map(Duration::from_secs_f64),
            thumbnail: json.thumbnail,
            url,
        },
        stream: json.url.map(|url| Stream { url, headers }),
    })
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

    #[test]
    fn parses_ytdlp_json() {
        let line = r#"{"title":"One More Time","webpage_url":"https://www.youtube.com/watch?v=FGBhQbmPwH8","duration":320.5,"thumbnail":"https://i.ytimg.com/x.jpg","url":"https://rr1.googlevideo.com/videoplayback?x=1","http_headers":{"User-Agent":"Mozilla/5.0","Accept":"*/*"}}"#;
        let r = parse_ytdlp_json(line).unwrap();
        assert_eq!(r.info.title, "One More Time");
        assert_eq!(r.info.url, "https://www.youtube.com/watch?v=FGBhQbmPwH8");
        assert_eq!(r.info.duration, Some(Duration::from_millis(320_500)));
        let stream = r.stream.unwrap();
        assert_eq!(stream.url, "https://rr1.googlevideo.com/videoplayback?x=1");
        assert_eq!(
            stream.headers,
            vec![
                ("Accept".into(), "*/*".into()),
                ("User-Agent".into(), "Mozilla/5.0".into())
            ]
        );
    }

    #[test]
    fn ytdlp_json_without_stream_or_page_url() {
        let r =
            parse_ytdlp_json(r#"{"webpage_url":"https://youtu.be/x","duration":null}"#).unwrap();
        assert_eq!(r.info.title, "https://youtu.be/x");
        assert!(r.stream.is_none());
        assert!(parse_ytdlp_json(r#"{"title":"no url"}"#).is_err());
    }

    #[test]
    fn direct_input_passes_headers_to_ffmpeg() {
        let stream = Stream {
            url: "https://example/audio".into(),
            headers: vec![("User-Agent".into(), "Mozilla/5.0".into())],
        };
        let args = direct_input_args(&stream);
        let h = args.iter().position(|a| a == "-headers").unwrap();
        assert_eq!(args[h + 1], "User-Agent: Mozilla/5.0\r\n");
        assert_eq!(args[args.len() - 2..], ["-i", "https://example/audio"]);
    }
}
