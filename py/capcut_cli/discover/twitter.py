"""Twitter/X viral clip discovery — live API search with guided fallback."""
import os
import urllib.parse

import httpx

from capcut_cli import output as out


TWITTER_SEARCH_V2 = "https://api.twitter.com/2/tweets/search/recent"


def _build_queries(query: str, min_likes: int) -> list[dict]:
    """Build Twitter advanced search queries with engagement filters."""
    raw_queries = [
        f"{query} min_faves:{min_likes} has:videos -is:retweet",
        f"{query} min_faves:{min_likes // 2} min_retweets:{min_likes // 10} has:videos -is:retweet",
    ]
    results = []
    for sq in raw_queries:
        encoded = urllib.parse.quote(sq)
        results.append({
            "query": sq,
            "url": f"https://x.com/search?q={encoded}&f=video",
            "description": f"Video search for '{query}' with engagement filter",
        })
    return results


def _try_api_search(query: str, limit: int, min_likes: int) -> list | None:
    """Execute a live search via Twitter API v2 if bearer token is available."""
    bearer = os.environ.get("TWITTER_BEARER_TOKEN")
    if not bearer:
        return None

    search_query = f"{query} has:videos -is:retweet"
    params = {
        "query": search_query,
        "max_results": min(limit, 100),
        "tweet.fields": "public_metrics,created_at,entities",
        "expansions": "author_id,attachments.media_keys",
        "media.fields": "url,preview_image_url,duration_ms,type",
        "user.fields": "username,name",
    }
    headers = {
        "Authorization": f"Bearer {bearer}",
        "User-Agent": "capcut-cli/0.1.0",
    }

    try:
        with httpx.Client(timeout=15) as client:
            resp = client.get(TWITTER_SEARCH_V2, params=params, headers=headers)
            resp.raise_for_status()
            body = resp.json()

        tweets = body.get("data", [])
        if not tweets:
            return None

        # Build user lookup from includes
        users = {}
        for u in body.get("includes", {}).get("users", []):
            users[u["id"]] = u

        # Build media lookup from includes
        media = {}
        for m in body.get("includes", {}).get("media", []):
            media[m["media_key"]] = m

        results = []
        for tweet in tweets:
            metrics = tweet.get("public_metrics", {})
            likes = metrics.get("like_count", 0)
            if likes < min_likes:
                continue

            author_id = tweet.get("author_id", "")
            user = users.get(author_id, {})
            username = user.get("username", "unknown")

            tweet_url = f"https://x.com/{username}/status/{tweet['id']}"

            results.append({
                "tweet_url": tweet_url,
                "text": tweet.get("text", "")[:200],
                "author": user.get("name", username),
                "username": username,
                "likes": likes,
                "retweets": metrics.get("retweet_count", 0),
                "views": metrics.get("impression_count", 0),
                "created_at": tweet.get("created_at", ""),
            })

        return results if results else None

    except (httpx.HTTPError, httpx.ConnectError, KeyError):
        return None


def find_viral_clips(query: str, limit: int = 10, min_likes: int = 1000) -> dict:
    """Find viral video clips on X/Twitter.

    Uses Twitter API v2 when TWITTER_BEARER_TOKEN is set.
    Falls back to guided discovery (search URLs + instructions) otherwise.
    """
    out.log(f"Searching X/Twitter for viral clips: '{query}' (min_likes={min_likes})...")

    search_urls = _build_queries(query, min_likes)

    # Try live API search first
    api_results = _try_api_search(query, limit, min_likes)

    if api_results:
        out.log(f"Found {len(api_results)} clips via Twitter API v2")
        return {
            "method": "api_search",
            "query": query,
            "min_likes": min_likes,
            "clips": api_results[:limit],
            "total_found": len(api_results),
            "search_urls": search_urls,
            "import_hint": "capcut-cli library import <tweet_url> --type clip",
        }

    # Fallback: guided discovery with search URLs
    reason = (
        "TWITTER_BEARER_TOKEN not set"
        if not os.environ.get("TWITTER_BEARER_TOKEN")
        else "API search returned no results matching filters"
    )
    out.log(f"Live search unavailable ({reason}), returning guided discovery URLs")

    return {
        "method": "guided_discovery",
        "query": query,
        "min_likes": min_likes,
        "search_urls": search_urls,
        "instructions": [
            "Open one of the search URLs below in a browser or use a browser-control agent",
            f"Find tweets with video content matching '{query}'",
            "Copy the tweet URL (e.g., https://x.com/user/status/123456)",
            "Import with: capcut-cli library import <tweet_url> --type clip",
        ],
        "import_hint": "capcut-cli library import <tweet_url> --type clip",
        "total_queries": len(search_urls),
        "setup_hint": "Set TWITTER_BEARER_TOKEN env var to enable live API search. "
                      "Get a free bearer token at https://developer.x.com/en/portal/dashboard",
        "note": "X/Twitter requires authenticated sessions for search scraping. "
                "The search URLs work in a logged-in browser. "
                "For automated discovery, set TWITTER_BEARER_TOKEN or use a browser-control MCP.",
    }
