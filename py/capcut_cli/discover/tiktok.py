"""TikTok trending sounds discovery via Creative Center page scraping."""
import json

import httpx
from bs4 import BeautifulSoup

from capcut_cli import output as out


CREATIVE_CENTER_URL = "https://ads.tiktok.com/business/creativecenter/inspiration/popular/music/pc/en"

HEADERS = {
    "User-Agent": "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
    "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
    "Accept-Language": "en-US,en;q=0.9",
}


def find_trending_sounds(limit: int = 10, region: str = "US") -> dict:
    """Fetch trending sounds by scraping TikTok Creative Center page data."""
    out.log(f"Fetching trending TikTok sounds (region={region}, limit={limit})...")

    try:
        with httpx.Client(timeout=30, follow_redirects=True) as client:
            resp = client.get(CREATIVE_CENTER_URL, headers=HEADERS)
            resp.raise_for_status()

            soup = BeautifulSoup(resp.text, "html.parser")

            # Find the script tag containing the embedded page data
            sound_list = None
            for script in soup.find_all("script"):
                text = script.string or ""
                if "soundList" in text:
                    try:
                        data = json.loads(text)
                        sound_list = (
                            data.get("props", {})
                            .get("pageProps", {})
                            .get("data", {})
                            .get("soundList", [])
                        )
                        if sound_list:
                            break
                    except json.JSONDecodeError:
                        continue

            if not sound_list:
                raise RuntimeError(
                    "Could not extract trending sounds from Creative Center page. "
                    "The page structure may have changed."
                )

            sounds = []
            for s in sound_list[:limit]:
                sound = {
                    "rank": s.get("rank", len(sounds) + 1),
                    "title": s.get("title", "Unknown"),
                    "artist": s.get("author", "Unknown"),
                    "tiktok_url": s.get("link", ""),
                    "cover_url": s.get("cover", ""),
                    "duration_seconds": s.get("duration", 0),
                    "is_promoted": s.get("promoted", False),
                }
                sounds.append(sound)

            return {
                "sounds": sounds,
                "source": "tiktok_creative_center",
                "region": region,
                "period": "7d",
                "total_found": len(sounds),
                "import_hint": "Import a sound with: capcut-cli library import <tiktok_url> --type sound",
            }

    except httpx.HTTPStatusError as e:
        raise RuntimeError(f"TikTok Creative Center returned HTTP {e.response.status_code}")
    except httpx.ConnectError:
        raise RuntimeError(
            "Could not connect to TikTok Creative Center. "
            "Try importing sounds directly: capcut-cli library import <tiktok_url> --type sound"
        )
