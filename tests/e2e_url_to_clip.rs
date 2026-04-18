//! End-to-end smoke test for the manual-URL spine: library import → compose.
//!
//! Proves that given an external URL, the CLI downloads (via a yt-dlp shim for
//! test isolation), registers the asset, and composes a real MP4. No network
//! required; the shim copies committed fixture media to yt-dlp's expected
//! output template, so the rest of the pipeline (metadata extraction via
//! ffprobe, loudness normalization via ffmpeg, compose) runs against real
//! bytes.
//!
//! Exercised because the product's honest minimum viable truth is
//! "fresh input in, finished clip out."

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn write_ytdlp_shim(workdir: &std::path::Path) -> PathBuf {
    let shim = workdir.join("ytdlp-shim.sh");
    let fixture_audio = repo_root().join("library/sounds/assets/snd_demo001/audio.mp3");
    let fixture_video = repo_root().join("library/clips/clp_demo001/video.mp4");

    // Shim behavior:
    //   --dump-json --no-download → emit a minimal JSON metadata blob
    //   -o <template>             → copy the matching fixture to the template's dir
    let script = format!(
        r#"#!/usr/bin/env bash
# Minimal yt-dlp stand-in for the e2e import test.
set -euo pipefail

for arg in "$@"; do
  if [[ "$arg" == "--dump-json" ]]; then
    printf '{{"title":"shim demo","duration":2.0,"id":"shim"}}\n'
    exit 0
  fi
done

out_template=""
prev=""
for arg in "$@"; do
  if [[ "$prev" == "-o" ]]; then
    out_template="$arg"
    break
  fi
  prev="$arg"
done

[[ -n "$out_template" ]] || {{ echo "shim: missing -o template" >&2; exit 2; }}
out_dir=$(dirname "$out_template")
mkdir -p "$out_dir"

case "$out_template" in
  *raw_audio*) cp '{audio}' "$out_dir/raw_audio.mp3" ;;
  *video*)     cp '{video}' "$out_dir/video.mp4" ;;
  *)           echo "shim: unexpected template $out_template" >&2; exit 3 ;;
esac
"#,
        audio = fixture_audio.display(),
        video = fixture_video.display(),
    );

    std::fs::write(&shim, script).unwrap();
    let perms = std::fs::Permissions::from_mode(0o755);
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&shim, perms).unwrap();
    shim
}

fn cli_bin() -> PathBuf {
    // Locate the built binary next to the test executable.
    let mut p = std::env::current_exe().unwrap();
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    p.push("capcut-cli");
    p
}

/// Run the CLI end-to-end: import a fake sound URL, import a fake clip URL,
/// compose them, and assert the output MP4 exists.
#[test]
fn url_in_finished_clip_out() {
    // Build a scratch workspace so we never touch the committed library/.
    let work = std::env::temp_dir().join(format!(
        "capcut-e2e-{}",
        uuid::Uuid::new_v4().simple()
    ));
    std::fs::create_dir_all(&work).unwrap();

    let shim = write_ytdlp_shim(&work);
    let bin = cli_bin();
    assert!(bin.exists(), "capcut-cli binary not found at {bin:?}");

    let sound_out = Command::new(&bin)
        .current_dir(&work)
        .env("CAPCUT_YTDLP_PATH", &shim)
        .args([
            "library",
            "import",
            "https://example.com/sound.mp3",
            "--type",
            "sound",
        ])
        .output()
        .expect("failed to invoke capcut-cli for sound import");
    assert!(
        sound_out.status.success(),
        "sound import failed: {}",
        String::from_utf8_lossy(&sound_out.stderr)
    );
    let sound_json: serde_json::Value =
        serde_json::from_slice(&sound_out.stdout).expect("sound import stdout is not JSON");
    let sound_id = sound_json["data"]["id"].as_str().expect("sound id").to_string();

    let clip_out = Command::new(&bin)
        .current_dir(&work)
        .env("CAPCUT_YTDLP_PATH", &shim)
        .args([
            "library",
            "import",
            "https://example.com/clip.mp4",
            "--type",
            "clip",
        ])
        .output()
        .expect("failed to invoke capcut-cli for clip import");
    assert!(
        clip_out.status.success(),
        "clip import failed: {}",
        String::from_utf8_lossy(&clip_out.stderr)
    );
    let clip_json: serde_json::Value =
        serde_json::from_slice(&clip_out.stdout).expect("clip import stdout is not JSON");
    let clip_id = clip_json["data"]["id"].as_str().expect("clip id").to_string();

    let final_mp4 = work.join("final.mp4");
    let compose_out = Command::new(&bin)
        .current_dir(&work)
        .args([
            "compose",
            "--sound",
            &sound_id,
            "--clip",
            &clip_id,
            "--duration",
            "1",
            "--resolution",
            "540x960",
            "--output",
            final_mp4.to_str().unwrap(),
        ])
        .output()
        .expect("failed to invoke capcut-cli for compose");
    assert!(
        compose_out.status.success(),
        "compose failed: {}",
        String::from_utf8_lossy(&compose_out.stderr)
    );

    assert!(
        final_mp4.exists(),
        "compose did not produce {}",
        final_mp4.display()
    );
    let size = std::fs::metadata(&final_mp4).unwrap().len();
    assert!(size > 0, "composed MP4 is empty");

    let _ = std::fs::remove_dir_all(&work);
}
