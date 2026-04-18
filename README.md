# capcut-cli

An open source, agent-first Rust CLI for discovering source media, managing a local asset library, and composing short-form social clips without touching a timeline.

## Status

This repository was rewritten from Python to Rust. The current implementation is the Rust crate in `src/`; the old Python app described by earlier docs is no longer the source of truth.

Today the CLI supports:

- checking and installing runtime dependencies
- discovering trending TikTok sounds through TikTok Research API with Creative Center fallback
- discovering X/Twitter clips through authenticated API search plus lower-barrier fallback strategies
- importing sounds and clips into a local JSON-backed library
- composing a final MP4 from one sound and one or more clips
- running a one-shot `autopilot` workflow that discovers, imports, and composes automatically

## Quick start

```bash
cargo run -- deps check

# If yt-dlp is missing, download it to ~/.capcut-cli/bin/yt-dlp
cargo run -- deps install

# Inspect the local asset library
cargo run -- library list

# Discover trending TikTok sounds
export TIKTOK_RESEARCH_ACCESS_TOKEN=...
cargo run -- discover tiktok-sounds --limit 5 --region US --window-days 7

# Reliable X discovery requires a bearer token
export TWITTER_BEARER_TOKEN=...

# Discover ranked X clips for a topic
cargo run -- discover x-clips --query "ai agents" --limit 5 --min-likes 1000

# One-shot agent workflow (discover + import + compose)
cargo run -- autopilot --query "ai agents" --duration 15

# Lower-barrier sound strategies for agents
cargo run -- discover tiktok-sounds --strategy library --limit 5
cargo run -- discover tiktok-sounds --strategy manual-url --sound-url "https://www.tiktok.com/music/_-123"

# Lower-barrier clip strategies for agents
cargo run -- discover x-clips --query "ai agents" --strategy guided
cargo run -- discover x-clips --query "ai agents" --strategy library --limit 5
cargo run -- discover x-clips --query "ai agents" --strategy manual-url --clip-url "https://x.com/user/status/123"
```

You can also install the binary locally:

```bash
cargo install --path .
capcut-cli --help
```

## Requirements

- Rust toolchain for building and running the crate
- `ffmpeg` available on `PATH`, or placed at `~/.capcut-cli/bin/ffmpeg`
- `yt-dlp` available at `~/.capcut-cli/bin/yt-dlp`

Notes:

- `capcut-cli deps install` downloads `yt-dlp` automatically for macOS and Linux.
- `capcut-cli deps install` does not install `ffmpeg`; it only verifies whether `ffmpeg` is already available.
- On macOS, `brew install ffmpeg` is the simplest way to satisfy the `ffmpeg` requirement.
- Reliable X/Twitter media import expects a logged-in local browser. The downloader tries browsers from `CAPCUT_X_COOKIE_BROWSERS`, or `chrome,safari,firefox,edge` by default.
- Reliable X/Twitter discovery expects `TWITTER_BEARER_TOKEN`.
- Official TikTok sound discovery expects `TIKTOK_RESEARCH_ACCESS_TOKEN`; when it is missing, the CLI falls back to best-effort Creative Center scraping.
- TikTok music imports can still be brittle when upstream extractor behavior changes; when that happens, use `manual-url` with another supported source or import fresh URLs directly into the library.

## Credential Safety

- `TWITTER_BEARER_TOKEN` is only read from the environment at runtime; the CLI does not persist it in repo files or library manifests.
- `TIKTOK_RESEARCH_ACCESS_TOKEN` is only read from the environment at runtime; the CLI does not persist it in repo files or library manifests.
- X media import uses `yt-dlp --cookies-from-browser`, which reads your browser session from the local machine instead of asking you to paste cookie values into the repo.
- command logs redact token-like query parameters and signed URL fragments before printing to stderr.
- imported asset metadata strips token-like query parameters before saving `source_url` into `library/manifest.json`.
- `.env`, `.env.*`, and `*.local` are ignored by git so local credential files are less likely to be committed accidentally.
- copy `.env.example` to `.env` if you want a local template for the supported variables.
- you should still prefer a dedicated low-scope X API token for this tool and avoid sharing terminals/log captures from authenticated runs.
- see [SECURITY.md](SECURITY.md) for the short operational checklist we recommend before using real API tokens.

## Commands

### `deps`

Manage runtime dependencies.

```bash
cargo run -- deps check
cargo run -- deps install
```

`deps check` returns structured JSON describing whether `ffmpeg` and `yt-dlp` are installed.

### `discover`

Find candidate sounds and clips before importing them.

```bash
# TikTok Creative Center discovery
cargo run -- discover tiktok-sounds --limit 10 --region US --window-days 7

# X/Twitter discovery (recommended strong-yes path)
cargo run -- discover x-clips --query "ai agents" --limit 10 --min-likes 1000

# Lower-barrier X/Twitter options
cargo run -- discover x-clips --query "ai agents" --strategy guided
cargo run -- discover x-clips --query "ai agents" --strategy library --limit 5
```

Important behavior:

- `discover tiktok-sounds` first tries the TikTok Research API, then falls back to Creative Center JSON, song-detail crawling, and HTML scraping.
- `discover tiktok-sounds` returns ranked candidates with `music_id`, `ranking_score`, `source_path`, and an `import_url`; prefer `import_url` when you want the CLI to ingest the sound immediately.
- `discover tiktok-sounds` uses a rolling discovery window; `--window-days` defaults to `7`.
- `discover tiktok-sounds` supports explicit strategies: `auto`, `research`, `creative-center`, `library`, and `manual-url`.
- `auto` chooses the lowest-friction working path in this order: `manual-url` when `--sound-url` is provided, then `research` when a token is configured, then `creative-center`, then `library`.
- `discover x-clips` supports explicit strategies: `auto`, `api`, `guided`, `library`, and `manual-url`.
- `discover x-clips` returns ranked clip candidates with `import_url`, engagement metrics, and `ranking_score` when the API strategy succeeds.
- `auto` chooses the lowest-friction working path in this order: `manual-url` when `--clip-url` is provided, then `api` when `TWITTER_BEARER_TOKEN` is configured, then `guided`, then `library`.
- `guided` returns browser search URLs and an import hint instead of live API results; it is useful when auth is not configured, but it is not the recommended strong-yes path.
- `library` reuses previously imported clip assets for the fastest fully local workflow.

### `library`

Manage local media assets stored under `library/`.

```bash
# Import from a supported URL
cargo run -- library import "https://www.tiktok.com/embed/v2/..." --type sound --tags trending,tiktok
cargo run -- library import "https://x.com/user/status/123" --type clip --tags viral,demo
cargo run -- library import "https://www.youtube.com/watch?v=..." --type clip --tags fresh,youtube
cargo run -- library import "https://www.youtube.com/watch?v=..." --type sound --tags fresh,youtube

# Inspect the library
cargo run -- library list
cargo run -- library list --type sound
cargo run -- library show snd_bf6bbb0a

# Remove an asset
cargo run -- library delete snd_bf6bbb0a
```

Import behavior:

- `--type` is optional; TikTok `/music/` URLs are auto-detected as sounds and everything else defaults to clips.
- for TikTok sound imports discovered via Creative Center or Research API enrichment, prefer the returned `import_url`
- sounds are downloaded with `yt-dlp`, converted to MP3, and stored under `library/sounds/assets/<asset_id>/`
- clips are downloaded with `yt-dlp` and stored under `library/clips/<asset_id>/`
- imported assets are indexed in `library/manifest.json`
- X/Twitter clip imports use authenticated browser cookies by default and emit distinct structured errors for missing auth, suspended tweets, missing video media, unavailable video, and rate limiting
- manual URL import is the most reliable way to guarantee fresh content when platform discovery or extractors are temporarily degraded

Supported source platforms currently detected by the downloader:

- TikTok
- X/Twitter
- YouTube
- Instagram

### `compose`

Render one final MP4 from one sound plus one or more clips.

```bash
cargo run -- compose \
  --sound snd_bf6bbb0a \
  --clip clp_31cd891e \
  --duration 20 \
  --resolution 1080x1920 \
  --loudness viral
```

Options:

- `--sound <ID>`: required sound asset ID
- `--clip <ID>`: required, repeatable clip asset ID
- `--duration <SECONDS>`: output duration, default `30`
- `--output <PATH>`: optional explicit output path
- `--resolution <WxH>`: default `1080x1920`
- `--loudness <PRESET|LUFS>`: preset or numeric LUFS value

Built-in loudness presets:

- `viral`: `-8 LUFS` default
- `social`: `-10 LUFS`
- `podcast`: `-14 LUFS`
- `broadcast`: `-23 LUFS`

Compose pipeline:

1. normalize the chosen sound with `ffmpeg` loudness normalization
2. trim audio to the requested duration
3. trim each clip to its segment duration
4. scale and center-crop clips to the requested resolution
5. concatenate clips and mux AAC audio into the final MP4

If `--output` is omitted, the CLI writes to `library/output/comp_<job_id>/final.mp4`.

### `autopilot`

Run one agent-facing command that:
1. discovers TikTok sounds
2. discovers X clips for your topic
3. imports the first successful sound + clip candidates
4. composes the final MP4

This command works best when:
- `TIKTOK_RESEARCH_ACCESS_TOKEN` is set for official TikTok sound discovery
- `TWITTER_BEARER_TOKEN` is set for official X clip discovery
- a supported local browser is logged into X for media import

Sound strategy options for agents:
- `auto`: choose the best available option from repo/runtime context
- `research`: official TikTok Research API path
- `creative-center`: public scrape with no token, but more brittle
- `library`: reuse local sound assets for the lowest barrier to entry
- `manual-url`: use a caller-provided sound URL directly

Clip strategy options for agents:
- `auto`: choose the best available option from repo/runtime context
- `api`: official X API path when `TWITTER_BEARER_TOKEN` is configured
- `guided`: browser-search fallback that returns search URLs and an import hint
- `library`: reuse local clip assets for the lowest barrier to entry
- `manual-url`: use a caller-provided X clip URL directly

Practical agent guidance:
- use `auto` when credentials are configured and freshness matters more than determinism
- use `library` when you need the fastest guaranteed local success
- use `manual-url` when you already have a fresh source URL and want the most predictable non-library path
- if TikTok or X discovery is degraded, importing fresh URLs from another supported platform such as YouTube is still a valid path to a brand-new output

```bash
cargo run -- autopilot \
  --query "ai agents" \
  --region US \
  --window-days 7 \
  --sound-strategy auto \
  --clip-strategy auto \
  --sound-limit 5 \
  --clip-limit 5 \
  --min-likes 1000 \
  --duration 15 \
  --resolution 1080x1920
```

## Agent-first output contract

Every successful command prints a structured JSON envelope to stdout. Progress logs go to stderr.

Example:

```json
{
  "status": "ok",
  "command": "library list",
  "data": {
    "count": 2,
    "assets": []
  },
  "errors": [],
  "meta": {
    "version": "0.1.0",
    "duration_ms": 2
  }
}
```

Behavior guarantees:

- stdout is machine-readable JSON
- stderr is for human-readable progress messages
- success exits with code `0`
- `deps check` exits with code `2` when dependencies are missing
- all imported asset paths and compose output paths are emitted as absolute paths
- structured error codes distinguish setup failures from media/data failures on X/Twitter

## Repository layout

```text
src/
  cli.rs               # clap command tree and dispatch
  config.rs            # paths, version, loudness presets
  deps.rs              # ffmpeg checks and yt-dlp installation
  discover/
    tiktok.rs          # TikTok Creative Center discovery
    twitter.rs         # X/Twitter API or guided discovery
  library.rs           # import/list/show/delete asset workflow
  media/
    compose.rs         # end-to-end composition pipeline
    downloader.rs      # yt-dlp integration
    ffmpeg.rs          # ffmpeg wrappers
  models.rs            # asset and compose result models
  output.rs            # JSON envelope helpers
library/
  manifest.json        # imported asset index used by the CLI
  sounds/              # sound assets and committed sound notes
  clips/               # imported clip assets
  output/              # composed videos
```

## Committed demo assets

This repository currently includes real local demo assets in `library/manifest.json`, including:

- `snd_bf6bbb0a`
- `clp_31cd891e`

That means you can run `compose` immediately on a freshly cloned repo once `ffmpeg` is available.

There is also a smaller committed seed audio sample at `library/sounds/samples/seed-preview-loop.wav` for library documentation and inspection.

## Testing

Run the Rust test suite with:

```bash
cargo test
```

At the time of this update, the suite contains coverage for:

- X clip scoring and guided-fallback labeling
- TikTok `import_url` normalization
- downloader error classification for X auth/media failures
- import metadata enrichment for TikTok embeds
- loudness preset resolution
- numeric loudness parsing
- duration parsing in the ffmpeg helpers
- a compose smoke test over existing library assets

## Live Acceptance Flow

The intended strong-yes flow is:

1. `cargo run -- deps check`
2. `cargo run -- discover tiktok-sounds --limit 5 --region US --window-days 7`
3. `cargo run -- discover x-clips --query "<topic>" --limit 5 --min-likes 1000`
4. import the TikTok sound using the returned `import_url`
5. import the X clip using the returned `import_url`
6. `cargo run -- compose --sound <sound_id> --clip <clip_id> --duration 10 --resolution 1080x1920`

Or run the same flow in one command:

- `cargo run -- autopilot --query "<topic>" --region US --window-days 7 --sound-strategy auto --clip-strategy auto --duration 15`

Expected environment for that path:

- `TWITTER_BEARER_TOKEN` is set
- at least one supported logged-in browser is available locally for X media import
- `ffmpeg` is installed

## Batch: three finished clips from trending discovery

Two phone-friendly paths that run real discovery and produce three composed
MP4s plus their real source assets under `clips/`.

Both paths require:

- `TIKTOK_RESEARCH_ACCESS_TOKEN` — for trending TikTok sound discovery
- `TWITTER_BEARER_TOKEN` — for ranked X clip discovery

### Path A — GitHub Actions (one tap from the mobile app)

1. Add the two tokens under **Settings → Secrets and variables → Actions**.
2. From the GitHub mobile app: **Actions → build-clips → Run workflow**.
   Optional inputs: `query`, `region`, `window_days`, `duration`, `resolution`,
   `min_likes`.
3. When the run finishes, download the `clips` artifact from the run page.

Caveat: discovery APIs work from Actions runners, but the `yt-dlp` download
step can be rate-limited or blocked on data-center IPs, and X media import
has no logged-in browser here. Path B is the more reliable lane.

### Path B — Codespaces (also mobile-viable)

1. Add the two tokens as **Codespace secrets** on this repo.
2. Open a Codespace from the mobile app (the devcontainer builds the CLI).
3. In the terminal: `make clips`.
4. The finished clips and source references land in `./clips/`; download the
   folder from the Codespace file browser.

### What lands in `clips/`

- `clip_1.mp4`, `clip_2.mp4`, `clip_3.mp4` — finished vertical MP4s
- `source_sound.mp3` — the real trending audio used by all three
- `source_1.*`, `source_2.*`, `source_3.*` — the real source clips
- `manifest.json` — provenance (discovery response snippets for sound + clips)

You can also run the pipeline directly: `./scripts/build-clips.sh`. It accepts
the same knobs via env vars (`QUERY`, `REGION`, `WINDOW_DAYS`, `DURATION`,
`RESOLUTION`, `MIN_LIKES`, `CLIPS_DIR`).

## What changed from the old Python version

- the production CLI is now Rust, built with `clap`
- runtime behavior lives in `src/`, not `py/`
- dependency bootstrapping is handled in Rust
- the README no longer assumes virtualenvs, `pip`, or Click-based commands
