/// Extract audio from embedded `video` components and synthesise `AudioTrack`
/// entries that can be appended to `scenario.audio` before the existing mixer.
///
/// # Limitations (v1)
/// - **World views**: skipped — their camera timeline makes scene offsets
///   non-trivial. A warning is printed to stderr.
/// - **loop_video**: audio does not loop. The audio track plays once from
///   `trim_start` to `trim_end` (or natural end of file), regardless of
///   whether `loop_video` is set. Document this limitation to users.
/// - Temporary WAV files are left in `std::env::temp_dir()` keyed by a hash
///   of `(src, trim_start, trim_end, playback_rate)`.  A re-render with the
///   same parameters reuses the cached file.
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;

use crate::components::{ChildComponent, Component};
use crate::schema::{AudioTrack, ResolvedScenario, ViewType};

// ─── Scene offset computation ─────────────────────────────────────────────────

/// Compute the absolute start-time offset (in seconds) of each scene inside
/// each view, mirroring the frame-emission order of `build_frame_tasks`.
///
/// Return value: `offsets[view_idx][scene_idx] = start_time_in_output_video`.
///
/// Rules (same as `build_frame_tasks`):
/// - Between views: an optional inter-view `transition` occupies
///   `transition.duration` seconds **before** the first frame of the next view.
///   Those transition frames are emitted *between* views in output order, so
///   view `N` starts at `cursor + transition.duration`.
/// - Within a slide view: scenes are consecutive but their transitions
///   **overlap** with the preceding scene's tail. Scene `i+1` starts at
///   `scene_i_end - transition_duration` where the transition duration is
///   `scenes[i+1].transition.duration` (the *incoming* transition of i+1).
/// - World views: all scenes share the same view window. We record `cursor`
///   as the start of that window for all scenes and advance by the total
///   world-view duration.
///
/// # Panics
/// Never panics — uses saturating arithmetic.
pub fn scene_start_offsets(scenario: &ResolvedScenario) -> Vec<Vec<f64>> {
    let fps = scenario.video.fps as f64;
    let mut result: Vec<Vec<f64>> = Vec::with_capacity(scenario.views.len());
    let mut cursor = 0.0_f64;

    for (view_idx, view) in scenario.views.iter().enumerate() {
        // ── Inter-view transition (slides in *before* this view's first frame)
        if view_idx > 0 {
            if let Some(ref vt) = view.transition {
                cursor += vt.duration;
            }
        }

        match view.view_type {
            ViewType::Slide => {
                let mut scene_offsets: Vec<f64> = Vec::with_capacity(view.scenes.len());
                let mut scene_cursor = cursor;

                for (i, scene) in view.scenes.iter().enumerate() {
                    // Incoming transition of *this* scene overlaps with the
                    // previous scene's tail: the overlap was already "paid for"
                    // by the previous scene, so we move *backwards* by it.
                    if i > 0 {
                        let incoming = scene.transition.as_ref().map(|t| t.duration).unwrap_or(0.0);
                        scene_cursor -= incoming;
                    }
                    scene_offsets.push(scene_cursor);

                    // Advance by scene duration (in whole-frame units to stay
                    // consistent with frame-task rounding).
                    let scene_frames = (scene.duration * fps).round() / fps;
                    scene_cursor += scene_frames;
                }

                // Advance the global cursor to the end of this slide view's
                // last scene.
                cursor = scene_cursor;
                result.push(scene_offsets);
            }

            ViewType::World => {
                eprintln!(
                    "rustmotion: embedded-video audio: view {} is a World view — \
                     audio extraction from embedded video components in world views \
                     is not supported in v1. Skipping.",
                    view_idx
                );

                // All scenes share the view-level window start.
                let scene_offsets = vec![cursor; view.scenes.len()];

                // Compute world-view total duration to advance cursor.
                let world_duration: f64 = view
                    .scenes
                    .iter()
                    .map(|s| (s.duration * fps).round() / fps)
                    .sum();
                cursor += world_duration;

                result.push(scene_offsets);
            }
        }
    }

    result
}

// ─── Component walk ───────────────────────────────────────────────────────────

/// Collected metadata for a single video component found in the scene tree.
#[derive(Debug)]
struct VideoOccurrence {
    src: String,
    trim_start: f64,
    trim_end: Option<f64>,
    playback_rate: f64,
    volume: f32,
    /// start_at from the component's TimingConfig (0 if absent)
    start_at: f64,
    /// end_at from the component's TimingConfig
    end_at: Option<f64>,
}

fn collect_videos_in_child(child: &ChildComponent, out: &mut Vec<VideoOccurrence>) {
    match &child.component {
        Component::Video(v) => {
            if v.volume > 0.0 {
                out.push(VideoOccurrence {
                    src: v.src.clone(),
                    trim_start: v.trim_start.unwrap_or(0.0),
                    trim_end: v.trim_end,
                    playback_rate: v.playback_rate.unwrap_or(1.0),
                    volume: v.volume,
                    start_at: v.timing.start_at.unwrap_or(0.0),
                    end_at: v.timing.end_at,
                });
            }
        }
        Component::Card(c) => {
            for ch in &c.children {
                collect_videos_in_child(ch, out);
            }
        }
        Component::Flex(c) => {
            for ch in &c.children {
                collect_videos_in_child(ch, out);
            }
        }
        Component::Grid(c) => {
            for ch in &c.children {
                collect_videos_in_child(ch, out);
            }
        }
        Component::Positioned(c) => {
            for ch in &c.children {
                collect_videos_in_child(ch, out);
            }
        }
        Component::Container(c) => {
            for ch in &c.children {
                collect_videos_in_child(ch, out);
            }
        }
        _ => {}
    }
}

fn collect_videos_in_scene(scene: &crate::schema::Scene, out: &mut Vec<VideoOccurrence>) {
    let children: Vec<ChildComponent> = scene
        .children
        .iter()
        .filter_map(|v| serde_json::from_value(v.clone()).ok())
        .collect();
    for child in &children {
        collect_videos_in_child(child, out);
    }
}

// ─── ffmpeg probe ─────────────────────────────────────────────────────────────

/// Returns `true` if `ffmpeg` is available on PATH.
fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .args(["-version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ─── atempo filter chain ──────────────────────────────────────────────────────

/// Build an `atempo` filter-graph string for the given playback rate.
///
/// `atempo` only accepts values in `[0.5, 2.0]`.  For factors outside that
/// range we chain multiple `atempo` filters:
///   rate 4.0 → `atempo=2.0,atempo=2.0`
///   rate 0.1 → `atempo=0.5,atempo=0.2`  (0.5 * 0.2 = 0.1)
///
/// Returns `None` if rate == 1.0 (no filter needed).
pub fn build_atempo_filter(rate: f64) -> Option<String> {
    const EPSILON: f64 = 1e-9;
    if (rate - 1.0).abs() < EPSILON {
        return None;
    }

    let mut parts: Vec<String> = Vec::new();
    let mut remaining = rate;

    if rate > 1.0 {
        // Each stage multiplies by at most 2.0
        while remaining > 2.0 + EPSILON {
            parts.push("atempo=2.0".to_string());
            remaining /= 2.0;
        }
        parts.push(format!("atempo={:.6}", remaining));
    } else {
        // Each stage multiplies by at least 0.5
        while remaining < 0.5 - EPSILON {
            parts.push("atempo=0.5".to_string());
            remaining /= 0.5;
        }
        parts.push(format!("atempo={:.6}", remaining));
    }

    Some(parts.join(","))
}

// ─── Cache-keyed temp WAV path ────────────────────────────────────────────────

fn wav_cache_path(src: &str, trim_start: f64, trim_end: Option<f64>, rate: f64) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    src.hash(&mut hasher);
    trim_start.to_bits().hash(&mut hasher);
    trim_end.map(|v| v.to_bits()).hash(&mut hasher);
    rate.to_bits().hash(&mut hasher);

    // Constat #10: the cache key used to depend only on
    // (src, trim_start, trim_end, rate) — editing `src` in place (same
    // path, new bytes) left the old extraction cached under the same key
    // forever, silently serving stale audio. Folding in the source's size
    // and mtime means a modified file gets a different cache path
    // automatically. Best-effort: if `metadata` fails (source vanished
    // between validation and extraction), the hash simply falls back to the
    // path-only key, matching the previous behavior exactly.
    if let Ok(meta) = std::fs::metadata(src) {
        meta.len().hash(&mut hasher);
        if let Ok(modified) = meta.modified() {
            if let Ok(dur) = modified.duration_since(std::time::UNIX_EPOCH) {
                dur.as_nanos().hash(&mut hasher);
            }
        }
    }

    let hash = hasher.finish();
    std::env::temp_dir().join(format!("rustmotion_vidaud_{:016x}.wav", hash))
}

/// Scratch path ffmpeg writes to before a successful extraction is promoted
/// (renamed) onto `wav_path`.
///
/// Deviates from the audit's literal suggestion of a `<hash>.wav.partial`
/// suffix: `Path::with_extension` on a path already ending in `.wav`
/// replaces the extension rather than appending, so `<hash>.wav.partial`
/// really means "last extension is `.partial`" — and ffmpeg picks its output
/// muxer from the *last* extension. Pointing it at a `.partial`-suffixed
/// path makes it fail with "Unable to choose an output format", which
/// looked identical to the transient-failure case this fix exists to guard
/// against until traced back to this naming choice. Keeping `.wav` as the
/// final extension (`<hash>.partial.wav`) keeps ffmpeg's format
/// autodetection working while still being unambiguously distinct from the
/// real cache path.
fn partial_wav_path(wav_path: &std::path::Path) -> PathBuf {
    let stem = wav_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio");
    wav_path.with_file_name(format!("{stem}.partial.wav"))
}

// ─── Audio extraction ─────────────────────────────────────────────────────────

/// Extract audio from a video file into a WAV using ffmpeg.
///
/// Returns the path of the temporary WAV file, or `None` if ffmpeg is absent
/// or the extraction fails (warning is printed to stderr in both cases).
fn extract_audio_to_wav(
    src: &str,
    trim_start: f64,
    trim_end: Option<f64>,
    rate: f64,
) -> Option<PathBuf> {
    let wav_path = wav_cache_path(src, trim_start, trim_end, rate);

    // Reuse cached extraction.
    if wav_path.exists() {
        return Some(wav_path);
    }

    // Constat #10: ffmpeg used to write straight to `wav_path`. An
    // interrupted extraction (Ctrl-C, disk full, the source still being
    // written) left a truncated file sitting exactly at the path
    // `wav_path.exists()` treats as a valid cache hit above — every
    // subsequent render silently reused the corrupt WAV, with no error and
    // no way to detect it short of manually clearing `temp_dir()`. Writing
    // to a scratch sibling and renaming onto `wav_path` only after ffmpeg
    // reports success means a failed extraction can never become a false
    // cache hit.
    let partial_path = partial_wav_path(&wav_path);

    let mut args: Vec<String> = Vec::new();

    // Input seek (trim_start)
    if trim_start > 0.0 {
        args.push("-ss".to_string());
        args.push(format!("{:.6}", trim_start));
    }

    if let Some(end) = trim_end {
        args.push("-to".to_string());
        args.push(format!("{:.6}", end));
    }

    args.push("-i".to_string());
    args.push(src.to_string());

    // No video
    args.push("-vn".to_string());

    // atempo chain for playback rate != 1.0
    if let Some(filter) = build_atempo_filter(rate) {
        args.push("-af".to_string());
        args.push(filter);
    }

    // Overwrite output
    args.push("-y".to_string());
    args.push(partial_path.to_str().unwrap_or_default().to_string());

    let status = std::process::Command::new("ffmpeg")
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match status {
        Ok(s) if s.success() => match std::fs::rename(&partial_path, &wav_path) {
            Ok(()) => Some(wav_path),
            Err(e) => {
                eprintln!(
                    "rustmotion: embedded-video audio: failed to finalize cached WAV for '{}': {}. Skipping.",
                    src, e
                );
                let _ = std::fs::remove_file(&partial_path);
                None
            }
        },
        Ok(_) => {
            eprintln!(
                "rustmotion: embedded-video audio: ffmpeg failed to extract audio from '{}' \
                 (trim_start={:.3}, trim_end={:?}, rate={:.3}). Skipping.",
                src, trim_start, trim_end, rate
            );
            let _ = std::fs::remove_file(&partial_path);
            None
        }
        Err(e) => {
            eprintln!(
                "rustmotion: embedded-video audio: could not spawn ffmpeg for '{}': {}. Skipping.",
                src, e
            );
            let _ = std::fs::remove_file(&partial_path);
            None
        }
    }
}

// ─── Public entry point ───────────────────────────────────────────────────────

/// Enumerate all `video` components with `volume > 0` in slide views,
/// extract their audio streams via ffmpeg, and return a list of `AudioTrack`
/// entries to append to `scenario.audio` before calling the existing mixer.
///
/// World views are skipped with a warning.  If ffmpeg is not installed the
/// whole function returns an empty `Vec` after printing a single warning.
pub fn collect_video_audio_tracks(scenario: &ResolvedScenario) -> Vec<AudioTrack> {
    if !ffmpeg_available() {
        // Only warn once — the video encoder path will also warn on ffmpeg
        // absence so we keep this low-noise.
        eprintln!(
            "rustmotion: ffmpeg not found — embedded video audio will be silent. \
             Install ffmpeg to include audio from video components."
        );
        return Vec::new();
    }

    let offsets = scene_start_offsets(scenario);
    let mut tracks: Vec<AudioTrack> = Vec::new();

    for (view_idx, view) in scenario.views.iter().enumerate() {
        // World views: offsets are computed but audio extraction is skipped
        // (the warning was already emitted inside scene_start_offsets).
        if matches!(view.view_type, ViewType::World) {
            continue;
        }

        for (scene_idx, scene) in view.scenes.iter().enumerate() {
            let scene_start = offsets
                .get(view_idx)
                .and_then(|v| v.get(scene_idx))
                .copied()
                .unwrap_or(0.0);

            let mut occurrences: Vec<VideoOccurrence> = Vec::new();
            collect_videos_in_scene(scene, &mut occurrences);

            for occ in occurrences {
                let Some(wav_path) =
                    extract_audio_to_wav(&occ.src, occ.trim_start, occ.trim_end, occ.playback_rate)
                else {
                    continue;
                };

                let Some(wav_str) = wav_path.to_str() else {
                    eprintln!(
                        "rustmotion: embedded-video audio: temp WAV path is not UTF-8 — skipping."
                    );
                    continue;
                };

                // Absolute start in the output video timeline
                let abs_start = scene_start + occ.start_at;

                // Duration of the extracted audio (post-rate adjustment)
                // and optional end constraint from end_at.
                let end = occ.end_at.map(|ea| {
                    let component_duration = ea - occ.start_at;
                    abs_start + component_duration
                });

                tracks.push(AudioTrack {
                    src: wav_str.to_string(),
                    start: abs_start,
                    end,
                    volume: occ.volume,
                    fade_in: None,
                    fade_out: None,
                    volume_keyframes: Vec::new(),
                });
            }
        }
    }

    tracks
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::load_scenario_from_source;

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn load(json: &str) -> ResolvedScenario {
        load_scenario_from_source(None, Some(json)).expect("load")
    }

    // ── scene_start_offsets ───────────────────────────────────────────────────

    /// Single slide view, no transitions → scenes are back-to-back.
    #[test]
    fn offsets_single_view_no_transitions() {
        let s = load(
            r#"{
            "video": {"width": 32, "height": 32, "fps": 10},
            "scenes": [
                {"duration": 1.0, "children": []},
                {"duration": 2.0, "children": []},
                {"duration": 0.5, "children": []}
            ]
        }"#,
        );

        let offsets = scene_start_offsets(&s);
        assert_eq!(offsets.len(), 1);
        let v = &offsets[0];
        assert_eq!(v.len(), 3);

        assert!((v[0] - 0.0).abs() < 1e-9, "scene 0 starts at 0");
        assert!((v[1] - 1.0).abs() < 1e-9, "scene 1 starts at 1.0s");
        assert!((v[2] - 3.0).abs() < 1e-9, "scene 2 starts at 3.0s");
    }

    /// Single slide view with an incoming transition on scene 1:
    /// scene 1 overlaps the tail of scene 0 by `transition.duration`.
    #[test]
    fn offsets_single_view_with_scene_transition() {
        // Scene 0: 2s.  Scene 1: 1s, incoming fade of 0.5s.
        // Expected:
        //   scene 0 → 0.0s
        //   scene 1 → 2.0 - 0.5 = 1.5s
        let s = load(
            r#"{
            "video": {"width": 32, "height": 32, "fps": 10},
            "scenes": [
                {"duration": 2.0, "children": []},
                {"duration": 1.0, "transition": {"type": "fade", "duration": 0.5}, "children": []}
            ]
        }"#,
        );

        let offsets = scene_start_offsets(&s);
        let v = &offsets[0];
        assert!((v[0] - 0.0).abs() < 1e-9, "scene 0 at 0");
        assert!((v[1] - 1.5).abs() < 1e-3, "scene 1 at 1.5s got {}", v[1]);
    }

    /// Two slide views with an inter-view transition.
    /// View 0: scene 0 (1s) + scene 1 (1s) = 2s total.
    /// Inter-view transition: 0.5s.
    /// View 1: scene 0 starts at 2.0 + 0.5 = 2.5s.
    #[test]
    fn offsets_two_views_with_view_transition() {
        let s = load(
            r#"{
            "video": {"width": 32, "height": 32, "fps": 10},
            "composition": [
                {"type": "slide", "scenes": [
                    {"duration": 1.0, "children": []},
                    {"duration": 1.0, "children": []}
                ]},
                {"type": "slide",
                 "transition": {"type": "fade", "duration": 0.5},
                 "scenes": [
                    {"duration": 1.0, "children": []}
                ]}
            ]
        }"#,
        );

        let offsets = scene_start_offsets(&s);
        assert_eq!(offsets.len(), 2);

        let v0 = &offsets[0];
        assert!((v0[0] - 0.0).abs() < 1e-9, "view0 scene0 at 0");
        assert!((v0[1] - 1.0).abs() < 1e-9, "view0 scene1 at 1.0");

        let v1 = &offsets[1];
        // View 0 ends at 2.0s, then 0.5s view transition, so view 1 starts at 2.5s.
        assert!(
            (v1[0] - 2.5).abs() < 1e-3,
            "view1 scene0 expected 2.5s, got {}",
            v1[0]
        );
    }

    // ── build_atempo_filter ───────────────────────────────────────────────────

    #[test]
    fn atempo_rate_1_returns_none() {
        assert_eq!(build_atempo_filter(1.0), None);
    }

    #[test]
    fn atempo_rate_2_single_stage() {
        let f = build_atempo_filter(2.0).unwrap();
        assert!(f.starts_with("atempo=2.0"), "got: {f}");
        assert!(!f.contains(','), "should be single stage: {f}");
    }

    #[test]
    fn atempo_rate_4_two_stages() {
        let f = build_atempo_filter(4.0).unwrap();
        // Should be: atempo=2.0,atempo=2.0
        let parts: Vec<&str> = f.split(',').collect();
        assert_eq!(parts.len(), 2, "rate 4.0 → 2 stages: {f}");
        assert!(parts[0].starts_with("atempo=2.0"), "first stage: {f}");
        assert!(parts[1].starts_with("atempo=2.0"), "second stage: {f}");
    }

    #[test]
    fn atempo_rate_0_5_single_stage() {
        let f = build_atempo_filter(0.5).unwrap();
        assert!(f.starts_with("atempo=0.5"), "got: {f}");
        assert!(!f.contains(','), "single stage: {f}");
    }

    #[test]
    fn atempo_rate_0_25_two_stages() {
        // 0.25 = 0.5 * 0.5
        let f = build_atempo_filter(0.25).unwrap();
        let parts: Vec<&str> = f.split(',').collect();
        assert_eq!(parts.len(), 2, "rate 0.25 → 2 stages: {f}");
        assert!(parts[0].starts_with("atempo=0.5"), "first: {f}");
        assert!(parts[1].starts_with("atempo=0.5"), "second: {f}");
    }

    // ── collect_video_audio_tracks (pure-logic parts) ─────────────────────────

    /// Video with volume==0 must be excluded even if it has a valid src.
    #[test]
    fn volume_zero_is_excluded() {
        let s = load(
            r#"{
            "video": {"width": 32, "height": 32, "fps": 10},
            "scenes": [
                {"duration": 1.0, "children": [
                    {"type": "video", "src": "test.mp4", "volume": 0.0,
                     "style": {"width": "32px", "height": "32px"}}
                ]}
            ]
        }"#,
        );

        // collect_videos_in_scene uses the same logic: check directly
        let mut occs: Vec<VideoOccurrence> = Vec::new();
        let children: Vec<ChildComponent> = s.views[0].scenes[0]
            .children
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        for ch in &children {
            collect_videos_in_child(ch, &mut occs);
        }
        assert!(occs.is_empty(), "volume=0 must not be collected");
    }

    /// Video nested inside a card is found by the recursive walk.
    #[test]
    fn nested_video_in_card_is_collected() {
        let s = load(
            r#"{
            "video": {"width": 32, "height": 32, "fps": 10},
            "scenes": [
                {"duration": 1.0, "children": [
                    {"type": "card", "children": [
                        {"type": "video", "src": "nested.mp4", "volume": 0.8,
                         "style": {"width": "32px", "height": "32px"}}
                    ]}
                ]}
            ]
        }"#,
        );

        let mut occs: Vec<VideoOccurrence> = Vec::new();
        let children: Vec<ChildComponent> = s.views[0].scenes[0]
            .children
            .iter()
            .filter_map(|v| serde_json::from_value(v.clone()).ok())
            .collect();
        for ch in &children {
            collect_videos_in_child(ch, &mut occs);
        }
        assert_eq!(occs.len(), 1, "nested video must be found");
        assert_eq!(occs[0].src, "nested.mp4");
        assert!((occs[0].volume - 0.8).abs() < 1e-6);
    }

    /// AudioTrack offsets: video in scene 1 of a two-scene slide.
    /// Scene 0: 1s, Scene 1: 1s, no transitions → scene 1 starts at 1.0s.
    /// Video has start_at=0.2 → track.start = 1.0 + 0.2 = 1.2s.
    ///
    /// This test skips if ffmpeg is absent (integration guard).
    #[test]
    #[cfg_attr(not(feature = "ffmpeg_integration"), ignore)]
    fn audio_track_offset_scene2_with_start_at() {
        // This test needs a real video file; guard it.
        // When run in CI with ffmpeg available, it verifies placement.
    }

    /// wav_cache_path is deterministic: same inputs → same path.
    #[test]
    fn wav_cache_path_is_deterministic() {
        let p1 = wav_cache_path("foo.mp4", 0.5, Some(3.0), 1.5);
        let p2 = wav_cache_path("foo.mp4", 0.5, Some(3.0), 1.5);
        assert_eq!(p1, p2);
    }

    /// Different params → different path (collision check)
    #[test]
    fn wav_cache_path_differs_on_params() {
        let p1 = wav_cache_path("foo.mp4", 0.0, None, 1.0);
        let p2 = wav_cache_path("foo.mp4", 0.5, None, 1.0);
        assert_ne!(p1, p2);
    }

    /// Constat #10: the cache key must fold in the source file's own
    /// metadata, not just its path — otherwise editing a video in place
    /// (same path, new bytes) keeps serving audio extracted from the file's
    /// *previous* contents forever, with no error and no way to detect it.
    #[test]
    fn wav_cache_path_changes_when_source_file_is_modified() {
        let src = std::env::temp_dir().join(format!(
            "rm_vidaud_src_test_{}_{}.mp4",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&src, b"version one").unwrap();
        let p1 = wav_cache_path(src.to_str().unwrap(), 0.0, None, 1.0);

        // Best-effort: push the mtime forward too, in case the filesystem's
        // mtime resolution is coarser than the write below.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&src, b"version two, a longer and different payload").unwrap();
        let p2 = wav_cache_path(src.to_str().unwrap(), 0.0, None, 1.0);

        assert_ne!(
            p1, p2,
            "modifying the source file's contents must invalidate the cached WAV path"
        );

        let _ = std::fs::remove_file(&src);
    }

    /// Constat #10: a failed extraction must never leave a residue file —
    /// neither the promoted `wav_path` (a false cache hit on the next
    /// render, per `wav_path.exists()` above) nor the `.partial` scratch
    /// file ffmpeg wrote to along the way.
    #[test]
    fn failed_extraction_leaves_no_residue_on_disk() {
        if !ffmpeg_available() {
            eprintln!("failed_extraction_leaves_no_residue_on_disk: ffmpeg not found — skipping");
            return;
        }
        let missing_src = std::env::temp_dir().join(format!(
            "rm_vidaud_missing_{}_{}.mp4",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&missing_src); // guarantee it does not exist

        let wav_path = wav_cache_path(missing_src.to_str().unwrap(), 0.0, None, 1.0);
        let partial_path = partial_wav_path(&wav_path);
        let _ = std::fs::remove_file(&wav_path);
        let _ = std::fs::remove_file(&partial_path);

        let result = extract_audio_to_wav(missing_src.to_str().unwrap(), 0.0, None, 1.0);

        assert!(
            result.is_none(),
            "extraction from a nonexistent source must fail"
        );
        assert!(
            !wav_path.exists(),
            "a failed extraction must not leave a cached WAV that a later render would reuse"
        );
        assert!(
            !partial_path.exists(),
            "a failed extraction must not leave a .partial scratch file behind"
        );
    }

    /// Constat #10: a successful extraction promotes the `.partial` scratch
    /// file to the real cache path and leaves no `.partial` behind.
    #[test]
    fn successful_extraction_leaves_no_partial_file_behind() {
        if !ffmpeg_available() {
            eprintln!(
                "successful_extraction_leaves_no_partial_file_behind: ffmpeg not found — skipping"
            );
            return;
        }
        let fixture = std::env::temp_dir().join("rustmotion_test_vidaud_partial_fixture.mp4");
        let fixture_str = fixture.to_str().unwrap();
        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                fixture_str,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
        if !matches!(status, Ok(s) if s.success()) {
            eprintln!(
                "successful_extraction_leaves_no_partial_file_behind: fixture generation failed — skipping"
            );
            return;
        }

        let wav_path = wav_cache_path(fixture_str, 0.0, None, 1.0);
        let partial_path = partial_wav_path(&wav_path);
        let _ = std::fs::remove_file(&wav_path);
        let _ = std::fs::remove_file(&partial_path);

        let result = extract_audio_to_wav(fixture_str, 0.0, None, 1.0);

        assert!(
            result.is_some(),
            "extraction from a valid fixture must succeed"
        );
        assert!(
            wav_path.exists(),
            "successful extraction must leave the cached WAV at its final path"
        );
        assert!(
            !partial_path.exists(),
            "successful extraction must not leave the .partial scratch file behind"
        );

        let _ = std::fs::remove_file(&wav_path);
        let _ = std::fs::remove_file(&fixture);
    }

    // ── Integration test (gated on ffmpeg) ───────────────────────────────────

    /// Full round-trip: generate a 1-second sine+test-video fixture with ffmpeg,
    /// build a 2-scene scenario (video in scene 1), call collect_video_audio_tracks,
    /// verify the returned track has the correct start offset and non-empty src.
    #[test]
    fn integration_audio_track_from_embedded_video() {
        // Skip if ffmpeg is not available.
        if !ffmpeg_available() {
            eprintln!("integration_audio_track_from_embedded_video: ffmpeg not found — skipping");
            return;
        }

        // Generate a 1s lavfi sine video fixture.
        let fixture = std::env::temp_dir().join("rustmotion_test_vidaud_fixture.mp4");
        let fixture_str = fixture.to_str().unwrap();

        let status = std::process::Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-f",
                "lavfi",
                "-i",
                "testsrc=duration=1:size=32x32:rate=30",
                "-shortest",
                fixture_str,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("spawn ffmpeg for fixture");

        if !status.success() {
            eprintln!(
                "integration_audio_track_from_embedded_video: fixture generation failed — skipping"
            );
            return;
        }

        // Two-scene scenario: scene 0 (1s, no video), scene 1 (1s, video at start_at=0.2).
        let json = format!(
            r#"{{
            "video": {{"width": 32, "height": 32, "fps": 30}},
            "scenes": [
                {{"duration": 1.0, "children": []}},
                {{"duration": 1.0, "children": [
                    {{"type": "video", "src": "{}", "volume": 0.9,
                     "start_at": 0.2,
                     "style": {{"width": "32px", "height": "32px"}}}}
                ]}}
            ]
        }}"#,
            fixture_str.replace('\\', "\\\\")
        );

        let scenario = load_scenario_from_source(None, Some(&json)).expect("load");
        let tracks = collect_video_audio_tracks(&scenario);

        // Clean up fixture
        let _ = std::fs::remove_file(&fixture);

        assert_eq!(tracks.len(), 1, "expected one audio track");
        let t = &tracks[0];

        // scene 1 starts at 1.0s, start_at=0.2 → abs_start = 1.2s
        assert!(
            (t.start - 1.2).abs() < 1e-9,
            "expected start=1.2, got {}",
            t.start
        );
        assert!(
            (t.volume - 0.9).abs() < 1e-6,
            "expected volume=0.9, got {}",
            t.volume
        );
        assert!(!t.src.is_empty(), "src must be a WAV path");
        assert!(
            std::path::Path::new(&t.src).exists(),
            "WAV file must exist: {}",
            t.src
        );
    }
}
