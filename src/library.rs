use anyhow::{Result, bail};
use chrono::Utc;


use crate::config::{clips_dir, manifest_path, sounds_dir};
use crate::media::downloader;
use crate::models::{Asset, Manifest};
use crate::output;

fn gen_id(asset_type: &str) -> String {
    let prefix = if asset_type == "sound" { "snd" } else { "clp" };
    let hex = &uuid::Uuid::new_v4().to_string().replace('-', "")[..8];
    format!("{prefix}_{hex}")
}

fn read_manifest() -> Manifest {
    let path = manifest_path();
    if path.exists() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(m) = serde_json::from_str::<Manifest>(&data) {
                return m;
            }
        }
    }
    Manifest::default()
}

fn write_manifest(manifest: &Manifest) -> Result<()> {
    let path = manifest_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(manifest)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Download and import an asset from a URL.
pub fn import_asset(url: &str, asset_type: Option<&str>, tags: &[String]) -> Result<Asset> {
    let platform = downloader::detect_platform(url);
    let atype = downloader::detect_asset_type(url, asset_type);
    let asset_id = gen_id(atype);

    output::log(&format!("Importing {atype} from {platform}: {url}"));

    // Create asset directory
    let asset_dir = if atype == "sound" {
        sounds_dir().join(&asset_id)
    } else {
        clips_dir().join(&asset_id)
    };
    std::fs::create_dir_all(&asset_dir)?;

    // Get metadata
    output::log("Extracting metadata...");
    let (title, meta_duration) = match downloader::get_info(url) {
        Ok(info) => (
            info.get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled")
                .to_string(),
            info.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0),
        ),
        Err(_) => ("Untitled".to_string(), 0.0),
    };

    // Download
    output::log(&format!("Downloading {atype}..."));
    let file_path = if atype == "sound" {
        downloader::download_sound(url, &asset_dir)?
    } else {
        downloader::download_clip(url, &asset_dir)?
    };

    // Get file info
    let file_size = std::fs::metadata(&file_path)?.len();
    let duration = {
        let d = crate::media::ffmpeg::get_duration(&file_path.to_string_lossy()).unwrap_or(0.0);
        if d > 0.0 { d } else { meta_duration }
    };

    let format = file_path
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();

    let asset = Asset {
        id: asset_id.clone(),
        asset_type: atype.to_string(),
        title: title.clone(),
        source_url: url.to_string(),
        source_platform: platform.to_string(),
        downloaded_at: Utc::now().to_rfc3339(),
        duration_seconds: (duration * 100.0).round() / 100.0,
        file_path: file_path
            .canonicalize()
            .unwrap_or(file_path.clone())
            .to_string_lossy()
            .to_string(),
        file_size_bytes: file_size,
        format,
        tags: tags.to_vec(),
    };

    // Save meta.json
    let meta_path = asset_dir.join("meta.json");
    std::fs::write(&meta_path, serde_json::to_string_pretty(&asset)?)?;

    // Update manifest
    let mut manifest = read_manifest();
    manifest.assets.push(serde_json::to_value(&asset)?);
    write_manifest(&manifest)?;

    output::log(&format!("Imported: {asset_id} ({title})"));
    Ok(asset)
}

/// List all assets, optionally filtered by type.
pub fn list_assets(asset_type: Option<&str>) -> Result<Vec<Asset>> {
    let manifest = read_manifest();
    let mut assets = Vec::new();
    for entry in &manifest.assets {
        if let Some(filter) = asset_type {
            if entry.get("type").and_then(|v| v.as_str()) != Some(filter) {
                continue;
            }
        }
        if let Ok(asset) = serde_json::from_value::<Asset>(entry.clone()) {
            assets.push(asset);
        }
    }
    Ok(assets)
}

/// Get a specific asset by ID.
pub fn get_asset(asset_id: &str) -> Result<Option<Asset>> {
    let manifest = read_manifest();
    for entry in &manifest.assets {
        if entry.get("id").and_then(|v| v.as_str()) == Some(asset_id) {
            let asset = serde_json::from_value::<Asset>(entry.clone())?;
            return Ok(Some(asset));
        }
    }
    Ok(None)
}

/// Delete an asset from the library.
pub fn delete_asset(asset_id: &str) -> Result<()> {
    let mut manifest = read_manifest();
    let mut found = false;
    let mut new_assets = Vec::new();

    for entry in &manifest.assets {
        if entry.get("id").and_then(|v| v.as_str()) == Some(asset_id) {
            found = true;
            // Remove asset directory
            if let Some(fp) = entry.get("file_path").and_then(|v| v.as_str()) {
                let path = std::path::Path::new(fp);
                if let Some(parent) = path.parent() {
                    if parent.exists() {
                        let _ = std::fs::remove_dir_all(parent);
                    }
                }
            }
        } else {
            new_assets.push(entry.clone());
        }
    }

    if !found {
        bail!("Asset '{asset_id}' not found.");
    }

    manifest.assets = new_assets;
    write_manifest(&manifest)?;
    Ok(())
}
