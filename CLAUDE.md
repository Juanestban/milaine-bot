# CLAUDE.md

Guidance for AI agents working in this repository.

## 🚫 Privacy rule — FORBIDDEN

**Never share, send or embed the owner's email address. Ever.** This includes:

- HTTP headers of any outgoing request (`User-Agent`, `From`, custom headers) — made by the bot, by tools, scripts, or by the agent itself.
- Request URLs, query strings, payloads, telemetry.
- Source code, `Cargo.toml` (`authors`), docs, README, ROADMAP, commit messages, PR descriptions, logs, agent output.

All HTTP clients must set an explicit neutral User-Agent (`milaine-bot/<version>`, see `src/http.rs`). If a tool would attach the email automatically, don't use it that way.

## Project context — use graphify first

A knowledge graph of the project lives in `graphify-out/` (gitignored, local only).

- Before reading many files to answer an architecture/codebase question, run `graphify query "<question>"` and read `graphify-out/GRAPH_REPORT.md`.
- After finishing a feature, refresh it with `/graphify . --update`.

Other context sources: `docs/DECISIONS.md` (agreed design), `ROADMAP.md` (progress).

## Language

- Code, comments, docs, README, ROADMAP, commit messages: **English**.
- Bot replies to users: **English and Spanish**, chosen from the user's Discord locale (`src/i18n.rs`).
- Chat with the owner: Spanish is fine.

## Stack

Rust 2024 · serenity 0.12 · poise 0.7 (slash commands) · songbird 0.6 (voice) · yt-dlp (external, must be on PATH) · ffmpeg.

## Workflow

1. Pick the next unchecked item in `ROADMAP.md`.
2. Implement it with unit tests for pure logic.
3. `cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test` must pass.
4. Tick the ROADMAP checkbox, commit (`feat: ...`, `fix: ...`, `docs: ...`), refresh graphify.
5. No pushing (no remote configured) unless the owner asks.

## Commands

```sh
cargo run                 # run the bot locally (needs .env)
cargo test                # unit tests
cargo clippy --all-targets -- -D warnings
# logo: assets/logo.png is the source; assets/logo.svg embeds it (regenerate command in README)
```
