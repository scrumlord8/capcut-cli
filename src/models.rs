use serde::{Deserialize, Serialize};

/// An imported asset (sound or clip) in the library.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub id: String,
    #[serde(rename = "type")]
    pub asset_type: String,
    pub title: String,
    pub source_url: String,
    pub source_platform: String,
    pub downloaded_at: String,
    pub duration_seconds: f64,
    pub file_path: String,
    pub file_size_bytes: u64,
    pub format: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Result of the compose pipeline.
#[derive(Debug, Serialize)]
pub struct ComposeResult {
    pub output_path: String,
    pub duration_seconds: f64,
    pub file_size_bytes: u64,
    pub sound_id: String,
    pub clip_ids: Vec<String>,
    pub resolution: String,
}

/// JSON manifest for the library.
#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub assets: Vec<serde_json::Value>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: 1,
            assets: vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_asset() -> Asset {
        Asset {
            id: "snd_abc12345".to_string(),
            asset_type: "sound".to_string(),
            title: "Test Song".to_string(),
            source_url: "https://youtube.com/watch?v=test".to_string(),
            source_platform: "youtube".to_string(),
            downloaded_at: "2026-04-12T00:00:00Z".to_string(),
            duration_seconds: 120.5,
            file_path: "/tmp/test/audio.mp3".to_string(),
            file_size_bytes: 4096,
            format: "mp3".to_string(),
            tags: vec!["trending".to_string(), "hyperpop".to_string()],
        }
    }

    #[test]
    fn asset_serializes_type_field_as_type() {
        let asset = sample_asset();
        let json = serde_json::to_value(&asset).unwrap();
        // asset_type field should serialize as "type" due to #[serde(rename)]
        assert_eq!(json["type"], "sound");
        assert!(json.get("asset_type").is_none());
    }

    #[test]
    fn asset_roundtrip_json() {
        let asset = sample_asset();
        let json = serde_json::to_string(&asset).unwrap();
        let restored: Asset = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.id, asset.id);
        assert_eq!(restored.asset_type, asset.asset_type);
        assert_eq!(restored.title, asset.title);
        assert_eq!(restored.duration_seconds, asset.duration_seconds);
        assert_eq!(restored.tags, asset.tags);
    }

    #[test]
    fn asset_deserializes_with_empty_tags_default() {
        let json = r#"{
            "id": "clp_00000000",
            "type": "clip",
            "title": "No Tags",
            "source_url": "https://example.com",
            "source_platform": "unknown",
            "downloaded_at": "2026-01-01T00:00:00Z",
            "duration_seconds": 10.0,
            "file_path": "/tmp/clip.mp4",
            "file_size_bytes": 1024,
            "format": "mp4"
        }"#;
        let asset: Asset = serde_json::from_str(json).unwrap();
        assert!(asset.tags.is_empty());
    }

    #[test]
    fn compose_result_serializes() {
        let result = ComposeResult {
            output_path: "/tmp/output/final.mp4".to_string(),
            duration_seconds: 30.0,
            file_size_bytes: 1048576,
            sound_id: "snd_abc12345".to_string(),
            clip_ids: vec!["clp_def67890".to_string()],
            resolution: "1080x1920".to_string(),
        };
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(json["output_path"], "/tmp/output/final.mp4");
        assert_eq!(json["duration_seconds"], 30.0);
        assert_eq!(json["sound_id"], "snd_abc12345");
        assert_eq!(json["clip_ids"][0], "clp_def67890");
        assert_eq!(json["resolution"], "1080x1920");
    }

    #[test]
    fn manifest_default_is_version_1_empty() {
        let m = Manifest::default();
        assert_eq!(m.version, 1);
        assert!(m.assets.is_empty());
    }

    #[test]
    fn manifest_roundtrip_with_assets() {
        let asset = sample_asset();
        let mut m = Manifest::default();
        m.assets.push(serde_json::to_value(&asset).unwrap());

        let json = serde_json::to_string(&m).unwrap();
        let restored: Manifest = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.version, 1);
        assert_eq!(restored.assets.len(), 1);
        assert_eq!(restored.assets[0]["id"], "snd_abc12345");
    }
}
