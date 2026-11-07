# Roadmap

Requirements and progress for Milaine Bot. Tick a box only when the item is implemented, tested
(`cargo clippy` + `cargo test` green) and committed.

Legend: ✅ done · 🚧 in progress · ⬜ todo

---

## M0 — Foundation ✅

- [x] Cargo project with serenity + poise + songbird + yt-dlp stack
- [x] Config from env / `.env` (`DISCORD_TOKEN`, `DEV_GUILD_ID`, `YOUTUBE_API_KEY`, timeouts, cookies) with tests
- [x] Shared HTTP client with neutral User-Agent (no personal data in headers)
- [x] Slash command registration (dev guild = instant, otherwise global)
- [x] `/ping` health check
- [x] Locale → language detection (`es-*` → Spanish, else English)
- [x] Logo (PNG + SVG), README, CLAUDE.md, DECISIONS.md, `.env.example`, MIT license

## M1 — Play from YouTube ✅

- [x] Per-guild player state (`GuildPlayer`) stored in `Data`, safe across concurrent guilds
- [x] YouTube URL parsing: `youtube.com/watch?v=`, `youtu.be/`, `music.youtube.com`, `/shorts/`, `/live/`; strip `&list=` (unit tests)
- [x] Resolve a query or URL to track metadata (title, url, duration, thumbnail) via yt-dlp
- [x] `/play <query|url>`:
  - [x] Caller must be in a voice channel → otherwise friendly error
  - [x] Bot joins the caller's voice channel (self-deafened)
  - [x] Bot already in another channel of the guild → reject with the channel name
  - [x] Nothing playing → start immediately; otherwise enqueue and reply with position
- [x] Defer the interaction reply (resolution takes ~5–8 s)
- [x] Pass `YTDLP_COOKIES` to yt-dlp when set
- [x] Stream through `yt-dlp | ffmpeg` → raw 48 kHz PCM (direct googlevideo fetch returns 403)
- [x] Log track playback errors (`track failed`)
- [x] Bilingual replies for every `/play` outcome (`i18n::Msg`)
- [x] Manual test: plays audio in a real voice channel

## M2 — Autocomplete ✅

- [x] `/play` query autocomplete from YouTube suggestions (no API key)
- [x] Optional YouTube Data API v3 search when `YOUTUBE_API_KEY` is set (real videos with duration; choice value = video URL)
- [x] Debounce (350 ms per user) + 10 min cache so typing doesn't spam requests; 2 s timeout keeps answers < 3 s
- [x] Data API failure / quota exhausted → fall back to suggestions for 1 h
- [x] Pasted URLs are returned as-is (no lookup)
- [x] API key never logged (HTTP errors are stripped of their URL)
- [x] Dev mode registers commands in `DEV_GUILD_ID` and clears global ones (avoids stale client cache / duplicates)
- [x] Unit tests for suggestion parsing, ISO 8601 durations, title unescaping, truncation
- [x] Manual test: suggestions appear while typing

## M3 — Queue & navigation 🚧

- [x] Queue model: current, upcoming (max 100), history (last 50) — pure logic with unit tests
- [x] Auto-advance to next track when one ends or fails (songbird End/Error events, stale handles ignored)
- [x] Queued tracks are re-resolved when their turn comes (stream URLs expire)
- [x] Prefetch the next track's stream while the current one plays (instant transitions)
- [x] Announce "Now Playing" in the text channel when auto-advancing
- [x] After the queue ends, the next `/play` starts immediately (was stuck as "queued")
- [x] `/next` — skip to next track
- [x] `/prev` — go back to the previous track (current is pushed back to the front of the queue)
- [x] `/replay` — restart the current track from 0:00 (reuses its stream when fresh)
- [x] `/queue` — paginated list (now playing + upcoming, 10 per page, total duration)
- [x] Only users in the bot's voice channel can control it
- [ ] Manual test of every command

## M4 — Playback control 🚧

- [x] `/pause` — toggles pause/resume
- [x] `/resume` — explicit resume
- [x] `/stop` — stop playback and clear the queue, stay in channel (history kept)
- [x] `/disconnect` — leave the channel and clear all state
- [x] Idle timer after `/stop` (see M5)
- [ ] Manual test of every command

## M5 — Idle & auto-disconnect 🚧

- [x] Empty queue → idle state
- [x] Leave after `IDLE_TIMEOUT_SECS` (default 10 min) idle; timer resets when a new song plays
- [x] Paused does **not** count as idle
- [x] Alone in the channel (no humans) → leave after `ALONE_TIMEOUT_SECS` (default 2 min)
- [x] Announce why the bot left, in the language of whoever used `/play` last
- [x] Clean state when kicked; follow the bot when a moderator moves it
- [x] Unit tests for the idle/alone clock (deterministic instants)
- [ ] Manual test: idle and alone timeouts in a real channel

## M6 — Bilingual UX & embeds ⬜

- [ ] All user-facing messages available in English and Spanish (catalog in `i18n.rs`, tests that every key has both)
- [ ] Command names/descriptions/options localized for `es-ES` and `es-419`
- [x] "Now Playing" / "Added to Queue" embeds: requester avatar, `Title · [mm:ss]` linking to the video, queue length
- [ ] ETA until a queued track plays
- [ ] Friendly error embeds (not in voice, nothing playing, video unavailable, age-restricted…)

## M7 — Hardening ⬜

- [ ] Graceful shutdown (Ctrl+C leaves all voice channels)
- [ ] Startup check: `yt-dlp` and `ffmpeg` present on PATH, with a clear error if missing
- [ ] Structured logs per guild
- [ ] Manual test checklist (below) passes

## M8 — Production (later) ⬜

- [ ] Dockerfile (multi-stage: build Rust binary; runtime with yt-dlp + ffmpeg)
- [ ] `docker-compose.yml` with `.env`
- [ ] systemd unit for bare-metal VPS
- [ ] Docs: YouTube cookies for datacenter IPs, keeping yt-dlp up to date
- [ ] CI (GitHub Actions): fmt, clippy, test

## Backlog (v1.1+)

- [ ] Now-playing buttons (⏮ ⏯ ⏭ ⏹)
- [ ] YouTube playlist links
- [ ] `/volume`, `/shuffle`, `/loop`, `/remove`, `/nowplaying`
- [ ] `/language` per-guild override

---

## Manual test checklist

Run against a test server with `DEV_GUILD_ID` set.

- [ ] `/play` while not in voice → error message
- [ ] `/play <search text>` → joins my channel and plays
- [ ] `/play <youtube link>` and `youtu.be` link → plays
- [ ] `/play` twice → second track queued, `/queue` shows it
- [ ] `/next`, `/prev`, `/replay` behave as described
- [ ] `/pause` → `/pause` (resume) → `/resume`
- [ ] `/stop` → silence, still in channel; leaves after idle timeout
- [ ] `/disconnect` → leaves immediately
- [ ] Everyone leaves the channel → bot leaves after alone timeout
- [ ] Discord client in Spanish → replies in Spanish; in English → English
