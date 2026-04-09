"""Twitter/X viral clip discovery — search URL generation + guidance."""
import urllib.parse


def find_viral_clips(query: str, limit: int = 10, min_likes: int = 1000) -> dict:
    """Generate X search URLs and instructions for finding viral clips.

    Direct X/Twitter scraping without API keys is extremely brittle (requires
    authenticated sessions, TLS fingerprinting, rotating cookies). Instead,
    we generate the optimal search URLs and instructions for the agent or user
    to follow.
    """
    # Build Twitter advanced search queries
    search_queries = [
        f"{query} min_faves:{min_likes} filter:videos",
        f"{query} min_faves:{min_likes // 2} min_retweets:{min_likes // 10} filter:videos",
    ]

    search_urls = []
    for sq in search_queries:
        encoded = urllib.parse.quote(sq)
        search_urls.append({
            "query": sq,
            "url": f"https://x.com/search?q={encoded}&f=video",
            "description": f"Video search for '{query}' with engagement filter",
        })

    return {
        "method": "guided_discovery",
        "query": query,
        "min_likes": min_likes,
        "search_urls": search_urls,
        "instructions": [
            f"Open one of the search URLs below in a browser or use a browser-control agent",
            f"Find tweets with video content matching '{query}'",
            "Copy the tweet URL (e.g., https://x.com/user/status/123456)",
            "Import with: capcut-cli library import <tweet_url> --type clip",
        ],
        "import_hint": "capcut-cli library import <tweet_url> --type clip",
        "total_queries": len(search_urls),
        "note": "X/Twitter requires authenticated sessions for search scraping. "
                "The search URLs work in a logged-in browser. "
                "For automated discovery, use a browser-control MCP or Twitter API key.",
    }
