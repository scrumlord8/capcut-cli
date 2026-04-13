use anyhow::{bail, Result};
use chrono::{DateTime, Duration, TimeZone, Utc};
use regex::Regex;
use scraper::{Html, Selector};
use serde::Serialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};

use crate::library;
use crate::media::downloader;
use crate::output;

const RESEARCH_API_URL: &str = "https://open.tiktokapis.com/v2/research/video/query/";
const CREATIVE_CENTER_API: &str =
    "https://ads.tiktok.com/creative_radar_api/v1/popular/sound/list";
const CREATIVE_CENTER_URL: &str =
    "https://ads.tiktok.com/business/creativecenter/pc/en";
const CREATIVE_CENTER_SONG_URL: &str =
    "https://ads.tiktok.com/business/creativecenter/song/{slug}/pc/en?countryCode={region}&period={period}";
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
const RESEARCH_FIELDS: &str = "id,create_time,region_code,video_description,music_id,like_count,comment_count,share_count,view_count,username,video_duration";
const RESEARCH_PAGE_SIZE: u32 = 100;
const RESEARCH_SAMPLE_CAP: u32 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundDiscoveryStrategy {
    Auto,
    Research,
    CreativeCenter,
    Library,
    ManualUrl,
}

impl SoundDiscoveryStrategy {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "research" => Ok(Self::Research),
            "creative-center" | "creative_center" | "creativecenter" => Ok(Self::CreativeCenter),
            "library" => Ok(Self::Library),
            "manual-url" | "manual_url" | "manual" => Ok(Self::ManualUrl),
            other => bail!(
                "Unknown TikTok sound discovery strategy '{other}'. Available: auto, research, creative-center, library, manual-url."
            ),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Research => "research",
            Self::CreativeCenter => "creative-center",
            Self::Library => "library",
            Self::ManualUrl => "manual-url",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SoundDiscoveryOptions {
    pub limit: u32,
    pub region: String,
    pub window_days: u32,
    pub strategy: SoundDiscoveryStrategy,
    pub manual_url: Option<String>,
}

fn debug_enabled() -> bool {
    std::env::var("CAPCUT_DEBUG_DISCOVERY").ok().as_deref() == Some("1")
}

fn debug_log(message: &str) {
    if debug_enabled() {
        output::log(message);
    }
}

fn http_client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent(USER_AGENT)
        .build()?)
}

fn configured_research_token() -> Option<String> {
    for name in [
        "TIKTOK_RESEARCH_ACCESS_TOKEN",
        "TIKTOK_RESEARCH_CLIENT_ACCESS_TOKEN",
    ] {
        if let Ok(value) = std::env::var(name) {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn value_str(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .or_else(|| value.get(key).and_then(|v| v.as_i64().map(|n| n.to_string())))
        .or_else(|| value.get(key).and_then(|v| v.as_u64().map(|n| n.to_string())))
}

fn value_u64(value: &serde_json::Value, key: &str) -> u64 {
    value
        .get(key)
        .and_then(|v| v.as_u64())
        .or_else(|| value.get(key).and_then(|v| v.as_i64()).map(|v| v.max(0) as u64))
        .unwrap_or(0)
}

fn value_i64(value: &serde_json::Value, key: &str) -> Option<i64> {
    value
        .get(key)
        .and_then(|v| v.as_i64())
        .or_else(|| value.get(key).and_then(|v| v.as_u64()).map(|v| v as i64))
}

fn utc_date_range(window_days: u32) -> Result<(String, String)> {
    if window_days == 0 {
        bail!("window_days must be at least 1.");
    }

    let end = Utc::now().date_naive();
    let start = end
        .checked_sub_signed(Duration::days(window_days.saturating_sub(1) as i64))
        .unwrap_or(end);
    Ok((
        start.format("%Y%m%d").to_string(),
        end.format("%Y%m%d").to_string(),
    ))
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct ResearchVideo {
    id: String,
    create_time: i64,
    region_code: String,
    video_description: String,
    music_id: String,
    like_count: u64,
    comment_count: u64,
    share_count: u64,
    view_count: u64,
    username: String,
}

impl ResearchVideo {
    fn age_days(&self, now_ts: i64) -> f64 {
        let age_seconds = now_ts.saturating_sub(self.create_time).max(0) as f64;
        age_seconds / 86_400.0
    }

    fn engagement_score(&self) -> f64 {
        (self.like_count as f64).ln_1p() * 2.0
            + (self.share_count as f64).ln_1p() * 2.8
            + (self.comment_count as f64).ln_1p() * 1.3
            + (self.view_count as f64).ln_1p() * 0.35
    }

    fn recency_weight(&self, now_ts: i64, window_days: u32) -> f64 {
        let window = window_days.max(1) as f64;
        let freshness = (1.0 - (self.age_days(now_ts) / window)).clamp(0.05, 1.0);
        freshness.powf(1.35)
    }

    fn contribution(&self, now_ts: i64, window_days: u32) -> f64 {
        self.recency_weight(now_ts, window_days) * (1.0 + self.engagement_score())
    }
}

#[derive(Debug, Clone)]
struct CandidateAggregate {
    music_id: String,
    score: f64,
    video_count: u64,
    total_views: u64,
    total_likes: u64,
    total_comments: u64,
    total_shares: u64,
    latest_video: Option<ResearchVideo>,
}

impl CandidateAggregate {
    fn new(music_id: String) -> Self {
        Self {
            music_id,
            score: 0.0,
            video_count: 0,
            total_views: 0,
            total_likes: 0,
            total_comments: 0,
            total_shares: 0,
            latest_video: None,
        }
    }

    fn add_video(&mut self, video: ResearchVideo, now_ts: i64, window_days: u32) {
        self.video_count += 1;
        self.total_views += video.view_count;
        self.total_likes += video.like_count;
        self.total_comments += video.comment_count;
        self.total_shares += video.share_count;
        self.score += 40.0 + video.contribution(now_ts, window_days) * 25.0;

        match &self.latest_video {
            Some(existing) if existing.create_time >= video.create_time => {}
            _ => self.latest_video = Some(video),
        }
    }

    fn finalize_score(&mut self) {
        self.score += (self.video_count as f64).powf(1.15) * 75.0;
        self.score += (self.total_views as f64).ln_1p() * 1.5;
    }
}

#[derive(Debug, Clone, Serialize)]
struct TrendingSoundCandidate {
    rank: u64,
    music_id: String,
    title: String,
    artist: String,
    tiktok_url: String,
    import_url: String,
    import_hint: String,
    source_path: String,
    source_url: String,
    ranking_score: f64,
    video_count: u64,
    total_views: u64,
    total_likes: u64,
    total_comments: u64,
    total_shares: u64,
    latest_video_create_time: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    enrichment_source: Option<String>,
}

fn tiktok_music_url(music_id: &str) -> String {
    format!("https://www.tiktok.com/music/_-{music_id}")
}

fn creative_center_song_url(slug: &str, region: &str, period: u32) -> String {
    CREATIVE_CENTER_SONG_URL
        .replace("{slug}", slug)
        .replace("{region}", region)
        .replace("{period}", &period.to_string())
}

fn parse_title_artist(page_title: &str) -> Option<(String, String)> {
    let leading = page_title.split(" | ").next()?.trim();
    if let Some((title, artist)) = leading.split_once(" created by ") {
        return Some((title.trim().to_string(), artist.trim().to_string()));
    }
    if let Some((title, artist)) = leading.split_once(" by ") {
        return Some((title.trim().to_string(), artist.trim().to_string()));
    }
    None
}

fn extract_cover_url(document: &Html) -> String {
    let selector = match Selector::parse("meta[property=\"og:image\"], meta[name=\"twitter:image\"]")
    {
        Ok(sel) => sel,
        Err(_) => return String::new(),
    };

    for meta in document.select(&selector) {
        if let Some(content) = meta.value().attr("content") {
            if !content.trim().is_empty() {
                return content.to_string();
            }
        }
    }

    String::new()
}

fn extract_view_more_link(document: &Html) -> String {
    let selector = match Selector::parse("a[href]") {
        Ok(sel) => sel,
        Err(_) => return String::new(),
    };

    for anchor in document.select(&selector) {
        let label = anchor.text().collect::<String>();
        let Some(href) = anchor.value().attr("href") else {
            continue;
        };
        if label.contains("View more on TikTok") || label.contains("View on TikTok") {
            return href.to_string();
        }
    }

    String::new()
}

fn extract_detail_payload(document: &Html) -> Option<serde_json::Value> {
    let selector = Selector::parse("script#__NEXT_DATA__").ok()?;
    let text = document.select(&selector).next()?.text().collect::<String>();
    let data: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some(data.get("props")?.get("pageProps")?.get("data")?.clone())
}

fn build_import_url(payload: Option<&serde_json::Value>, tiktok_url: &str) -> (String, String) {
    let preview_audio_url = payload
        .and_then(|p| p.get("musicUrl"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let import_url = if !preview_audio_url.is_empty() {
        preview_audio_url.clone()
    } else if let Some(item_id) = payload
        .and_then(|p| p.get("relatedItems"))
        .and_then(|v| v.as_array())
        .and_then(|items| items.first())
        .and_then(|item| item.get("itemId"))
        .and_then(|v| v.as_str())
    {
        format!("https://www.tiktok.com/embed/v2/{item_id}")
    } else {
        tiktok_url.to_string()
    };

    (import_url, preview_audio_url)
}

fn extract_song_detail_links(document: &Html) -> Vec<String> {
    let selector = match Selector::parse("a[href]") {
        Ok(sel) => sel,
        Err(_) => return Vec::new(),
    };

    let mut seen = HashSet::new();
    let mut links = Vec::new();

    for anchor in document.select(&selector) {
        let Some(href) = anchor.value().attr("href") else {
            continue;
        };
        if !href.contains("/business/creativecenter/song/") {
            continue;
        }

        let url = if href.starts_with("http://") || href.starts_with("https://") {
            href.to_string()
        } else {
            format!("https://ads.tiktok.com{href}")
        };
        if seen.insert(url.clone()) {
            links.push(url);
        }
    }

    links
}

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

    if list.is_empty() {
        None
    } else {
        Some(list.clone())
    }
}

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

fn parse_research_video(value: &serde_json::Value) -> Option<ResearchVideo> {
    let id = value_str(value, "id")?;
    let create_time = value_i64(value, "create_time")?;
    let music_id = value_str(value, "music_id")?;

    Some(ResearchVideo {
        id,
        create_time,
        region_code: value_str(value, "region_code").unwrap_or_else(|| "unknown".to_string()),
        video_description: value_str(value, "video_description").unwrap_or_default(),
        music_id,
        like_count: value_u64(value, "like_count"),
        comment_count: value_u64(value, "comment_count"),
        share_count: value_u64(value, "share_count"),
        view_count: value_u64(value, "view_count"),
        username: value_str(value, "username").unwrap_or_default(),
    })
}

fn parse_research_response(body: &serde_json::Value) -> Result<(Vec<ResearchVideo>, bool, i64)> {
    let data = body
        .get("data")
        .ok_or_else(|| anyhow::anyhow!("TikTok Research API response missing data field."))?;
    let videos = data
        .get("videos")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("TikTok Research API response missing videos array."))?;

    let has_more = data
        .get("has_more")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let cursor = data
        .get("cursor")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    let parsed = videos.iter().filter_map(parse_research_video).collect();
    Ok((parsed, has_more, cursor))
}

fn research_video_page(
    client: &reqwest::blocking::Client,
    token: &str,
    region: &str,
    window_days: u32,
    cursor: i64,
) -> Result<(Vec<ResearchVideo>, bool, i64)> {
    let (start_date, end_date) = utc_date_range(window_days)?;
    let body = json!({
        "query": {
            "and": [
                {
                    "operation": "EQ",
                    "field_name": "region_code",
                    "field_values": [region],
                }
            ]
        },
        "max_count": RESEARCH_PAGE_SIZE,
        "cursor": cursor,
        "start_date": start_date,
        "end_date": end_date,
    });

    let resp = client
        .post(RESEARCH_API_URL)
        .bearer_auth(token)
        .header("Content-Type", "application/json")
        .query(&[("fields", RESEARCH_FIELDS)])
        .json(&body)
        .send()
        .map_err(|e| anyhow::anyhow!("TikTok Research API request failed: {e}"))?;

    if resp.status() == reqwest::StatusCode::UNAUTHORIZED
        || resp.status() == reqwest::StatusCode::FORBIDDEN
    {
        bail!("TikTok Research API access token was rejected.");
    }

    if !resp.status().is_success() {
        bail!(
            "TikTok Research API returned status {}.",
            resp.status().as_u16()
        );
    }

    let body: serde_json::Value = resp
        .json()
        .map_err(|e| anyhow::anyhow!("TikTok Research API response could not be parsed: {e}"))?;

    if let Some(error) = body.get("error") {
        let code = error.get("code").and_then(|v| v.as_str()).unwrap_or("");
        if code != "ok" && !code.is_empty() {
            let message = error.get("message").and_then(|v| v.as_str()).unwrap_or("");
            bail!("TikTok Research API returned error {code}: {message}");
        }
    }

    parse_research_response(&body)
}

fn fetch_research_videos(limit: u32, region: &str, window_days: u32) -> Result<Vec<ResearchVideo>> {
    let token = configured_research_token().ok_or_else(|| {
        anyhow::anyhow!(
            "TikTok Research API access token not configured. Set TIKTOK_RESEARCH_ACCESS_TOKEN or TIKTOK_RESEARCH_CLIENT_ACCESS_TOKEN."
        )
    })?;
    let client = http_client()?;

    let mut cursor = 0;
    let mut collected = Vec::new();
    let target = RESEARCH_SAMPLE_CAP.max(limit.saturating_mul(40));

    loop {
        let (videos, has_more, next_cursor) =
            research_video_page(&client, &token, region, window_days, cursor)?;
        if videos.is_empty() {
            break;
        }

        collected.extend(videos);
        if collected.len() as u32 >= target || !has_more || next_cursor == cursor {
            break;
        }
        cursor = next_cursor;
    }

    Ok(collected)
}

fn parse_song_detail_html(html: &str, page_url: &str, region: &str, period: u32) -> Option<serde_json::Value> {
    let document = Html::parse_document(html);
    let payload = extract_detail_payload(&document);

    let title_selector = Selector::parse("title").ok()?;
    let page_title = document
        .select(&title_selector)
        .next()
        .map(|el| el.text().collect::<String>())
        .unwrap_or_default();

    let (fallback_title, fallback_artist) = parse_title_artist(&page_title)?;
    let title = payload
        .as_ref()
        .and_then(|p| p.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or(&fallback_title)
        .to_string();
    let artist = payload
        .as_ref()
        .and_then(|p| p.get("author"))
        .and_then(|v| v.as_str())
        .unwrap_or(&fallback_artist)
        .to_string();
    let tiktok_url = payload
        .as_ref()
        .and_then(|p| p.get("link"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| extract_view_more_link(&document));
    let cover_url = payload
        .as_ref()
        .and_then(|p| p.get("cover"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| extract_cover_url(&document));
    let duration_seconds = payload
        .as_ref()
        .and_then(|p| p.get("duration"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let (import_url, preview_audio_url) = build_import_url(payload.as_ref(), &tiktok_url);

    Some(json!({
        "rank": 0,
        "title": title,
        "artist": artist,
        "music_id": payload
            .as_ref()
            .and_then(|p| p.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        "tiktok_url": tiktok_url,
        "import_url": import_url,
        "preview_audio_url": preview_audio_url,
        "cover_url": cover_url,
        "duration_seconds": duration_seconds,
        "is_promoted": false,
        "analytics_url": page_url,
        "source_path": "tiktok_creative_center_song_page",
        "source_region": region,
        "source_period_days": period,
    }))
}

fn parse_candidate_from_raw(
    raw: &serde_json::Value,
    rank: usize,
    source_path: &str,
    _region: &str,
    _period: u32,
) -> TrendingSoundCandidate {
    let title = raw
        .get("title")
        .and_then(|v| v.as_str())
        .or_else(|| raw.get("musicName").and_then(|v| v.as_str()))
        .unwrap_or("Unknown")
        .to_string();
    let artist = raw
        .get("artist")
        .and_then(|v| v.as_str())
        .or_else(|| raw.get("author").and_then(|v| v.as_str()))
        .or_else(|| raw.get("artistName").and_then(|v| v.as_str()))
        .or_else(|| raw.get("creator").and_then(|c| c.get("nickname")).and_then(|v| v.as_str()))
        .unwrap_or("Unknown")
        .to_string();
    let music_id = raw
        .get("music_id")
        .and_then(|v| v.as_str())
        .or_else(|| raw.get("musicId").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string();
    let tiktok_url = raw
        .get("tiktok_url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let import_url = raw
        .get("import_url")
        .and_then(|v| v.as_str())
        .unwrap_or(&tiktok_url)
        .to_string();
    let import_hint = format!("capcut-cli library import \"{import_url}\" --type sound");
    let latest_video_create_time = raw
        .get("latest_video_create_time")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    TrendingSoundCandidate {
        rank: rank as u64,
        music_id,
        title,
        artist,
        tiktok_url,
        import_url: import_url.clone(),
        import_hint,
        source_path: source_path.to_string(),
        source_url: raw
            .get("source_url")
            .and_then(|v| v.as_str())
            .or_else(|| raw.get("analytics_url").and_then(|v| v.as_str()))
            .unwrap_or(&import_url)
            .to_string(),
        ranking_score: raw
            .get("ranking_score")
            .and_then(|v| v.as_f64())
            .unwrap_or_else(|| limit_rank_bonus(rank) as f64),
        video_count: raw.get("video_count").and_then(|v| v.as_u64()).unwrap_or(0),
        total_views: raw.get("total_views").and_then(|v| v.as_u64()).unwrap_or(0),
        total_likes: raw.get("total_likes").and_then(|v| v.as_u64()).unwrap_or(0),
        total_comments: raw.get("total_comments").and_then(|v| v.as_u64()).unwrap_or(0),
        total_shares: raw.get("total_shares").and_then(|v| v.as_u64()).unwrap_or(0),
        latest_video_create_time,
        enrichment_source: raw
            .get("enrichment_source")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    }
}

fn limit_rank_bonus(rank: usize) -> u64 {
    1000_u64.saturating_sub(rank as u64)
}

fn enrich_research_candidate(candidate: &mut TrendingSoundCandidate, region: &str, window_days: u32) {
    let slug = candidate.music_id.clone();
    let page_url = creative_center_song_url(&slug, region, window_days);

    let Ok(client) = http_client() else {
        return;
    };

    let Ok(resp) = client
        .get(&page_url)
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
    else {
        return;
    };

    if !resp.status().is_success() {
        return;
    }

    let Ok(html) = resp.text() else {
        return;
    };

    let Some(detail) = parse_song_detail_html(&html, &page_url, region, window_days) else {
        return;
    };

    if let Some(title) = detail.get("title").and_then(|v| v.as_str()) {
        candidate.title = title.to_string();
    }
    if let Some(artist) = detail.get("artist").and_then(|v| v.as_str()) {
        candidate.artist = artist.to_string();
    }
    if let Some(import_url) = detail.get("import_url").and_then(|v| v.as_str()) {
        candidate.import_url = import_url.to_string();
        candidate.import_hint = format!("capcut-cli library import \"{import_url}\" --type sound");
    }
    if let Some(tiktok_url) = detail.get("tiktok_url").and_then(|v| v.as_str()) {
        candidate.tiktok_url = tiktok_url.to_string();
        candidate.source_url = tiktok_url.to_string();
    } else {
        candidate.source_url = page_url.clone();
    }
    candidate.enrichment_source = Some("tiktok_creative_center_song_page".to_string());
}

fn candidate_from_research(
    agg: CandidateAggregate,
    rank: usize,
    region: &str,
    _window_days: u32,
) -> TrendingSoundCandidate {
    let latest = agg.latest_video.unwrap_or_else(|| ResearchVideo {
        id: String::new(),
        create_time: Utc::now().timestamp(),
        region_code: region.to_string(),
        video_description: String::new(),
        music_id: agg.music_id.clone(),
        like_count: 0,
        comment_count: 0,
        share_count: 0,
        view_count: 0,
        username: String::new(),
    });
    let tiktok_url = tiktok_music_url(&agg.music_id);
    let latest_video_create_time = Utc
        .timestamp_opt(latest.create_time, 0)
        .single()
        .unwrap_or_else(Utc::now)
        .to_rfc3339();

    TrendingSoundCandidate {
        rank: rank as u64,
        music_id: agg.music_id,
        title: format!("TikTok sound {}", latest.music_id),
        artist: "Unknown".to_string(),
        tiktok_url: tiktok_url.clone(),
        import_url: tiktok_url.clone(),
        import_hint: format!("capcut-cli library import \"{tiktok_url}\" --type sound"),
        source_path: "tiktok_research_api".to_string(),
        source_url: tiktok_url,
        ranking_score: (agg.score * 1000.0).round() / 1000.0,
        video_count: agg.video_count,
        total_views: agg.total_views,
        total_likes: agg.total_likes,
        total_comments: agg.total_comments,
        total_shares: agg.total_shares,
        latest_video_create_time,
        enrichment_source: None,
    }
}

fn research_candidates(limit: u32, region: &str, window_days: u32) -> Result<Vec<TrendingSoundCandidate>> {
    let videos = fetch_research_videos(limit, region, window_days)?;
    if videos.is_empty() {
        return Ok(vec![]);
    }

    let now_ts = Utc::now().timestamp();
    let mut buckets: HashMap<String, CandidateAggregate> = HashMap::new();
    for video in videos {
        let entry = buckets
            .entry(video.music_id.clone())
            .or_insert_with(|| CandidateAggregate::new(video.music_id.clone()));
        entry.add_video(video, now_ts, window_days);
    }

    let mut aggregates: Vec<_> = buckets
        .into_values()
        .map(|mut agg| {
            agg.finalize_score();
            agg
        })
        .collect();

    aggregates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.video_count.cmp(&a.video_count))
            .then_with(|| b.total_views.cmp(&a.total_views))
            .then_with(|| a.music_id.cmp(&b.music_id))
    });

    let mut candidates: Vec<_> = aggregates
        .into_iter()
        .take(limit as usize)
        .enumerate()
        .map(|(index, agg)| candidate_from_research(agg, index + 1, region, window_days))
        .collect();

    for candidate in &mut candidates {
        enrich_research_candidate(candidate, region, window_days);
    }

    Ok(candidates)
}

fn normalize_candidate_scores(candidates: &mut [TrendingSoundCandidate]) {
    if candidates.is_empty() {
        return;
    }

    let max_score = candidates
        .iter()
        .map(|candidate| candidate.ranking_score)
        .fold(f64::MIN, f64::max);
    if !max_score.is_finite() || max_score <= 0.0 {
        return;
    }

    for candidate in candidates {
        candidate.ranking_score = (candidate.ranking_score / max_score * 1000.0).round() / 1000.0;
    }
}

fn try_creative_center_api(limit: u32, region: &str, period: u32) -> Option<Vec<serde_json::Value>> {
    let client = http_client().ok()?;
    let period_s = period.to_string();
    let limit_s = limit.to_string();
    let resp = client
        .get(CREATIVE_CENTER_API)
        .header("Accept", "application/json")
        .query(&[
            ("period", period_s.as_str()),
            ("page", "1"),
            ("limit", limit_s.as_str()),
            ("country_code", region),
            ("sort_by", "popularity"),
        ])
        .send()
        .ok()?;

    debug_log(&format!("TikTok Creative Center API status: {}", resp.status()));
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

fn try_creative_center_detail_crawl(limit: u32, region: &str, period: u32) -> Option<Vec<serde_json::Value>> {
    let client = http_client().ok()?;
    let period_s = period.to_string();
    let overview_url = format!("{CREATIVE_CENTER_URL}?countryCode={region}&period={period_s}");
    let resp = client
        .get(&overview_url)
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .ok()?;

    debug_log(&format!("TikTok Creative Center overview status: {}", resp.status()));
    if !resp.status().is_success() {
        return None;
    }

    let html = resp.text().ok()?;
    debug_log(&format!("TikTok Creative Center overview HTML bytes: {}", html.len()));
    let document = Html::parse_document(&html);
    let links = extract_song_detail_links(&document);
    debug_log(&format!("TikTok song detail links found: {}", links.len()));
    if links.is_empty() {
        return None;
    }

    output::log("Source: Creative Center overview HTML (song detail crawl)");
    let mut songs = Vec::new();
    for url in links {
        let rank = songs.len() + 1;
        let Ok(resp) = client
            .get(&url)
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("Accept-Language", "en-US,en;q=0.9")
            .send()
        else {
            continue;
        };
        if !resp.status().is_success() {
            continue;
        }
        let Ok(html) = resp.text() else {
            continue;
        };
        if let Some(parsed) = parse_song_detail_html(&html, &url, region, period) {
            let mut parsed = parsed;
            if let Some(obj) = parsed.as_object_mut() {
                obj.insert("rank".to_string(), json!(rank));
            }
            songs.push(parsed);
        }
        if songs.len() >= limit as usize {
            break;
        }
    }

    if songs.is_empty() {
        None
    } else {
        Some(songs)
    }
}

fn try_creative_center_html(_region: &str, period: u32) -> Option<Vec<serde_json::Value>> {
    let client = http_client().ok()?;
    let period_s = period.to_string();
    let resp = client
        .get(CREATIVE_CENTER_URL)
        .query(&[("period", period_s.as_str())])
        .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .header("Accept-Language", "en-US,en;q=0.9")
        .send()
        .ok()?;

    debug_log(&format!("TikTok legacy HTML status: {}", resp.status()));
    if !resp.status().is_success() {
        return None;
    }

    let html = resp.text().ok()?;
    debug_log(&format!("TikTok legacy HTML bytes: {}", html.len()));
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

fn fallback_creative_center_sounds(limit: u32, region: &str, period: u32) -> Result<Vec<TrendingSoundCandidate>> {
    let raw_sounds = try_creative_center_api(limit, region, period)
        .or_else(|| try_creative_center_detail_crawl(limit, region, period))
        .or_else(|| try_creative_center_html(region, period))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Could not extract trending sounds from TikTok Creative Center. Both the JSON API and HTML extraction failed — the page structure may have changed."
            )
        })?;

    let mut candidates: Vec<_> = raw_sounds
        .iter()
        .take(limit as usize)
        .enumerate()
        .map(|(i, raw)| {
            let mut candidate = parse_candidate_from_raw(raw, i + 1, "tiktok_creative_center", region, period);
            if candidate.music_id.is_empty() {
                candidate.music_id = candidate
                    .import_url
                    .rsplit('/')
                    .next()
                    .unwrap_or("unknown")
                    .to_string();
            }
            candidate.source_url = raw
                .get("analytics_url")
                .and_then(|v| v.as_str())
                .unwrap_or(&candidate.import_url)
                .to_string();
            candidate
        })
        .collect();

    normalize_candidate_scores(&mut candidates);
    Ok(candidates)
}

fn candidates_to_json(
    candidates: Vec<TrendingSoundCandidate>,
    method: &str,
    region: &str,
    window_days: u32,
    recommended: bool,
) -> serde_json::Value {
    json!({
        "method": method,
        "recommended": recommended,
        "source_path": method,
        "region": region,
        "window_days": window_days,
        "total_found": candidates.len(),
        "sounds": candidates,
        "import_hint": "Use a candidate's import_url with: capcut-cli library import <url> --type sound",
    })
}

fn library_sound_score(asset: &crate::models::Asset) -> f64 {
    let recency = DateTime::parse_from_rfc3339(&asset.downloaded_at)
        .ok()
        .map(|dt| {
            let age_hours = (Utc::now() - dt.with_timezone(&Utc)).num_hours().max(0) as f64;
            (72.0 - age_hours).max(0.0)
        })
        .unwrap_or(0.0);
    let trending_bonus = if asset.tags.iter().any(|tag| {
        let lowered = tag.to_ascii_lowercase();
        lowered.contains("trend") || lowered.contains("tiktok")
    }) {
        100.0
    } else {
        0.0
    };
    recency + trending_bonus + asset.duration_seconds.min(60.0)
}

fn library_candidates(limit: u32, region: &str, window_days: u32) -> Result<Vec<TrendingSoundCandidate>> {
    let mut assets = library::list_assets(Some("sound"))?;
    if assets.is_empty() {
        bail!("No local sound assets are available in the library.");
    }

    assets.sort_by(|a, b| {
        library_sound_score(b)
            .partial_cmp(&library_sound_score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.downloaded_at.cmp(&a.downloaded_at))
    });

    let candidates = assets
        .into_iter()
        .take(limit as usize)
        .enumerate()
        .map(|(index, asset)| TrendingSoundCandidate {
            rank: (index + 1) as u64,
            music_id: asset.id.clone(),
            title: asset.title.clone(),
            artist: "Library".to_string(),
            tiktok_url: asset.source_url.clone(),
            import_url: asset.source_url.clone(),
            import_hint: format!("Reuse existing library sound asset: {}", asset.id),
            source_path: "library".to_string(),
            source_url: asset.source_url.clone(),
            ranking_score: (library_sound_score(&asset) * 1000.0).round() / 1000.0,
            video_count: 0,
            total_views: 0,
            total_likes: 0,
            total_comments: 0,
            total_shares: 0,
            latest_video_create_time: asset.downloaded_at.clone(),
            enrichment_source: Some("library_asset".to_string()),
        })
        .collect();

    let _ = region;
    let _ = window_days;
    Ok(candidates)
}

fn manual_url_candidates(
    limit: u32,
    region: &str,
    window_days: u32,
    manual_url: &str,
) -> Result<Vec<TrendingSoundCandidate>> {
    let info = downloader::get_info(manual_url).ok();
    let title = info
        .as_ref()
        .and_then(|v| v.get("title"))
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .unwrap_or("Manual sound URL")
        .to_string();
    let artist = info
        .as_ref()
        .and_then(|v| v.get("uploader"))
        .and_then(|v| v.as_str())
        .or_else(|| info.as_ref().and_then(|v| v.get("channel")).and_then(|v| v.as_str()))
        .unwrap_or("Manual")
        .to_string();

    let candidate = TrendingSoundCandidate {
        rank: 1,
        music_id: "manual-url".to_string(),
        title,
        artist,
        tiktok_url: manual_url.to_string(),
        import_url: manual_url.to_string(),
        import_hint: format!("capcut-cli library import \"{manual_url}\" --type sound"),
        source_path: "manual-url".to_string(),
        source_url: manual_url.to_string(),
        ranking_score: 1.0,
        video_count: 0,
        total_views: 0,
        total_likes: 0,
        total_comments: 0,
        total_shares: 0,
        latest_video_create_time: Utc::now().to_rfc3339(),
        enrichment_source: Some("manual_url".to_string()),
    };

    let _ = limit;
    let _ = region;
    let _ = window_days;
    Ok(vec![candidate])
}

/// Fetch trending sounds using an explicit strategy or `auto`.
pub fn find_trending_sounds_with_options(options: &SoundDiscoveryOptions) -> Result<serde_json::Value> {
    let limit = options.limit;
    let region = options.region.as_str();
    let window_days = options.window_days;
    output::log(&format!(
        "Fetching trending TikTok sounds (strategy={}, region={region}, window_days={window_days}, limit={limit})...",
        options.strategy.as_str()
    ));

    match options.strategy {
        SoundDiscoveryStrategy::Research => {
            let candidates = research_candidates(limit, region, window_days)?;
            if candidates.is_empty() {
                bail!("TikTok Research API returned no ranked sounds for the requested window.");
            }
            output::log("Source: TikTok Research API");
            return Ok(candidates_to_json(
                candidates,
                "tiktok_research_api",
                region,
                window_days,
                true,
            ));
        }
        SoundDiscoveryStrategy::CreativeCenter => {
            let candidates = fallback_creative_center_sounds(limit, region, window_days)?;
            return Ok(candidates_to_json(
                candidates,
                "tiktok_creative_center",
                region,
                window_days,
                false,
            ));
        }
        SoundDiscoveryStrategy::Library => {
            let candidates = library_candidates(limit, region, window_days)?;
            return Ok(candidates_to_json(candidates, "library", region, window_days, false));
        }
        SoundDiscoveryStrategy::ManualUrl => {
            let manual_url = options
                .manual_url
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| anyhow::anyhow!("manual-url strategy requires --sound-url."))?;
            let candidates = manual_url_candidates(limit, region, window_days, manual_url)?;
            return Ok(candidates_to_json(candidates, "manual-url", region, window_days, false));
        }
        SoundDiscoveryStrategy::Auto => {}
    }

    if options
        .manual_url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .is_some()
    {
        let manual_url = options.manual_url.as_deref().unwrap();
        let candidates = manual_url_candidates(limit, region, window_days, manual_url)?;
        return Ok(candidates_to_json(candidates, "manual-url", region, window_days, false));
    }

    if configured_research_token().is_some() {
        match research_candidates(limit, region, window_days) {
            Ok(candidates) if !candidates.is_empty() => {
                output::log("Source: TikTok Research API");
                return Ok(candidates_to_json(
                    candidates,
                    "tiktok_research_api",
                    region,
                    window_days,
                    true,
                ));
            }
            Ok(_) => {
                output::log("TikTok Research API returned no ranked sounds; falling back to Creative Center.");
            }
            Err(err) => {
                debug_log(&format!("TikTok Research API fallback path: {err}"));
                output::log("TikTok Research API discovery failed; falling back to Creative Center.");
            }
        }
    }

    match fallback_creative_center_sounds(limit, region, window_days) {
        Ok(candidates) => Ok(candidates_to_json(
            candidates,
            "tiktok_creative_center",
            region,
            window_days,
            false,
        )),
        Err(err) => {
            debug_log(&format!("Creative Center fallback path: {err}"));
            let candidates = library_candidates(limit, region, window_days)?;
            output::log("Creative Center discovery failed; falling back to local library sounds.");
            Ok(candidates_to_json(candidates, "library", region, window_days, false))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_research_fixture(name: &str) -> serde_json::Value {
        let json = match name {
            "research_response_page_1.json" => {
                include_str!("../../tests/fixtures/tiktok/research_response_page_1.json")
            }
            "research_response_page_2.json" => {
                include_str!("../../tests/fixtures/tiktok/research_response_page_2.json")
            }
            other => panic!("unknown research fixture: {other}"),
        };
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn test_music_url_format_uses_music_id() {
        assert_eq!(
            tiktok_music_url("7310129403294828545"),
            "https://www.tiktok.com/music/_-7310129403294828545"
        );
    }

    #[test]
    fn test_strategy_parse_accepts_aliases() {
        assert_eq!(
            SoundDiscoveryStrategy::parse("creative-center").unwrap(),
            SoundDiscoveryStrategy::CreativeCenter
        );
        assert_eq!(
            SoundDiscoveryStrategy::parse("manual").unwrap(),
            SoundDiscoveryStrategy::ManualUrl
        );
    }

    #[test]
    fn test_strategy_parse_rejects_unknown_values() {
        let error = SoundDiscoveryStrategy::parse("totally-unknown").unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Unknown TikTok sound discovery strategy")
        );
    }

    #[test]
    fn test_manual_url_candidates_use_manual_source() {
        let candidates = manual_url_candidates(5, "US", 7, "https://www.tiktok.com/music/_-123")
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].source_path, "manual-url");
        assert_eq!(candidates[0].import_url, "https://www.tiktok.com/music/_-123");
    }

    #[test]
    fn test_find_trending_sounds_with_options_manual_url_returns_manual_method() {
        let payload = find_trending_sounds_with_options(&SoundDiscoveryOptions {
            limit: 3,
            region: "US".to_string(),
            window_days: 7,
            strategy: SoundDiscoveryStrategy::ManualUrl,
            manual_url: Some("https://www.tiktok.com/music/_-123".to_string()),
        })
        .unwrap();

        assert_eq!(payload.get("method").and_then(|v| v.as_str()), Some("manual-url"));
        assert_eq!(payload.get("recommended").and_then(|v| v.as_bool()), Some(false));
    }

    #[test]
    fn test_library_candidates_return_existing_assets_when_available() {
        let candidates = library_candidates(10, "US", 7).unwrap();
        assert!(!candidates.is_empty());
        assert_eq!(candidates[0].source_path, "library");
    }

    #[test]
    fn test_ranking_prefers_frequent_recent_sound() {
        let now = 1_735_689_600;
        let videos = vec![
            ResearchVideo {
                id: "1".into(),
                create_time: now - 3600,
                region_code: "US".into(),
                video_description: "recent one".into(),
                music_id: "100".into(),
                like_count: 300,
                comment_count: 20,
                share_count: 15,
                view_count: 20_000,
                username: "a".into(),
            },
            ResearchVideo {
                id: "2".into(),
                create_time: now - 4200,
                region_code: "US".into(),
                video_description: "recent two".into(),
                music_id: "100".into(),
                like_count: 250,
                comment_count: 18,
                share_count: 11,
                view_count: 18_000,
                username: "b".into(),
            },
            ResearchVideo {
                id: "3".into(),
                create_time: now - 10 * 86_400,
                region_code: "US".into(),
                video_description: "older but loud".into(),
                music_id: "200".into(),
                like_count: 5_000,
                comment_count: 400,
                share_count: 300,
                view_count: 90_000,
                username: "c".into(),
            },
        ];

        let mut buckets: HashMap<String, CandidateAggregate> = HashMap::new();
        for video in videos {
            let entry = buckets
                .entry(video.music_id.clone())
                .or_insert_with(|| CandidateAggregate::new(video.music_id.clone()));
            entry.add_video(video, now, 7);
        }
        let mut aggregates: Vec<_> = buckets
            .into_values()
            .map(|mut agg| {
                agg.finalize_score();
                agg
            })
            .collect();
        aggregates.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        assert_eq!(aggregates.first().unwrap().music_id, "100");
    }

    #[test]
    fn test_parse_research_response_fixture() {
        let body = parse_research_fixture("research_response_page_1.json");
        let (videos, has_more, cursor) = parse_research_response(&body).unwrap();
        assert!(has_more);
        assert_eq!(cursor, 100);
        assert_eq!(videos.len(), 4);
        assert_eq!(videos[0].music_id, "7001");
    }

    #[test]
    fn test_research_ranking_fixture_orders_by_frequency() {
        let page_1 = parse_research_fixture("research_response_page_1.json");
        let page_2 = parse_research_fixture("research_response_page_2.json");

        let mut videos = Vec::new();
        videos.extend(parse_research_response(&page_1).unwrap().0);
        videos.extend(parse_research_response(&page_2).unwrap().0);

        let candidates = {
            let now_ts = Utc::now().timestamp();
            let mut buckets: HashMap<String, CandidateAggregate> = HashMap::new();
            for video in videos {
                let entry = buckets
                    .entry(video.music_id.clone())
                    .or_insert_with(|| CandidateAggregate::new(video.music_id.clone()));
                entry.add_video(video, now_ts, 7);
            }
            let mut aggregates: Vec<_> = buckets
                .into_values()
                .map(|mut agg| {
                    agg.finalize_score();
                    agg
                })
                .collect();
            aggregates.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            aggregates
        };

        assert_eq!(candidates.first().unwrap().music_id, "7001");
        assert!(candidates.first().unwrap().video_count >= 3);
    }

    #[test]
    fn test_parse_song_detail_fixture_extracts_import_data() {
        let html = include_str!("../../tests/fixtures/tiktok/creative_center_song_detail.html");
        let parsed = parse_song_detail_html(
            html,
            "https://ads.tiktok.com/business/creativecenter/song/example/pc/en?countryCode=US&period=7",
            "US",
            7,
        )
        .unwrap();

        assert_eq!(parsed.get("title").and_then(|v| v.as_str()), Some("Mặt Trời Đã Khuất"));
        assert_eq!(parsed.get("artist").and_then(|v| v.as_str()), Some("VORTEXX BAND"));
        assert!(parsed
            .get("import_url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .contains("tiktok.com"));
    }

    #[test]
    fn test_extract_song_links_from_overview_fixture() {
        let html = include_str!("../../tests/fixtures/tiktok/creative_center_overview.html");
        let document = Html::parse_document(html);
        let links = extract_song_detail_links(&document);
        assert_eq!(links.len(), 2);
        assert!(links[0].contains("/business/creativecenter/song/"));
    }
}
