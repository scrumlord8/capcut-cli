"""Composition pipeline: combine sounds + clips into final video."""
import shutil
import uuid
from pathlib import Path

from capcut_cli.config import OUTPUT_DIR, TMP_DIR, LOUDNESS_PRESETS, DEFAULT_LOUDNESS
from capcut_cli.library.store import get_asset
from capcut_cli.media import ffmpeg
from capcut_cli.models import ComposeResult
from capcut_cli import output as out


def resolve_loudness(preset: str | None) -> dict:
    """Resolve a loudness preset name to its parameters."""
    name = preset or DEFAULT_LOUDNESS
    if name in LOUDNESS_PRESETS:
        return LOUDNESS_PRESETS[name]
    # Allow raw LUFS value like "-8" or "-14.0"
    try:
        lufs = float(name)
        return {"lufs": lufs, "tp": -1.0, "lra": 9, "label": f"custom ({lufs} LUFS)"}
    except ValueError:
        available = ", ".join(LOUDNESS_PRESETS.keys())
        raise RuntimeError(
            f"Unknown loudness preset '{name}'. "
            f"Available: {available} — or pass a numeric LUFS value (e.g. -10)."
        )


def run_compose(
    sound_id: str,
    clip_ids: list,
    duration_seconds: float = 30.0,
    output_path: str = None,
    resolution: str = "1080x1920",
    loudness: str = None,
) -> ComposeResult:
    """Run the full composition pipeline."""
    # Parse resolution
    width, height = map(int, resolution.split("x"))

    # Validate inputs
    sound = get_asset(sound_id)
    if sound is None:
        raise RuntimeError(f"Sound '{sound_id}' not found in library.")
    if sound.type != "sound":
        raise RuntimeError(f"Asset '{sound_id}' is a {sound.type}, not a sound.")

    clips = []
    for cid in clip_ids:
        clip = get_asset(cid)
        if clip is None:
            raise RuntimeError(f"Clip '{cid}' not found in library.")
        clips.append(clip)

    # Set up working directory
    job_id = uuid.uuid4().hex[:8]
    work_dir = TMP_DIR / f"compose_{job_id}"
    work_dir.mkdir(parents=True, exist_ok=True)

    try:
        # Step 1: Normalize audio to target loudness
        loud = resolve_loudness(loudness)
        out.log(f"Step 1/5: Normalizing audio to {loud['lufs']} LUFS ({loud['label']})...")
        normalized_audio = str(work_dir / "audio_normalized.mp3")
        ffmpeg.normalize_audio(
            sound.file_path, normalized_audio,
            target_lufs=loud["lufs"],
            true_peak=loud["tp"],
            loudness_range=loud["lra"],
        )

        # Step 2: Trim audio to target duration
        out.log("Step 2/5: Trimming audio...")
        trimmed_audio = str(work_dir / "audio_trimmed.mp3")
        ffmpeg.trim_audio(normalized_audio, trimmed_audio, duration_seconds)

        # Step 3: Trim and process each clip — trim first, then scale/crop
        out.log("Step 3/5: Processing clips...")
        processed_clips = []
        n_clips = len(clips)
        segment_duration = duration_seconds / n_clips

        for i, clip in enumerate(clips):
            clip_duration = clip.duration_seconds
            if clip_duration <= 0:
                clip_duration = ffmpeg.get_duration(clip.file_path)

            # Trim clip to its allocated segment BEFORE scaling (fast, uses stream copy)
            trim_dur = min(segment_duration, clip_duration)
            trimmed_path = str(work_dir / f"clip_{i}_trimmed.mp4")
            ffmpeg.trim_media(clip.file_path, trimmed_path, 0, trim_dur)

            # Scale and crop the trimmed clip to target resolution
            scaled_path = str(work_dir / f"clip_{i}_scaled.mp4")
            ffmpeg.scale_and_crop(trimmed_path, scaled_path, width, height)
            processed_clips.append(scaled_path)

        # Step 4: Concatenate clips (or loop if single clip is shorter than target)
        out.log("Step 4/5: Concatenating clips...")
        concat_path = str(work_dir / "concat.mp4")

        if n_clips == 1:
            actual_clip_dur = ffmpeg.get_duration(processed_clips[0])
            if actual_clip_dur < duration_seconds:
                ffmpeg.loop_video(processed_clips[0], concat_path, duration_seconds)
            else:
                import shutil as _sh
                _sh.copy2(processed_clips[0], concat_path)
        else:
            ffmpeg.concat_videos(processed_clips, concat_path)

        # Step 5: Mux audio + video
        out.log("Step 5/5: Muxing final output...")
        if output_path is None:
            out_dir = OUTPUT_DIR / f"comp_{job_id}"
            out_dir.mkdir(parents=True, exist_ok=True)
            output_path = str(out_dir / "final.mp4")

        ffmpeg.mux_audio_video(concat_path, trimmed_audio, output_path, duration_seconds)

        # Get final file info
        final_path = Path(output_path)
        file_size = final_path.stat().st_size
        actual_duration = ffmpeg.get_duration(output_path)

        out.log(f"Composed: {output_path} ({actual_duration:.1f}s, {file_size} bytes)")

        return ComposeResult(
            output_path=str(final_path.resolve()),
            duration_seconds=round(actual_duration, 2),
            file_size_bytes=file_size,
            sound_id=sound_id,
            clip_ids=clip_ids,
            resolution=resolution,
        )
    finally:
        # Clean up working directory
        shutil.rmtree(work_dir, ignore_errors=True)
