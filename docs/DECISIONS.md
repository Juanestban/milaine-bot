# Design decisions

Agreed design for Milaine Bot (decided 2026-11-07). Update this file when a decision changes.

## Stack

| Concern | Choice | Why |
| --- | --- | --- |
| Discord client | serenity 0.12 | Mature, required by songbird/poise |
| Commands | poise 0.7, **slash commands only** | Autocomplete only exists for slash commands |
| Voice | songbird 0.6 (+ symphonia 0.5 codecs) | De-facto Rust voice library |
| YouTube audio | `yt-dlp` external process | Most reliable YouTube extractor; updated often |
| HTTP | reqwest 0.12 (shared with songbird) | One client, neutral User-Agent |
| Runtime | tokio | Required by the above |

## Behaviour

- **Joining**: `/play` joins the caller's voice channel. Caller not in voice → error.
  Bot already playing in another channel of the same guild → reject (it does not move).
- **Control permission**: only users in the bot's voice channel can control it.
- **Multi-guild**: independent player state per guild.
- **Commands**:
  - `/play <query|url>` — play now if idle, otherwise enqueue.
  - `/pause` — toggle; `/resume` — explicit alias.
  - `/stop` — stop and clear the queue, stay in channel, start idle timer.
  - `/disconnect` — leave and clear everything.
  - `/next` — skip. `/prev` — previous track from history. `/replay` — restart current track.
  - `/queue` — paginated view.
- **Idle**: empty queue = idle. Leave after 10 min idle (`IDLE_TIMEOUT_SECS`). Paused is not idle.
  Alone in channel → leave after 2 min (`ALONE_TIMEOUT_SECS`).
- **Limits**: queue max 100, history last 50, no per-track duration limit (configurable later).
- **Links**: single YouTube videos only in v1 (`watch`, `youtu.be`, `music.youtube.com`, `shorts`).
  `&list=` is stripped. Playlists are backlog.

## Autocomplete

- Default: YouTube's public suggestion endpoint (fast, no key), debounced + cached.
- If `YOUTUBE_API_KEY` is set: YouTube Data API v3 search (real video results).
- Discord gives 3 s to answer an autocomplete request; never block on yt-dlp there.

## Language

- Code, docs, README, ROADMAP, commits: English.
- Bot replies: English or Spanish from the interaction's user locale (`es-*` → Spanish, else English).
  Command descriptions localized natively for `es-ES` and `es-419`.

## Presentation

- Embeds for now playing / queued (title, thumbnail, duration, requester) in v1.
- Buttons in v1.1.

## Privacy

- The owner's email must never appear in HTTP headers, code, docs or commits.
  All HTTP clients use `User-Agent: milaine-bot/<version>`.

## Delivery

- Local first. Docker / systemd / cookies for datacenter IPs come last (M8).
- Each feature: unit tests for pure logic, clippy clean, ROADMAP ticked, one commit, graphify refreshed.
- MIT license. Logo: owner-provided artwork in `assets/logo.png` (source of truth);
  `assets/logo.svg` embeds the same PNG as base64 (the painted style does not vectorize well).
  Replaced the first hand-drawn SVG logo on 2026-11-07.
