use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use rustmotion::engine::preload::{
    video_frame_byte_size, would_exceed_cache_budget, VIDEO_FRAME_CACHE_BUDGET_BYTES,
};

#[test]
fn frame_byte_size_matches_plain_multiplication_for_ordinary_dimensions() {
    assert_eq!(video_frame_byte_size(1920, 1080), 1920u64 * 1080 * 4);
    assert_eq!(video_frame_byte_size(0, 0), 0);
}

#[test]
fn frame_byte_size_saturates_instead_of_wrapping_on_extreme_dimensions() {
    let huge = video_frame_byte_size(u32::MAX, u32::MAX);
    assert_eq!(
        huge,
        u64::MAX,
        "must saturate at u64::MAX, never wrap silently to a small number"
    );
    assert!(
        would_exceed_cache_budget(0, huge),
        "a saturated size must always fail the budget check"
    );
}

#[test]
fn would_exceed_cache_budget_rejects_only_once_the_sum_crosses_the_ceiling() {
    assert!(!would_exceed_cache_budget(
        0,
        VIDEO_FRAME_CACHE_BUDGET_BYTES
    ));
    assert!(would_exceed_cache_budget(
        0,
        VIDEO_FRAME_CACHE_BUDGET_BYTES + 1
    ));
    assert!(would_exceed_cache_budget(VIDEO_FRAME_CACHE_BUDGET_BYTES, 1));
    assert!(
        would_exceed_cache_budget(u64::MAX, 1),
        "saturating add must not wrap past the ceiling"
    );
}

struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(label: &str) -> Self {
        let unique = format!(
            "rustmotion-audit-ws-d-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system clock before UNIX epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).expect("create scratch dir");
        Self(path)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn write_file(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).expect("write scratch file");
    path
}

fn run_batch(file: &Path, data: &Path, output_dir: &Path, name_template: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rustmotion"))
        .arg("--quiet")
        .arg("batch")
        .arg("--file")
        .arg(file)
        .arg("--data")
        .arg(data)
        .arg("--output-dir")
        .arg(output_dir)
        .arg("--name-template")
        .arg(name_template)
        .arg("--format")
        .arg("png-seq")
        .output()
        .expect("failed to spawn `rustmotion batch`")
}

#[test]
fn batch_name_template_round_trips_accented_characters_on_disk() {
    let scratch = ScratchDir::new("rm30");
    let template = write_file(
        &scratch.0,
        "template.json",
        &serde_json::json!({
            "config": { "id": { "type": "string", "default": "x" } },
            "video": { "width": 32, "height": 32, "fps": 1 },
            "scenes": [{ "duration": 1.0, "children": [] }]
        })
        .to_string(),
    );
    let data = write_file(
        &scratch.0,
        "data.jsonl",
        &serde_json::json!({"id": "abc"}).to_string(),
    );
    let output_dir = scratch.0.join("out");
    std::fs::create_dir_all(&output_dir).expect("create output dir");

    let result = run_batch(&template, &data, &output_dir, "résumé-{id}.png");

    assert!(
        result.status.success(),
        "batch must succeed: stdout={}\nstderr={}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );

    let expected = output_dir.join("résumé-abc.png");
    assert!(
        expected.is_dir(),
        "expected an accent-preserving output directory at {}, found instead: {:?}",
        expected.display(),
        std::fs::read_dir(&output_dir)
            .map(|entries| entries
                .filter_map(|e| e.ok().map(|e| e.file_name()))
                .collect::<Vec<_>>())
            .unwrap_or_default()
    );
    assert!(expected.join("frame_00000.png").exists());
}

#[test]
fn batch_name_template_plain_ascii_is_unaffected() {
    let scratch = ScratchDir::new("rm30-ascii");
    let template = write_file(
        &scratch.0,
        "template.json",
        &serde_json::json!({
            "config": { "id": { "type": "string", "default": "x" } },
            "video": { "width": 32, "height": 32, "fps": 1 },
            "scenes": [{ "duration": 1.0, "children": [] }]
        })
        .to_string(),
    );
    let data = write_file(
        &scratch.0,
        "data.jsonl",
        &serde_json::json!({"id": "abc"}).to_string(),
    );
    let output_dir = scratch.0.join("out");
    std::fs::create_dir_all(&output_dir).expect("create output dir");

    let result = run_batch(&template, &data, &output_dir, "clip-{id}.png");
    assert!(result.status.success());
    assert!(output_dir.join("clip-abc.png").is_dir());
}
