use anyhow::Result;
use regex::Regex;
use scraper::{Html, Selector};
use serde_json::json;

use crate::output;

const CREATIVE_CENTER_API: &str =
    "https://ads.tiktok.com/creative_radar_api/v1/popular/sound/list";

const CREATIVE_CENTER_URL: &str =
    "https://ads.tiktok.com/business/creativecenter/inspiration/popular/music/pc/en";

const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// Normalize a sound entry from any of the known payload shapes.
fn normalize_sound(raw: &serde_json::Value, rank: usize) -> serde_json::Value {
    json!({
        "rank": raw.get("rank").and_then(|v| v.as_u64()).unwrap_or(rank as u64),
        "title": raw.get("title").and_then(|v| v.as_str())
            .or_else(|| raw.get("musicName").and_then(|v| v.as_str()))
            .unwrap_or("Unknown"),
        "artist": raw.get("author").and_then(|v| v.as_str())
            .or_else(|| raw.get("artistName").and_then(|v| v.as_str()))
            .or_else(|| raw.get("creator").and_then(|c| c.get("nickname")).and_then(|v| v.as_str()))
            .unwrap_or("Unknown"),
        "tiktok_url": raw.get("link").and_then(|v| v.as_str())
            .or_else(|| raw.get("playUrl").and_then(|v| v.as_str()))
            .unwrap_or(""),
        "cover_url": raw.get("cover").and_then(|v| v.as_str())
            .or_else(|| raw.get("coverUrl").and_then(|v| v.as_str()))
            .unwrap_or(""),
        "duration_seconds": raw.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0),
        "is_promoted": raw.get("promoted").and_then(|v| v.as_bool()).unwrap_or(false),
    })
}

fn http_client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent(USER_AGENT)
        .build()?)
}

/// Try the Creative Center JSON API (no HTML parsing).
fn try_api(limit: u32, region: &str) -> Option<Vec<serde_json::Value>> {
    let client = http_client().ok()?;
    let resp = client
        .get(CREATIVE_CENTER_API)
        .header("Accept", "application/json")
        .query(&[
            ("period", "7"),
            ("page", "1"),
            ("limit", &limit.to_string()),
            ("country_code", region),
            ("sort_by", "popularity"),
        ])
        .send()
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let body: serde_json::Value = resp.json().ok()?;
    let data = body.get("data")?;

    let sound_list = data
        .get("sound_list")
        .or_else(|| data.get("soundList"))
        .or_else(|| data.get("list"))?;

    let arr = sound_list.as_array()?;
    if arr.is_empty() {
        return None;
    }

    output::log("Source: Creative Center API (JSON)");
    Some(arr.clone())
}

// ── HTML extraction strategies ──────────────────────────────────────

/// Next.js __NEXT_DATA__ script tag.
fn extract_next_data(document: &Html) -> Option<Vec<serde_json::Value>> {
    let sel = Selector::parse("script#__NEXT_DATA__").ok()?;
    let el = document.select(&sel).next()?;
    let text = el.text().collect::<String>();
    let data: serde_json::Value = serde_json::from_str(&text).ok()?;

    let list = data
        .get("props")?
        .get("pageProps")?
        .get("data")?
        .get("soundList")?
        .as_array()?;

    if list.is_empty() { None } else { Some(list.clone()) }
}

/// Scan all <script> tags for any JSON blob containing soundList.
fn extract_script_scan(document: &Html) -> Option<Vec<serde_json::Value>> {
    let sel = Selector::parse("script").ok()?;

    for el in document.select(&sel) {
        let text = el.text().collect::<String>();
        if !text.contains("soundList") && !text.contains("sound_list") {
            continue;
        }
        let data: serde_json::Value = match serde_json::from_str(&text) {
            Ok(d) => d,
            Err(_) => continue,
        };

        // Try known nesting patterns
        let paths: Vec<Box<dyn Fn(&serde_json::Value) -> Option<&serde_json::Value>>> = vec![
            Box::new(|d| d.get("props")?.get("pageProps")?.get("data")?.get("soundList")),
            Box::new(|d| d.get("props")?.get("pageProps")?.get("soundList")),
            Box::new(|d| d.get("data")?.get("soundList")),
            Box::new(|d| d.get("data")?.get("sound_list")),
            Box::new(|d| d.get("soundList")),
        ];

        for path_fn in &paths {
            if let Some(list) = path_fn(&data).and_then(|v| v.as_array()) {
                if !list.is_empty() {
                    return Some(list.clone());
                }
            }
        }
    }
    None
}

/// Last resort — regex for JSON arrays keyed by soundList / sound_list.
fn extract_regex(html: &str) -> Option<Vec<serde_json::Value>> {
    for key in &["soundList", "sound_list"] {
        let pattern = format!(r#""{key}"\s*:\s*(\[.*?\])\s*[,}}\]]"#);
        if let Ok(re) = Regex::new(&pattern) {
            if let Some(caps) = re.captures(html) {
                if let Some(arr_str) = caps.get(1) {
                    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(arr_str.as_str())
                    {
                        if !arr.is_empty() {
                            return Some(arr);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Fetch the Creative Center page and try every extraction strategy.
fn try_html(_region: &str) -> Option<Vec<serde_json::Value>> {
    let client = http_client().ok()?;
    let resp = client
        .get(CREATIVE_CENTER_URL)
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let html = resp.text().ok()?;
    let document = Html::parse_document(&html);

    let strategies: Vec<(&str, Box<dyn Fn() -> Option<Vec<serde_json::Value>>>)> = vec![
        ("__NEXT_DATA__", Box::new(|| extract_next_data(&document))),
        ("script-scan", Box::new(|| extract_script_scan(&document))),
        ("regex", Box::new(|| extract_regex(&html))),
    ];

    for (name, strategy) in &strategies {
        if let Some(result) = strategy() {
            output::log(&format!("Source: Creative Center HTML ({name})"));
            return Some(result);
        }
    }
    None
}

/// Fetch trending sounds — API first, HTML fallback, multiple strategies.
pub fn find_trending_sounds(limit: u32, region: &str) -> Result<serde_json::Value> {
    output::log(&format!(
        "Fetching trending TikTok sounds (region={region}, limit={limit})..."
    ));

    // Strategy 1: JSON API
    let mut sound_list = try_api(limit, region);

    // Strategy 2: HTML scraping with layered extraction
    if sound_list.is_none() {
        output::log("API unavailable, falling back to HTML scraping...");
        sound_list = try_html(region);
    }

    let sound_list = sound_list.ok_or_else(|| {
        anyhow::anyhow!(
            "Could not extract trending sounds from TikTok Creative Center. \
             Both the JSON API and HTML extraction failed — the page structure \
             may have changed. File an issue or import sounds directly with: \
             capcut-cli library import <tiktok_url> --type sound"
        )
    })?;

    let sounds: Vec<serde_json::Value> = sound_list
        .iter()
        .take(limit as usize)
        .enumerate()
        .map(|(i, s)| normalize_sound(s, i + 1))
        .collect();

    Ok(json!({
        "sounds": sounds,
        "source": "tiktok_creative_center",
        "region": region,
        "period": "7d",
        "total_found": sounds.len(),
        "import_hint": "Import a sound with: capcut-cli library import <tiktok_url> --type sound",
    }))
}
