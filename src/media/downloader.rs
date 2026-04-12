use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{bin_dir, ytdlp_path};
use crate::deps::get_ffmpeg_path;
use crate::output;

fn base_args() -> Vec<String> {
    let ffmpeg_dir = bin_dir();
    // Ensure ffmpeg/ffprobe symlinks exist in bin dir for yt-dlp
    let _ = ensure_ffmpeg_symlinks();
    vec!["--ffmpeg-location".to_string(), ffmpeg_dir.to_string_lossy().to_string()]
}

fn ensure_ffmpeg_symlinks() -> Result<()> {
    let ffmpeg_real = get_ffmpeg_path()?;
    let bin = bin_dir();
    let ffmpeg_link = bin.join("ffmpeg");
    let ffprobe_link = bin.join("ffprobe");

    if !ffmpeg_link.exists() && ffmpeg_real != "ffmpeg" {
        let _ = std::os::unix::fs::symlink(&ffmpeg_real, &ffmpeg_link);
    }
    if !ffprobe_link.exists() && ffmpeg_real != "ffmpeg" {
        let _ = std::os::unix::fs::symlink(&ffmpeg_real, &ffprobe_link);
    }
    Ok(())
}

fn run_ytdlp(args: &[&str], use_cookies: bool) -> Result<std::process::Output> {
    let ytdlp = ytdlp_path();
    if !ytdlp.exists() {
        bail!(
            "yt-dlp not found at {}. Run 'capcut-cli deps install' first.",
            ytdlp.display()
        );
    }

    let mut cmd_args: Vec<String> = base_args();
    if use_cookies {
        cmd_args.push("--cookies-from-browser".to_string());
        cmd_args.push("chrome".to_string());
    }
    for a in args {
        cmd_args.push(a.to_string());
    }

    output::log(&format!("Running: yt-dlp {}", cmd_args.join(" ")));

    let result = Command::new(ytdlp.to_string_lossy().as_ref())
        .args(&cmd_args)
        .output()
        .context("Failed to run yt-dlp")?;

    // If blocked, retry with cookies
    if !result.status.success() && !use_cookies {
        let stderr = String::from_utf8_lossy(&result.stderr).to_lowercase();
        if stderr.contains("blocked") {
            output::log("Blocked without cookies, retrying with Chrome cookies...");
            return run_ytdlp(args, true);
        }
    }
    Ok(result)
}

/// Extract metadata from a URL without downloading.
pub fn get_info(url: &str) -> Result<serde_json::Value> {
    let result = run_ytdlp(&["--dump-json", "--no-download", url], false)?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        bail!("yt-dlp metadata extraction failed: {}", stderr.trim());
    }
    let info: serde_json::Value = serde_json::from_slice(&result.stdout)
        .context("Failed to parse yt-dlp JSON output")?;
    Ok(info)
}

/// Detect the platform from a URL.
pub fn detect_platform(url: &str) -> &'static str {
    let lower = url.to_lowercase();
    if lower.contains("tiktok.com") {
        "tiktok"
    } else if lower.contains("x.com") || lower.contains("twitter.com") {
        "twitter"
    } else if lower.contains("youtube.com") || lower.contains("youtu.be") {
        "youtube"
    } else if lower.contains("instagram.com") {
        "instagram"
    } else {
        "unknown"
    }
}

/// Detect whether a URL is a sound or clip.
pub fn detect_asset_type(url: &str, explicit: Option<&str>) -> &'static str {
    if let Some(t) = explicit {
        if t == "sound" { return "sound"; }
        return "clip";
    }
    let lower = url.to_lowercase();
    if detect_platform(url) == "tiktok" && lower.contains("/music/") {
        return "sound";
    }
    "clip"
}

/// Download audio from a URL, extract as mp3.
pub fn download_sound(url: &str, output_dir: &Path) -> Result<PathBuf> {
    // Step 1: Download best audio
    let raw_template = output_dir.join("raw_audio.%(ext)s");
    let result = run_ytdlp(
        &["-f", "bestaudio/best", "-o", &raw_template.to_string_lossy(), "--no-playlist", url],
        false,
    )?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        bail!("yt-dlp download failed: {}", stderr.trim());
    }

    // Find the downloaded raw file
    let raw_path = find_file_matching(output_dir, "raw_audio.")?;

    // Step 2: Convert to mp3
    let mp3_path = output_dir.join("audio.mp3");
    let ffmpeg = get_ffmpeg_path()?;
    let conv = Command::new(&ffmpeg)
        .args([
            "-i", &raw_path.to_string_lossy(),
            "-vn", "-acodec", "libmp3lame", "-q:a", "0",
            &mp3_path.to_string_lossy(), "-y",
        ])
        .output()
        .context("Failed to run ffmpeg for audio conversion")?;

    if !conv.status.success() {
        let stderr = String::from_utf8_lossy(&conv.stderr);
        bail!("ffmpeg audio conversion failed: {}", stderr.trim());
    }

    // Clean up raw file
    let _ = std::fs::remove_file(&raw_path);
    Ok(mp3_path)
}

/// Download video from a URL as mp4.
pub fn download_clip(url: &str, output_dir: &Path) -> Result<PathBuf> {
    let output_template = output_dir.join("video.%(ext)s");
    let result = run_ytdlp(
        &[
            "-f", "bestvideo[height<=1080]+bestaudio/best[height<=1080]/best",
            "--merge-output-format", "mp4",
            "-o", &output_template.to_string_lossy(),
            "--no-playlist",
            url,
        ],
        false,
    )?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        bail!("yt-dlp download failed: {}", stderr.trim());
    }

    find_file_matching(output_dir, "video.")
}

fn find_file_matching(dir: &Path, prefix: &str) -> Result<PathBuf> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with(prefix) {
            return Ok(entry.path());
        }
    }
    bail!("Download succeeded but no file matching '{prefix}*' found in {}", dir.display())
}
