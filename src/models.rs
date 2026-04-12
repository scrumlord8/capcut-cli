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
