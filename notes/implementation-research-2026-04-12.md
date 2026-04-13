# Implementation research - 2026-04-12

## Goal

Document the repo's actual acquisition and rendering strategy for the "strong yes" path:

- discover trending TikTok sounds programmatically
- discover viral X/Twitter clips programmatically
- import both assets without manual timeline work
- compose a Twitter-postable short in the CLI

## TikTok sound acquisition

### Surface used

The repo uses TikTok Creative Center pages, not an official public TikTok API for trending sounds.

Current path:

1. try the Creative Center JSON endpoint
2. fall back to Creative Center HTML crawling
3. crawl song detail pages
4. normalize each result into a stable JSON shape

### Why this is unofficial

There is no stable official TikTok developer API in this repo for "trending sounds" the way we need it.
Creative Center is a public web surface and can change without notice.

### Import fallback chain

For each discovered sound:

- `tiktok_url` is the canonical/reference music page
- `import_url` is the URL the CLI should actually ingest

Preferred `import_url` order:

1. direct preview audio URL when available
2. related TikTok embed URL from the song detail payload
3. canonical TikTok music URL as last resort

This is intentional because direct TikTok music-page downloads are currently less reliable than related embed imports through `yt-dlp`.

## X/Twitter clip discovery

### Surface used

Discovery uses the official X recent search API.

Current path:

1. require `TWITTER_BEARER_TOKEN`
2. search for query + `has:videos -is:retweet`
3. expand author and media metadata
4. filter to tweets with video or animated GIF media
5. rank deterministically by engagement + recency

### Why discovery and import are split

Official API search is good for finding and ranking posts.
It is not the same thing as obtaining a downloadable media asset.

So the repo intentionally splits X handling into:

- official API for discovery and ranking
- authenticated `yt-dlp` retrieval for media import

## Downloader and auth assumptions

### X/Twitter

Reliable X import is treated as authenticated by default.

The downloader:

- tries `--cookies-from-browser`
- uses `CAPCUT_X_COOKIE_BROWSERS` if set
- otherwise tries `chrome,safari,firefox,edge`

Structured failure cases are intentionally separated:

- auth required
- rate limited
- suspended tweet
- no downloadable video
- unavailable video

### TikTok

TikTok imports currently rely on `yt-dlp` plus Creative Center-derived `import_url` values.
The fallback chain is important because the canonical music pages are not always directly downloadable.

## ffmpeg composition pipeline

The render path is:

1. normalize audio loudness
2. trim audio to target duration
3. trim each clip to its segment duration
4. scale and center-crop to target resolution
5. concatenate clips
6. mux H.264 video with AAC audio

Default target format is suitable for Twitter/X posting:

- vertical `1080x1920` by default
- H.264 video
- AAC audio
- MP4 container

## Strong-yes acceptance path

The repo should be judged against this exact flow:

1. `deps check` passes
2. TikTok discovery returns at least one result with a non-empty `import_url`
3. X discovery returns ranked live clip candidates when `TWITTER_BEARER_TOKEN` is configured
4. TikTok sound import succeeds from `import_url`
5. X clip import succeeds with browser-cookie auth
6. compose succeeds on those freshly imported assets
7. `ffprobe` confirms H.264 + AAC output
