#!/usr/bin/env bash
# Compose three finished MP4s from one supplied trending sound URL plus three
# supplied source clip URLs. No discovery APIs; the caller brings the links.
#
# Usage:
#   SOUND_URL=https://... CLIP_URLS="https://a https://b https://c" \
#     ./scripts/build-clips-from-urls.sh
#
# Tunables (with defaults):
#   DURATION="15"   RESOLUTION="1080x1920"   CLIPS_DIR="./clips"

set -euo pipefail

: "${SOUND_URL:?SOUND_URL is required}"
: "${CLIP_URLS:?CLIP_URLS is required (space-separated list of three URLs)}"

DURATION="${DURATION:-15}"
RESOLUTION="${RESOLUTION:-1080x1920}"
CLIPS_DIR="${CLIPS_DIR:-./clips}"
BIN="${BIN:-./target/release/capcut-cli}"

log() { printf '[build-clips] %s\n' "$*" >&2; }
die() { log "ERROR: $*"; exit 1; }

command -v jq >/dev/null || die "jq is required"
[[ -x "$BIN" ]] || die "capcut-cli binary not found at $BIN (run 'cargo build --release')"

read -r -a CLIP_ARR <<< "$CLIP_URLS"
[[ ${#CLIP_ARR[@]} -eq 3 ]] || die "CLIP_URLS must contain exactly three URLs (got ${#CLIP_ARR[@]})"

"$BIN" deps check >/dev/null || die "deps check failed"

# ── Import the supplied sound ────────────────────────────────────────
log "importing sound: $SOUND_URL"
SOUND_JSON=$("$BIN" library import "$SOUND_URL" --type sound --tags "manual,supplied" || true)
echo "$SOUND_JSON" | jq -e '.status == "ok"' >/dev/null 2>&1 \
  || { echo "$SOUND_JSON" >&2; die "sound import failed"; }
SOUND_ID=$(echo "$SOUND_JSON" | jq -r '.data.id')
SOUND_PATH=$(echo "$SOUND_JSON" | jq -r '.data.file_path')
SOUND_FMT=$(echo "$SOUND_JSON" | jq -r '.data.format')

# ── Import each supplied clip ────────────────────────────────────────
CLIP_IDS=(); CLIP_PATHS=(); CLIP_FMTS=()
for url in "${CLIP_ARR[@]}"; do
  log "importing clip: $url"
  OUT=$("$BIN" library import "$url" --type clip --tags "manual,supplied" || true)
  echo "$OUT" | jq -e '.status == "ok"' >/dev/null 2>&1 \
    || { echo "$OUT" >&2; die "clip import failed for $url"; }
  CLIP_IDS+=("$(echo "$OUT" | jq -r '.data.id')")
  CLIP_PATHS+=("$(echo "$OUT" | jq -r '.data.file_path')")
  CLIP_FMTS+=("$(echo "$OUT" | jq -r '.data.format')")
done

# ── Compose three finished clips ─────────────────────────────────────
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

# ── Stage real source references alongside the finished clips ────────
cp "$SOUND_PATH" "$CLIPS_DIR/source_sound.${SOUND_FMT}"
for i in 0 1 2; do
  n=$((i + 1))
  cp "${CLIP_PATHS[$i]}" "$CLIPS_DIR/source_${n}.${CLIP_FMTS[$i]}"
done

# ── Provenance manifest ──────────────────────────────────────────────
jq -n \
  --arg sound_url "$SOUND_URL" \
  --arg duration "$DURATION" --arg resolution "$RESOLUTION" \
  --argjson clip_urls "$(printf '%s\n' "${CLIP_ARR[@]}" | jq -R . | jq -s .)" \
  '{source:"supplied-urls", sound_url:$sound_url, clip_urls:$clip_urls,
    duration_seconds:($duration|tonumber), resolution:$resolution}' \
  > "$CLIPS_DIR/manifest.json"

log "done — contents of $CLIPS_DIR:"
ls -la "$CLIPS_DIR" >&2
