# capcut-cli

An open source, agent-first Rust CLI for importing short-form source material,
managing a local asset library, and composing vertical clips without touching a
timeline.

## Status

The honest minimum viable truth is **fresh input in, finished clip out.**

Given a trending sound URL and one or more source clip URLs, the CLI imports,
normalizes, trims, scales, center-crops, concatenates, and muxes them into a
final MP4 — reliably, locally, and with real bytes end-to-end.

Discovery of trending material exists in the codebase but is scoped down in
the docs: every official path is gated by an external API (TikTok Research,
X/Twitter v2 search) that is either hard to obtain or paywalled, and the
unauthenticated fallbacks are brittle by design. Treat discovery as an
optional convenience on top of the manual-URL spine, not the spine itself.

What's solid today:

- importing sounds and clips from supported URLs into a local library
- composing one final vertical MP4 from one sound and one or more clips
- loudness normalization presets for social, viral, podcast, broadcast
- structured JSON output on stdout; progress logs on stderr
- committed demo library assets so `compose` works immediately after clone
- end-to-end integration test that exercises import → compose with real media

## Quick start

Build and verify dependencies:

```bash
cargo build --release
./target/release/capcut-cli deps check

# If yt-dlp is missing, install the standalone binary into ~/.capcut-cli/bin
./target/release/capcut-cli deps install
```

Run the primary flow — import one sound URL plus one or more clip URLs, then
compose:

```bash
# 1. Import a trending audio source (TikTok music, YouTube, Instagram, X)
./target/release/capcut-cli library import \
  "https://www.tiktok.com/music/<slug>-<id>" --type sound --tags trending

# 2. Import one or more source clips
./target/release/capcut-cli library import \
  "https://x.com/<user>/status/<id>" --type clip --tags source

# 3. Compose a finished vertical MP4
./target/release/capcut-cli compose \
  --sound <sound_id> --clip <clip_id> \
  --duration 15 --resolution 1080x1920 --loudness viral
```

The CLI writes to `library/output/comp_<job_id>/final.mp4` unless
`--output` is supplied. Asset IDs are returned in each import's JSON envelope
(`.data.id`).

A batch script that wraps the above for three clips at once is documented
below under **Batch: three finished clips**.

## Requirements

- Rust toolchain to build the crate
- `ffmpeg` on `PATH` (or at `~/.capcut-cli/bin/ffmpeg`)
- `yt-dlp` at `~/.capcut-cli/bin/yt-dlp` (the CLI installs this itself via
  `deps install`, no other runtime needed)

On macOS, `brew install ffmpeg` is the simplest way to satisfy ffmpeg.

## Batch: three finished clips

`scripts/build-clips-from-urls.sh` takes one supplied sound URL plus three
supplied clip URLs and produces a self-contained `clips/` folder:

- `clip_1.mp4`, `clip_2.mp4`, `clip_3.mp4` — finished vertical MP4s
- `source_sound.<ext>` — the imported audio used by all three
- `source_1.<ext>`, `source_2.<ext>`, `source_3.<ext>` — the imported clips
- `manifest.json` — provenance (the supplied URLs and compose settings)

Local invocation:

```bash
SOUND_URL="https://..." \
CLIP_URLS="https://url1 https://url2 https://url3" \
  ./scripts/build-clips-from-urls.sh
```

### Path A — GitHub Actions (phone-friendly)

1. Open **Actions → build-clips → Run workflow** in the GitHub mobile app.
2. Leave `mode` at `urls` (the default).
3. Paste `sound_url`, `clip_url_1`, `clip_url_2`, `clip_url_3`. Tweak
   `duration` and `resolution` if desired.
4. When the run finishes, download the `clips` artifact.

### Path B — Codespaces

1. Open a Codespace on this repo (the devcontainer builds the CLI and
   installs ffmpeg + yt-dlp).
2. Run:
   ```bash
   SOUND_URL="..." CLIP_URLS="... ... ..." make clips
   ```
3. The `clips/` folder is in the workspace; grab it from the file browser.

## Commands

### `deps`

Manage runtime dependencies.

```bash
cargo run --release -- deps check
cargo run --release -- deps install
```

`deps check` returns structured JSON describing whether `ffmpeg` and `yt-dlp`
are installed. `deps install` downloads the standalone `yt-dlp` binary from
the upstream GitHub release for macOS and Linux.

### `library`

Manage local media assets stored under `library/`.

```bash
# Import from a supported URL
./target/release/capcut-cli library import \
  "https://www.tiktok.com/music/..." --type sound --tags trending,tiktok
./target/release/capcut-cli library import \
  "https://x.com/user/status/123" --type clip --tags source

# Inspect the library
./target/release/capcut-cli library list
./target/release/capcut-cli library list --type sound
./target/release/capcut-cli library show snd_demo001

# Remove an asset
./target/release/capcut-cli library delete snd_demo001
```

Import behavior:

- `--type` is optional; TikTok `/music/` URLs are auto-detected as sounds,
  everything else defaults to clip
- sounds are downloaded with `yt-dlp`, converted to MP3, and stored under
  `library/sounds/assets/<asset_id>/`
- clips are downloaded with `yt-dlp` and stored under
  `library/clips/<asset_id>/`
- imported assets are indexed in `library/manifest.json`
- X/Twitter imports use authenticated browser cookies via
  `yt-dlp --cookies-from-browser` and emit distinct structured error codes
  for missing auth, suspended tweets, missing video media, unavailable video,
  and rate limiting

Supported source platforms detected by the downloader:

- TikTok
- X/Twitter
- YouTube
- Instagram

### `compose`

Render one final MP4 from one sound plus one or more clips.

```bash
./target/release/capcut-cli compose \
  --sound snd_demo001 \
  --clip clp_demo001 \
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

- `viral`: `-8 LUFS`
- `social`: `-10 LUFS`
- `podcast`: `-14 LUFS`
- `broadcast`: `-23 LUFS`

Compose pipeline:

1. normalize the chosen sound with `ffmpeg` loudness normalization
2. trim audio to the requested duration
3. trim each clip to its segment duration
4. scale and center-crop clips to the requested resolution
5. concatenate clips and mux AAC audio into the final MP4

If `--output` is omitted, the CLI writes to
`library/output/comp_<job_id>/final.mp4`.

## Optional: API-gated discovery (experimental)

> ⚠️ These commands depend on external APIs that are hard to obtain or paywalled,
> and public fallbacks are brittle. Use them as a convenience on top of the
> manual-URL spine, not as the primary path.

### Token availability at a glance

- **TikTok Research API** (`TIKTOK_RESEARCH_ACCESS_TOKEN`): restricted to
  academic researchers at non-profit institutions; commercial applicants are
  routinely rejected and approval takes weeks. Unauthenticated Creative Center
  scraping exists as a fallback but is frequently degraded upstream.
- **X/Twitter API** (`TWITTER_BEARER_TOKEN`): the recent-search endpoint this
  CLI uses is not on the Free tier. Minimum is Basic at $200/month.

If you have the tokens:

```bash
export TIKTOK_RESEARCH_ACCESS_TOKEN=...
export TWITTER_BEARER_TOKEN=...

./target/release/capcut-cli discover tiktok-sounds --limit 5 --region US --window-days 7
./target/release/capcut-cli discover x-clips --query "ai agents" --limit 5 --min-likes 1000

# Or end-to-end:
./target/release/capcut-cli autopilot --query "ai agents" --duration 15
```

The discovery-mode batch path is also available in the Actions workflow by
setting `mode: discovery` and adding both tokens as repo secrets. Downloads
from Actions may be rate-limited or blocked on data-center IPs even when
discovery succeeds — this is why the manual-URL path is the recommended one.

## Agent-first output contract

Every successful command prints a structured JSON envelope to stdout. Progress
logs go to stderr.

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

## Credential safety

- `TWITTER_BEARER_TOKEN` and `TIKTOK_RESEARCH_ACCESS_TOKEN` are only read from
  the environment at runtime; the CLI does not persist them in repo files or
  library manifests.
- X media import uses `yt-dlp --cookies-from-browser`, which reads your local
  browser session instead of asking you to paste cookie values into the repo.
- command logs redact token-like query parameters and signed URL fragments
  before printing to stderr.
- imported asset metadata strips token-like query parameters before saving
  `source_url` into `library/manifest.json`.
- `.env`, `.env.*`, and `*.local` are ignored by git so local credential files
  are less likely to be committed accidentally.
- copy `.env.example` to `.env` if you want a local template for the supported
  variables.
- prefer a dedicated low-scope X API token for this tool and avoid sharing
  terminals or log captures from authenticated runs.
- see [SECURITY.md](SECURITY.md) for the operational checklist we recommend
  before using real API tokens.

## Repository layout

```text
src/
  cli.rs               # clap command tree and dispatch
  config.rs            # paths, version, loudness presets
  deps.rs              # ffmpeg checks and yt-dlp installation
  discover/
    tiktok.rs          # TikTok discovery (API-gated, optional)
    twitter.rs         # X/Twitter discovery (API-gated, optional)
  library.rs           # import/list/show/delete asset workflow
  media/
    compose.rs         # end-to-end composition pipeline
    downloader.rs      # yt-dlp integration
    ffmpeg.rs          # ffmpeg wrappers
  models.rs            # asset and compose result models
  output.rs            # JSON envelope helpers
library/
  manifest.json        # imported asset index used by the CLI
  sounds/              # sound assets and committed seed media
  clips/               # imported clip assets
  output/              # composed videos
scripts/
  build-clips-from-urls.sh   # primary: compose 3 clips from supplied URLs
  build-clips.sh             # optional: discovery-driven batch
tests/
  e2e_url_to_clip.rs   # end-to-end import → compose smoke test
```

## Committed demo assets

`library/manifest.json` references two small committed fixtures so `compose`
works immediately on a freshly cloned repo:

- `snd_demo001` — 2-second 440 Hz sine tone at `library/sounds/assets/snd_demo001/audio.mp3`
- `clp_demo001` — 3-second solid-color vertical MP4 at `library/clips/clp_demo001/video.mp4`

These are synthetic, not "trending" — they exist so the compose pipeline is
inspectable without network access. For real trending material, use the
manual-URL import flow.

## Testing

Run the full Rust test suite with:

```bash
cargo test --all-targets
```

Coverage currently includes:

- X clip scoring and guided-fallback labeling
- TikTok `import_url` normalization
- downloader error classification for X auth/media failures
- import metadata enrichment for TikTok embeds
- loudness preset resolution
- numeric loudness parsing
- duration parsing in the ffmpeg helpers
- a compose smoke test over the committed demo assets
- **an end-to-end integration test (`tests/e2e_url_to_clip.rs`) that exercises
  the full import-from-URL → compose spine via a yt-dlp shim, so the honest
  minimum viable truth is verifiable in CI**

The `test` GitHub Actions workflow runs `cargo test --all-targets` on every
push.
