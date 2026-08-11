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
