"""Click command tree for capcut-cli."""
import sys
import time

import click

from capcut_cli import output


@click.group()
def main():
    """Agent-first video editing CLI."""
    pass


# ── deps ──────────────────────────────────────────────────────────────

@main.group()
def deps():
    """Manage dependencies (yt-dlp, ffmpeg)."""
    pass


@deps.command("check")
def deps_check():
    """Check if all dependencies are installed."""
    from capcut_cli.deps.bootstrap import check_all
    t = time.time()
    result = check_all()
    all_ok = all(v.get("installed") for v in result.values())
    if all_ok:
        output.emit(output.success("deps check", result, t))
    else:
        env = output.error(
            "deps check", "MISSING_DEPS",
            "Some dependencies are not installed.",
            hint="Run 'capcut-cli deps install' to install them.",
        )
        env["data"] = result
        output.emit(env)
        sys.exit(2)


@deps.command("install")
def deps_install():
    """Download and install all dependencies."""
    from capcut_cli.deps.bootstrap import install_all
    from capcut_cli.config import ensure_dirs
    t = time.time()
    ensure_dirs()
    output.log("Installing dependencies...")
    result = install_all()
    output.emit(output.success("deps install", result, t))


# ── discover ──────────────────────────────────────────────────────────

@main.group()
def discover():
    """Discover trending sounds and viral clips."""
    pass


@discover.command("tiktok-sounds")
@click.option("--limit", default=10, help="Max results to return.")
@click.option("--region", default="US", help="Region code.")
def discover_tiktok(limit, region):
    """Find currently trending TikTok sounds."""
    from capcut_cli.discover.tiktok import find_trending_sounds
    t = time.time()
    try:
        data = find_trending_sounds(limit=limit, region=region)
        output.emit(output.success("discover tiktok-sounds", data, t))
    except Exception as e:
        output.emit(output.error(
            "discover tiktok-sounds", "DISCOVERY_FAILED", str(e),
            hint="TikTok endpoints may be rate-limited. Try again later or import sounds manually with 'capcut-cli library import <url>'.",
        ))
        sys.exit(1)


@discover.command("x-clips")
@click.option("--query", required=True, help="Search query for viral clips.")
@click.option("--limit", default=10, help="Max results.")
@click.option("--min-likes", default=1000, help="Minimum likes filter.")
def discover_x(query, limit, min_likes):
    """Find viral video clips on X/Twitter."""
    from capcut_cli.discover.twitter import find_viral_clips
    t = time.time()
    data = find_viral_clips(query=query, limit=limit, min_likes=min_likes)
    output.emit(output.success("discover x-clips", data, t))


# ── library ───────────────────────────────────────────────────────────

@main.group("library")
def library():
    """Manage the local asset library."""
    pass


@library.command("import")
@click.argument("url")
@click.option("--type", "asset_type", type=click.Choice(["sound", "clip"]), default=None,
              help="Asset type. Auto-detected from URL if omitted.")
@click.option("--tags", default="", help="Comma-separated tags.")
def library_import(url, asset_type, tags):
    """Download a sound or clip from a URL into the library."""
    from capcut_cli.library.store import import_asset
    from capcut_cli.config import ensure_dirs
    t = time.time()
    ensure_dirs()
    tag_list = [t.strip() for t in tags.split(",") if t.strip()] if tags else []
    try:
        asset = import_asset(url, asset_type=asset_type, tags=tag_list)
        output.emit(output.success("library import", asset.to_dict(), t))
    except Exception as e:
        output.emit(output.error(
            "library import", "IMPORT_FAILED", str(e),
            hint="Run 'capcut-cli deps check' to verify yt-dlp is installed.",
        ))
        sys.exit(1)


@library.command("list")
@click.option("--type", "asset_type", type=click.Choice(["sound", "clip"]), default=None,
              help="Filter by type.")
def library_list(asset_type):
    """List all assets in the library."""
    from capcut_cli.library.store import list_assets
    t = time.time()
    assets = list_assets(asset_type=asset_type)
    output.emit(output.success("library list", {
        "count": len(assets),
        "assets": [a.to_dict() for a in assets],
    }, t))


@library.command("show")
@click.argument("asset_id")
def library_show(asset_id):
    """Show details of a specific asset."""
    from capcut_cli.library.store import get_asset
    t = time.time()
    asset = get_asset(asset_id)
    if asset is None:
        output.emit(output.error(
            "library show", "NOT_FOUND", f"Asset '{asset_id}' not found.",
            hint="Run 'capcut-cli library list' to see available assets.",
        ))
        sys.exit(1)
    output.emit(output.success("library show", asset.to_dict(), t))


@library.command("delete")
@click.argument("asset_id")
def library_delete(asset_id):
    """Remove an asset from the library."""
    from capcut_cli.library.store import delete_asset
    t = time.time()
    try:
        delete_asset(asset_id)
        output.emit(output.success("library delete", {"deleted": asset_id}, t))
    except Exception as e:
        output.emit(output.error("library delete", "DELETE_FAILED", str(e)))
        sys.exit(1)


# ── compose ───────────────────────────────────────────────────────────

@main.command()
@click.option("--sound", required=True, help="Sound asset ID from the library.")
@click.option("--clip", "clips", required=True, multiple=True, help="Clip asset ID (repeatable).")
@click.option("--duration", "duration_seconds", type=float, default=30.0, help="Output duration in seconds.")
@click.option("--output", "output_path", default=None, help="Output file path. Auto-generated if omitted.")
@click.option("--resolution", default="1080x1920", help="Output resolution WxH (default: vertical 1080x1920).")
@click.option("--loudness", default=None,
              help="Loudness preset or LUFS value. Presets: viral (-8 LUFS, default), "
                   "social (-10), podcast (-14), broadcast (-23). Or pass a number like -12.")
def compose(sound, clips, duration_seconds, output_path, resolution, loudness):
    """Compose clips with a sound into a final video."""
    from capcut_cli.media.compose import run_compose
    from capcut_cli.config import ensure_dirs
    t = time.time()
    ensure_dirs()
    try:
        result = run_compose(
            sound_id=sound,
            clip_ids=list(clips),
            duration_seconds=duration_seconds,
            output_path=output_path,
            resolution=resolution,
            loudness=loudness,
        )
        output.emit(output.success("compose", result.to_dict(), t))
    except Exception as e:
        output.emit(output.error(
            "compose", "COMPOSE_FAILED", str(e),
            hint="Ensure assets exist with 'capcut-cli library list' and deps are installed with 'capcut-cli deps check'.",
        ))
        sys.exit(1)


if __name__ == "__main__":
    main()
