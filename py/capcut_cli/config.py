"""Paths and constants."""
import os
from pathlib import Path

# Root of the capcut-cli repo (two levels up from this file)
REPO_ROOT = Path(__file__).resolve().parent.parent.parent

# Library paths
LIBRARY_DIR = REPO_ROOT / "library"
SOUNDS_DIR = LIBRARY_DIR / "sounds" / "assets"
CLIPS_DIR = LIBRARY_DIR / "clips"
OUTPUT_DIR = LIBRARY_DIR / "output"
TMP_DIR = LIBRARY_DIR / ".tmp"
MANIFEST_PATH = LIBRARY_DIR / "manifest.json"

# Tool paths
CAPCUT_HOME = Path.home() / ".capcut-cli"
BIN_DIR = CAPCUT_HOME / "bin"
YTDLP_PATH = BIN_DIR / "yt-dlp"

VERSION = "0.1.0"

# Loudness presets — target integrated loudness (LUFS), true peak (dBTP), range (LU).
# "viral" is the default because this tool exists to make social-media content.
LOUDNESS_PRESETS = {
    "viral":     {"lufs": -8.0,  "tp": -1.0, "lra": 7,  "label": "Social/viral — loud, punchy, cuts through feed scroll"},
    "social":    {"lufs": -10.0, "tp": -1.0, "lra": 9,  "label": "General social media"},
    "podcast":   {"lufs": -14.0, "tp": -1.5, "lra": 11, "label": "Podcast / spoken word (Apple, Spotify spec)"},
    "broadcast": {"lufs": -23.0, "tp": -1.0, "lra": 15, "label": "EBU R128 broadcast standard"},
}
DEFAULT_LOUDNESS = "viral"


def ensure_dirs():
    """Create all required directories."""
    for d in [SOUNDS_DIR, CLIPS_DIR, OUTPUT_DIR, TMP_DIR, BIN_DIR]:
        d.mkdir(parents=True, exist_ok=True)
