"""TikTok trending sounds discovery — API-first with HTML fallback."""
import json
import re

import httpx
from bs4 import BeautifulSoup

from capcut_cli import output as out


# API endpoint returns JSON directly — no HTML parsing needed.
CREATIVE_CENTER_API = (
    "https://ads.tiktok.com/creative_radar_api/v1/popular/sound/list"
)

# HTML fallback — only used when the API is unreachable or restructured.
CREATIVE_CENTER_URL = (
    "https://ads.tiktok.com/business/creativecenter/inspiration/popular/music/pc/en"
)

HEADERS = {
    "User-Agent": (
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
        "AppleWebKit/537.36 (KHTML, like Gecko) "
        "Chrome/124.0.0.0 Safari/537.36"
    ),
    "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
    "Accept-Language": "en-US,en;q=0.9",
}


def _normalize_sound(raw: dict, rank: int) -> dict:
    """Extract sound fields from any of the known payload shapes."""
    return {
        "rank": raw.get("rank", rank),
        "title": raw.get("title") or raw.get("musicName") or "Unknown",
        "artist": (
            raw.get("author")
            or raw.get("artistName")
            or raw.get("creator", {}).get("nickname", "Unknown")
            if isinstance(raw.get("creator"), dict)
            else raw.get("author") or raw.get("artistName") or "Unknown"
        ),
        "tiktok_url": raw.get("link") or raw.get("playUrl") or "",
        "cover_url": raw.get("cover") or raw.get("coverUrl") or "",
        "duration_seconds": raw.get("duration", 0),
        "is_promoted": raw.get("promoted", False),
    }


def _try_api(limit: int, region: str) -> list | None:
    """Try the Creative Center JSON API (no HTML parsing)."""
    params = {
        "period": 7,
        "page": 1,
        "limit": limit,
        "country_code": region,
        "sort_by": "popularity",
    }
    try:
        with httpx.Client(timeout=15, follow_redirects=True) as client:
            resp = client.get(CREATIVE_CENTER_API, params=params, headers={
                "User-Agent": HEADERS["User-Agent"],
                "Accept": "application/json",
            })
            resp.raise_for_status()
            body = resp.json()

            # Known API response shapes
            sound_list = (
                body.get("data", {}).get("sound_list")
                or body.get("data", {}).get("soundList")
                or body.get("data", {}).get("list")
            )
            if sound_list and isinstance(sound_list, list):
                out.log("Source: Creative Center API (JSON)")
                return sound_list
    except (httpx.HTTPError, httpx.ConnectError, json.JSONDecodeError, KeyError):
        pass
    return None


# ── HTML extraction strategies, ordered from most to least likely ────

def _extract_next_data(soup: BeautifulSoup) -> list | None:
    """Next.js __NEXT_DATA__ script tag (original SSR shape)."""
    tag = soup.find("script", id="__NEXT_DATA__")
    if tag and tag.string:
        try:
            data = json.loads(tag.string)
            return (
                data.get("props", {})
                .get("pageProps", {})
                .get("data", {})
                .get("soundList")
            )
        except (json.JSONDecodeError, AttributeError):
            pass
    return None


def _extract_script_scan(soup: BeautifulSoup) -> list | None:
    """Scan all <script> tags for any JSON blob containing soundList."""
    for script in soup.find_all("script"):
        text = script.string or ""
        if "soundList" not in text and "sound_list" not in text:
            continue
        try:
            data = json.loads(text)
            # Walk common nesting patterns
            for path in [
                lambda d: d["props"]["pageProps"]["data"]["soundList"],
                lambda d: d["props"]["pageProps"]["soundList"],
                lambda d: d["data"]["soundList"],
                lambda d: d["data"]["sound_list"],
                lambda d: d["soundList"],
            ]:
                try:
                    result = path(data)
                    if isinstance(result, list) and result:
                        return result
                except (KeyError, TypeError):
                    continue
        except json.JSONDecodeError:
            continue
    return None


def _extract_regex(html: str) -> list | None:
    """Last resort — regex for JSON arrays keyed by soundList / sound_list."""
    for key in ("soundList", "sound_list"):
        pattern = rf'"{key}"\s*:\s*(\[.*?\])\s*[,}}\]]'
        m = re.search(pattern, html, re.DOTALL)
        if m:
            try:
                return json.loads(m.group(1))
            except json.JSONDecodeError:
                continue
    return None


def _try_html(region: str) -> list | None:
    """Fetch the Creative Center page and try every extraction strategy."""
    try:
        with httpx.Client(timeout=30, follow_redirects=True) as client:
            resp = client.get(CREATIVE_CENTER_URL, headers=HEADERS)
            resp.raise_for_status()
            html = resp.text

        soup = BeautifulSoup(html, "html.parser")

        for strategy_name, strategy in [
            ("__NEXT_DATA__", lambda: _extract_next_data(soup)),
            ("script-scan", lambda: _extract_script_scan(soup)),
            ("regex", lambda: _extract_regex(html)),
        ]:
            result = strategy()
            if result:
                out.log(f"Source: Creative Center HTML ({strategy_name})")
                return result

    except (httpx.HTTPError, httpx.ConnectError):
        pass
    return None


def find_trending_sounds(limit: int = 10, region: str = "US") -> dict:
    """Fetch trending sounds — API first, HTML fallback, multiple strategies."""
    out.log(f"Fetching trending TikTok sounds (region={region}, limit={limit})...")

    # Strategy 1: JSON API (stable, no HTML parsing)
    sound_list = _try_api(limit, region)

    # Strategy 2: HTML scraping with layered extraction
    if not sound_list:
        out.log("API unavailable, falling back to HTML scraping...")
        sound_list = _try_html(region)

    if not sound_list:
        raise RuntimeError(
            "Could not extract trending sounds from TikTok Creative Center. "
            "Both the JSON API and HTML extraction failed — the page structure "
            "may have changed. File an issue or import sounds directly with: "
            "capcut-cli library import <tiktok_url> --type sound"
        )

    sounds = [
        _normalize_sound(s, rank=i + 1) for i, s in enumerate(sound_list[:limit])
    ]

    return {
        "sounds": sounds,
        "source": "tiktok_creative_center",
        "region": region,
        "period": "7d",
        "total_found": len(sounds),
        "import_hint": "Import a sound with: capcut-cli library import <tiktok_url> --type sound",
    }
