use rustmotion::components::intrinsic::{GradientTextIntrinsic, TextIntrinsic};
use rustmotion::components::{ChildComponent, Component};
use rustmotion::core::engine::box_tree::{AvailableSpace, IntrinsicMeasure};
use rustmotion::engine::animator::spring_rest_time;
use rustmotion::engine::render::deserialize_children;
use rustmotion::error::Result;
use rustmotion::loader::load_input;
use rustmotion::schema::{self, AnimationEffect, ResolvedScenario, SpringConfig};
use std::path::PathBuf;

pub fn cmd_info(input: &PathBuf) -> Result<()> {
    let scenario = load_input(input)?;
    let fps = scenario.video.fps;
    let all_scenes: Vec<_> = scenario.all_scenes().collect();
    let total_duration: f64 = all_scenes.iter().map(|s| s.duration).sum();
    let total_frames: u32 = all_scenes
        .iter()
        .map(|s| (s.duration * fps as f64).round() as u32)
        .sum();

    let total_layers: usize = all_scenes.iter().map(|s| s.children.len()).sum();

    println!("File: {}", input.display());
    println!(
        "Resolution: {}x{}",
        scenario.video.width, scenario.video.height
    );
    println!("FPS: {}", fps);
    println!("Duration: {:.1}s ({} frames)", total_duration, total_frames);
    println!("Views: {}", scenario.views.len());
    println!("Scenes: {}", all_scenes.len());
    println!("Total layers: {}", total_layers);
    println!("Audio tracks: {}", scenario.audio.len());

    for (vi, view) in scenario.views.iter().enumerate() {
        let vtype = match view.view_type {
            schema::ViewType::Slide => "Slide",
            schema::ViewType::World => "World",
        };
        println!(
            "  View {}: {} ({} scenes)",
            vi + 1,
            vtype,
            view.scenes.len()
        );
        for (si, scene) in view.scenes.iter().enumerate() {
            let scene_frames = (scene.duration * fps as f64).round() as u32;
            println!(
                "    Scene {}: {:.1}s ({} frames, {} layers{})",
                si + 1,
                scene.duration,
                scene_frames,
                scene.children.len(),
                scene
                    .transition
                    .as_ref()
                    .map(|t| format!(", transition: {:?} {:.1}s", t.transition_type, t.duration))
                    .unwrap_or_default()
            );
        }
    }

    let springs = collect_springs(&scenario);
    if !springs.is_empty() {
        println!("Springs:");
        for report in &springs {
            println!("  {}", report.describe());
        }
    }

    let text_sizes = collect_text_measurements(&scenario);
    if !text_sizes.is_empty() {
        println!("Text sizes:");
        for report in &text_sizes {
            println!("  {}", report.describe());
        }
    }

    let media_assets = collect_media_assets(&scenario);
    if !media_assets.is_empty() {
        println!("Media assets:");
        for report in &media_assets {
            println!("  {}", report.describe());
        }
    }

    Ok(())
}

/// "Quelle largeur/hauteur fait ce texte, à cette taille, dans cette
/// police" (text-autofit workstream, lot text-autofit) exposed the same way
/// `rustmotion info` already exposes spring settle times (see
/// `SpringReport` above) rather than as a bespoke, separate command:
/// `rustmotion info` walks the scenario and reports; this adds one more
/// thing it reports.
///
/// The measurement is the natural (unconstrained) size at the declared
/// `font-size`/family — via `TextIntrinsic`/`GradientTextIntrinsic`, the
/// exact same Skia-backed measurer the layout engine and the geometry
/// validator use, so this is never a second, independently-drifting
/// estimate of what the same text measures elsewhere.
struct TextMeasurement {
    label: String,
    kind: &'static str,
    preview: String,
    font_size: f32,
    natural_width: f32,
    natural_height: f32,
    autofit: bool,
}

impl TextMeasurement {
    fn describe(&self) -> String {
        let autofit_note = if self.autofit {
            " (text-autofit: true — shrinks further if its box is smaller than this)"
        } else {
            ""
        };
        format!(
            "{}: {} \"{}\" @ {:.0}px → natural {:.0}×{:.0}px{}",
            self.label,
            self.kind,
            self.preview,
            self.font_size,
            self.natural_width,
            self.natural_height,
            autofit_note,
        )
    }
}

fn collect_text_measurements(scenario: &ResolvedScenario) -> Vec<TextMeasurement> {
    let mut out = Vec::new();
    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            let children = deserialize_children(scene);
            let path = format!("view {} / scene {}", vi + 1, si + 1);
            collect_text_measurements_in_children(&children, &path, &mut out);
        }
    }
    out
}

fn collect_text_measurements_in_children(
    children: &[ChildComponent],
    path: &str,
    out: &mut Vec<TextMeasurement>,
) {
    let natural = (AvailableSpace::MaxContent, AvailableSpace::MaxContent);
    for (i, child) in children.iter().enumerate() {
        let p = format!("{path} / layer {}", i + 1);
        match &child.component {
            Component::Text(t) => {
                let (w, h) = TextIntrinsic::from_text(t).measure((None, None), natural);
                out.push(TextMeasurement {
                    label: p.clone(),
                    kind: "text",
                    preview: preview(&t.content),
                    font_size: t.style.font_size_px_or(48.0),
                    natural_width: w,
                    natural_height: h,
                    autofit: matches!(t.style.text_autofit, Some(true)),
                });
            }
            Component::GradientText(t) => {
                let (w, h) =
                    GradientTextIntrinsic::from_gradient_text(t).measure((None, None), natural);
                out.push(TextMeasurement {
                    label: p.clone(),
                    kind: "gradient_text",
                    preview: preview(&t.content),
                    font_size: t.style.font_size_px_or(48.0),
                    natural_width: w,
                    natural_height: h,
                    autofit: matches!(t.style.text_autofit, Some(true)),
                });
            }
            _ => {}
        }
        match &child.component {
            Component::Card(c) => collect_text_measurements_in_children(&c.children, &p, out),
            Component::Flex(c) => collect_text_measurements_in_children(&c.children, &p, out),
            Component::Grid(c) => collect_text_measurements_in_children(&c.children, &p, out),
            Component::Positioned(c) => collect_text_measurements_in_children(&c.children, &p, out),
            Component::Container(c) => collect_text_measurements_in_children(&c.children, &p, out),
            _ => {}
        }
    }
}

/// Truncate a long content string for a single-line report — the full
/// content is already visible in the source scenario file; this is a label,
/// not a transcript.
fn preview(content: &str) -> String {
    const MAX_CHARS: usize = 40;
    let char_count = content.chars().count();
    if char_count <= MAX_CHARS {
        content.to_string()
    } else {
        let truncated: String = content.chars().take(MAX_CHARS).collect();
        format!("{truncated}…")
    }
}

/// Where a `SpringConfig` was found, and the settle time computed for it —
/// the "measure du repos" issue #167 lot E asks `rustmotion info` to
/// surface, so an author can size the enclosing animation's `duration`
/// around a spring instead of guessing (see `SpringConfig::duration`'s doc
/// comment for why the two are not automatically kept in sync).
#[derive(Debug)]
struct SpringReport {
    label: String,
    rest_seconds: f64,
    duration_was_set: bool,
}

impl SpringReport {
    fn describe(&self) -> String {
        if self.duration_was_set {
            format!(
                "{}: settles at {:.3}s (spring.duration set explicitly)",
                self.label, self.rest_seconds
            )
        } else {
            format!(
                "{}: settles at {:.3}s (natural — no spring.duration set; \
                 pin the enclosing animation's duration to at least this to \
                 avoid cutting the spring short)",
                self.label, self.rest_seconds
            )
        }
    }
}

fn collect_springs(scenario: &ResolvedScenario) -> Vec<SpringReport> {
    let mut out = Vec::new();
    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            let children = deserialize_children(scene);
            let path = format!("view {} / scene {}", vi + 1, si + 1);
            collect_springs_in_children(&children, &path, &mut out);
        }
    }
    out
}

fn collect_springs_in_children(
    children: &[ChildComponent],
    path: &str,
    out: &mut Vec<SpringReport>,
) {
    for (i, child) in children.iter().enumerate() {
        let p = format!("{path} / layer {}", i + 1);
        if let Some(anim) = child.component.as_animatable() {
            for effect in anim.animation_effects() {
                if let Some((_, timing)) = effect.as_preset() {
                    if let Some(spring) = &timing.spring {
                        out.push(spring_report(&p, spring));
                    }
                }
                if let AnimationEffect::Keyframes(k) = effect {
                    for kf_anim in &k.keyframes {
                        if let Some(spring) = &kf_anim.spring {
                            out.push(spring_report(&p, spring));
                        }
                    }
                }
            }
        }
        match &child.component {
            Component::Card(c) => collect_springs_in_children(&c.children, &p, out),
            Component::Flex(c) => collect_springs_in_children(&c.children, &p, out),
            Component::Grid(c) => collect_springs_in_children(&c.children, &p, out),
            Component::Positioned(c) => collect_springs_in_children(&c.children, &p, out),
            Component::Container(c) => collect_springs_in_children(&c.children, &p, out),
            _ => {}
        }
    }
}

fn spring_report(label: &str, spring: &SpringConfig) -> SpringReport {
    SpringReport {
        label: label.to_string(),
        rest_seconds: spring_rest_time(spring),
        duration_was_set: spring.duration.is_some(),
    }
}

/// "How long is this audio file? What are the dimensions of this image?" —
/// answered the same way `rustmotion info` already answers "what does this
/// spring settle at" and "how big does this text measure": walk the
/// scenario, report a line per asset found. `audio[].src` plus every
/// component whose `src` names a required, standalone media file (`image`,
/// `gif`, `video`, `avatar`, `avatar_group`, `mockup`) is covered.
///
/// `lottie` and `svg` also carry a `src`, but it is optional there — an
/// inline `data` (or, for `lottie`, a pre-rendered `frames_dir`) is a fully
/// valid alternative — and what "metadata" even means for them differs from
/// a raster header or a media container's duration (a Lottie/Bodymovin JSON
/// document has its own `w`/`h`/`fr`/`ip`/`op` fields; an SVG has its own
/// `viewBox`). Deliberately not covered here.
///
/// Two rules, applied uniformly to every kind below, are what make this safe
/// to run unconditionally as part of `info` rather than needing an opt-in
/// flag:
///
/// 1. **Never touch the network.** A `src` starting with `http://`/
///    `https://` is reported as [`MediaStatus::Remote`] without ever being
///    opened — probing a remote asset could mean downloading an unbounded
///    amount of data just to read a header.
/// 2. **A bad asset reports a reason, it never aborts the walk.** A missing
///    local file is [`MediaStatus::Missing`]; one that exists but can't be
///    decoded/probed (corrupt, wrong format, `ffprobe` absent, ...) is
///    [`MediaStatus::Unreadable`] carrying the underlying error's message.
///    `cmd_info` still exits `0` and still prints every other section.
#[derive(Debug)]
struct MediaAssetReport {
    label: String,
    kind: &'static str,
    src: String,
    status: MediaStatus,
}

#[derive(Debug)]
enum MediaStatus {
    Remote,
    Missing,
    Unreadable(String),
    Image {
        width: u32,
        height: u32,
    },
    Video {
        width: u32,
        height: u32,
        duration_secs: f64,
        fps: Option<f64>,
    },
    Audio {
        duration_secs: f64,
        sample_rate: u32,
        channels: u32,
    },
}

impl MediaAssetReport {
    fn describe(&self) -> String {
        let head = format!("{}: {} \"{}\"", self.label, self.kind, self.src);
        match &self.status {
            MediaStatus::Remote => format!(
                "{head} → remote asset, not fetched (http/https — would risk an unbounded \
                 download just to read a header)"
            ),
            MediaStatus::Missing => format!("{head} → file not found"),
            MediaStatus::Unreadable(reason) => format!("{head} → could not read: {reason}"),
            MediaStatus::Image { width, height } => format!("{head} → {width}×{height}"),
            MediaStatus::Video {
                width,
                height,
                duration_secs,
                fps,
            } => {
                let fps_part = fps
                    .map(|f| format!(" @ {f:.2}fps"))
                    .unwrap_or_else(|| " (fps unknown)".to_string());
                format!("{head} → {duration_secs:.2}s, {width}×{height}{fps_part}")
            }
            MediaStatus::Audio {
                duration_secs,
                sample_rate,
                channels,
            } => {
                let ch = match channels {
                    1 => "mono".to_string(),
                    2 => "stereo".to_string(),
                    n => format!("{n}ch"),
                };
                format!("{head} → {duration_secs:.2}s, {sample_rate}Hz {ch}")
            }
        }
    }
}

/// `true` for a `src` this codebase does not attempt to probe: everything
/// here resolves `src` as a local filesystem path (see the module-level
/// research this fix is built on — no component's `src` supports a remote
/// URL today, video's ffmpeg-backed path only reaches one incidentally by
/// handing the string straight to ffmpeg's own demuxer). Probing would mean
/// this command deciding, on an author's behalf, to make a network request —
/// and for a container whose metadata sits at the end of the file, that can
/// mean downloading the whole thing just to answer "how long is this?".
fn is_remote(src: &str) -> bool {
    src.starts_with("http://") || src.starts_with("https://")
}

fn probe_local_image(src: &str) -> MediaStatus {
    if is_remote(src) {
        return MediaStatus::Remote;
    }
    if !std::path::Path::new(src).exists() {
        return MediaStatus::Missing;
    }
    match rustmotion::core::engine::renderer::probe_image_dimensions(src) {
        Ok((width, height)) => MediaStatus::Image { width, height },
        Err(e) => MediaStatus::Unreadable(e.to_string()),
    }
}

fn probe_local_video(src: &str) -> MediaStatus {
    if is_remote(src) {
        return MediaStatus::Remote;
    }
    if !std::path::Path::new(src).exists() {
        return MediaStatus::Missing;
    }
    match rustmotion::core::engine::renderer::probe_video_metadata(src) {
        Ok(p) => MediaStatus::Video {
            width: p.width,
            height: p.height,
            duration_secs: p.duration_secs,
            fps: p.fps,
        },
        Err(e) => MediaStatus::Unreadable(e.to_string()),
    }
}

fn probe_local_audio(src: &str) -> MediaStatus {
    if is_remote(src) {
        return MediaStatus::Remote;
    }
    if !std::path::Path::new(src).exists() {
        return MediaStatus::Missing;
    }
    match rustmotion::encode::audio::probe_audio_metadata(src) {
        Ok(p) => MediaStatus::Audio {
            duration_secs: p.duration_secs,
            sample_rate: p.sample_rate,
            channels: p.channels,
        },
        Err(e) => MediaStatus::Unreadable(e.to_string()),
    }
}

fn collect_media_assets(scenario: &ResolvedScenario) -> Vec<MediaAssetReport> {
    let mut out = Vec::new();
    for (i, track) in scenario.audio.iter().enumerate() {
        out.push(MediaAssetReport {
            label: format!("audio track {}", i + 1),
            kind: "audio",
            status: probe_local_audio(&track.src),
            src: track.src.clone(),
        });
    }
    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            let children = deserialize_children(scene);
            let path = format!("view {} / scene {}", vi + 1, si + 1);
            collect_media_assets_in_children(&children, &path, &mut out);
        }
    }
    out
}

fn collect_media_assets_in_children(
    children: &[ChildComponent],
    path: &str,
    out: &mut Vec<MediaAssetReport>,
) {
    for (i, child) in children.iter().enumerate() {
        let p = format!("{path} / layer {}", i + 1);
        match &child.component {
            Component::Image(c) => out.push(MediaAssetReport {
                label: p.clone(),
                kind: "image",
                status: probe_local_image(&c.src),
                src: c.src.clone(),
            }),
            Component::Gif(c) => out.push(MediaAssetReport {
                label: p.clone(),
                kind: "gif",
                status: probe_local_image(&c.src),
                src: c.src.clone(),
            }),
            Component::Video(c) => out.push(MediaAssetReport {
                label: p.clone(),
                kind: "video",
                status: probe_local_video(&c.src),
                src: c.src.clone(),
            }),
            Component::Avatar(c) => out.push(MediaAssetReport {
                label: p.clone(),
                kind: "avatar",
                status: probe_local_image(&c.src),
                src: c.src.clone(),
            }),
            Component::AvatarGroup(c) => {
                for (ai, avatar) in c.avatars.iter().enumerate() {
                    out.push(MediaAssetReport {
                        label: format!("{p} / avatar {}", ai + 1),
                        kind: "avatar_group",
                        status: probe_local_image(&avatar.src),
                        src: avatar.src.clone(),
                    });
                }
            }
            Component::Mockup(c) => out.push(MediaAssetReport {
                label: p.clone(),
                kind: "mockup",
                status: probe_local_image(&c.src),
                src: c.src.clone(),
            }),
            _ => {}
        }
        match &child.component {
            Component::Card(c) => collect_media_assets_in_children(&c.children, &p, out),
            Component::Flex(c) => collect_media_assets_in_children(&c.children, &p, out),
            Component::Grid(c) => collect_media_assets_in_children(&c.children, &p, out),
            Component::Positioned(c) => collect_media_assets_in_children(&c.children, &p, out),
            Component::Container(c) => collect_media_assets_in_children(&c.children, &p, out),
            _ => {}
        }
    }
}

#[cfg(test)]
mod media_asset_tests {
    //! media-io lot: `rustmotion info` must report duration/dimensions for
    //! every media asset a scenario references, and must never fail the
    //! whole command over one bad asset — it reports why instead.
    use super::*;

    fn scratch_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rm_info_media_test_{}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            name
        ))
    }

    fn write_test_png(path: &std::path::Path, w: u32, h: u32) {
        let img = image::RgbImage::from_pixel(w, h, image::Rgb([1, 2, 3]));
        img.save(path).expect("write PNG fixture");
    }

    #[test]
    fn remote_image_is_reported_without_touching_the_network() {
        let status = probe_local_image("https://example.com/does-not-matter.png");
        assert!(matches!(status, MediaStatus::Remote));
    }

    #[test]
    fn remote_audio_is_reported_without_touching_the_network() {
        let status = probe_local_audio("http://example.com/does-not-matter.mp3");
        assert!(matches!(status, MediaStatus::Remote));
    }

    #[test]
    fn missing_local_image_is_reported_as_missing_not_an_error() {
        let path = scratch_path("missing.png");
        let status = probe_local_image(path.to_str().unwrap());
        assert!(matches!(status, MediaStatus::Missing));
    }

    #[test]
    fn unreadable_local_image_is_reported_with_a_reason() {
        let path = scratch_path("corrupt.png");
        std::fs::write(&path, b"not a real png").unwrap();
        let status = probe_local_image(path.to_str().unwrap());
        match status {
            MediaStatus::Unreadable(reason) => assert!(!reason.is_empty()),
            other => panic!("expected Unreadable, got {other:?}"),
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn readable_local_image_reports_its_dimensions() {
        let path = scratch_path("ok.png");
        write_test_png(&path, 9, 4);
        let status = probe_local_image(path.to_str().unwrap());
        assert!(matches!(
            status,
            MediaStatus::Image {
                width: 9,
                height: 4
            }
        ));
        let _ = std::fs::remove_file(&path);
    }

    /// The core "never fails a valid scenario" proof: a scenario whose only
    /// problem is one missing image must still walk to completion and
    /// report every other section — `collect_media_assets` never returns an
    /// `Err`, and never panics, on a missing/unreadable asset.
    #[test]
    fn collect_media_assets_never_fails_the_walk_on_a_bad_asset() {
        let json = serde_json::json!({
            "video": { "width": 64, "height": 64, "fps": 10 },
            "audio": [ { "src": "/definitely/does/not/exist.mp3" } ],
            "scenes": [ { "duration": 1.0, "children": [
                { "type": "image", "src": "/definitely/does/not/exist.png",
                  "style": { "width": 10, "height": 10 } },
                { "type": "text", "content": "hello" }
            ] } ]
        });
        let scenario: ResolvedScenario =
            rustmotion::loader::load_scenario_from_source(None, Some(&json.to_string()))
                .expect("scenario must load and validate structurally");
        let reports = collect_media_assets(&scenario);
        assert_eq!(
            reports.len(),
            2,
            "expected the audio track and the image: {reports:?}"
        );
        for r in &reports {
            assert!(
                matches!(r.status, MediaStatus::Missing),
                "expected Missing for {}: got {}",
                r.src,
                r.describe()
            );
        }
    }

    #[test]
    fn recurses_into_containers_the_same_way_springs_and_text_sizes_do() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "card",
            "children": [{
                "type": "image",
                "src": "/definitely/does/not/exist.png",
                "style": { "width": 10, "height": 10 }
            }]
        }))
        .unwrap();
        let mut out = Vec::new();
        collect_media_assets_in_children(&[child], "test", &mut out);
        assert_eq!(
            out.len(),
            1,
            "image nested inside a card must be found: {out:?}"
        );
        assert!(out[0].label.contains("layer 1"), "label: {}", out[0].label);
    }

    #[test]
    fn avatar_group_reports_one_entry_per_avatar() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "avatar_group",
            "avatars": [
                { "src": "/does/not/exist/a.png" },
                { "src": "/does/not/exist/b.png" }
            ]
        }))
        .unwrap();
        let mut out = Vec::new();
        collect_media_assets_in_children(&[child], "test", &mut out);
        assert_eq!(out.len(), 2, "expected one report per avatar: {out:?}");
        assert!(out[0].label.contains("avatar 1"));
        assert!(out[1].label.contains("avatar 2"));
    }

    #[test]
    fn a_scenario_with_no_media_produces_an_empty_report() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi"
        }))
        .unwrap();
        let mut out = Vec::new();
        collect_media_assets_in_children(&[child], "test", &mut out);
        assert!(out.is_empty(), "unexpected media reports: {out:?}");
    }
}

#[cfg(test)]
mod spring_report_tests {
    //! Issue #167 lot E: `rustmotion info` must surface the settle time of
    //! every spring it finds, recursing into containers the same way
    //! `validate_schema::validate_children` already does.
    use super::*;
    use rustmotion::components::ChildComponent;

    #[test]
    fn finds_a_spring_on_a_top_level_preset() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": {
                "animation": [{ "name": "bounce_in", "duration": 0.6, "spring": { "damping": 12, "stiffness": 100, "mass": 1 } }]
            }
        }))
        .unwrap();
        let mut out = Vec::new();
        collect_springs_in_children(&[child], "test", &mut out);
        assert_eq!(
            out.len(),
            1,
            "expected exactly one spring report: {:?}",
            out.iter().map(|r| &r.label).collect::<Vec<_>>()
        );
        assert!(out[0].rest_seconds > 0.0);
        assert!(!out[0].duration_was_set);
    }

    #[test]
    fn finds_a_spring_nested_inside_a_card() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "card",
            "children": [{
                "type": "text",
                "content": "hi",
                "style": {
                    "animation": [{ "name": "fade_in_up", "duration": 0.6, "spring": { "damping": 15, "stiffness": 100, "mass": 1 } }]
                }
            }]
        }))
        .unwrap();
        let mut out = Vec::new();
        collect_springs_in_children(&[child], "test", &mut out);
        assert_eq!(
            out.len(),
            1,
            "spring nested inside a card must be found: {out:?}"
        );
        assert!(
            out[0].label.contains("layer 1"),
            "expected the nested layer to be labelled: {}",
            out[0].label
        );
    }

    #[test]
    fn reports_the_pinned_duration_when_spring_duration_is_set() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": {
                "animation": [{
                    "name": "bounce_in",
                    "duration": 0.6,
                    "spring": { "damping": 6, "stiffness": 120, "mass": 1, "duration": 0.8 }
                }]
            }
        }))
        .unwrap();
        let mut out = Vec::new();
        collect_springs_in_children(&[child], "test", &mut out);
        assert_eq!(out.len(), 1);
        assert!(out[0].duration_was_set);
        assert!(
            (out[0].rest_seconds - 0.8).abs() < 1e-9,
            "spring.duration must be reported verbatim as the settle time, got {}",
            out[0].rest_seconds
        );
    }

    #[test]
    fn no_springs_produces_an_empty_report() {
        let child: ChildComponent = serde_json::from_value(serde_json::json!({
            "type": "text",
            "content": "hi",
            "style": {
                "animation": [{ "name": "fade_in_up", "duration": 0.6 }]
            }
        }))
        .unwrap();
        let mut out = Vec::new();
        collect_springs_in_children(&[child], "test", &mut out);
        assert!(out.is_empty(), "unexpected spring reports: {out:?}");
    }
}
