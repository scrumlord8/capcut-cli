#!/usr/bin/env bash
# One-shot Codespace bootstrap: install ffmpeg, build the CLI, let the CLI
# install its own yt-dlp binary. No Python toolchain required.
set -euo pipefail

sudo apt-get update
sudo apt-get install -y ffmpeg jq

cargo build --release
./target/release/capcut-cli deps install
./target/release/capcut-cli deps check >/dev/null && echo "deps ok" >&2

echo "Run 'make clips' once TIKTOK_RESEARCH_ACCESS_TOKEN and TWITTER_BEARER_TOKEN are set." >&2
