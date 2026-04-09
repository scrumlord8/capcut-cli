"""Asset storage: filesystem + JSON manifest."""
import json
import os
import shutil
import subprocess
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional, List

from capcut_cli.config import SOUNDS_DIR, CLIPS_DIR, MANIFEST_PATH, YTDLP_PATH
from capcut_cli.models import Asset
from capcut_cli.media.downloader import (
    detect_platform, detect_asset_type, download_sound, download_clip, get_info,
)
from capcut_cli import output as out


def _gen_id(asset_type: str) -> str:
    prefix = "snd" if asset_type == "sound" else "clp"
    return f"{prefix}_{uuid.uuid4().hex[:8]}"


def _get_duration(file_path: str) -> float:
    """Get duration via ffprobe."""
    try:
        from capcut_cli.deps.bootstrap import get_ffmpeg_path
        ffmpeg = get_ffmpeg_path()
        ffprobe = str(Path(ffmpeg).parent / "ffprobe")
        if not Path(ffprobe).exists():
            ffprobe = ffmpeg  # fallback

        # Use ffmpeg to probe
        result = subprocess.run(
            [ffmpeg, "-i", file_path, "-f", "null", "-"],
            capture_output=True, text=True, timeout=30,
        )
        # Parse duration from stderr
        for line in result.stderr.split("\n"):
            if "Duration:" in line:
                parts = line.split("Duration:")[1].split(",")[0].strip()
                h, m, s = parts.split(":")
                return float(h) * 3600 + float(m) * 60 + float(s)
    except Exception:
        pass
    return 0.0


def _read_manifest() -> dict:
    """Read the manifest file."""
    if MANIFEST_PATH.exists():
        with open(MANIFEST_PATH) as f:
            return json.load(f)
    return {"version": 1, "assets": []}


def _write_manifest(manifest: dict):
    """Write the manifest file."""
    MANIFEST_PATH.parent.mkdir(parents=True, exist_ok=True)
    with open(MANIFEST_PATH, "w") as f:
        json.dump(manifest, f, indent=2, default=str)


def import_asset(url: str, asset_type: Optional[str] = None, tags: Optional[List[str]] = None) -> Asset:
    """Download and import an asset from a URL."""
    platform = detect_platform(url)
    atype = detect_asset_type(url, asset_type)
    asset_id = _gen_id(atype)
    tags = tags or []

    out.log(f"Importing {atype} from {platform}: {url}")

    # Create asset directory
    if atype == "sound":
        asset_dir = SOUNDS_DIR / asset_id
    else:
        asset_dir = CLIPS_DIR / asset_id
    asset_dir.mkdir(parents=True, exist_ok=True)

    # Get metadata first
    out.log("Extracting metadata...")
    try:
        info = get_info(url)
        title = info.get("title", "Untitled")
    except Exception:
        info = {}
        title = "Untitled"

    # Download
    out.log(f"Downloading {atype}...")
    if atype == "sound":
        file_path = download_sound(url, asset_dir)
    else:
        file_path = download_clip(url, asset_dir)

    # Get file info
    file_size = file_path.stat().st_size
    duration = _get_duration(str(file_path))
    if duration == 0.0 and info.get("duration"):
        duration = float(info["duration"])

    asset = Asset(
        id=asset_id,
        type=atype,
        title=title,
        source_url=url,
        source_platform=platform,
        downloaded_at=datetime.now(timezone.utc).isoformat(),
        duration_seconds=round(duration, 2),
        file_path=str(file_path.resolve()),
        file_size_bytes=file_size,
        format=file_path.suffix.lstrip("."),
        tags=tags,
    )

    # Save meta.json
    meta_path = asset_dir / "meta.json"
    with open(meta_path, "w") as f:
        json.dump(asset.to_dict(), f, indent=2, default=str)

    # Update manifest
    manifest = _read_manifest()
    manifest["assets"].append(asset.to_dict())
    _write_manifest(manifest)

    out.log(f"Imported: {asset_id} ({title})")
    return asset


def list_assets(asset_type: Optional[str] = None) -> List[Asset]:
    """List all assets, optionally filtered by type."""
    manifest = _read_manifest()
    assets = []
    for entry in manifest.get("assets", []):
        if asset_type and entry.get("type") != asset_type:
            continue
        assets.append(Asset(**{k: v for k, v in entry.items() if k in Asset.__dataclass_fields__}))
    return assets


def get_asset(asset_id: str) -> Optional[Asset]:
    """Get a specific asset by ID."""
    manifest = _read_manifest()
    for entry in manifest.get("assets", []):
        if entry.get("id") == asset_id:
            return Asset(**{k: v for k, v in entry.items() if k in Asset.__dataclass_fields__})
    return None


def delete_asset(asset_id: str):
    """Delete an asset from the library."""
    manifest = _read_manifest()
    found = False
    new_assets = []
    for entry in manifest.get("assets", []):
        if entry.get("id") == asset_id:
            found = True
            # Remove the asset directory
            file_path = Path(entry.get("file_path", ""))
            asset_dir = file_path.parent
            if asset_dir.exists():
                shutil.rmtree(asset_dir)
        else:
            new_assets.append(entry)

    if not found:
        raise RuntimeError(f"Asset '{asset_id}' not found.")

    manifest["assets"] = new_assets
    _write_manifest(manifest)
