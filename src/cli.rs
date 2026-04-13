use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::time::Instant;

use crate::{config, deps, discover, library, media, output};

#[derive(Debug, Parser)]
#[command(
    name = "capcut-cli",
    version,
    about = "Agent-first CLI for discovering and composing short-form social video"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Manage dependencies (yt-dlp, ffmpeg).
    Deps(DepsArgs),
    /// Discover trending sounds and viral clips.
    Discover(DiscoverArgs),
    /// Manage the local asset library.
    Library(LibraryArgs),
    /// Compose clips with a sound into a final video.
    Compose(ComposeArgs),
    /// One-shot agent workflow: discover, import, and compose automatically.
    Autopilot(AutoPilotArgs),
}

impl Cli {
    pub fn run(self) -> Result<()> {
        match self.command {
            Command::Deps(args) => args.run(),
            Command::Discover(args) => args.run(),
            Command::Library(args) => args.run(),
            Command::Compose(args) => args.run(),
            Command::Autopilot(args) => args.run(),
        }
    }
}

// ── deps ────────────────────────────────────────────────────────────

#[derive(Debug, Args)]
struct DepsArgs {
    #[command(subcommand)]
    action: DepsAction,
}

#[derive(Debug, Subcommand)]
enum DepsAction {
    /// Check if all dependencies are installed.
    Check,
    /// Download and install all dependencies.
    Install,
}

impl DepsArgs {
    fn run(self) -> Result<()> {
        match self.action {
            DepsAction::Check => {
                let t = Instant::now();
                let result = deps::check_all();
                let all_ok = result
                    .as_object()
                    .map(|m| {
                        m.values()
                            .all(|v| v.get("installed").and_then(|i| i.as_bool()).unwrap_or(false))
                    })
                    .unwrap_or(false);

                if all_ok {
                    output::emit(&output::success("deps check", result, Some(t)));
                } else {
                    let mut env = output::error(
                        "deps check",
                        "MISSING_DEPS",
                        "Some dependencies are not installed.",
                        Some("Run 'capcut-cli deps install' to install them."),
                    );
                    env.data = result;
                    output::emit(&env);
                    std::process::exit(2);
                }
            }
            DepsAction::Install => {
                let t = Instant::now();
                config::ensure_dirs();
                output::log("Installing dependencies...");
                match deps::install_all() {
                    Ok(result) => {
                        output::emit(&output::success("deps install", result, Some(t)));
                    }
                    Err(e) => {
                        output::emit(&output::error(
                            "deps install",
                            "INSTALL_FAILED",
                            &e.to_string(),
                            None,
                        ));
                        std::process::exit(1);
                    }
                }
            }
        }
        Ok(())
    }
}

// ── discover ────────────────────────────────────────────────────────

#[derive(Debug, Args)]
struct DiscoverArgs {
    #[command(subcommand)]
    action: DiscoverAction,
}

#[derive(Debug, Subcommand)]
enum DiscoverAction {
    /// Find currently trending TikTok sounds.
    #[command(name = "tiktok-sounds")]
    TiktokSounds {
        /// Max results to return.
        #[arg(long, default_value_t = 10)]
        limit: u32,
        /// Region code.
        #[arg(long, default_value = "US")]
        region: String,
        /// Rolling discovery window in days.
        #[arg(long = "window-days", default_value_t = 7, value_parser = clap::value_parser!(u32).range(1..))]
        window_days: u32,
        /// Sound discovery strategy: auto, research, creative-center, library, manual-url.
        #[arg(long, default_value = "auto")]
        strategy: String,
        /// Manual sound URL used when strategy is `manual-url`, or as an `auto` fallback.
        #[arg(long = "sound-url")]
        sound_url: Option<String>,
    },
    /// Find viral video clips on X/Twitter.
    #[command(name = "x-clips")]
    XClips {
        /// Search query for viral clips.
        #[arg(long)]
        query: String,
        /// Max results.
        #[arg(long, default_value_t = 10)]
        limit: u32,
        /// Minimum likes filter.
        #[arg(long, default_value_t = 1000)]
        min_likes: u64,
        /// Clip discovery strategy: auto, api, guided, library, manual-url.
        #[arg(long, default_value = "auto")]
        strategy: String,
        /// Manual X clip URL used when strategy is `manual-url`, or as an `auto` fallback.
        #[arg(long = "clip-url")]
        clip_url: Option<String>,
    },
}

impl DiscoverArgs {
    fn run(self) -> Result<()> {
        match self.action {
            DiscoverAction::TiktokSounds {
                limit,
                region,
                window_days,
                strategy,
                sound_url,
            } => {
                let t = Instant::now();
                let strategy = match discover::tiktok::SoundDiscoveryStrategy::parse(&strategy) {
                    Ok(value) => value,
                    Err(error) => {
                        output::emit(&output::error(
                            "discover tiktok-sounds",
                            "INVALID_STRATEGY",
                            &error.to_string(),
                            None,
                        ));
                        std::process::exit(1);
                    }
                };
                let options = discover::tiktok::SoundDiscoveryOptions {
                    limit,
                    region: region.clone(),
                    window_days,
                    strategy,
                    manual_url: sound_url,
                };
                match discover::tiktok::find_trending_sounds_with_options(&options) {
                    Ok(data) => {
                        output::emit(&output::success("discover tiktok-sounds", data, Some(t)));
                    }
                    Err(e) => {
                        output::emit(&output::error(
                            "discover tiktok-sounds",
                            "DISCOVERY_FAILED",
                            &e.to_string(),
                            Some(
                                "Set TIKTOK_RESEARCH_ACCESS_TOKEN for official discovery. If the fallback scraper is failing, try again later or import a sound manually with 'capcut-cli library import <url> --type sound'.",
                            ),
                        ));
                        std::process::exit(1);
                    }
                }
            }
            DiscoverAction::XClips {
                query,
                limit,
                min_likes,
                strategy,
                clip_url,
            } => {
                let t = Instant::now();
                let strategy = match discover::twitter::ClipDiscoveryStrategy::parse(&strategy) {
                    Ok(value) => value,
                    Err(error) => {
                        output::emit(&output::error(
                            "discover x-clips",
                            "INVALID_STRATEGY",
                            &error.to_string(),
                            None,
                        ));
                        std::process::exit(1);
                    }
                };
                let options = discover::twitter::ClipDiscoveryOptions {
                    query,
                    limit,
                    min_likes,
                    strategy,
                    manual_url: clip_url,
                };
                match discover::twitter::find_viral_clips_with_options(&options) {
                    Ok(data) => {
                        output::emit(&output::success("discover x-clips", data, Some(t)));
                    }
                    Err(e) => {
                        let (code, hint) = match e
                            .downcast_ref::<discover::twitter::TwitterDiscoveryError>()
                        {
                            Some(discover::twitter::TwitterDiscoveryError::AuthRequired) => (
                                "X_AUTH_REQUIRED",
                                Some(
                                    "Set TWITTER_BEARER_TOKEN for official X discovery, or pass \
                                     --allow-guided-fallback to get browser search URLs instead.",
                                ),
                            ),
                            Some(discover::twitter::TwitterDiscoveryError::RateLimited) => (
                                "X_RATE_LIMITED",
                                Some("Retry later or reduce request frequency."),
                            ),
                            Some(discover::twitter::TwitterDiscoveryError::ApiRequest { .. }) => (
                                "X_API_REQUEST_FAILED",
                                Some("Verify network access and your TWITTER_BEARER_TOKEN."),
                            ),
                            Some(discover::twitter::TwitterDiscoveryError::ApiStatus { .. }) => (
                                "X_API_STATUS_ERROR",
                                Some("Verify your TWITTER_BEARER_TOKEN and X API access tier."),
                            ),
                            None => ("DISCOVERY_FAILED", None),
                        };
                        output::emit(&output::error(
                            "discover x-clips",
                            code,
                            &e.to_string(),
                            hint,
                        ));
                        std::process::exit(1);
                    }
                }
            }
        }
        Ok(())
    }
}

// ── library ─────────────────────────────────────────────────────────

#[derive(Debug, Args)]
struct LibraryArgs {
    #[command(subcommand)]
    action: LibraryAction,
}

#[derive(Debug, Subcommand)]
enum LibraryAction {
    /// Download a sound or clip from a URL into the library.
    Import {
        /// URL to import.
        url: String,
        /// Asset type. Auto-detected from URL if omitted.
        #[arg(long = "type")]
        asset_type: Option<String>,
        /// Comma-separated tags.
        #[arg(long, default_value = "")]
        tags: String,
    },
    /// List all assets in the library.
    List {
        /// Filter by type.
        #[arg(long = "type")]
        asset_type: Option<String>,
    },
    /// Show details of a specific asset.
    Show {
        /// Asset ID.
        asset_id: String,
    },
    /// Remove an asset from the library.
    Delete {
        /// Asset ID.
        asset_id: String,
    },
}

impl LibraryArgs {
    fn run(self) -> Result<()> {
        match self.action {
            LibraryAction::Import {
                url,
                asset_type,
                tags,
            } => {
                let t = Instant::now();
                config::ensure_dirs();
                let tag_list: Vec<String> = tags
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                match library::import_asset(&url, asset_type.as_deref(), &tag_list) {
                    Ok(asset) => {
                        let data = serde_json::to_value(&asset)?;
                        output::emit(&output::success("library import", data, Some(t)));
                    }
                    Err(e) => {
                        let (code, hint) =
                            match e.downcast_ref::<media::downloader::DownloadError>() {
                                Some(media::downloader::DownloadError::XAuthRequired { .. }) => (
                                    "X_AUTH_REQUIRED",
                                    Some(
                                        "Log into X in a supported local browser and rerun the \
                                         import. Configure CAPCUT_X_COOKIE_BROWSERS if needed.",
                                    ),
                                ),
                                Some(media::downloader::DownloadError::XRateLimited) => (
                                    "X_RATE_LIMITED",
                                    Some("Retry later; X temporarily rate-limited media access."),
                                ),
                                Some(media::downloader::DownloadError::XSuspended { .. }) => (
                                    "X_TWEET_SUSPENDED",
                                    Some("Pick another clip candidate; this tweet is suspended."),
                                ),
                                Some(media::downloader::DownloadError::XNoVideo { .. }) => (
                                    "X_NO_VIDEO",
                                    Some(
                                        "Use a tweet URL that actually contains downloadable video \
                                         media.",
                                    ),
                                ),
                                Some(media::downloader::DownloadError::XVideoUnavailable { .. }) => (
                                    "X_VIDEO_UNAVAILABLE",
                                    Some("Pick another clip candidate; this video is unavailable."),
                                ),
                                Some(media::downloader::DownloadError::AudioConversionFailed { .. }) => (
                                    "AUDIO_CONVERSION_FAILED",
                                    Some("Verify ffmpeg is installed and supports MP3 encoding."),
                                ),
                                Some(media::downloader::DownloadError::YtDlpFailure { .. }) => (
                                    "IMPORT_FAILED",
                                    Some(
                                        "Run 'capcut-cli deps check' to verify yt-dlp is \
                                         installed.",
                                    ),
                                ),
                                None => (
                                    "IMPORT_FAILED",
                                    Some(
                                        "Run 'capcut-cli deps check' to verify yt-dlp is \
                                         installed.",
                                    ),
                                ),
                            };
                        output::emit(&output::error(
                            "library import",
                            code,
                            &e.to_string(),
                            hint,
                        ));
                        std::process::exit(1);
                    }
                }
            }
            LibraryAction::List { asset_type } => {
                let t = Instant::now();
                let assets = library::list_assets(asset_type.as_deref())?;
                let data = serde_json::json!({
                    "count": assets.len(),
                    "assets": assets.iter().map(|a| serde_json::to_value(a).unwrap()).collect::<Vec<_>>(),
                });
                output::emit(&output::success("library list", data, Some(t)));
            }
            LibraryAction::Show { asset_id } => {
                let t = Instant::now();
                match library::get_asset(&asset_id)? {
                    Some(asset) => {
                        let data = serde_json::to_value(&asset)?;
                        output::emit(&output::success("library show", data, Some(t)));
                    }
                    None => {
                        output::emit(&output::error(
                            "library show",
                            "NOT_FOUND",
                            &format!("Asset '{asset_id}' not found."),
                            Some("Run 'capcut-cli library list' to see available assets."),
                        ));
                        std::process::exit(1);
                    }
                }
            }
            LibraryAction::Delete { asset_id } => {
                let t = Instant::now();
                match library::delete_asset(&asset_id) {
                    Ok(()) => {
                        output::emit(&output::success(
                            "library delete",
                            serde_json::json!({"deleted": asset_id}),
                            Some(t),
                        ));
                    }
                    Err(e) => {
                        output::emit(&output::error(
                            "library delete",
                            "DELETE_FAILED",
                            &e.to_string(),
                            None,
                        ));
                        std::process::exit(1);
                    }
                }
            }
        }
        Ok(())
    }
}

// ── compose ─────────────────────────────────────────────────────────

#[derive(Debug, Args)]
struct ComposeArgs {
    /// Sound asset ID from the library.
    #[arg(long)]
    sound: String,

    /// Clip asset ID (repeatable).
    #[arg(long = "clip", required = true)]
    clips: Vec<String>,

    /// Output duration in seconds.
    #[arg(long, default_value_t = 30.0)]
    duration: f64,

    /// Output file path. Auto-generated if omitted.
    #[arg(long)]
    output: Option<String>,

    /// Output resolution WxH (default: vertical 1080x1920).
    #[arg(long, default_value = "1080x1920")]
    resolution: String,

    /// Loudness preset or LUFS value. Presets: viral (-8, default),
    /// social (-10), podcast (-14), broadcast (-23). Or pass a number like -12.
    #[arg(long)]
    loudness: Option<String>,
}

// Make ComposeArgs fields accessible for testing
#[cfg(test)]
impl ComposeArgs {
    fn resolution(&self) -> &str { &self.resolution }
    fn duration(&self) -> f64 { self.duration }
}

impl ComposeArgs {
    fn run(self) -> Result<()> {
        let t = Instant::now();
        config::ensure_dirs();
        match media::compose::run_compose(
            &self.sound,
            &self.clips,
            self.duration,
            self.output.as_deref(),
            &self.resolution,
            self.loudness.as_deref(),
        ) {
            Ok(result) => {
                let data = serde_json::to_value(&result)?;
                output::emit(&output::success("compose", data, Some(t)));
            }
            Err(e) => {
                output::emit(&output::error(
                    "compose",
                    "COMPOSE_FAILED",
                    &e.to_string(),
                    Some(
                        "Ensure assets exist with 'capcut-cli library list' and deps are \
                         installed with 'capcut-cli deps check'.",
                    ),
                ));
                std::process::exit(1);
            }
        }
        Ok(())
    }
}

// ── autopilot ───────────────────────────────────────────────────────

#[derive(Debug, Args)]
struct AutoPilotArgs {
    /// Topic/query used to discover relevant X clips.
    #[arg(long)]
    query: String,

    /// Region code used for TikTok sound discovery.
    #[arg(long, default_value = "US")]
    region: String,

    /// Rolling window in days for TikTok sound discovery.
    #[arg(long = "window-days", default_value_t = 7, value_parser = clap::value_parser!(u32).range(1..))]
    window_days: u32,

    /// Number of sound candidates to discover.
    #[arg(long = "sound-limit", default_value_t = 5)]
    sound_limit: u32,

    /// Number of clip candidates to discover.
    #[arg(long = "clip-limit", default_value_t = 5)]
    clip_limit: u32,

    /// Minimum likes threshold for clip discovery.
    #[arg(long, default_value_t = 1000)]
    min_likes: u64,

    /// Output duration in seconds.
    #[arg(long, default_value_t = 15.0)]
    duration: f64,

    /// Output file path. Auto-generated if omitted.
    #[arg(long)]
    output: Option<String>,

    /// Output resolution WxH.
    #[arg(long, default_value = "1080x1920")]
    resolution: String,

    /// Loudness preset or LUFS value.
    #[arg(long)]
    loudness: Option<String>,

    /// Sound discovery strategy: auto, research, creative-center, library, manual-url.
    #[arg(long = "sound-strategy", default_value = "auto")]
    sound_strategy: String,

    /// Manual sound URL used when sound strategy is `manual-url`, or as an `auto` fallback.
    #[arg(long = "sound-url")]
    sound_url: Option<String>,

    /// Clip discovery strategy: auto, api, guided, library, manual-url.
    #[arg(long = "clip-strategy", default_value = "auto")]
    clip_strategy: String,

    /// Manual X clip URL used when clip strategy is `manual-url`, or as an `auto` fallback.
    #[arg(long = "clip-url")]
    clip_url: Option<String>,
}

impl AutoPilotArgs {
    fn run(self) -> Result<()> {
        let t = Instant::now();
        config::ensure_dirs();

        let sound_strategy = match discover::tiktok::SoundDiscoveryStrategy::parse(&self.sound_strategy) {
            Ok(value) => value,
            Err(error) => {
                output::emit(&output::error(
                    "autopilot",
                    "INVALID_SOUND_STRATEGY",
                    &error.to_string(),
                    None,
                ));
                std::process::exit(1);
            }
        };
        let sound_options = discover::tiktok::SoundDiscoveryOptions {
            limit: self.sound_limit,
            region: self.region.clone(),
            window_days: self.window_days,
            strategy: sound_strategy,
            manual_url: self.sound_url.clone(),
        };
        let clip_strategy = match discover::twitter::ClipDiscoveryStrategy::parse(&self.clip_strategy) {
            Ok(value) => value,
            Err(error) => {
                output::emit(&output::error(
                    "autopilot",
                    "INVALID_CLIP_STRATEGY",
                    &error.to_string(),
                    None,
                ));
                std::process::exit(1);
            }
        };
        let clip_options = discover::twitter::ClipDiscoveryOptions {
            query: self.query.clone(),
            limit: self.clip_limit,
            min_likes: self.min_likes,
            strategy: clip_strategy,
            manual_url: self.clip_url.clone(),
        };

        let sound_discovery = match discover::tiktok::find_trending_sounds_with_options(&sound_options) {
            Ok(data) => data,
            Err(error) => {
                output::emit(&output::error(
                    "autopilot",
                    "SOUND_DISCOVERY_FAILED",
                    &error.to_string(),
                    Some("Set TIKTOK_RESEARCH_ACCESS_TOKEN or retry later."),
                ));
                std::process::exit(1);
            }
        };
        let clip_discovery =
            match discover::twitter::find_viral_clips_with_options(&clip_options) {
                Ok(data) => data,
                Err(error) => {
                    let hint = if error
                        .downcast_ref::<discover::twitter::TwitterDiscoveryError>()
                        .is_some()
                    {
                        Some("Set TWITTER_BEARER_TOKEN for official X discovery.")
                    } else {
                        None
                    };
                    output::emit(&output::error(
                        "autopilot",
                        "CLIP_DISCOVERY_FAILED",
                        &error.to_string(),
                        hint,
                    ));
                    std::process::exit(1);
                }
            };

        let sound_candidates = extract_candidates(&sound_discovery, "sounds");
        if sound_candidates.is_empty() {
            output::emit(&output::error(
                "autopilot",
                "NO_SOUND_CANDIDATES",
                "No TikTok sound candidates were returned by discovery.",
                Some("Set TIKTOK_RESEARCH_ACCESS_TOKEN or retry later when Creative Center is available."),
            ));
            std::process::exit(1);
        }

        let clip_candidates = extract_candidates(&clip_discovery, "clips");
        if clip_candidates.is_empty() {
            output::emit(&output::error(
                "autopilot",
                "NO_CLIP_CANDIDATES",
                "No X/Twitter clip candidates were returned by discovery.",
                Some("Set TWITTER_BEARER_TOKEN and retry clip discovery."),
            ));
            std::process::exit(1);
        }

        let sound_tags = vec![
            "auto".to_string(),
            "workflow".to_string(),
            "tiktok".to_string(),
            "trending".to_string(),
        ];
        let clip_tags = vec![
            "auto".to_string(),
            "workflow".to_string(),
            "x".to_string(),
            "viral".to_string(),
        ];

        let (sound_asset, sound_source, sound_failures) =
            match import_first_success(&sound_candidates, "sound", &sound_tags) {
                Ok(result) => result,
                Err(error) => {
                    output::emit(&output::error(
                        "autopilot",
                        "SOUND_IMPORT_FAILED",
                        &error.to_string(),
                        Some("No discovered sound candidate could be imported."),
                    ));
                    std::process::exit(1);
                }
            };
        let (clip_asset, clip_source, clip_failures) =
            match import_first_success(&clip_candidates, "clip", &clip_tags) {
                Ok(result) => result,
                Err(error) => {
                    output::emit(&output::error(
                        "autopilot",
                        "CLIP_IMPORT_FAILED",
                        &error.to_string(),
                        Some("No discovered clip candidate could be imported."),
                    ));
                    std::process::exit(1);
                }
            };

        let composed = match media::compose::run_compose(
            &sound_asset.id,
            &[clip_asset.id.clone()],
            self.duration,
            self.output.as_deref(),
            &self.resolution,
            self.loudness.as_deref(),
        ) {
            Ok(result) => result,
            Err(error) => {
                output::emit(&output::error(
                    "autopilot",
                    "COMPOSE_FAILED",
                    &error.to_string(),
                    Some("Discovery and import succeeded, but compose failed."),
                ));
                std::process::exit(1);
            }
        };

        let data = serde_json::json!({
            "workflow": "autopilot",
            "query": self.query,
            "region": self.region,
            "window_days": self.window_days,
            "sound_strategy": self.sound_strategy,
            "clip_strategy": self.clip_strategy,
            "selected": {
                "sound_source_url": sound_source,
                "clip_source_url": clip_source,
                "sound_asset_id": sound_asset.id,
                "clip_asset_id": clip_asset.id,
            },
            "attempts": {
                "sound_candidates_considered": sound_candidates.len(),
                "clip_candidates_considered": clip_candidates.len(),
                "sound_import_failures": sound_failures,
                "clip_import_failures": clip_failures,
            },
            "compose": serde_json::to_value(composed)?,
        });

        output::emit(&output::success("autopilot", data, Some(t)));
        Ok(())
    }
}

fn extract_candidates(data: &serde_json::Value, key: &str) -> Vec<serde_json::Value> {
    data.get(key)
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

fn candidate_import_url(candidate: &serde_json::Value) -> Option<String> {
    candidate
        .get("import_url")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn candidate_asset_id(candidate: &serde_json::Value) -> Option<String> {
    candidate
        .get("asset_id")
        .and_then(|v| v.as_str())
        .or_else(|| candidate.get("music_id").and_then(|v| v.as_str()))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn import_first_success(
    candidates: &[serde_json::Value],
    asset_type: &str,
    tags: &[String],
) -> Result<(crate::models::Asset, String, Vec<serde_json::Value>)> {
    let mut failures = Vec::new();

    for candidate in candidates {
        if candidate.get("source_path").and_then(|v| v.as_str()) == Some("library") {
            if let Some(asset_id) = candidate_asset_id(candidate) {
                if let Some(asset) = library::get_asset(&asset_id)? {
                    return Ok((asset, asset_id, failures));
                }
                failures.push(serde_json::json!({
                    "asset_id": asset_id,
                    "error": "candidate referenced library asset that no longer exists"
                }));
                continue;
            }
        }

        let Some(url) = candidate_import_url(candidate) else {
            failures.push(serde_json::json!({
                "reason": "candidate_missing_import_url"
            }));
            continue;
        };

        match library::import_asset(&url, Some(asset_type), tags) {
            Ok(asset) => return Ok((asset, url, failures)),
            Err(err) => {
                failures.push(serde_json::json!({
                    "import_url": url,
                    "error": err.to_string(),
                }));
            }
        }
    }

    let err = if asset_type == "sound" {
        anyhow::anyhow!("Autopilot could not import any discovered sound candidate.")
    } else {
        anyhow::anyhow!("Autopilot could not import any discovered clip candidate.")
    };
    Err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::library;

    #[test]
    fn test_extract_candidates_reads_array_field() {
        let payload = serde_json::json!({
            "sounds": [
                { "import_url": "https://example.com/a" },
                { "import_url": "https://example.com/b" }
            ]
        });

        let candidates = extract_candidates(&payload, "sounds");
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn test_candidate_import_url_skips_blank_values() {
        let blank = serde_json::json!({ "import_url": "   " });
        let valid = serde_json::json!({ "import_url": "https://example.com/sound" });

        assert!(candidate_import_url(&blank).is_none());
        assert_eq!(
            candidate_import_url(&valid).as_deref(),
            Some("https://example.com/sound")
        );
    }

    #[test]
    fn test_candidate_asset_id_prefers_asset_id_then_music_id() {
        let asset = serde_json::json!({
            "asset_id": "clp_123",
            "music_id": "snd_456"
        });
        let music = serde_json::json!({
            "music_id": "snd_456"
        });

        assert_eq!(candidate_asset_id(&asset).as_deref(), Some("clp_123"));
        assert_eq!(candidate_asset_id(&music).as_deref(), Some("snd_456"));
    }

    #[test]
    fn test_import_first_success_reuses_existing_library_asset() {
        let existing_asset = library::list_assets(Some("sound"))
            .unwrap()
            .into_iter()
            .next()
            .expect("expected at least one sound asset in test library");
        let candidates = vec![serde_json::json!({
            "source_path": "library",
            "asset_id": existing_asset.id,
            "import_url": "https://example.com/should-not-be-used"
        })];

        let (asset, source, failures) =
            import_first_success(&candidates, "sound", &["auto".to_string()]).unwrap();

        assert_eq!(asset.id, existing_asset.id);
        assert_eq!(source, existing_asset.id);
        assert!(failures.is_empty());
    }

    #[test]
    fn test_import_first_success_records_missing_library_asset_failure() {
        let candidates = vec![serde_json::json!({
            "source_path": "library",
            "asset_id": "snd_missing"
        })];

        let error = import_first_success(&candidates, "sound", &["auto".to_string()])
            .expect_err("missing library asset should fail");

        assert!(
            error
                .to_string()
                .contains("Autopilot could not import any discovered sound candidate.")
        );
    }
}
