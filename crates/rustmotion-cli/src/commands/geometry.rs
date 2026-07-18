//! Geometry validator — walks the resolved box tree of every scene and
//! reports nodes whose absolute bounding box leaves the device viewport.
//!
//! Scope of v1:
//!   * detect absolute positions placed past the viewport edge
//!   * detect text components whose unwrapped natural width exceeds the
//!     allocated width when `wrap: false` is set
//!   * detect terminal/codeblock content that overflows their box when
//!     `auto_scroll: false`
//!   * exempt `marquee` and `cursor` (designed to bleed)
//!
//! Animation handling is layered: by default we only check the resting
//! (untransformed) layout. With `--strict-anim`, we additionally sample
//! frames and reapply the same canvas transforms the renderer uses to verify
//! each frame stays inside the viewport.
//!
//! This walker runs the new CSS-engine pipeline (taffy + cosmic-text) so the
//! geometry it checks matches what the renderer will actually paint.

use rustmotion::components::box_builder::build_scene_from_refs;
use rustmotion::components::intrinsic::TextIntrinsic;
use rustmotion::components::{ChildComponent, Component};
use rustmotion::core::css::taffy_bridge::ConversionContext;
use rustmotion::core::engine::box_tree::{AvailableSpace, BoxNode, IntrinsicMeasure};
use rustmotion::core::engine::layout_pass::{run_layout, BoxLayout, LayoutResult};
use rustmotion::engine::animator::{
    apply_orbits, apply_wiggles, extract_effects, resolve_animations, AnimatedProperties,
};
use rustmotion::engine::render;
use rustmotion::schema::ResolvedScenario;
use serde::Serialize;

/// One detected layout violation.
#[derive(Debug, Clone, Serialize)]
pub struct GeometryViolation {
    pub view_index: usize,
    pub scene_index: usize,
    /// JSON-style path to the offending child (e.g. `views[0].scenes[1].children[2].children[0]`).
    pub path: String,
    /// Component type name (e.g. "text", "counter").
    pub component: String,
    pub axis: Axis,
    pub kind: ViolationKind,
    /// Node bounding box, in viewport coordinates.
    pub bbox: BBox,
    /// Viewport size at validation time.
    pub viewport: (u32, u32),
    /// Human-readable hint suggesting a fix.
    pub hint: String,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    X,
    Y,
    Both,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ViolationKind {
    /// Component bbox crosses the viewport edge.
    ViewportOverflow,
    /// `wrap: false` set but the natural width exceeds the allocated width.
    UnwrappableTextOverflow,
    /// terminal/codeblock has `auto_scroll: false` but content > box.
    AutoScrollDisabledOverflow,
    /// Animated transform (scale/translate/wiggle/orbit) pushes the bbox out
    /// of the viewport at some sampled time. Only emitted with `--strict-anim`.
    AnimatedTextOverflow,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct BBox {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Top-level entry: validate every scene of every view.
pub fn validate_geometry(scenario: &ResolvedScenario) -> Vec<GeometryViolation> {
    let mut violations = Vec::new();
    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            let children = render::deserialize_children(scene);
            let viewport = (scenario.video.width, scenario.video.height);
            let viewport_f = (viewport.0 as f32, viewport.1 as f32);

            let root_css = render::root_style(scene.layout.as_ref());
            let built = build_scene_from_refs(children.iter(), viewport_f, root_css, None);
            let layouts = run_layout(&built.root, viewport_f, &ConversionContext::default());

            let path_root = format!("views[{}].scenes[{}]", vi, si);
            walk(
                &children,
                &built.root.children,
                &layouts,
                viewport,
                vi,
                si,
                &path_root,
                /*parent_clips=*/ true, // scene root always clips
                &mut violations,
            );
        }
    }
    violations
}

#[allow(clippy::too_many_arguments)]
fn walk(
    children: &[ChildComponent],
    boxes: &[BoxNode],
    layouts: &LayoutResult,
    viewport: (u32, u32),
    vi: usize,
    si: usize,
    path: &str,
    _parent_clips: bool,
    out: &mut Vec<GeometryViolation>,
) {
    for (i, (child, box_node)) in children.iter().zip(boxes.iter()).enumerate() {
        let child_path = format!("{}.children[{}]", path, i);
        let layout = match layouts.get(box_node.id) {
            Some(l) => l,
            None => continue,
        };
        let bbox = bbox_of(layout);

        if !is_exempted(&child.component) {
            check_viewport(&child.component, &child_path, &bbox, viewport, vi, si, out);
            check_unwrappable_text(&child.component, &child_path, &bbox, viewport, vi, si, out);
            check_auto_scroll(&child.component, &child_path, &bbox, viewport, vi, si, out);
        }

        if let Some(grandchildren) = container_children(&child.component) {
            walk(
                grandchildren,
                &box_node.children,
                layouts,
                viewport,
                vi,
                si,
                &child_path,
                /*parent_clips=*/ container_clips(&child.component),
                out,
            );
        }
    }
}

fn bbox_of(layout: &BoxLayout) -> BBox {
    BBox {
        x: layout.x,
        y: layout.y,
        w: layout.width,
        h: layout.height,
    }
}

fn is_exempted(c: &Component) -> bool {
    matches!(c, Component::Marquee(_) | Component::Cursor(_))
}

fn container_children(c: &Component) -> Option<&[ChildComponent]> {
    match c {
        Component::Card(card) => Some(&card.children),
        Component::Flex(flex) => Some(&flex.children),
        Component::Grid(grid) => Some(&grid.children),
        Component::Positioned(pos) => Some(&pos.children),
        Component::Container(c) => Some(&c.children),
        _ => None,
    }
}

/// Whether a container clips its children. Used for nested overflow checks
/// (out of scope for v1: we only validate against the viewport, never the
/// parent box, since CSS-style overflow:visible is the default).
fn container_clips(c: &Component) -> bool {
    let style = c.as_styled().style_config();
    matches!(
        style.overflow,
        Some(rustmotion::core::css::style::Overflow::Hidden)
    ) || style.background.is_some() // card with background already clips
}

fn check_viewport(
    component: &Component,
    path: &str,
    bbox: &BBox,
    viewport: (u32, u32),
    vi: usize,
    si: usize,
    out: &mut Vec<GeometryViolation>,
) {
    let vw = viewport.0 as f32;
    let vh = viewport.1 as f32;
    let right = bbox.x + bbox.w;
    let bottom = bbox.y + bbox.h;
    // Sub-pixel tolerance for floating-point rounding. 0.5 px is well below
    // the human-visible threshold and avoids false positives from layout math
    // that produces e.g. 1080.0001 on a 1080-px viewport.
    let eps = 0.5;

    let x_over = bbox.x < -eps || right > vw + eps;
    let y_over = bbox.y < -eps || bottom > vh + eps;

    if !x_over && !y_over {
        return;
    }
    let axis = match (x_over, y_over) {
        (true, true) => Axis::Both,
        (true, false) => Axis::X,
        (false, true) => Axis::Y,
        (false, false) => return,
    };
    out.push(GeometryViolation {
        view_index: vi,
        scene_index: si,
        path: path.to_string(),
        component: component_kind(component).to_string(),
        axis,
        kind: ViolationKind::ViewportOverflow,
        bbox: *bbox,
        viewport,
        hint: hint_for_viewport(component, axis, bbox, viewport),
    });
}

fn hint_for_viewport(component: &Component, axis: Axis, bbox: &BBox, vp: (u32, u32)) -> String {
    let vw = vp.0 as f32;
    let vh = vp.1 as f32;
    match component_kind(component) {
        "text" | "rich_text" | "gradient_text" | "caption" => {
            "set wrap: true on the text or reduce font_size".to_string()
        }
        "counter" => format!(
            "card width must be ≥ {:.0}px (counter natural width)",
            bbox.w
        ),
        _ => match axis {
            Axis::X => format!(
                "shift x to fit [0..{:.0}], current right edge is {:.0}",
                vw,
                bbox.x + bbox.w
            ),
            Axis::Y => format!(
                "shift y to fit [0..{:.0}], current bottom edge is {:.0}",
                vh,
                bbox.y + bbox.h
            ),
            Axis::Both => "reposition the component to stay inside the viewport".to_string(),
        },
    }
}

fn check_unwrappable_text(
    component: &Component,
    path: &str,
    bbox: &BBox,
    viewport: (u32, u32),
    vi: usize,
    si: usize,
    out: &mut Vec<GeometryViolation>,
) {
    // Only meaningful for components whose intrinsic natural width can exceed
    // the allocated width when wrap is disabled. (Text only — other components
    // either wrap or use fixed dimensions.)
    if let Component::Text(t) = component {
        let nowrap = matches!(
            t.style.white_space,
            Some(
                rustmotion::core::css::style::WhiteSpace::Nowrap
                    | rustmotion::core::css::style::WhiteSpace::Pre
            )
        );
        if !nowrap {
            return;
        }
        // Measure the text at its natural (unbounded) width via the same
        // cosmic-text–backed intrinsic the layout engine uses.
        let intrinsic = TextIntrinsic::from_text(t);
        let (natural_w, _) = intrinsic.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        if natural_w > bbox.w + 0.5 {
            out.push(GeometryViolation {
                view_index: vi,
                scene_index: si,
                path: path.to_string(),
                component: "text".to_string(),
                axis: Axis::X,
                kind: ViolationKind::UnwrappableTextOverflow,
                bbox: *bbox,
                viewport,
                hint: format!(
                    "text natural width is {:.0}px but only {:.0}px available — set wrap: true or reduce font_size",
                    natural_w, bbox.w
                ),
            });
        }
    }
}

fn check_auto_scroll(
    component: &Component,
    path: &str,
    bbox: &BBox,
    viewport: (u32, u32),
    vi: usize,
    si: usize,
    out: &mut Vec<GeometryViolation>,
) {
    match component {
        Component::Codeblock(cb) if !cb.auto_scroll => {
            let font_size = cb.style.font_size_px_or(14.0);
            let actual_line_height = cb.style.line_height_for(font_size);
            let line_count = cb.code.lines().count().max(1) as f32;
            let chrome_h = if cb.chrome.as_ref().is_some_and(|c| c.enabled) {
                36.0
            } else {
                0.0
            };
            let pad = 32.0; // ~16 top + 16 bottom default
            let natural_h = chrome_h + pad + line_count * actual_line_height;
            if natural_h > bbox.h + 0.5 {
                out.push(GeometryViolation {
                    view_index: vi,
                    scene_index: si,
                    path: path.to_string(),
                    component: "codeblock".to_string(),
                    axis: Axis::Y,
                    kind: ViolationKind::AutoScrollDisabledOverflow,
                    bbox: *bbox,
                    viewport,
                    hint: format!(
                        "codeblock content needs ~{:.0}px but box is {:.0}px — enable auto_scroll or shorten code",
                        natural_h, bbox.h
                    ),
                });
            }
        }
        Component::Terminal(t) if !t.auto_scroll => {
            let font_size = t.style.font_size_px_or(16.0);
            let actual_line_height = t.style.line_height_for(font_size);
            let chrome_h = if t.show_chrome { 36.0 } else { 0.0 };
            let pad = 32.0;
            let natural_h = chrome_h + pad + t.lines.len() as f32 * actual_line_height;
            if natural_h > bbox.h + 0.5 {
                out.push(GeometryViolation {
                    view_index: vi,
                    scene_index: si,
                    path: path.to_string(),
                    component: "terminal".to_string(),
                    axis: Axis::Y,
                    kind: ViolationKind::AutoScrollDisabledOverflow,
                    bbox: *bbox,
                    viewport,
                    hint: format!(
                        "terminal content needs ~{:.0}px but box is {:.0}px — enable auto_scroll or remove lines",
                        natural_h, bbox.h
                    ),
                });
            }
        }
        _ => {}
    }
}

fn component_kind(c: &Component) -> &'static str {
    match c {
        Component::Text(_) => "text",
        Component::Shape(_) => "shape",
        Component::Image(_) => "image",
        Component::Icon(_) => "icon",
        Component::Svg(_) => "svg",
        Component::Video(_) => "video",
        Component::Gif(_) => "gif",
        Component::Counter(_) => "counter",
        Component::Cursor(_) => "cursor",
        Component::Caption(_) => "caption",
        Component::Codeblock(_) => "codeblock",
        Component::Avatar(_) => "avatar",
        Component::AvatarGroup(_) => "avatar_group",
        Component::Arrow(_) => "arrow",
        Component::Connector(_) => "connector",
        Component::Badge(_) => "badge",
        Component::Callout(_) => "callout",
        Component::Chart(_) => "chart",
        Component::Comparison(_) => "comparison",
        Component::Countdown(_) => "countdown",
        Component::Divider(_) => "divider",
        Component::DotMap(_) => "dot_map",
        Component::Gauge(_) => "gauge",
        Component::GradientText(_) => "gradient_text",
        Component::Heatmap(_) => "heatmap",
        Component::Kbd(_) => "kbd",
        Component::Line(_) => "line",
        Component::List(_) => "list",
        Component::Lottie(_) => "lottie",
        Component::Marquee(_) => "marquee",
        Component::Mockup(_) => "mockup",
        Component::Notification(_) => "notification",
        Component::Particle(_) => "particle",
        Component::PillNav(_) => "pill_nav",
        Component::Progress(_) => "progress",
        Component::QrCode(_) => "qrcode",
        Component::Rating(_) => "rating",
        Component::Skeleton(_) => "skeleton",
        Component::Slider(_) => "slider",
        Component::Sparkline(_) => "sparkline",
        Component::Stat(_) => "stat",
        Component::Stepper(_) => "stepper",
        Component::Switch(_) => "switch",
        Component::RichText(_) => "rich_text",
        Component::Table(_) => "table",
        Component::TagCloud(_) => "tag_cloud",
        Component::Terminal(_) => "terminal",
        Component::Timeline(_) => "timeline",
        Component::Tooltip(_) => "tooltip",
        Component::Treemap(_) => "treemap",
        Component::Positioned(_) => "positioned",
        Component::Flex(_) => "flex",
        Component::Grid(_) => "grid",
        Component::Card(_) => "card",
        Component::Container(_) => "container",
    }
}

// ─── Animated overflow sampling (--strict-anim) ─────────────────────────────

/// Number of timestamps sampled per scene when `strict_anim` is enabled. We
/// sample at quarters of the scene duration plus the boundaries, which is
/// enough to catch the worst case for typical entrance/exit presets without
/// blowing up validation time.
const ANIM_SAMPLES: &[f64] = &[0.0, 0.15, 0.3, 0.5, 0.7, 0.85, 1.0];

/// Walk every scene at multiple sampled times, apply the resolved animation
/// transform to each widget's bbox, and report viewport overflows. Only
/// emits `AnimatedTextOverflow` violations: the resting-layout checks live in
/// `validate_geometry`.
pub fn validate_geometry_animated(scenario: &ResolvedScenario) -> Vec<GeometryViolation> {
    let mut violations = Vec::new();
    let mut seen: std::collections::HashSet<(usize, usize, String)> =
        std::collections::HashSet::new();
    for (vi, view) in scenario.views.iter().enumerate() {
        for (si, scene) in view.scenes.iter().enumerate() {
            let children = render::deserialize_children(scene);
            let viewport = (scenario.video.width, scenario.video.height);
            let viewport_f = (viewport.0 as f32, viewport.1 as f32);

            // Use the resting layout as the base bbox. Animation transforms
            // (translate/scale/wiggle/orbit) are applied analytically per
            // sample — they don't reflow taffy.
            let root_css = render::root_style(scene.layout.as_ref());
            let built = build_scene_from_refs(children.iter(), viewport_f, root_css, None);
            let layouts = run_layout(&built.root, viewport_f, &ConversionContext::default());

            let path_root = format!("views[{}].scenes[{}]", vi, si);
            let scene_duration = scene.duration;

            for ratio in ANIM_SAMPLES {
                let time = scene_duration * *ratio;
                walk_anim(
                    &children,
                    &built.root.children,
                    &layouts,
                    viewport,
                    vi,
                    si,
                    &path_root,
                    time,
                    scene_duration,
                    &mut seen,
                    &mut violations,
                );
            }
        }
    }
    violations
}

#[allow(clippy::too_many_arguments)]
fn walk_anim(
    children: &[ChildComponent],
    boxes: &[BoxNode],
    layouts: &LayoutResult,
    viewport: (u32, u32),
    vi: usize,
    si: usize,
    path: &str,
    time: f64,
    scene_duration: f64,
    seen: &mut std::collections::HashSet<(usize, usize, String)>,
    out: &mut Vec<GeometryViolation>,
) {
    for (i, (child, box_node)) in children.iter().zip(boxes.iter()).enumerate() {
        let child_path = format!("{}.children[{}]", path, i);
        let layout = match layouts.get(box_node.id) {
            Some(l) => l,
            None => continue,
        };
        let base_bbox = bbox_of(layout);

        if !is_exempted(&child.component) && layout.width > 0.5 && layout.height > 0.5 {
            let props = resolve_props_for_validation(&child.component, time, scene_duration);
            if let Some(transformed) = transform_bbox(&base_bbox, &props) {
                let vw = viewport.0 as f32;
                let vh = viewport.1 as f32;
                let eps = 0.5;
                let right = transformed.x + transformed.w;
                let bottom = transformed.y + transformed.h;
                let x_over = transformed.x < -eps || right > vw + eps;
                let y_over = transformed.y < -eps || bottom > vh + eps;
                if x_over || y_over {
                    let axis = match (x_over, y_over) {
                        (true, true) => Axis::Both,
                        (true, false) => Axis::X,
                        (false, true) => Axis::Y,
                        _ => unreachable!(),
                    };
                    // Dedupe across samples: one violation per (view, scene, path).
                    let key = (vi, si, child_path.clone());
                    if seen.insert(key) {
                        let component_name = component_kind(&child.component).to_string();
                        out.push(GeometryViolation {
                            view_index: vi,
                            scene_index: si,
                            path: child_path.clone(),
                            component: component_name,
                            axis,
                            kind: ViolationKind::AnimatedTextOverflow,
                            bbox: transformed,
                            viewport,
                            hint: hint_for_animated(&child.component, &props, time, scene_duration),
                        });
                    }
                }
            }
        }

        if let Some(grandchildren) = container_children(&child.component) {
            walk_anim(
                grandchildren,
                &box_node.children,
                layouts,
                viewport,
                vi,
                si,
                &child_path,
                time,
                scene_duration,
                seen,
                out,
            );
        }
    }
}

/// Resolve animation properties for a component at a specific time. This is a
/// validation-flavored copy of the renderer's logic in
/// `engine::render::render_component`: we deliberately ignore motion paths,
/// timeline steps, char animations, and rotation, which are too renderer-bound
/// to emulate cheaply. The returned props cover the cases that can shift a
/// widget's bbox out of the viewport (translate, scale, wiggle, orbit).
fn resolve_props_for_validation(
    component: &Component,
    time: f64,
    scene_duration: f64,
) -> AnimatedProperties {
    let effects = component
        .as_animatable()
        .map(|a| a.animation_effects())
        .unwrap_or(&[]);
    if effects.is_empty() {
        return AnimatedProperties::default();
    }
    let extracted = extract_effects(effects);

    // Apply timing offset (start_at / end_at). We mirror the renderer rule:
    // animations only run inside [start_at, end_at]; outside, props default.
    let anim_time = if let Some(timed) = component.as_timed() {
        let (start_at, end_at) = timed.timing();
        if let Some(start) = start_at {
            if time < start {
                return AnimatedProperties::default();
            }
        }
        if let Some(end) = end_at {
            if time > end {
                return AnimatedProperties::default();
            }
        }
        let base = if let Some(start) = start_at {
            time - start
        } else {
            time
        };
        base.max(0.0)
    } else {
        time
    };

    let mut props = AnimatedProperties::default();
    for (preset, preset_config) in &extracted.presets {
        let p = resolve_animations(
            &[],
            Some(preset),
            Some(preset_config),
            anim_time,
            scene_duration,
        );
        props.merge(&p);
    }
    if !extracted.keyframes.is_empty() {
        let kf: Vec<_> = extracted.keyframes.iter().copied().cloned().collect();
        let kp = resolve_animations(&kf, None, None, anim_time, scene_duration);
        props.merge(&kp);
    }
    if !extracted.wiggles.is_empty() {
        let wiggles: Vec<_> = extracted.wiggles.into_iter().cloned().collect();
        apply_wiggles(&mut props, &wiggles, time);
    }
    if !extracted.orbits.is_empty() {
        let orbits: Vec<_> = extracted.orbits.into_iter().cloned().collect();
        apply_orbits(&mut props, &orbits, time);
    }
    props
}

/// Apply the canvas transforms the renderer applies (translate then scale
/// around the bbox center) to a base bbox. Returns `None` if the resulting
/// box is degenerate (fully transparent / zero size).
fn transform_bbox(base: &BBox, props: &AnimatedProperties) -> Option<BBox> {
    if props.opacity <= 0.001 {
        return None;
    }
    // Conservative: also account for char animations that overshoot the box
    // (e.g. char_scale_in defaults to 1.08). One extra factor on each axis.
    let char_overshoot = props
        .char_animation
        .as_ref()
        .map(|c| 1.0 + c.overshoot.max(0.0))
        .unwrap_or(1.0);
    let sx = props.scale_x.abs().max(0.001) * char_overshoot;
    let sy = props.scale_y.abs().max(0.001) * char_overshoot;
    let center_x = base.x + base.w / 2.0 + props.translate_x;
    let center_y = base.y + base.h / 2.0 + props.translate_y;
    let new_w = base.w * sx;
    let new_h = base.h * sy;
    Some(BBox {
        x: center_x - new_w / 2.0,
        y: center_y - new_h / 2.0,
        w: new_w,
        h: new_h,
    })
}

fn hint_for_animated(
    component: &Component,
    props: &AnimatedProperties,
    time: f64,
    scene_duration: f64,
) -> String {
    let ratio = if scene_duration > 1e-6 {
        time / scene_duration
    } else {
        0.0
    };
    let base = format!(
        "at t={:.2}s ({:.0}% of scene), animation transforms (tx={:.0}, ty={:.0}, sx={:.2}, sy={:.2}) push the bbox out of the viewport",
        time,
        ratio * 100.0,
        props.translate_x,
        props.translate_y,
        props.scale_x,
        props.scale_y,
    );
    match component_kind(component) {
        "text" | "rich_text" | "gradient_text" | "caption" | "counter" => format!(
            "{} — reduce font_size, soften the preset (e.g. fade_in instead of slide_in_left), or add max_width",
            base
        ),
        _ => format!("{} — soften the preset or pull the resting position inward", base),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion::loader::load_scenario_from_source;

    fn parse(json: &str) -> rustmotion::schema::ResolvedScenario {
        load_scenario_from_source(None, Some(json)).expect("scenario parses")
    }

    #[test]
    fn clean_scenario_has_no_violations() {
        // 100×80 shape at (10, 10) in a 1920×1080 viewport — well inside.
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "size": { "width": 100, "height": 80 },
                    "x": 10, "y": 10,
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations.is_empty(),
            "expected clean, got: {:?}",
            violations
        );
    }

    #[test]
    fn shape_past_right_edge_triggers_x_overflow() {
        // A 400×100 shape positioned at x=1700 in a 1920-wide viewport spills
        // 180 px past the right edge.
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "shape",
                    "shape": "rect",
                    "style": { "width": "400px", "height": "100px" },
                    "position": "absolute",
                    "x": 1700, "y": 100,
                    "fill": "#ff0000"
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let viewport = violations
            .iter()
            .find(|v| v.kind == ViolationKind::ViewportOverflow);
        assert!(
            viewport.is_some(),
            "missing viewport violation in {:?}",
            violations
        );
        let v = viewport.unwrap();
        assert_eq!(
            v.axis,
            Axis::X,
            "expected X-axis overflow, got {:?}",
            v.axis
        );
        assert_eq!(v.component, "shape");
        assert!(
            v.path.contains("children[0]"),
            "path missing index: {}",
            v.path
        );
    }

    #[test]
    fn unwrappable_text_in_narrow_card_is_flagged() {
        // A card 200 px wide with a 96 px font-size unwrapped text. Natural
        // width far exceeds 200, so we should get UnwrappableTextOverflow.
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "card",
                    "x": 100, "y": 100,
                    "style": { "width": "200px", "height": "200px", "background": "#222244" },
                    "children": [{
                        "type": "text",
                        "content": "this string is too long to fit",
                        "style": { "color": "#ffffff", "font-size": "96px", "white-space": "nowrap" }
                    }]
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::UnwrappableTextOverflow);
        assert!(
            v.is_some(),
            "missing UnwrappableTextOverflow in {:?}",
            violations
        );
        let v = v.unwrap();
        assert_eq!(v.component, "text");
        assert_eq!(v.axis, Axis::X);
    }

    #[test]
    fn marquee_is_exempted_from_overflow() {
        // A marquee that bleeds past the viewport: no violation should fire,
        // marquee is by-design designed to scroll content past edges.
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "marquee",
                    "content": "scrolling text that bleeds",
                    "x": 1900, "y": 100
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        assert!(
            violations.iter().all(|v| v.component != "marquee"),
            "marquee should be exempt, got: {:?}",
            violations
        );
    }

    #[test]
    fn auto_scroll_disabled_codeblock_overflows() {
        // 20 lines × 14 px × 1.5 line-height + chrome + padding ≈ 487 px.
        // Box height capped at 200 → AutoScrollDisabledOverflow.
        let json = r##"{
            "video": { "width": 1920, "height": 1080 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "codeblock",
                    "code": "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n13\n14\n15\n16\n17\n18\n19\n20",
                    "auto_scroll": false,
                    "size": { "width": 800, "height": 200 },
                    "x": 100, "y": 100
                }]
            }]
        }"##;
        let scenario = parse(json);
        let violations = validate_geometry(&scenario);
        let v = violations
            .iter()
            .find(|v| v.kind == ViolationKind::AutoScrollDisabledOverflow);
        assert!(
            v.is_some(),
            "missing AutoScrollDisabledOverflow in {:?}",
            violations
        );
        assert_eq!(v.unwrap().component, "codeblock");
    }
}

/// Render a violation for human consumption (multi-line, color-free).
pub fn format_violation(v: &GeometryViolation) -> String {
    let axis_str = match v.axis {
        Axis::X => "x",
        Axis::Y => "y",
        Axis::Both => "x+y",
    };
    let kind_str = match v.kind {
        ViolationKind::ViewportOverflow => "viewport overflow",
        ViolationKind::UnwrappableTextOverflow => "wrap=false but text too wide",
        ViolationKind::AutoScrollDisabledOverflow => "auto_scroll=false but content too tall",
        ViolationKind::AnimatedTextOverflow => "animation pushes content outside viewport",
    };
    format!(
        "ERROR: {} ({})\n  view: {}, scene: {}\n  path: {}\n  bbox: [{:.0}, {:.0}] -> [{:.0}, {:.0}]   (viewport: {}x{})\n  axis: {}\n  hint: {}",
        v.component,
        kind_str,
        v.view_index,
        v.scene_index,
        v.path,
        v.bbox.x,
        v.bbox.y,
        v.bbox.x + v.bbox.w,
        v.bbox.y + v.bbox.h,
        v.viewport.0,
        v.viewport.1,
        axis_str,
        v.hint,
    )
}
