use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use poise::serenity_prelude::UserId;
use serde::Deserialize;
use tokio::sync::Mutex;
use tracing::warn;

use crate::i18n::fmt_duration;
use crate::state::Error;
use crate::youtube::{self, Query};

/// Discord's limit for autocomplete choice names and string values.
const CHOICE_MAX_CHARS: usize = 100;
const MAX_CHOICES: usize = 10;
const MIN_QUERY_CHARS: usize = 2;
/// Wait this long before searching; if the user typed again meanwhile, skip this request.
const DEBOUNCE: Duration = Duration::from_millis(350);
/// Discord drops autocomplete answers after 3 s.
const REQUEST_TIMEOUT: Duration = Duration::from_millis(2000);
const CACHE_TTL: Duration = Duration::from_secs(600);
const CACHE_MAX_ENTRIES: usize = 500;
/// After a Data API failure (e.g. daily quota exceeded), use only suggestions for this long.
const API_BACKOFF: Duration = Duration::from_secs(3600);

/// One autocomplete entry: `name` is shown, `value` is what `/play` receives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub name: String,
    pub value: String,
}

impl Choice {
    fn new(name: &str, value: &str) -> Self {
        Self {
            name: truncate(name, CHOICE_MAX_CHARS),
            value: truncate(value, CHOICE_MAX_CHARS),
        }
    }
}

/// Autocomplete for `/play`: YouTube Data API v3 videos when a key is configured,
/// otherwise (or when the API fails) YouTube's public search suggestions.
pub struct Search {
    http: reqwest::Client,
    api_key: Option<String>,
    cache: Mutex<HashMap<String, (Instant, Vec<Choice>)>>,
    latest: Mutex<HashMap<UserId, u64>>,
    sequence: AtomicU64,
    api_disabled_until: Mutex<Option<Instant>>,
}

impl Search {
    pub fn new(http: reqwest::Client, api_key: Option<String>) -> Self {
        Self {
            http,
            api_key,
            cache: Mutex::default(),
            latest: Mutex::default(),
            sequence: AtomicU64::new(0),
            api_disabled_until: Mutex::default(),
        }
    }

    pub async fn autocomplete(&self, user: UserId, partial: &str) -> Vec<Choice> {
        let partial = partial.trim();
        match youtube::parse_query(partial) {
            // A pasted link is played as-is.
            Ok(Query::Video(url)) => return vec![Choice::new(&url, &url)],
            Ok(Query::Search(_)) if partial.chars().count() >= MIN_QUERY_CHARS => {}
            _ => return Vec::new(),
        }

        let key = partial.to_lowercase();
        if let Some(choices) = self.cached(&key).await {
            return choices;
        }

        if !self.still_latest_after_debounce(user).await {
            return Vec::new();
        }

        let choices = self.fetch(partial).await;
        if !choices.is_empty() {
            self.store(key, choices.clone()).await;
        }
        choices
    }

    /// Discord sends one request per keystroke; only the last one in a burst does real work.
    async fn still_latest_after_debounce(&self, user: UserId) -> bool {
        let id = self.sequence.fetch_add(1, Ordering::Relaxed);
        self.latest.lock().await.insert(user, id);
        tokio::time::sleep(DEBOUNCE).await;
        self.latest.lock().await.get(&user) == Some(&id)
    }

    async fn fetch(&self, query: &str) -> Vec<Choice> {
        if let Some(key) = self.api_key.as_deref()
            && self.api_enabled().await
        {
            match self.api_search(key, query).await {
                Ok(choices) => return choices,
                Err(error) => {
                    warn!(%error, "YouTube Data API failed; using suggestions for a while");
                    *self.api_disabled_until.lock().await = Some(Instant::now() + API_BACKOFF);
                }
            }
        }
        match self.suggestions(query).await {
            Ok(choices) => choices,
            Err(error) => {
                warn!(%error, "YouTube suggestions failed");
                Vec::new()
            }
        }
    }

    async fn api_enabled(&self) -> bool {
        let mut until = self.api_disabled_until.lock().await;
        match *until {
            Some(t) if Instant::now() < t => false,
            Some(_) => {
                *until = None;
                true
            }
            None => true,
        }
    }

    /// `search.list` (100 quota units) + `videos.list` for durations (1 unit).
    async fn api_search(&self, key: &str, query: &str) -> Result<Vec<Choice>, Error> {
        let max = MAX_CHOICES.to_string();
        let search: ApiSearch = get_json(
            self.http
                .get("https://www.googleapis.com/youtube/v3/search")
                .query(&[
                    ("part", "snippet"),
                    ("type", "video"),
                    ("maxResults", max.as_str()),
                    ("q", query),
                    ("key", key),
                ]),
        )
        .await?;

        let ids: Vec<&str> = search
            .items
            .iter()
            .map(|i| i.id.video_id.as_str())
            .collect();
        let durations = self.api_durations(key, &ids).await.unwrap_or_else(|error| {
            warn!(%error, "could not fetch video durations");
            HashMap::new()
        });

        Ok(search
            .items
            .iter()
            .map(|item| {
                let id = &item.id.video_id;
                let title = unescape_html(&item.snippet.title);
                Choice::new(
                    &video_label(&title, durations.get(id).copied()),
                    &youtube::watch_url(id),
                )
            })
            .collect())
    }

    async fn api_durations(
        &self,
        key: &str,
        ids: &[&str],
    ) -> Result<HashMap<String, Duration>, Error> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let ids = ids.join(",");
        let videos: ApiVideos = get_json(
            self.http
                .get("https://www.googleapis.com/youtube/v3/videos")
                .query(&[
                    ("part", "contentDetails"),
                    ("id", ids.as_str()),
                    ("key", key),
                ]),
        )
        .await?;
        Ok(videos
            .items
            .into_iter()
            .filter_map(|v| Some((v.id, parse_iso8601_duration(&v.content_details.duration)?)))
            .collect())
    }

    /// Public YouTube search suggestions: fast, no key, returns query strings.
    async fn suggestions(&self, query: &str) -> Result<Vec<Choice>, Error> {
        let body: serde_json::Value = get_json(
            self.http
                .get("https://suggestqueries.google.com/complete/search")
                .query(&[("client", "firefox"), ("ds", "yt"), ("q", query)]),
        )
        .await?;
        Ok(parse_suggestions(&body))
    }

    async fn cached(&self, key: &str) -> Option<Vec<Choice>> {
        let cache = self.cache.lock().await;
        let (at, choices) = cache.get(key)?;
        (at.elapsed() < CACHE_TTL).then(|| choices.clone())
    }

    async fn store(&self, key: String, choices: Vec<Choice>) {
        let mut cache = self.cache.lock().await;
        if cache.len() >= CACHE_MAX_ENTRIES {
            cache.retain(|_, (at, _)| at.elapsed() < CACHE_TTL);
            if cache.len() >= CACHE_MAX_ENTRIES {
                cache.clear();
            }
        }
        cache.insert(key, (Instant::now(), choices));
    }
}

/// Sends `request` and decodes JSON. Errors never contain the URL, which may carry the API key.
async fn get_json<T: serde::de::DeserializeOwned>(
    request: reqwest::RequestBuilder,
) -> Result<T, reqwest::Error> {
    async {
        request
            .timeout(REQUEST_TIMEOUT)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
    }
    .await
    .map_err(reqwest::Error::without_url)
}

#[derive(Deserialize)]
struct ApiSearch {
    #[serde(default)]
    items: Vec<ApiSearchItem>,
}

#[derive(Deserialize)]
struct ApiSearchItem {
    id: ApiVideoId,
    snippet: ApiSnippet,
}

#[derive(Deserialize)]
struct ApiVideoId {
    #[serde(rename = "videoId")]
    video_id: String,
}

#[derive(Deserialize)]
struct ApiSnippet {
    title: String,
}

#[derive(Deserialize)]
struct ApiVideos {
    #[serde(default)]
    items: Vec<ApiVideo>,
}

#[derive(Deserialize)]
struct ApiVideo {
    id: String,
    #[serde(rename = "contentDetails")]
    content_details: ApiContentDetails,
}

#[derive(Deserialize)]
struct ApiContentDetails {
    duration: String,
}

/// Parses `["query", ["s1", "s2", ...], ...]`.
fn parse_suggestions(body: &serde_json::Value) -> Vec<Choice> {
    body.get(1)
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str())
        .take(MAX_CHOICES)
        .map(|s| Choice::new(s, s))
        .collect()
}

/// `Title · 3:45`, keeping the duration visible when the title is truncated.
fn video_label(title: &str, duration: Option<Duration>) -> String {
    match duration {
        Some(d) => {
            let suffix = format!(" · {}", fmt_duration(d));
            let room = CHOICE_MAX_CHARS - suffix.chars().count();
            format!("{}{suffix}", truncate(title, room))
        }
        None => title.to_owned(),
    }
}

/// ISO 8601 durations as used by the Data API, e.g. `PT1H2M3S`, `PT4M`, `P1DT2H`.
fn parse_iso8601_duration(s: &str) -> Option<Duration> {
    let rest = s.strip_prefix('P')?;
    let (mut secs, mut number, mut in_time) = (0u64, String::new(), false);
    for c in rest.chars() {
        match c {
            'T' => in_time = true,
            '0'..='9' => number.push(c),
            unit => {
                let n: u64 = std::mem::take(&mut number).parse().ok()?;
                secs += n * match (unit, in_time) {
                    ('D', false) => 86_400,
                    ('H', true) => 3600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
            }
        }
    }
    number.is_empty().then(|| Duration::from_secs(secs))
}

fn unescape_html(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Truncates to at most `max` chars, adding `…` when cut.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_suggestion_response() {
        let body = serde_json::json!(["daft pu", ["daft punk", "daft punk get lucky"], [], {}]);
        assert_eq!(
            parse_suggestions(&body),
            vec![
                Choice::new("daft punk", "daft punk"),
                Choice::new("daft punk get lucky", "daft punk get lucky"),
            ]
        );
        assert!(parse_suggestions(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn parses_iso8601_durations() {
        let secs = |s| parse_iso8601_duration(s).map(|d| d.as_secs());
        assert_eq!(secs("PT3M45S"), Some(225));
        assert_eq!(secs("PT1H2M3S"), Some(3723));
        assert_eq!(secs("PT4M"), Some(240));
        assert_eq!(secs("P1DT1S"), Some(86_401));
        assert_eq!(secs("P0D"), Some(0));
        assert_eq!(secs("3M"), None);
        assert_eq!(secs("PT5"), None);
    }

    #[test]
    fn unescapes_titles() {
        assert_eq!(
            unescape_html("Guns N&#39; Roses &amp; &quot;Friends&quot;"),
            "Guns N' Roses & \"Friends\""
        );
    }

    #[test]
    fn labels_keep_duration_when_truncated() {
        let long = "x".repeat(150);
        let label = video_label(&long, Some(Duration::from_secs(225)));
        assert_eq!(label.chars().count(), CHOICE_MAX_CHARS);
        assert!(label.ends_with("… · 3:45"));
        assert_eq!(video_label("Song", None), "Song");
    }

    #[test]
    fn truncates_by_chars_not_bytes() {
        assert_eq!(truncate("canción", 10), "canción");
        assert_eq!(truncate("ñññññ", 3), "ññ…");
    }

    #[tokio::test]
    async fn links_and_short_input_skip_network() {
        let search = Search::new(reqwest::Client::new(), None);
        let user = UserId::new(1);
        let url = youtube::watch_url("dQw4w9WgXcQ");
        assert_eq!(
            search
                .autocomplete(user, "https://youtu.be/dQw4w9WgXcQ")
                .await,
            vec![Choice::new(&url, &url)]
        );
        assert!(search.autocomplete(user, "a").await.is_empty());
        assert!(
            search
                .autocomplete(user, "https://example.com")
                .await
                .is_empty()
        );
    }
}
