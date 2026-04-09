"""yt-dlp subprocess wrapper for downloading sounds and clips."""
import json
import subprocess
from pathlib import Path
from typing import Optional

from capcut_cli.config import YTDLP_PATH
from capcut_cli import output as out


def _get_ffmpeg_dir() -> str:
    """Get a directory containing properly-named ffmpeg/ffprobe binaries."""
    from capcut_cli.deps.bootstrap import get_ffmpeg_path
    from capcut_cli.config import BIN_DIR
    import os

    ffmpeg_real = get_ffmpeg_path()
    # Create symlinks with standard names in our bin dir
    ffmpeg_link = BIN_DIR / "ffmpeg"
    ffprobe_link = BIN_DIR / "ffprobe"
    if not ffmpeg_link.exists():
        os.symlink(ffmpeg_real, str(ffmpeg_link))
    if not ffprobe_link.exists():
        os.symlink(ffmpeg_real, str(ffprobe_link))
    return str(BIN_DIR)


def _base_args(use_cookies: bool = False) -> list:
    """Common yt-dlp arguments."""
    args = ["--ffmpeg-location", _get_ffmpeg_dir()]
    if use_cookies:
        args += ["--cookies-from-browser", "chrome"]
    return args


def _run_ytdlp(args: list, timeout: int = 300, use_cookies: bool = False) -> subprocess.CompletedProcess:
    """Run yt-dlp with the given arguments."""
    cmd = [str(YTDLP_PATH)] + _base_args(use_cookies) + args
    out.log(f"Running: {' '.join(cmd)}")
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    # If blocked, retry with cookies from Chrome
    if result.returncode != 0 and not use_cookies and "blocked" in result.stderr.lower():
        out.log("Blocked without cookies, retrying with Chrome cookies...")
        cmd = [str(YTDLP_PATH)] + _base_args(use_cookies=True) + args
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    return result


def get_info(url: str) -> dict:
    """Extract metadata from a URL without downloading."""
    result = _run_ytdlp(["--dump-json", "--no-download", url])
    if result.returncode != 0:
        raise RuntimeError(f"yt-dlp metadata extraction failed: {result.stderr.strip()}")
    return json.loads(result.stdout)


def detect_platform(url: str) -> str:
    """Detect the platform from a URL."""
    url_lower = url.lower()
    if "tiktok.com" in url_lower:
        return "tiktok"
    elif "x.com" in url_lower or "twitter.com" in url_lower:
        return "twitter"
    elif "youtube.com" in url_lower or "youtu.be" in url_lower:
        return "youtube"
    elif "instagram.com" in url_lower:
        return "instagram"
    return "unknown"


def detect_asset_type(url: str, explicit_type: Optional[str] = None) -> str:
    """Detect whether a URL is a sound or clip."""
    if explicit_type:
        return explicit_type
    platform = detect_platform(url)
    url_lower = url.lower()
    if platform == "tiktok" and "/music/" in url_lower:
        return "sound"
    # Default: clips for video URLs, sounds for audio-only
    return "clip"


def download_sound(url: str, output_dir: Path) -> Path:
    """Download audio from a URL, extract as mp3."""
    # Step 1: Download best audio in native format
    raw_template = str(output_dir / "raw_audio.%(ext)s")
    result = _run_ytdlp([
        "-f", "bestaudio/best",
        "-o", raw_template,
        "--no-playlist",
        url,
    ])
    if result.returncode != 0:
        raise RuntimeError(f"yt-dlp download failed: {result.stderr.strip()}")

    raw_files = list(output_dir.glob("raw_audio.*"))
    if not raw_files:
        raise RuntimeError("Download succeeded but no audio file found.")
    raw_path = raw_files[0]

    # Step 2: Convert to mp3 using our ffmpeg
    from capcut_cli.deps.bootstrap import get_ffmpeg_path
    mp3_path = output_dir / "audio.mp3"
    ffmpeg_bin = get_ffmpeg_path()
    conv = subprocess.run(
        [ffmpeg_bin, "-i", str(raw_path), "-vn", "-acodec", "libmp3lame", "-q:a", "0", str(mp3_path), "-y"],
        capture_output=True, text=True, timeout=120,
    )
    if conv.returncode != 0:
        raise RuntimeError(f"ffmpeg audio conversion failed: {conv.stderr.strip()}")

    # Clean up raw file
    raw_path.unlink(missing_ok=True)
    return mp3_path


def download_clip(url: str, output_dir: Path) -> Path:
    """Download video from a URL as mp4."""
    output_template = str(output_dir / "video.%(ext)s")
    result = _run_ytdlp([
        "-f", "bestvideo[height<=1080]+bestaudio/best[height<=1080]/best",
        "--merge-output-format", "mp4",
        "-o", output_template,
        "--no-playlist",
        url,
    ])
    if result.returncode != 0:
        raise RuntimeError(f"yt-dlp download failed: {result.stderr.strip()}")

    # Find the output file
    video_files = list(output_dir.glob("video.*"))
    if not video_files:
        raise RuntimeError("Download succeeded but no video file found.")
    return video_files[0]
