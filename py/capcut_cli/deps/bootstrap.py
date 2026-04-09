"""Dependency management: download yt-dlp binary, check ffmpeg."""
import os
import platform
import stat
import subprocess
import urllib.request
from pathlib import Path

from capcut_cli.config import BIN_DIR, YTDLP_PATH


def get_ffmpeg_path() -> str:
    """Get ffmpeg binary path from imageio-ffmpeg."""
    import imageio_ffmpeg
    return imageio_ffmpeg.get_ffmpeg_exe()


def get_ffprobe_path() -> str:
    """Get ffprobe path — imageio-ffmpeg bundles ffmpeg, we derive ffprobe from it."""
    ffmpeg = get_ffmpeg_path()
    ffprobe = Path(ffmpeg).parent / "ffprobe"
    if ffprobe.exists():
        return str(ffprobe)
    # Fallback: try system ffprobe
    try:
        subprocess.run(["ffprobe", "-version"], capture_output=True, check=True)
        return "ffprobe"
    except (FileNotFoundError, subprocess.CalledProcessError):
        return None


def download_ytdlp():
    """Download the yt-dlp standalone binary for the current platform."""
    BIN_DIR.mkdir(parents=True, exist_ok=True)

    system = platform.system().lower()
    if system == "darwin":
        url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos"
    elif system == "linux":
        url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux"
    else:
        raise RuntimeError(f"Unsupported platform: {system}")

    print(f"Downloading yt-dlp from {url}...", flush=True)
    urllib.request.urlretrieve(url, str(YTDLP_PATH))

    # Make executable
    st = os.stat(YTDLP_PATH)
    os.chmod(YTDLP_PATH, st.st_mode | stat.S_IEXEC | stat.S_IXGRP | stat.S_IXOTH)
    print(f"yt-dlp installed to {YTDLP_PATH}")


def check_ytdlp() -> dict:
    """Check if yt-dlp is available and return version info."""
    if not YTDLP_PATH.exists():
        return {"installed": False, "path": None, "version": None}
    try:
        result = subprocess.run(
            [str(YTDLP_PATH), "--version"],
            capture_output=True, text=True, timeout=10,
        )
        return {
            "installed": True,
            "path": str(YTDLP_PATH),
            "version": result.stdout.strip(),
        }
    except Exception as e:
        return {"installed": False, "path": str(YTDLP_PATH), "error": str(e)}


def check_ffmpeg() -> dict:
    """Check if ffmpeg is available via imageio-ffmpeg."""
    try:
        ffmpeg = get_ffmpeg_path()
        result = subprocess.run(
            [ffmpeg, "-version"],
            capture_output=True, text=True, timeout=10,
        )
        version_line = result.stdout.split("\n")[0] if result.stdout else "unknown"
        return {
            "installed": True,
            "path": ffmpeg,
            "version": version_line,
        }
    except Exception as e:
        return {"installed": False, "path": None, "error": str(e)}


def check_all() -> dict:
    """Check all dependencies."""
    return {
        "yt_dlp": check_ytdlp(),
        "ffmpeg": check_ffmpeg(),
    }


def install_all():
    """Install all dependencies."""
    results = {}

    # yt-dlp
    if not YTDLP_PATH.exists():
        download_ytdlp()
    results["yt_dlp"] = check_ytdlp()

    # ffmpeg — comes with imageio-ffmpeg pip package
    results["ffmpeg"] = check_ffmpeg()

    return results
