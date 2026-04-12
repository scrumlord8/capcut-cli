use anyhow::{Context, Result, bail};
use serde_json::json;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

use crate::config::{bin_dir, ytdlp_path};
use crate::output;

/// Find ffmpeg on the system. Checks PATH first, then ~/.capcut-cli/bin/.
pub fn get_ffmpeg_path() -> Result<String> {
    // Check PATH
    if let Ok(out) = Command::new("ffmpeg").arg("-version").output() {
        if out.status.success() {
            return Ok("ffmpeg".to_string());
        }
    }
    // Check bin dir
    let local = bin_dir().join("ffmpeg");
    if local.exists() {
        return Ok(local.to_string_lossy().to_string());
    }
    bail!(
        "ffmpeg not found. Install it via your package manager:\n  \
         macOS:  brew install ffmpeg\n  \
         Linux:  sudo apt install ffmpeg\n  \
         Or place the binary in ~/.capcut-cli/bin/"
    )
}

/// Download the yt-dlp standalone binary for the current platform.
pub fn download_ytdlp() -> Result<PathBuf> {
    let dest = ytdlp_path();
    fs::create_dir_all(dest.parent().unwrap())?;

    let url = if cfg!(target_os = "macos") {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos"
    } else if cfg!(target_os = "linux") {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux"
    } else {
        bail!("Unsupported platform for yt-dlp binary download");
    };

    output::log(&format!("Downloading yt-dlp from {url}..."));

    let resp = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?
        .get(url)
        .send()
        .context("Failed to download yt-dlp")?;

    if !resp.status().is_success() {
        bail!("yt-dlp download returned HTTP {}", resp.status());
    }

    let bytes = resp.bytes()?;
    fs::write(&dest, &bytes)?;

    // Make executable
    let mut perms = fs::metadata(&dest)?.permissions();
    perms.set_mode(perms.mode() | 0o755);
    fs::set_permissions(&dest, perms)?;

    output::log(&format!("yt-dlp installed to {}", dest.display()));
    Ok(dest)
}

/// Check if yt-dlp is available and return status.
pub fn check_ytdlp() -> serde_json::Value {
    let path = ytdlp_path();
    if !path.exists() {
        return json!({ "installed": false, "path": null, "version": null });
    }
    match Command::new(path.to_string_lossy().as_ref())
        .arg("--version")
        .output()
    {
        Ok(out) => json!({
            "installed": true,
            "path": path.to_string_lossy(),
            "version": String::from_utf8_lossy(&out.stdout).trim().to_string(),
        }),
        Err(e) => json!({
            "installed": false,
            "path": path.to_string_lossy(),
            "error": e.to_string(),
        }),
    }
}

/// Check if ffmpeg is available and return status.
pub fn check_ffmpeg() -> serde_json::Value {
    match get_ffmpeg_path() {
        Ok(ffmpeg) => {
            match Command::new(&ffmpeg).arg("-version").output() {
                Ok(out) => {
                    let version = String::from_utf8_lossy(&out.stdout)
                        .lines()
                        .next()
                        .unwrap_or("unknown")
                        .to_string();
                    json!({
                        "installed": true,
                        "path": ffmpeg,
                        "version": version,
                    })
                }
                Err(e) => json!({
                    "installed": false,
                    "path": ffmpeg,
                    "error": e.to_string(),
                }),
            }
        }
        Err(_) => json!({ "installed": false, "path": null }),
    }
}

/// Check all dependencies.
pub fn check_all() -> serde_json::Value {
    json!({
        "yt_dlp": check_ytdlp(),
        "ffmpeg": check_ffmpeg(),
    })
}

/// Install all dependencies.
pub fn install_all() -> Result<serde_json::Value> {
    let ytdlp = if !ytdlp_path().exists() {
        download_ytdlp()?;
        check_ytdlp()
    } else {
        check_ytdlp()
    };

    Ok(json!({
        "yt_dlp": ytdlp,
        "ffmpeg": check_ffmpeg(),
    }))
}
