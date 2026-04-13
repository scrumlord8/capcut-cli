use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;
use thiserror::Error;

use crate::config::{bin_dir, ytdlp_path};
use crate::deps::get_ffmpeg_path;
use crate::output;

const REDACT_KEYS: &[&str] = &[
    "token",
    "access_token",
    "refresh_token",
    "authorization",
    "signature",
    "sig",
    "x-signature",
    "x-amz-signature",
    "cookie",
    "cookies",
];

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("X/Twitter media import requires authenticated browser cookies. Tried browsers: {browsers}.")]
    XAuthRequired { browsers: String },
    #[error("X/Twitter media import is rate limited.")]
    XRateLimited,
    #[error("Tweet {tweet_id} is suspended.")]
    XSuspended { tweet_id: String },
    #[error("Tweet {tweet_id} does not contain downloadable video media.")]
    XNoVideo { tweet_id: String },
    #[error("Tweet {tweet_id} has unavailable video media.")]
    XVideoUnavailable { tweet_id: String },
    #[error("yt-dlp download failed: {message}")]
    YtDlpFailure { message: String },
    #[error("ffmpeg audio conversion failed: {message}")]
    AudioConversionFailed { message: String },
}

fn base_args() -> Vec<String> {
    let ffmpeg_dir = bin_dir();
    let _ = ensure_ffmpeg_symlinks();
    vec![
        "--ffmpeg-location".to_string(),
        ffmpeg_dir.to_string_lossy().to_string(),
    ]
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

fn run_ytdlp_process(cmd_args: &[String]) -> Result<std::process::Output> {
    let ytdlp = ytdlp_path();
    if !ytdlp.exists() {
        bail!(
            "yt-dlp not found at {}. Run 'capcut-cli deps install' first.",
            ytdlp.display()
        );
    }

    output::log(&format!("Running: yt-dlp {}", redact_command_args(cmd_args)));

    Command::new(ytdlp.to_string_lossy().as_ref())
        .args(cmd_args)
        .output()
        .context("Failed to run yt-dlp")
}

fn redact_url_like(value: &str) -> String {
    let Some((base, query)) = value.split_once('?') else {
        return value.to_string();
    };

    let mut redacted = Vec::new();
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or("");
        let val = parts.next().unwrap_or("");
        let lowered = key.to_ascii_lowercase();
        if REDACT_KEYS.iter().any(|candidate| lowered.contains(candidate)) {
            redacted.push(format!("{key}=REDACTED"));
        } else {
            redacted.push(format!("{key}={val}"));
        }
    }

    format!("{base}?{}", redacted.join("&"))
}

fn redact_command_args(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            if arg.starts_with("http://") || arg.starts_with("https://") {
                redact_url_like(arg)
            } else {
                arg.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn sanitize_error_text(message: &str) -> String {
    message
        .split_whitespace()
        .map(|part| {
            if part.starts_with("http://") || part.starts_with("https://") {
                redact_url_like(part)
            } else {
                part.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn cookie_browsers() -> Vec<String> {
    let configured = std::env::var("CAPCUT_X_COOKIE_BROWSERS")
        .or_else(|_| std::env::var("CAPCUT_COOKIE_BROWSERS"))
        .unwrap_or_else(|_| "chrome,safari,firefox,edge".to_string());

    let mut browsers = Vec::new();
    for browser in configured.split(',') {
        let browser = browser.trim();
        if !browser.is_empty() && !browsers.iter().any(|item| item == browser) {
            browsers.push(browser.to_string());
        }
    }
    browsers
}

fn extract_tweet_id(url: &str) -> String {
    url.split("/status/")
        .nth(1)
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("unknown")
        .to_string()
}

fn classify_twitter_failure(stderr: &str, url: &str, browsers_tried: &[String]) -> DownloadError {
    let lower = stderr.to_lowercase();
    let tweet_id = extract_tweet_id(url);

    if lower.contains("rate limit") || lower.contains("too many requests") || lower.contains("http error 429") {
        return DownloadError::XRateLimited;
    }
    if lower.contains("suspended") {
        return DownloadError::XSuspended { tweet_id };
    }
    if lower.contains("no video could be found in this tweet")
        || lower.contains("does not contain downloadable video")
    {
        return DownloadError::XNoVideo { tweet_id };
    }
    if lower.contains("video #") && lower.contains("unavailable") {
        return DownloadError::XVideoUnavailable { tweet_id };
    }
    if lower.contains("login required")
        || lower.contains("authentication")
        || lower.contains("cookies")
        || lower.contains("not logged in")
        || lower.contains("cookie")
        || lower.contains("session")
        || lower.contains("sign in")
    {
        return DownloadError::XAuthRequired {
            browsers: browsers_tried.join(", "),
        };
    }

    DownloadError::YtDlpFailure {
        message: sanitize_error_text(stderr.trim()),
    }
}

fn parse_ytdlp_json_output(stdout: &[u8]) -> Result<serde_json::Value> {
    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(stdout) {
        return Ok(value);
    }

    for line in String::from_utf8_lossy(stdout).lines() {
        let line = line.trim();
        if line.is_empty() || !line.starts_with('{') {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            return Ok(value);
        }
    }

    bail!("Failed to parse yt-dlp JSON output")
}

fn run_ytdlp(url: &str, args: &[&str]) -> Result<std::process::Output> {
    if detect_platform(url) == "twitter" {
        return run_ytdlp_twitter(url, args);
    }

    let mut cmd_args = base_args();
    for arg in args {
        cmd_args.push(arg.to_string());
    }

    let result = run_ytdlp_process(&cmd_args)?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr).to_lowercase();
        if stderr.contains("blocked") {
            for browser in cookie_browsers() {
                let mut retry_args = base_args();
                retry_args.push("--cookies-from-browser".to_string());
                retry_args.push(browser.clone());
                for arg in args {
                    retry_args.push(arg.to_string());
                }
                let retry = run_ytdlp_process(&retry_args)?;
                if retry.status.success() {
                    return Ok(retry);
                }
            }
        }
    }

    Ok(result)
}

fn run_ytdlp_twitter(url: &str, args: &[&str]) -> Result<std::process::Output> {
    let browsers = cookie_browsers();
    let mut last_error: Option<DownloadError> = None;
    let mut last_output: Option<std::process::Output> = None;
    let mut tried = Vec::new();

    for browser in &browsers {
        tried.push(browser.clone());
        let mut cmd_args = base_args();
        cmd_args.push("--cookies-from-browser".to_string());
        cmd_args.push(browser.clone());
        for arg in args {
            cmd_args.push(arg.to_string());
        }

        let output = run_ytdlp_process(&cmd_args)?;
        if output.status.success() {
            return Ok(output);
        }

        let stderr = String::from_utf8_lossy(&output.stderr);
        let classified = classify_twitter_failure(&stderr, url, &tried);
        match classified {
            DownloadError::XAuthRequired { .. } => {
                last_error = Some(classified);
                last_output = Some(output);
            }
            _ => return Err(classified.into()),
        }
    }

    if let Some(error) = last_error {
        return Err(error.into());
    }
    if let Some(output) = last_output {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(classify_twitter_failure(&stderr, url, &browsers).into());
    }

    Err(DownloadError::XAuthRequired {
        browsers: browsers.join(", "),
    }
    .into())
}

/// Extract metadata from a URL without downloading.
pub fn get_info(url: &str) -> Result<serde_json::Value> {
    let result = run_ytdlp(url, &["--dump-json", "--no-download", url])?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        if detect_platform(url) == "twitter" {
            return Err(classify_twitter_failure(&stderr, url, &cookie_browsers()).into());
        }
        return Err(DownloadError::YtDlpFailure {
            message: sanitize_error_text(stderr.trim()),
        }
        .into());
    }

    parse_ytdlp_json_output(&result.stdout)
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
        if t == "sound" {
            return "sound";
        }
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
    let raw_template = output_dir.join("raw_audio.%(ext)s");
    let result = run_ytdlp(
        url,
        &[
            "-f",
            "bestaudio/best",
            "-o",
            &raw_template.to_string_lossy(),
            "--no-playlist",
            url,
        ],
    )?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        if detect_platform(url) == "twitter" {
            return Err(classify_twitter_failure(&stderr, url, &cookie_browsers()).into());
        }
        return Err(DownloadError::YtDlpFailure {
            message: sanitize_error_text(stderr.trim()),
        }
        .into());
    }

    let raw_path = find_file_matching(output_dir, "raw_audio.")?;
    let mp3_path = output_dir.join("audio.mp3");
    let ffmpeg = get_ffmpeg_path()?;
    let conv = Command::new(&ffmpeg)
        .args([
            "-i",
            &raw_path.to_string_lossy(),
            "-vn",
            "-acodec",
            "libmp3lame",
            "-q:a",
            "0",
            &mp3_path.to_string_lossy(),
            "-y",
        ])
        .output()
        .context("Failed to run ffmpeg for audio conversion")?;

    if !conv.status.success() {
        let stderr = String::from_utf8_lossy(&conv.stderr);
        return Err(DownloadError::AudioConversionFailed {
            message: sanitize_error_text(stderr.trim()),
        }
        .into());
    }

    let _ = std::fs::remove_file(&raw_path);
    Ok(mp3_path)
}

/// Download video from a URL as mp4.
pub fn download_clip(url: &str, output_dir: &Path) -> Result<PathBuf> {
    let output_template = output_dir.join("video.%(ext)s");
    let result = run_ytdlp(
        url,
        &[
            "-f",
            "bestvideo[height<=1080]+bestaudio/best[height<=1080]/best",
            "--merge-output-format",
            "mp4",
            "-o",
            &output_template.to_string_lossy(),
            "--no-playlist",
            url,
        ],
    )?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        if detect_platform(url) == "twitter" {
            return Err(classify_twitter_failure(&stderr, url, &cookie_browsers()).into());
        }
        return Err(DownloadError::YtDlpFailure {
            message: sanitize_error_text(stderr.trim()),
        }
        .into());
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
    bail!(
        "Download succeeded but no file matching '{prefix}*' found in {}",
        dir.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ytdlp_json_output_accepts_multiline_stream() {
        let stdout = br#"{"title":"TikTok Embed (1)","duration":36.2}
{"title":"TikTok Embed (2)","duration":36.2}
"#;

        let parsed = parse_ytdlp_json_output(stdout).unwrap();
        assert_eq!(parsed.get("title").and_then(|v| v.as_str()), Some("TikTok Embed (1)"));
    }

    #[test]
    fn test_classify_twitter_failure_auth_required() {
        let err = classify_twitter_failure(
            "ERROR: login required to view this content",
            "https://x.com/user/status/123",
            &["chrome".to_string(), "safari".to_string()],
        );

        assert!(matches!(err, DownloadError::XAuthRequired { .. }));
    }

    #[test]
    fn test_classify_twitter_failure_no_video() {
        let err = classify_twitter_failure(
            "ERROR: [twitter] 123: No video could be found in this tweet",
            "https://x.com/user/status/123",
            &["chrome".to_string()],
        );

        assert!(matches!(err, DownloadError::XNoVideo { .. }));
    }

    #[test]
    fn test_classify_twitter_failure_video_unavailable() {
        let err = classify_twitter_failure(
            "ERROR: [twitter] 123: Video #1 is unavailable",
            "https://x.com/user/status/123/video/1",
            &["chrome".to_string()],
        );

        assert!(matches!(err, DownloadError::XVideoUnavailable { .. }));
    }

    #[test]
    fn test_redact_url_like_hides_tokenish_query_values() {
        let redacted = redact_url_like(
            "https://example.com/video.mp4?token=abc123&x-signature=zzz&expires=60",
        );

        assert!(redacted.contains("token=REDACTED"));
        assert!(redacted.contains("x-signature=REDACTED"));
        assert!(redacted.contains("expires=60"));
        assert!(!redacted.contains("abc123"));
    }

    #[test]
    fn test_sanitize_error_text_redacts_signed_urls() {
        let message =
            "ERROR: request failed for https://example.com/a.mp4?refresh_token=abc&expires=1";
        let redacted = sanitize_error_text(message);
        assert!(redacted.contains("refresh_token=REDACTED"));
        assert!(!redacted.contains("refresh_token=abc"));
    }

    #[test]
    fn test_detect_platform_recognizes_manual_url_sources() {
        assert_eq!(
            detect_platform("https://www.youtube.com/watch?v=abc123"),
            "youtube"
        );
        assert_eq!(
            detect_platform("https://x.com/openai/status/123"),
            "twitter"
        );
    }

    #[test]
    fn test_detect_asset_type_respects_explicit_sound_for_manual_urls() {
        assert_eq!(
            detect_asset_type("https://www.youtube.com/watch?v=abc123", Some("sound")),
            "sound"
        );
        assert_eq!(
            detect_asset_type("https://www.youtube.com/watch?v=abc123", Some("clip")),
            "clip"
        );
    }
}
