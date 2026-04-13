use anyhow::Result;
use chrono::{DateTime, Utc};
use serde_json::json;
use thiserror::Error;

use crate::library;
use crate::output;

const TWITTER_SEARCH_V2: &str = "https://api.twitter.com/2/tweets/search/recent";

#[derive(Debug, Error)]
pub enum TwitterDiscoveryError {
    #[error("TWITTER_BEARER_TOKEN is required for reliable X/Twitter clip discovery.")]
    AuthRequired,
    #[error("X/Twitter API rate limit reached. Retry later.")]
    RateLimited,
    #[error("X/Twitter API request failed: {message}")]
    ApiRequest { message: String },
    #[error("X/Twitter API returned status {status}.")]
    ApiStatus { status: u16 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipDiscoveryStrategy {
    Auto,
    Api,
    Guided,
    Library,
    ManualUrl,
}

impl ClipDiscoveryStrategy {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "api" | "twitter-api" | "x-api" => Ok(Self::Api),
            "guided" | "guided-fallback" | "browser" => Ok(Self::Guided),
            "library" => Ok(Self::Library),
            "manual-url" | "manual_url" | "manual" => Ok(Self::ManualUrl),
            other => anyhow::bail!(
                "Unknown X clip discovery strategy '{other}'. Available: auto, api, guided, library, manual-url."
            ),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Api => "api",
            Self::Guided => "guided",
            Self::Library => "library",
            Self::ManualUrl => "manual-url",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClipDiscoveryOptions {
    pub query: String,
    pub limit: u32,
    pub min_likes: u64,
    pub strategy: ClipDiscoveryStrategy,
    pub manual_url: Option<String>,
}

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

fn metric_u64(metrics: &serde_json::Value, key: &str) -> u64 {
    metrics.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
}

fn age_hours(created_at: Option<&str>) -> f64 {
    created_at
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|value| {
            let age = Utc::now() - value.with_timezone(&Utc);
            (age.num_seconds().max(0) as f64) / 3600.0
        })
        .unwrap_or(24.0)
}

fn clip_score(metrics: &serde_json::Value, created_at: Option<&str>) -> f64 {
    let likes = metric_u64(metrics, "like_count") as f64;
    let retweets = metric_u64(metrics, "retweet_count") as f64;
    let replies = metric_u64(metrics, "reply_count") as f64;
    let quotes = metric_u64(metrics, "quote_count") as f64;
    let views = metric_u64(metrics, "impression_count").max(metric_u64(metrics, "view_count")) as f64;
    let age_penalty = age_hours(created_at) * 0.08;

    let raw_score = likes.ln_1p() * 2.0
        + retweets.ln_1p() * 2.4
        + replies.ln_1p() * 1.2
        + quotes.ln_1p() * 1.8
        + views.ln_1p() * 0.75
        - age_penalty;

    (raw_score * 1000.0).round() / 1000.0
}

fn fallback_guided_discovery(
    query: &str,
    min_likes: u64,
    search_urls: Vec<serde_json::Value>,
    reason: &str,
) -> serde_json::Value {
    json!({
        "method": "guided_discovery",
        "recommended": false,
        "fallback_mode": true,
        "query": query,
        "min_likes": min_likes,
        "reason": reason,
        "search_urls": search_urls,
        "instructions": [
            "Open one of the search URLs below in a browser or use a browser-control agent",
            format!("Find tweets with video content matching '{query}'"),
            "Copy the tweet URL (e.g., https://x.com/user/status/123456)",
            "Import with: capcut-cli library import <tweet_url> --type clip",
        ],
        "import_hint": "capcut-cli library import <tweet_url> --type clip",
        "setup_hint": "Set TWITTER_BEARER_TOKEN for official discovery and ensure a logged-in browser is available for X media import.",
        "note": "This is a fallback path. The recommended strong-yes path uses authenticated X discovery plus authenticated media retrieval.",
    })
}

fn library_candidates(query: &str, limit: u32) -> Result<serde_json::Value> {
    let mut assets = library::list_assets(Some("clip"))?;
    assets.sort_by(|a, b| {
        b.downloaded_at
            .cmp(&a.downloaded_at)
            .then_with(|| a.title.cmp(&b.title))
    });

    let clips: Vec<_> = assets
        .into_iter()
        .take(limit as usize)
        .enumerate()
        .map(|(index, asset)| {
            json!({
                "rank": index + 1,
                "asset_id": asset.id,
                "title": asset.title,
                "tweet_url": asset.source_url,
                "import_url": asset.source_url,
                "source_path": "library",
                "source_platform": asset.source_platform,
                "duration_seconds": asset.duration_seconds,
                "downloaded_at": asset.downloaded_at,
                "ranking_score": ((limit as usize).saturating_sub(index)) as f64,
                "auth_required_for_import": false,
            })
        })
        .collect();

    Ok(json!({
        "method": "library",
        "recommended": true,
        "query": query,
        "clips": clips,
        "total_found": clips.len(),
        "import_hint": "Reuse an existing clip asset directly from the local library.",
        "note": "This is the fastest and most reliable path when fresh X discovery is not required.",
    }))
}

fn manual_url_candidates(query: &str, manual_url: &str) -> Result<serde_json::Value> {
    let trimmed = manual_url.trim();
    if trimmed.is_empty() {
        anyhow::bail!("manual-url strategy requires a non-empty --clip-url value.");
    }

    Ok(json!({
        "method": "manual-url",
        "recommended": true,
        "query": query,
        "clips": [{
            "rank": 1,
            "tweet_url": trimmed,
            "import_url": trimmed,
            "source_path": "manual-url",
            "ranking_score": 1.0,
            "auth_required_for_import": true,
        }],
        "total_found": 1,
        "import_hint": "Import the provided clip URL with: capcut-cli library import <clip_url> --type clip",
    }))
}

/// Execute a live search via Twitter API v2 if bearer token is available.
fn try_api_search(query: &str, limit: u32, min_likes: u64) -> Result<Vec<serde_json::Value>> {
    let bearer = std::env::var("TWITTER_BEARER_TOKEN")
        .map_err(|_| TwitterDiscoveryError::AuthRequired)?;

    let search_query = format!("{query} has:videos -is:retweet");
    let max_results = limit.clamp(10, 100);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| TwitterDiscoveryError::ApiRequest {
            message: e.to_string(),
        })?;

    let resp = client
        .get(TWITTER_SEARCH_V2)
        .bearer_auth(&bearer)
        .header("User-Agent", "capcut-cli/0.1.0")
        .query(&[
            ("query", search_query.as_str()),
            ("max_results", &max_results.to_string()),
            ("tweet.fields", "attachments,created_at,public_metrics"),
            ("expansions", "author_id,attachments.media_keys"),
            ("media.fields", "duration_ms,preview_image_url,public_metrics,type,url"),
            ("user.fields", "username,name"),
        ])
        .send()
        .map_err(|e| TwitterDiscoveryError::ApiRequest {
            message: e.to_string(),
        })?;

    if resp.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Err(TwitterDiscoveryError::RateLimited.into());
    }
    if !resp.status().is_success() {
        return Err(TwitterDiscoveryError::ApiStatus {
            status: resp.status().as_u16(),
        }
        .into());
    }

    let body: serde_json::Value = resp.json().map_err(|e| TwitterDiscoveryError::ApiRequest {
        message: e.to_string(),
    })?;
    let tweets = body
        .get("data")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut users = std::collections::HashMap::new();
    let mut media_lookup = std::collections::HashMap::new();
    if let Some(includes) = body.get("includes") {
        if let Some(user_list) = includes.get("users").and_then(|v| v.as_array()) {
            for user in user_list {
                if let Some(id) = user.get("id").and_then(|v| v.as_str()) {
                    users.insert(id.to_string(), user.clone());
                }
            }
        }
        if let Some(media_list) = includes.get("media").and_then(|v| v.as_array()) {
            for media in media_list {
                if let Some(key) = media.get("media_key").and_then(|v| v.as_str()) {
                    media_lookup.insert(key.to_string(), media.clone());
                }
            }
        }
    }

    let mut results: Vec<(f64, serde_json::Value)> = Vec::new();
    for tweet in tweets {
        let metrics = tweet.get("public_metrics").cloned().unwrap_or(json!({}));
        let likes = metric_u64(&metrics, "like_count");
        if likes < min_likes {
            continue;
        }

        let media_keys: Vec<String> = tweet
            .get("attachments")
            .and_then(|v| v.get("media_keys"))
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|value| value.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        if media_keys.is_empty() {
            continue;
        }

        let video_media: Vec<serde_json::Value> = media_keys
            .iter()
            .filter_map(|key| media_lookup.get(key).cloned())
            .filter(|media| {
                matches!(
                    media.get("type").and_then(|v| v.as_str()),
                    Some("video") | Some("animated_gif")
                )
            })
            .collect();
        if video_media.is_empty() {
            continue;
        }

        let author_id = tweet.get("author_id").and_then(|v| v.as_str()).unwrap_or("");
        let user = users.get(author_id);
        let username = user
            .and_then(|u| u.get("username"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let author = user
            .and_then(|u| u.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or(username);

        let tweet_id = tweet.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let tweet_url = format!("https://x.com/{username}/status/{tweet_id}");
        let created_at = tweet.get("created_at").and_then(|v| v.as_str());
        let score = clip_score(&metrics, created_at);
        let preview_image_url = video_media
            .first()
            .and_then(|media| media.get("preview_image_url"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let duration_ms = video_media
            .first()
            .and_then(|media| media.get("duration_ms"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let text = tweet
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .chars()
            .take(200)
            .collect::<String>();

        results.push((
            score,
            json!({
                "tweet_url": tweet_url,
                "import_url": tweet_url,
                "text": text,
                "author": author,
                "username": username,
                "created_at": created_at.unwrap_or(""),
                "preview_image_url": preview_image_url,
                "duration_ms": duration_ms,
                "video_count": video_media.len(),
                "ranking_score": score,
                "auth_required_for_import": true,
                "engagement_metrics": {
                    "likes": likes,
                    "retweets": metric_u64(&metrics, "retweet_count"),
                    "replies": metric_u64(&metrics, "reply_count"),
                    "quotes": metric_u64(&metrics, "quote_count"),
                    "views": metric_u64(&metrics, "impression_count").max(metric_u64(&metrics, "view_count")),
                }
            }),
        ));
    }

    results.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.1.get("tweet_url")
                    .and_then(|v| v.as_str())
                    .cmp(&a.1.get("tweet_url").and_then(|v| v.as_str()))
            })
    });

    let ranked = results
        .into_iter()
        .enumerate()
        .map(|(index, (_, mut clip))| {
            if let Some(object) = clip.as_object_mut() {
                object.insert("rank".to_string(), json!(index + 1));
            }
            clip
        })
        .collect();

    Ok(ranked)
}

/// Find viral video clips on X/Twitter.
pub fn find_viral_clips(
    query: &str,
    limit: u32,
    min_likes: u64,
    allow_guided_fallback: bool,
) -> Result<serde_json::Value> {
    output::log(&format!(
        "Searching X/Twitter for viral clips: '{query}' (min_likes={min_likes})..."
    ));

    let search_urls = build_queries(query, min_likes);

    let api_results = match try_api_search(query, limit, min_likes) {
        Ok(results) => results,
        Err(error) => {
            if let Some(TwitterDiscoveryError::AuthRequired) =
                error.downcast_ref::<TwitterDiscoveryError>()
            {
                if allow_guided_fallback {
                    output::log(
                        "Reliable X discovery requires TWITTER_BEARER_TOKEN; returning guided fallback.",
                    );
                    return Ok(fallback_guided_discovery(
                        query,
                        min_likes,
                        search_urls,
                        "TWITTER_BEARER_TOKEN not set",
                    ));
                }
            }
            return Err(error);
        }
    };

    if api_results.is_empty() && allow_guided_fallback {
        output::log("X API returned no video candidates; returning guided fallback.");
        return Ok(fallback_guided_discovery(
            query,
            min_likes,
            search_urls,
            "API search returned no results matching filters",
        ));
    }

    let count = api_results.len();
    output::log(&format!("Found {count} clips via Twitter API v2"));
    let clipped: Vec<_> = api_results.into_iter().take(limit as usize).collect();
    Ok(json!({
        "method": "api_search",
        "recommended": true,
        "query": query,
        "min_likes": min_likes,
        "clips": clipped,
        "total_found": count,
        "search_urls": search_urls,
        "import_hint": "Import a clip with: capcut-cli library import <import_url> --type clip",
        "auth": {
            "discovery": "TWITTER_BEARER_TOKEN",
            "import": "browser_cookies"
        }
    }))
}

pub fn find_viral_clips_with_options(options: &ClipDiscoveryOptions) -> Result<serde_json::Value> {
    output::log(&format!(
        "Searching X/Twitter clips with strategy '{}' for query '{}'...",
        options.strategy.as_str(),
        options.query
    ));

    match options.strategy {
        ClipDiscoveryStrategy::Auto => {
            if let Some(url) = options.manual_url.as_deref() {
                return manual_url_candidates(&options.query, url);
            }

            if std::env::var("TWITTER_BEARER_TOKEN")
                .ok()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
            {
                if let Ok(results) = find_viral_clips_with_options(&ClipDiscoveryOptions {
                    query: options.query.clone(),
                    limit: options.limit,
                    min_likes: options.min_likes,
                    strategy: ClipDiscoveryStrategy::Api,
                    manual_url: None,
                }) {
                    return Ok(results);
                }
            }

            let guided = find_viral_clips_with_options(&ClipDiscoveryOptions {
                query: options.query.clone(),
                limit: options.limit,
                min_likes: options.min_likes,
                strategy: ClipDiscoveryStrategy::Guided,
                manual_url: None,
            })?;

            let has_urls = guided
                .get("search_urls")
                .and_then(|value| value.as_array())
                .map(|items| !items.is_empty())
                .unwrap_or(false);
            if has_urls {
                return Ok(guided);
            }

            return library_candidates(&options.query, options.limit);
        }
        ClipDiscoveryStrategy::Api => {
            find_viral_clips(&options.query, options.limit, options.min_likes, false)
        }
        ClipDiscoveryStrategy::Guided => {
            find_viral_clips(&options.query, options.limit, options.min_likes, true)
        }
        ClipDiscoveryStrategy::Library => library_candidates(&options.query, options.limit),
        ClipDiscoveryStrategy::ManualUrl => manual_url_candidates(
            &options.query,
            options.manual_url.as_deref().unwrap_or(""),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clip_score_prefers_more_engagement() {
        let low = json!({
            "like_count": 100,
            "retweet_count": 10,
            "reply_count": 5,
            "quote_count": 1,
            "impression_count": 1000
        });
        let high = json!({
            "like_count": 5000,
            "retweet_count": 800,
            "reply_count": 200,
            "quote_count": 120,
            "impression_count": 80000
        });

        assert!(clip_score(&high, Some("2026-04-12T00:00:00Z")) > clip_score(&low, Some("2026-04-12T00:00:00Z")));
    }

    #[test]
    fn test_clip_score_penalizes_age() {
        let metrics = json!({
            "like_count": 1000,
            "retweet_count": 150,
            "reply_count": 70,
            "quote_count": 20,
            "impression_count": 20000
        });

        assert!(clip_score(&metrics, Some("2026-04-12T23:00:00Z")) > clip_score(&metrics, Some("2026-04-01T00:00:00Z")));
    }

    #[test]
    fn test_fallback_guided_discovery_is_explicitly_not_recommended() {
        let payload = fallback_guided_discovery(
            "ai agents",
            1000,
            build_queries("ai agents", 1000),
            "TWITTER_BEARER_TOKEN not set",
        );

        assert_eq!(payload.get("recommended").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(payload.get("fallback_mode").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn test_strategy_parse_accepts_aliases() {
        assert_eq!(
            ClipDiscoveryStrategy::parse("x-api").unwrap(),
            ClipDiscoveryStrategy::Api
        );
        assert_eq!(
            ClipDiscoveryStrategy::parse("browser").unwrap(),
            ClipDiscoveryStrategy::Guided
        );
        assert_eq!(
            ClipDiscoveryStrategy::parse("manual").unwrap(),
            ClipDiscoveryStrategy::ManualUrl
        );
    }

    #[test]
    fn test_strategy_parse_rejects_unknown_values() {
        let error = ClipDiscoveryStrategy::parse("totally-unknown").unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Unknown X clip discovery strategy")
        );
    }

    #[test]
    fn test_manual_url_candidates_use_manual_source() {
        let payload = manual_url_candidates("ai agents", "https://x.com/openai/status/123").unwrap();
        let clip = payload
            .get("clips")
            .and_then(|value| value.as_array())
            .and_then(|items| items.first())
            .cloned()
            .unwrap();

        assert_eq!(clip.get("source_path").and_then(|v| v.as_str()), Some("manual-url"));
        assert_eq!(
            clip.get("import_url").and_then(|v| v.as_str()),
            Some("https://x.com/openai/status/123")
        );
    }

    #[test]
    fn test_find_viral_clips_with_options_manual_url_returns_manual_method() {
        let payload = find_viral_clips_with_options(&ClipDiscoveryOptions {
            query: "ai agents".to_string(),
            limit: 3,
            min_likes: 1000,
            strategy: ClipDiscoveryStrategy::ManualUrl,
            manual_url: Some("https://x.com/openai/status/123".to_string()),
        })
        .unwrap();

        assert_eq!(payload.get("method").and_then(|v| v.as_str()), Some("manual-url"));
        assert_eq!(payload.get("recommended").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn test_library_candidates_return_existing_assets_when_available() {
        let payload = library_candidates("ai agents", 3).unwrap();
        assert_eq!(payload.get("method").and_then(|v| v.as_str()), Some("library"));
        assert!(payload.get("clips").and_then(|v| v.as_array()).is_some());
    }
}
