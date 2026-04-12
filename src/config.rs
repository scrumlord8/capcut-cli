use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::LazyLock;

pub const VERSION: &str = "0.1.0";

/// Get the repository root (parent of the binary's directory, or CWD).
pub fn repo_root() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn library_dir() -> PathBuf {
    repo_root().join("library")
}
pub fn sounds_dir() -> PathBuf {
    library_dir().join("sounds").join("assets")
}
pub fn clips_dir() -> PathBuf {
    library_dir().join("clips")
}
pub fn output_dir() -> PathBuf {
    library_dir().join("output")
}
pub fn tmp_dir() -> PathBuf {
    library_dir().join(".tmp")
}
pub fn manifest_path() -> PathBuf {
    library_dir().join("manifest.json")
}

pub fn capcut_home() -> PathBuf {
    dirs_home().join(".capcut-cli")
}
pub fn bin_dir() -> PathBuf {
    capcut_home().join("bin")
}
pub fn ytdlp_path() -> PathBuf {
    bin_dir().join("yt-dlp")
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// Create all required directories.
pub fn ensure_dirs() {
    for d in &[sounds_dir(), clips_dir(), output_dir(), tmp_dir(), bin_dir()] {
        let _ = std::fs::create_dir_all(d);
    }
}

// ── Loudness presets ────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct LoudnessPreset {
    pub lufs: f64,
    pub tp: f64,
    pub lra: f64,
    pub label: &'static str,
}

pub const DEFAULT_LOUDNESS: &str = "viral";

pub static LOUDNESS_PRESETS: LazyLock<HashMap<&'static str, LoudnessPreset>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    m.insert("viral", LoudnessPreset {
        lufs: -8.0, tp: -1.0, lra: 7.0,
        label: "Social/viral — loud, punchy, cuts through feed scroll",
    });
    m.insert("social", LoudnessPreset {
        lufs: -10.0, tp: -1.0, lra: 9.0,
        label: "General social media",
    });
    m.insert("podcast", LoudnessPreset {
        lufs: -14.0, tp: -1.5, lra: 11.0,
        label: "Podcast / spoken word (Apple, Spotify spec)",
    });
    m.insert("broadcast", LoudnessPreset {
        lufs: -23.0, tp: -1.0, lra: 15.0,
        label: "EBU R128 broadcast standard",
    });
    m
});
