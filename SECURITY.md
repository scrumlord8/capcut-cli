# Security

This repo can work with real credentials, so treat local development runs as sensitive.

## What the CLI does

- reads `TWITTER_BEARER_TOKEN` from the environment at runtime
- reads `TIKTOK_RESEARCH_ACCESS_TOKEN` from the environment at runtime
- uses `yt-dlp --cookies-from-browser` for authenticated X/Twitter media retrieval
- redacts token-like query parameters and signed URL fragments from logs
- strips token-like query parameters before persisting imported asset source URLs

## What you should do

- use a dedicated low-scope X API token for this tool
- use a dedicated low-scope TikTok Research API token for this tool
- keep browser-cookie auth only on a trusted machine
- avoid pasting cookie values into files or commands when `--cookies-from-browser` is available
- do not share shell history, raw stderr logs, or screenshots from authenticated sessions
- rotate or revoke tokens if you suspect they were exposed

## Files to keep local

The repo ignores common local secret files:

- `.env`
- `.env.*`
- `*.local`

If you need environment variables, keep them in a local file that is not committed.

## Reporting issues

If you find a place where a token, cookie, signed URL, or other credential is being persisted or logged in clear text, treat it as a bug and fix it before using the repo with real credentials again.
