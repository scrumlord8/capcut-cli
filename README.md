# capcut-cli

An open source, agent-first video editing CLI for generating short social clips without touching a timeline.

## What this does

`capcut-cli` lets an agent (or human) discover trending audio, pull viral video clips, and compose them into short-form videos — all from the command line, all with structured JSON output.

**This is a working MVP, not a scaffold.** Every command below actually runs.

## Quick start

```bash
cd py
python3 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt && pip install -e .

# Install dependencies (yt-dlp + ffmpeg)
capcut-cli deps install

# Discover trending TikTok sounds
capcut-cli discover tiktok-sounds --limit 5

# Import a sound
capcut-cli library import "https://www.tiktok.com/music/..." --type sound

# Import a video clip
capcut-cli library import "https://x.com/user/status/123" --type clip

# List your library
capcut-cli library list

# Compose a video: sound + clip → MP4
capcut-cli compose --sound snd_abc123 --clip clp_def456 --duration 15
```

## CLI commands

### `deps` — Manage dependencies

```bash
capcut-cli deps install    # Download yt-dlp binary + verify ffmpeg
capcut-cli deps check      # Verify all deps are available
```

### `discover` — Find trending content

```bash
# Trending TikTok sounds (scraped from Creative Center)
capcut-cli discover tiktok-sounds --limit 10 --region US

# Viral X/Twitter clips (generates search URLs with engagement filters)
capcut-cli discover x-clips --query "ai agents" --limit 10 --min-likes 1000
```

### `library` — Manage assets

```bash
# Import from any supported URL (TikTok, X/Twitter, YouTube, etc.)
capcut-cli library import <url> --type sound --tags "trending,tiktok"
capcut-cli library import <url> --type clip --tags "viral,ai"

# Browse your library
capcut-cli library list                # All assets
capcut-cli library list --type sound   # Sounds only
capcut-cli library show <asset_id>     # Asset details
capcut-cli library delete <asset_id>   # Remove asset
```

### `compose` — Render videos

```bash
capcut-cli compose \
  --sound snd_abc123 \
  --clip clp_def456 \
  --clip clp_ghi789 \
  --duration 20 \
  --resolution 1080x1920
```

The compose pipeline:
1. Normalizes audio loudness (target -14 LUFS)
2. Trims audio to target duration
3. Scales and center-crops each clip to target resolution
4. Concatenates clips (loops single clips to fill duration)
5. Muxes audio + video into final MP4

Output: a real, playable MP4 file.

## Agent-first design

Every command outputs structured JSON to stdout:

```json
{
  "status": "ok",
  "command": "library list",
  "data": { ... },
  "errors": [],
  "meta": { "version": "0.1.0", "duration_ms": 42 }
}
```

- **stdout** = structured JSON (for agents to parse)
- **stderr** = human-readable progress logs
- **exit codes**: 0 = success, 1 = user error, 2 = missing dependency
- **errors include hints**: not just "failed" but "failed because X, try Y"
- **all file paths are absolute** so agents can use them directly

## Architecture

```
py/capcut_cli/
  cli.py              # Click command tree
  config.py           # Paths and constants
  models.py           # Asset, TrendingSound, ComposeResult
  output.py           # JSON envelope wrapper
  discover/
    tiktok.py          # Creative Center page scraping
    twitter.py         # Search URL generation
  library/
    store.py           # Filesystem + JSON manifest storage
  media/
    downloader.py      # yt-dlp subprocess wrapper
    ffmpeg.py          # ffmpeg subprocess wrappers
    compose.py         # Render pipeline
  deps/
    bootstrap.py       # yt-dlp binary download, ffmpeg check
```

## Dependencies

- **Python 3.9+**
- **yt-dlp** (standalone binary, auto-downloaded by `deps install`)
- **ffmpeg** (bundled via `imageio-ffmpeg` pip package)
- **httpx** — HTTP client for discovery scraping
- **beautifulsoup4** — HTML parsing for TikTok Creative Center
- **click** — CLI framework

## Supported platforms for import

| Platform | Sound | Clip | Notes |
|----------|-------|------|-------|
| TikTok | Yes | Yes | May need `--cookies-from-browser` if IP-blocked |
| X/Twitter | Yes | Yes | yt-dlp handles download |
| YouTube | Yes | Yes | Full support |
| Instagram | Yes | Yes | Via yt-dlp |

## Status

**Working MVP.** The full pipeline is operational:
- Discover trending TikTok sounds (live data from Creative Center)
- Generate X/Twitter search queries with engagement filters
- Download sounds and clips from any yt-dlp-supported URL
- Compose real MP4 videos with normalized audio + scaled/cropped clips
