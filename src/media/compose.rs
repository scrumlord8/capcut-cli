use anyhow::{Result, bail};
use std::path::PathBuf;

use crate::config::{output_dir, tmp_dir, LoudnessPreset, DEFAULT_LOUDNESS, LOUDNESS_PRESETS};
use crate::library;
use crate::media::ffmpeg;
use crate::models::ComposeResult;
use crate::output;

/// Resolve a loudness preset name (or raw LUFS value) to its parameters.
pub fn resolve_loudness(preset: Option<&str>) -> Result<LoudnessPreset> {
    let name = preset.unwrap_or(DEFAULT_LOUDNESS);

    if let Some(p) = LOUDNESS_PRESETS.get(name) {
        return Ok(p.clone());
    }

    // Allow raw LUFS value like "-8" or "-14.0"
    if let Ok(lufs) = name.parse::<f64>() {
        return Ok(LoudnessPreset {
            lufs,
            tp: -1.0,
            lra: 9.0,
            label: "custom",
        });
    }

    let available: Vec<_> = LOUDNESS_PRESETS.keys().collect();
    bail!(
        "Unknown loudness preset '{name}'. Available: {} — or pass a numeric LUFS value (e.g. -10).",
        available.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(", ")
    );
}

/// Run the full composition pipeline.
pub fn run_compose(
    sound_id: &str,
    clip_ids: &[String],
    duration_seconds: f64,
    output_path: Option<&str>,
    resolution: &str,
    loudness: Option<&str>,
) -> Result<ComposeResult> {
    // Parse resolution
    let parts: Vec<&str> = resolution.split('x').collect();
    if parts.len() != 2 {
        bail!("Invalid resolution format '{resolution}'. Expected WxH (e.g. 1080x1920).");
    }
    let width: u32 = parts[0].parse().unwrap_or(0);
    let height: u32 = parts[1].parse().unwrap_or(0);
    if width == 0 || height == 0 {
        bail!("Invalid resolution dimensions in '{resolution}'.");
    }

    // Validate inputs
    let sound = library::get_asset(sound_id)?
        .ok_or_else(|| anyhow::anyhow!("Sound '{sound_id}' not found in library."))?;
    if sound.asset_type != "sound" {
        bail!("Asset '{sound_id}' is a {}, not a sound.", sound.asset_type);
    }

    let mut clips = Vec::new();
    for cid in clip_ids {
        let clip = library::get_asset(cid)?
            .ok_or_else(|| anyhow::anyhow!("Clip '{cid}' not found in library."))?;
        clips.push(clip);
    }

    // Set up working directory
    let job_id = &uuid::Uuid::new_v4().to_string()[..8];
    let work_dir = tmp_dir().join(format!("compose_{job_id}"));
    std::fs::create_dir_all(&work_dir)?;

    let result = (|| -> Result<ComposeResult> {
        // Step 1: Normalize audio to target loudness
        let loud = resolve_loudness(loudness)?;
        output::log(&format!(
            "Step 1/5: Normalizing audio to {} LUFS ({})...",
            loud.lufs, loud.label
        ));
        let normalized_audio = work_dir.join("audio_normalized.mp3");
        ffmpeg::normalize_audio(
            &sound.file_path,
            &normalized_audio.to_string_lossy(),
            loud.lufs,
            loud.tp,
            loud.lra,
        )?;

        // Step 2: Trim audio to target duration
        output::log("Step 2/5: Trimming audio...");
        let trimmed_audio = work_dir.join("audio_trimmed.mp3");
        ffmpeg::trim_audio(
            &normalized_audio.to_string_lossy(),
            &trimmed_audio.to_string_lossy(),
            duration_seconds,
        )?;

        // Step 3: Trim and process each clip
        output::log("Step 3/5: Processing clips...");
        let mut processed_clips = Vec::new();
        let n_clips = clips.len();
        let segment_duration = duration_seconds / n_clips as f64;

        for (i, clip) in clips.iter().enumerate() {
            let mut clip_duration = clip.duration_seconds;
            if clip_duration <= 0.0 {
                clip_duration = ffmpeg::get_duration(&clip.file_path).unwrap_or(0.0);
            }

            let trim_dur = segment_duration.min(clip_duration);

            // Trim clip (fast, stream copy)
            let trimmed_path = work_dir.join(format!("clip_{i}_trimmed.mp4"));
            ffmpeg::trim_media(
                &clip.file_path,
                &trimmed_path.to_string_lossy(),
                0.0,
                trim_dur,
            )?;

            // Scale and crop to target resolution
            let scaled_path = work_dir.join(format!("clip_{i}_scaled.mp4"));
            ffmpeg::scale_and_crop(
                &trimmed_path.to_string_lossy(),
                &scaled_path.to_string_lossy(),
                width,
                height,
            )?;
            processed_clips.push(scaled_path.to_string_lossy().to_string());
        }

        // Step 4: Concatenate clips
        output::log("Step 4/5: Concatenating clips...");
        let concat_path = work_dir.join("concat.mp4");

        if n_clips == 1 {
            let actual_dur = ffmpeg::get_duration(&processed_clips[0]).unwrap_or(0.0);
            if actual_dur < duration_seconds {
                ffmpeg::loop_video(
                    &processed_clips[0],
                    &concat_path.to_string_lossy(),
                    duration_seconds,
                )?;
            } else {
                std::fs::copy(&processed_clips[0], &concat_path)?;
            }
        } else {
            ffmpeg::concat_videos(&processed_clips, &concat_path.to_string_lossy())?;
        }

        // Step 5: Mux audio + video
        output::log("Step 5/5: Muxing final output...");
        let final_path: PathBuf = if let Some(p) = output_path {
            PathBuf::from(p)
        } else {
            let out_dir = output_dir().join(format!("comp_{job_id}"));
            std::fs::create_dir_all(&out_dir)?;
            out_dir.join("final.mp4")
        };

        ffmpeg::mux_audio_video(
            &concat_path.to_string_lossy(),
            &trimmed_audio.to_string_lossy(),
            &final_path.to_string_lossy(),
            Some(duration_seconds),
        )?;

        let file_size = std::fs::metadata(&final_path)?.len();
        let actual_duration = ffmpeg::get_duration(&final_path.to_string_lossy()).unwrap_or(0.0);

        output::log(&format!(
            "Composed: {} ({actual_duration:.1}s, {file_size} bytes)",
            final_path.display()
        ));

        Ok(ComposeResult {
            output_path: final_path
                .canonicalize()
                .unwrap_or(final_path.clone())
                .to_string_lossy()
                .to_string(),
            duration_seconds: (actual_duration * 100.0).round() / 100.0,
            file_size_bytes: file_size,
            sound_id: sound_id.to_string(),
            clip_ids: clip_ids.to_vec(),
            resolution: resolution.to_string(),
        })
    })();

    // Clean up working directory
    let _ = std::fs::remove_dir_all(&work_dir);

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_resolve_loudness_default_is_viral() {
        let preset = resolve_loudness(None).unwrap();
        assert!((preset.lufs - (-8.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_resolve_loudness_named_preset() {
        let preset = resolve_loudness(Some("podcast")).unwrap();
        assert!((preset.lufs - (-14.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_resolve_loudness_numeric() {
        let preset = resolve_loudness(Some("-12")).unwrap();
        assert!((preset.lufs - (-12.0)).abs() < f64::EPSILON);
    }

    #[test]
    fn test_resolve_loudness_unknown_errors() {
        assert!(resolve_loudness(Some("nonexistent")).is_err());
    }

    #[test]
    fn test_viral_louder_than_podcast() {
        let viral = resolve_loudness(Some("viral")).unwrap();
        let podcast = resolve_loudness(Some("podcast")).unwrap();
        assert!(viral.lufs > podcast.lufs);
    }

    #[test]
    fn test_compose_smoke_with_existing_library_assets() {
        let assets = crate::library::list_assets(None).unwrap();
        let sound = assets
            .iter()
            .filter(|asset| asset.asset_type == "sound")
            .min_by(|a, b| a.duration_seconds.partial_cmp(&b.duration_seconds).unwrap())
            .expect("expected at least one sound asset in library manifest");
        let clip = assets
            .iter()
            .filter(|asset| asset.asset_type == "clip")
            .min_by(|a, b| a.duration_seconds.partial_cmp(&b.duration_seconds).unwrap())
            .expect("expected at least one clip asset in library manifest");

        let out = std::env::temp_dir().join(format!(
            "capcut-cli-compose-smoke-{}.mp4",
            uuid::Uuid::new_v4()
        ));
        let result = run_compose(
            &sound.id,
            &[clip.id.clone()],
            0.5,
            Some(out.to_str().unwrap()),
            "540x960",
            Some("social"),
        )
        .unwrap();

        assert!(Path::new(&result.output_path).exists());
        let _ = std::fs::remove_file(&result.output_path);
    }
}
