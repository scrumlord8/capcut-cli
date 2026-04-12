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
}

impl Cli {
    pub fn run(self) -> Result<()> {
        match self.command {
            Command::Deps(args) => args.run(),
            Command::Discover(args) => args.run(),
            Command::Library(args) => args.run(),
            Command::Compose(args) => args.run(),
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
    },
}

impl DiscoverArgs {
    fn run(self) -> Result<()> {
        match self.action {
            DiscoverAction::TiktokSounds { limit, region } => {
                let t = Instant::now();
                match discover::tiktok::find_trending_sounds(limit, &region) {
                    Ok(data) => {
                        output::emit(&output::success("discover tiktok-sounds", data, Some(t)));
                    }
                    Err(e) => {
                        output::emit(&output::error(
                            "discover tiktok-sounds",
                            "DISCOVERY_FAILED",
                            &e.to_string(),
                            Some(
                                "TikTok endpoints may be rate-limited. Try again later or import \
                                 sounds manually with 'capcut-cli library import <url>'.",
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
            } => {
                let t = Instant::now();
                match discover::twitter::find_viral_clips(&query, limit, min_likes) {
                    Ok(data) => {
                        output::emit(&output::success("discover x-clips", data, Some(t)));
                    }
                    Err(e) => {
                        output::emit(&output::error(
                            "discover x-clips",
                            "DISCOVERY_FAILED",
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
                        output::emit(&output::error(
                            "library import",
                            "IMPORT_FAILED",
                            &e.to_string(),
                            Some("Run 'capcut-cli deps check' to verify yt-dlp is installed."),
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
