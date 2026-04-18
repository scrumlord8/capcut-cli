# Sound library

This directory holds imported sound assets and a small committed sample set
for inspection.

## Structure

- `assets/` — imported sound assets, one directory per asset id
  (e.g. `assets/snd_demo001/audio.mp3`). New imports land here.
- `samples/` — standalone committed audio samples kept for reference
- `manifest.json` — legacy seed manifest from the first deliverable; the
  authoritative library index now lives at the repo-root
  `library/manifest.json`

## Where metadata actually lives

Every imported sound is indexed in the top-level `library/manifest.json`
with:

- stable local id (e.g. `snd_demo001`)
- asset type (`sound`)
- title
- source URL (redacted of token-like query parameters)
- source platform
- download timestamp
- duration in seconds
- absolute file path on disk
- file size in bytes
- file format
- tags

A per-asset `meta.json` is also written alongside the audio file in
`assets/<asset_id>/meta.json` during import.
