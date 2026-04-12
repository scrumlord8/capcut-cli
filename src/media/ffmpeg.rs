use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

use crate::deps::get_ffmpeg_path;
use crate::output;

fn run_ffmpeg(args: &[&str], _timeout_secs: u64) -> Result<String> {
    let ffmpeg = get_ffmpeg_path()?;
    let display_args: Vec<_> = args.iter().rev().take(6).rev().collect();
    output::log(&format!("ffmpeg: {}", display_args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(" ")));

    let child = Command::new(&ffmpeg)
        .args(args)
        .output()
        .context("Failed to run ffmpeg")?;

    if !child.status.success() {
        let stderr = String::from_utf8_lossy(&child.stderr);
        let tail: String = stderr.chars().rev().take(500).collect::<String>().chars().rev().collect();
        bail!("ffmpeg failed: {tail}");
    }
    Ok(String::from_utf8_lossy(&child.stderr).to_string())
}

/// Get media duration in seconds.
pub fn get_duration(file_path: &str) -> Result<f64> {
    let ffmpeg = get_ffmpeg_path()?;
    let out = Command::new(&ffmpeg)
        .args(["-i", file_path, "-f", "null", "-"])
        .output()
        .context("Failed to probe duration")?;

    let stderr = String::from_utf8_lossy(&out.stderr);
    for line in stderr.lines() {
        if line.contains("Duration:") {
            if let Some(dur_str) = line.split("Duration:").nth(1) {
                let parts = dur_str.split(',').next().unwrap_or("").trim();
                if parts == "N/A" {
                    return Ok(0.0);
                }
                let segs: Vec<&str> = parts.split(':').collect();
                if segs.len() == 3 {
                    let h: f64 = segs[0].parse().unwrap_or(0.0);
                    let m: f64 = segs[1].parse().unwrap_or(0.0);
                    let s: f64 = segs[2].parse().unwrap_or(0.0);
                    return Ok(h * 3600.0 + m * 60.0 + s);
                }
            }
        }
    }
    Ok(0.0)
}

/// Loudness-normalize audio using loudnorm filter.
pub fn normalize_audio(
    input_path: &str,
    output_path: &str,
    target_lufs: f64,
    true_peak: f64,
    loudness_range: f64,
) -> Result<()> {
    let af = format!("loudnorm=I={target_lufs}:TP={true_peak}:LRA={loudness_range}");
    run_ffmpeg(
        &["-i", input_path, "-af", &af, "-ar", "44100", "-y", output_path],
        300,
    )?;
    Ok(())
}

/// Trim media to a segment using stream copy (fast).
pub fn trim_media(input_path: &str, output_path: &str, start: f64, duration: f64) -> Result<()> {
    let start_s = start.to_string();
    let dur_s = duration.to_string();
    run_ffmpeg(
        &["-ss", &start_s, "-i", input_path, "-t", &dur_s, "-c", "copy", "-y", output_path],
        300,
    )?;
    Ok(())
}

/// Trim audio to a specific duration.
pub fn trim_audio(input_path: &str, output_path: &str, duration: f64) -> Result<()> {
    let dur_s = duration.to_string();
    run_ffmpeg(
        &["-i", input_path, "-t", &dur_s, "-acodec", "libmp3lame", "-y", output_path],
        300,
    )?;
    Ok(())
}

/// Scale and center-crop video to exact dimensions.
pub fn scale_and_crop(input_path: &str, output_path: &str, width: u32, height: u32) -> Result<()> {
    let vf = format!(
        "scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height}"
    );
    run_ffmpeg(
        &[
            "-i", input_path,
            "-vf", &vf,
            "-c:v", "libx264", "-preset", "fast", "-crf", "23",
            "-an",
            "-y", output_path,
        ],
        300,
    )?;
    Ok(())
}

/// Concatenate video files using the concat demuxer.
pub fn concat_videos(input_paths: &[String], output_path: &str) -> Result<()> {
    if input_paths.len() == 1 {
        std::fs::copy(&input_paths[0], output_path)?;
        return Ok(());
    }

    let out_dir = Path::new(output_path).parent().unwrap();
    let concat_file = out_dir.join("concat_list.txt");
    let contents: String = input_paths
        .iter()
        .map(|p| format!("file '{p}'"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&concat_file, &contents)?;

    run_ffmpeg(
        &[
            "-f", "concat",
            "-safe", "0",
            "-i", &concat_file.to_string_lossy(),
            "-c", "copy",
            "-y", output_path,
        ],
        300,
    )?;
    let _ = std::fs::remove_file(&concat_file);
    Ok(())
}

/// Combine video and audio into final output.
pub fn mux_audio_video(
    video_path: &str,
    audio_path: &str,
    output_path: &str,
    duration: Option<f64>,
) -> Result<()> {
    let mut args = vec![
        "-i", video_path,
        "-i", audio_path,
        "-c:v", "copy",
        "-c:a", "aac",
        "-b:a", "192k",
        "-map", "0:v:0",
        "-map", "1:a:0",
        "-shortest",
    ];
    let dur_s;
    if let Some(d) = duration {
        dur_s = d.to_string();
        args.push("-t");
        args.push(&dur_s);
    }
    args.push("-y");
    args.push(output_path);
    run_ffmpeg(&args, 300)?;
    Ok(())
}

/// Loop a video to fill a target duration.
pub fn loop_video(input_path: &str, output_path: &str, duration: f64) -> Result<()> {
    let dur_s = duration.to_string();
    run_ffmpeg(
        &[
            "-stream_loop", "-1",
            "-i", input_path,
            "-t", &dur_s,
            "-c:v", "libx264", "-preset", "fast", "-crf", "23",
            "-an",
            "-y", output_path,
        ],
        300,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_duration_format() {
        // Simulate ffmpeg duration line parsing
        let line = "  Duration: 00:01:30.50, start: 0.000000, bitrate: 128 kb/s";
        assert!(line.contains("Duration:"));
        let dur_str = line.split("Duration:").nth(1).unwrap();
        let parts = dur_str.split(',').next().unwrap().trim();
        let segs: Vec<&str> = parts.split(':').collect();
        let h: f64 = segs[0].parse().unwrap();
        let m: f64 = segs[1].parse().unwrap();
        let s: f64 = segs[2].parse().unwrap();
        let total = h * 3600.0 + m * 60.0 + s;
        assert!((total - 90.5).abs() < 0.01);
    }
}
