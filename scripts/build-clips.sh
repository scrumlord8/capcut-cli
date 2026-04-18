#!/usr/bin/env bash
# Discover one trending TikTok sound plus three ranked X clips, compose three
# finished MP4s, and stage them alongside the real source assets under ./clips.
#
# Required environment for real discovery:
#   TIKTOK_RESEARCH_ACCESS_TOKEN — TikTok Research API token
#   TWITTER_BEARER_TOKEN         — X/Twitter API bearer token
#
# Tunables (with defaults):
#   QUERY="ai agents"  REGION="US"  WINDOW_DAYS="7"
#   DURATION="15"      RESOLUTION="1080x1920"
#   MIN_LIKES="1000"   SOUND_LIMIT="5"  CLIP_LIMIT="10"
#   CLIPS_DIR="./clips"

set -euo pipefail

QUERY="${QUERY:-ai agents}"
REGION="${REGION:-US}"
WINDOW_DAYS="${WINDOW_DAYS:-7}"
DURATION="${DURATION:-15}"
RESOLUTION="${RESOLUTION:-1080x1920}"
MIN_LIKES="${MIN_LIKES:-1000}"
SOUND_LIMIT="${SOUND_LIMIT:-5}"
CLIP_LIMIT="${CLIP_LIMIT:-10}"
CLIPS_DIR="${CLIPS_DIR:-./clips}"
BIN="${BIN:-./target/release/capcut-cli}"

log() { printf '[build-clips] %s\n' "$*" >&2; }
die() { log "ERROR: $*"; exit 1; }

command -v jq >/dev/null || die "jq is required"
[[ -x "$BIN" ]] || die "capcut-cli binary not found at $BIN (run 'cargo build --release')"

"$BIN" deps check >/dev/null || die "deps check failed"

# ── 1. Discover trending TikTok sound ────────────────────────────────
log "discover tiktok-sounds (region=$REGION, window=${WINDOW_DAYS}d, limit=$SOUND_LIMIT)"
SOUND_JSON=$("$BIN" discover tiktok-sounds \
  --limit "$SOUND_LIMIT" --region "$REGION" --window-days "$WINDOW_DAYS" || true)
echo "$SOUND_JSON" | jq -e '.status == "ok"' >/dev/null 2>&1 \
  || { echo "$SOUND_JSON" >&2; die "tiktok-sounds discovery failed (is TIKTOK_RESEARCH_ACCESS_TOKEN set?)"; }

mapfile -t SOUND_URLS < <(echo "$SOUND_JSON" | jq -r '.data.sounds[].import_url // empty')
[[ ${#SOUND_URLS[@]} -gt 0 ]] || die "no sound candidates returned"

# ── 2. Discover trending X clips ─────────────────────────────────────
log "discover x-clips (query='$QUERY', min_likes=$MIN_LIKES, limit=$CLIP_LIMIT)"
CLIPS_JSON=$("$BIN" discover x-clips \
  --query "$QUERY" --limit "$CLIP_LIMIT" --min-likes "$MIN_LIKES" || true)
echo "$CLIPS_JSON" | jq -e '.status == "ok"' >/dev/null 2>&1 \
  || { echo "$CLIPS_JSON" >&2; die "x-clips discovery failed (is TWITTER_BEARER_TOKEN set?)"; }

mapfile -t CLIP_URLS < <(echo "$CLIPS_JSON" | jq -r '.data.clips[].import_url // empty')
[[ ${#CLIP_URLS[@]} -ge 3 ]] || die "need at least 3 clip candidates; got ${#CLIP_URLS[@]}"

# ── 3. Import the first sound that succeeds ──────────────────────────
SOUND_ID=""; SOUND_PATH=""; SOUND_FMT=""
for url in "${SOUND_URLS[@]}"; do
  log "importing sound: $url"
  if OUT=$("$BIN" library import "$url" --type sound --tags "trending,auto" 2>/dev/null); then
    if echo "$OUT" | jq -e '.status == "ok"' >/dev/null; then
      SOUND_ID=$(echo "$OUT" | jq -r '.data.id')
      SOUND_PATH=$(echo "$OUT" | jq -r '.data.file_path')
      SOUND_FMT=$(echo "$OUT" | jq -r '.data.format')
      break
    fi
  fi
  log "  skip (import failed)"
done
[[ -n "$SOUND_ID" ]] || die "no sound candidate imported successfully"
log "sound imported: id=$SOUND_ID path=$SOUND_PATH"

# ── 4. Import clips until we have three successes ────────────────────
CLIP_IDS=(); CLIP_PATHS=(); CLIP_FMTS=()
for url in "${CLIP_URLS[@]}"; do
  [[ ${#CLIP_IDS[@]} -ge 3 ]] && break
  log "importing clip: $url"
  if OUT=$("$BIN" library import "$url" --type clip --tags "trending,auto" 2>/dev/null); then
    if echo "$OUT" | jq -e '.status == "ok"' >/dev/null; then
      CLIP_IDS+=("$(echo "$OUT" | jq -r '.data.id')")
      CLIP_PATHS+=("$(echo "$OUT" | jq -r '.data.file_path')")
      CLIP_FMTS+=("$(echo "$OUT" | jq -r '.data.format')")
      continue
    fi
  fi
  log "  skip (import failed)"
done
[[ ${#CLIP_IDS[@]} -ge 3 ]] || die "fewer than 3 clips imported successfully (${#CLIP_IDS[@]})"

# ── 5. Compose three finished clips ──────────────────────────────────
rm -rf "$CLIPS_DIR"
mkdir -p "$CLIPS_DIR"

for i in 0 1 2; do
  n=$((i + 1))
  out="$CLIPS_DIR/clip_${n}.mp4"
  log "compose clip_${n} (sound=$SOUND_ID, clip=${CLIP_IDS[$i]})"
  "$BIN" compose \
    --sound "$SOUND_ID" \
    --clip "${CLIP_IDS[$i]}" \
    --duration "$DURATION" \
    --resolution "$RESOLUTION" \
    --output "$out" >/dev/null
  [[ -f "$out" ]] || die "compose did not produce $out"
done

# ── 6. Stage real source references alongside the finished clips ─────
cp "$SOUND_PATH" "$CLIPS_DIR/source_sound.${SOUND_FMT}"
for i in 0 1 2; do
  n=$((i + 1))
  cp "${CLIP_PATHS[$i]}" "$CLIPS_DIR/source_${n}.${CLIP_FMTS[$i]}"
done

# ── 7. Write a small manifest pointing at the real provenance ────────
jq -n \
  --arg query "$QUERY" --arg region "$REGION" \
  --arg duration "$DURATION" --arg resolution "$RESOLUTION" \
  --argjson sound "$(echo "$SOUND_JSON" | jq '.data.sounds[0]')" \
  --argjson clips "$(echo "$CLIPS_JSON" | jq "[.data.clips[0:${#CLIP_IDS[@]}][]]")" \
  '{query:$query, region:$region, duration_seconds:($duration|tonumber),
    resolution:$resolution, sound:$sound, clips:$clips}' \
  > "$CLIPS_DIR/manifest.json"

log "done — contents of $CLIPS_DIR:"
ls -la "$CLIPS_DIR" >&2
