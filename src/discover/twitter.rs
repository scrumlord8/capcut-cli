use anyhow::Result;
use serde_json::json;

use crate::output;

const TWITTER_SEARCH_V2: &str = "https://api.twitter.com/2/tweets/search/recent";

/// Build Twitter advanced search queries with engagement filters.
fn build_queries(query: &str, min_likes: u64) -> Vec<serde_json::Value> {
    let raw_queries = vec![
        format!("{query} min_faves:{min_likes} has:videos -is:retweet"),
        format!(
            "{query} min_faves:{} min_retweets:{} has:videos -is:retweet",
            min_likes / 2,
            min_likes / 10
        ),
    ];

    raw_queries
        .into_iter()
        .map(|sq| {
            let encoded = urlencoding(&sq);
            json!({
                "query": sq,
                "url": format!("https://x.com/search?q={encoded}&f=video"),
                "description": format!("Video search for '{query}' with engagement filter"),
            })
        })
        .collect()
}

/// Simple percent-encoding for URL query strings.
fn urlencoding(s: &str) -> String {
    let mut result = String::new();
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    result
}

/// Execute a live search via Twitter API v2 if bearer token is available.
fn try_api_search(query: &str, limit: u32, min_likes: u64) -> Option<Vec<serde_json::Value>> {
    let bearer = std::env::var("TWITTER_BEARER_TOKEN").ok()?;

    let search_query = format!("{query} has:videos -is:retweet");
    let max_results = limit.min(100).max(10); // API requires 10-100

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .ok()?;

    let resp = client
        .get(TWITTER_SEARCH_V2)
        .bearer_auth(&bearer)
        .header("User-Agent", "capcut-cli/0.1.0")
        .query(&[
            ("query", search_query.as_str()),
            ("max_results", &max_results.to_string()),
            ("tweet.fields", "public_metrics,created_at,entities"),
            ("expansions", "author_id,attachments.media_keys"),
            ("media.fields", "url,preview_image_url,duration_ms,type"),
            ("user.fields", "username,name"),
        ])
        .send()
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let body: serde_json::Value = resp.json().ok()?;
    let tweets = body.get("data")?.as_array()?;
    if tweets.is_empty() {
        return None;
    }

    // Build user lookup
    let mut users = std::collections::HashMap::new();
    if let Some(includes) = body.get("includes") {
        if let Some(user_list) = includes.get("users").and_then(|v| v.as_array()) {
            for u in user_list {
                if let Some(id) = u.get("id").and_then(|v| v.as_str()) {
                    users.insert(id.to_string(), u.clone());
                }
            }
        }
    }

    let mut results = Vec::new();
    for tweet in tweets {
        let metrics = tweet.get("public_metrics").cloned().unwrap_or(json!({}));
        let likes = metrics.get("like_count").and_then(|v| v.as_u64()).unwrap_or(0);
        if likes < min_likes {
            continue;
        }

        let author_id = tweet
            .get("author_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let user = users.get(author_id);
        let username = user
            .and_then(|u| u.get("username"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let name = user
            .and_then(|u| u.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or(username);

        let tweet_id = tweet.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let tweet_url = format!("https://x.com/{username}/status/{tweet_id}");

        let text = tweet
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let truncated: String = text.chars().take(200).collect();

        results.push(json!({
            "tweet_url": tweet_url,
            "text": truncated,
            "author": name,
            "username": username,
            "likes": likes,
            "retweets": metrics.get("retweet_count").and_then(|v| v.as_u64()).unwrap_or(0),
            "views": metrics.get("impression_count").and_then(|v| v.as_u64()).unwrap_or(0),
            "created_at": tweet.get("created_at").and_then(|v| v.as_str()).unwrap_or(""),
        }));
    }

    if results.is_empty() {
        None
    } else {
        Some(results)
    }
}

/// Find viral video clips on X/Twitter.
pub fn find_viral_clips(query: &str, limit: u32, min_likes: u64) -> Result<serde_json::Value> {
    output::log(&format!(
        "Searching X/Twitter for viral clips: '{query}' (min_likes={min_likes})..."
    ));

    let search_urls = build_queries(query, min_likes);

    // Try live API search first
    if let Some(api_results) = try_api_search(query, limit, min_likes) {
        let count = api_results.len();
        output::log(&format!("Found {count} clips via Twitter API v2"));
        let clipped: Vec<_> = api_results.into_iter().take(limit as usize).collect();
        return Ok(json!({
            "method": "api_search",
            "query": query,
            "min_likes": min_likes,
            "clips": clipped,
            "total_found": count,
            "search_urls": search_urls,
            "import_hint": "capcut-cli library import <tweet_url> --type clip",
        }));
    }

    // Fallback: guided discovery
    let reason = if std::env::var("TWITTER_BEARER_TOKEN").is_err() {
        "TWITTER_BEARER_TOKEN not set"
    } else {
        "API search returned no results matching filters"
    };
    output::log(&format!(
        "Live search unavailable ({reason}), returning guided discovery URLs"
    ));

    Ok(json!({
        "method": "guided_discovery",
        "query": query,
        "min_likes": min_likes,
        "search_urls": search_urls,
        "instructions": [
            "Open one of the search URLs below in a browser or use a browser-control agent",
            format!("Find tweets with video content matching '{query}'"),
            "Copy the tweet URL (e.g., https://x.com/user/status/123456)",
            "Import with: capcut-cli library import <tweet_url> --type clip",
        ],
        "import_hint": "capcut-cli library import <tweet_url> --type clip",
        "total_queries": search_urls.len(),
        "setup_hint": "Set TWITTER_BEARER_TOKEN env var to enable live API search. \
                        Get a free bearer token at https://developer.x.com/en/portal/dashboard",
        "note": "X/Twitter requires authenticated sessions for search scraping. \
                 The search URLs work in a logged-in browser. \
                 For automated discovery, set TWITTER_BEARER_TOKEN or use a browser-control MCP.",
    }))
}
