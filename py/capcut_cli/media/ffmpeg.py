"""FFmpeg subprocess wrappers for media processing."""
import subprocess
from pathlib import Path
from typing import List, Optional

from capcut_cli import output as out


def _get_ffmpeg() -> str:
    from capcut_cli.deps.bootstrap import get_ffmpeg_path
    return get_ffmpeg_path()


def _run_ffmpeg(args: list, timeout: int = 300) -> subprocess.CompletedProcess:
    cmd = [_get_ffmpeg()] + args
    out.log(f"ffmpeg: {' '.join(cmd[-6:])}")
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
    if result.returncode != 0:
        raise RuntimeError(f"ffmpeg failed: {result.stderr[-500:]}")
    return result


def get_duration(file_path: str) -> float:
    """Get media duration in seconds."""
    result = subprocess.run(
        [_get_ffmpeg(), "-i", file_path, "-f", "null", "-"],
        capture_output=True, text=True, timeout=30,
    )
    for line in result.stderr.split("\n"):
        if "Duration:" in line:
            parts = line.split("Duration:")[1].split(",")[0].strip()
            if parts == "N/A":
                return 0.0
            h, m, s = parts.split(":")
            return float(h) * 3600 + float(m) * 60 + float(s)
    return 0.0


def normalize_audio(
    input_path: str,
    output_path: str,
    target_lufs: float = -8.0,
    true_peak: float = -1.0,
    loudness_range: float = 7,
):
    """Loudness-normalize audio using loudnorm filter."""
    _run_ffmpeg([
        "-i", input_path,
        "-af", f"loudnorm=I={target_lufs}:TP={true_peak}:LRA={loudness_range}",
        "-ar", "44100",
        "-y", output_path,
    ])


def trim_media(input_path: str, output_path: str, start: float, duration: float):
    """Trim media to a segment."""
    _run_ffmpeg([
        "-ss", str(start),
        "-i", input_path,
        "-t", str(duration),
        "-c", "copy",
        "-y", output_path,
    ])


def trim_audio(input_path: str, output_path: str, duration: float):
    """Trim audio to a specific duration."""
    _run_ffmpeg([
        "-i", input_path,
        "-t", str(duration),
        "-acodec", "libmp3lame",
        "-y", output_path,
    ])


def scale_and_crop(input_path: str, output_path: str, width: int, height: int):
    """Scale and center-crop video to exact dimensions."""
    # Scale to fill, then crop to exact size
    _run_ffmpeg([
        "-i", input_path,
        "-vf", f"scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height}",
        "-c:v", "libx264",
        "-preset", "fast",
        "-crf", "23",
        "-an",  # strip audio, we'll mux separately
        "-y", output_path,
    ])


def concat_videos(input_paths: List[str], output_path: str):
    """Concatenate video files using the concat demuxer."""
    if len(input_paths) == 1:
        import shutil
        shutil.copy2(input_paths[0], output_path)
        return

    # Create concat file
    concat_file = Path(output_path).parent / "concat_list.txt"
    with open(concat_file, "w") as f:
        for p in input_paths:
            f.write(f"file '{p}'\n")

    _run_ffmpeg([
        "-f", "concat",
        "-safe", "0",
        "-i", str(concat_file),
        "-c", "copy",
        "-y", output_path,
    ])
    concat_file.unlink(missing_ok=True)


def mux_audio_video(video_path: str, audio_path: str, output_path: str, duration: Optional[float] = None):
    """Combine video and audio into final output."""
    args = [
        "-i", video_path,
        "-i", audio_path,
        "-c:v", "copy",
        "-c:a", "aac",
        "-b:a", "192k",
        "-map", "0:v:0",
        "-map", "1:a:0",
        "-shortest",
    ]
    if duration:
        args += ["-t", str(duration)]
    args += ["-y", output_path]
    _run_ffmpeg(args)


def loop_video(input_path: str, output_path: str, duration: float):
    """Loop a video to fill a target duration."""
    _run_ffmpeg([
        "-stream_loop", "-1",
        "-i", input_path,
        "-t", str(duration),
        "-c:v", "libx264",
        "-preset", "fast",
        "-crf", "23",
        "-an",
        "-y", output_path,
    ])
