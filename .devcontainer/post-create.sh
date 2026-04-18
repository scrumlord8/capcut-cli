#!/usr/bin/env bash
# One-shot Codespace bootstrap: install ffmpeg + yt-dlp, build the CLI.
set -euo pipefail

sudo apt-get update
sudo apt-get install -y ffmpeg jq

python3 -m pip install --quiet --upgrade yt-dlp
mkdir -p "$HOME/.capcut-cli/bin"
ln -sf "$(command -v yt-dlp)" "$HOME/.capcut-cli/bin/yt-dlp"

cargo build --release

./target/release/capcut-cli deps check >/dev/null && echo "deps ok" >&2
echo "Run 'make clips' once TIKTOK_RESEARCH_ACCESS_TOKEN and TWITTER_BEARER_TOKEN are set." >&2
