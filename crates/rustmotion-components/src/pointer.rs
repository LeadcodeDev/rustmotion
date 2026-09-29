use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Paint, PaintStyle, Path, PathBuilder};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{paint_from_hex, parse_hex_color};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

use crate::cursor::{CursorPathEasing, CursorWaypoint};

/// Colour scheme of the pointer, so a scene picks one word instead of two
/// hex values that have to stay in contrast with each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PointerTone {
    /// White arrow, dark outline — for dark frames.
    #[default]
    Light,
    /// Dark arrow, light outline — for light frames.
    Dark,
    /// Transparent fill, white outline — reads on top of any background,
    /// dark or light, without a filled shape competing with what it points at.
    Outline,
}

/// Which glyph the pointer draws. `click_glyph` can swap to a different one
/// only for the duration of a click, then it reverts to this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PointerGlyph {
    /// The classic pointer arrow, tip at the hotspot.
    #[default]
    Arrow,
    /// An open hand pointing with its index finger, fingertip at the hotspot.
    Hand,
    /// A closed fist, as if grabbing the point under the hotspot.
    Grab,
}

/// How loud the click ring is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ClickRing {
    /// A thin ring that stays close to the tip.
    Subtle,
    #[default]
    Standard,
    /// A thick ring travelling well past the tip.
    Bold,
    /// No ring — the arrow still nudges, nothing expands.
    None,
}

impl ClickRing {
    fn metrics(self) -> Option<(f32, f32)> {
        match self {
            Self::Subtle => Some((0.05, 0.55)),
            Self::Standard => Some((0.09, 0.85)),
            Self::Bold => Some((0.16, 1.25)),
            Self::None => None,
        }
    }
}

fn default_pointer_size() -> f32 {
    44.0
}

fn default_pointer_click_duration() -> f32 {
    0.45
}

/// A mouse pointer that travels a waypoint path and clicks along the way.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Pointer {
    /// Height of the arrow in px. The click ring scales with it.
    #[serde(default = "default_pointer_size")]
    pub size: f32,
    /// Which glyph is drawn. Defaults to the classic arrow.
    #[serde(default)]
    pub glyph: PointerGlyph,
    /// Glyph shown for the duration of a click, then back to `glyph`. Absent
    /// keeps `glyph` unchanged through the click — only the scale dip shows.
    #[serde(default)]
    pub click_glyph: Option<PointerGlyph>,
    /// Colour scheme. Overridden by `color` / `outline_color` when set.
    #[serde(default)]
    pub tone: PointerTone,
    /// Arrow fill (hex), overriding `tone`.
    #[serde(default)]
    pub color: Option<String>,
    /// Arrow outline (hex), overriding `tone`.
    #[serde(default)]
    pub outline_color: Option<String>,
    /// Click ring size. `none` removes it.
    #[serde(default)]
    pub click_ring: ClickRing,
    /// Ring colour (hex). Defaults to the arrow's fill.
    #[serde(default)]
    pub ring_color: Option<String>,
    /// Waypoints the pointer travels between, in scene-local seconds. Each
    /// `x`/`y` is relative to the component's own origin — place the
    /// component with `position: absolute` and read the waypoints as scene
    /// coordinates. The pointer clicks on arrival at each one.
    #[serde(default)]
    pub path: Vec<CursorWaypoint>,
    /// Extra click times (seconds), for a pointer that clicks without
    /// travelling. Ignored when `path` is set — the waypoints carry their
    /// own clicks.
    #[serde(default)]
    pub click_at: Vec<f64>,
    /// How long one click animation runs (seconds). Also how long the
    /// pointer pauses on a waypoint before setting off for the next.
    #[serde(default = "default_pointer_click_duration")]
    pub click_duration: f32,
    /// Easing between waypoints.
    #[serde(default)]
    pub path_easing: CursorPathEasing,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Pointer {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Pointer {
    fn click_times(&self) -> Vec<f64> {
        if self.path.is_empty() {
            self.click_at.clone()
        } else {
            self.path
                .iter()
                .filter(|w| w.click)
                .map(|w| w.time)
                .collect()
        }
    }

    fn click_progress(&self, time: f64) -> Option<f32> {
        if self.click_duration <= 0.0 {
            return None;
        }
        self.click_times()
            .into_iter()
            .rfind(|&t| time >= t && time < t + self.click_duration as f64)
            .map(|t| ((time - t) / self.click_duration as f64) as f32)
    }

    fn colors(&self) -> (String, String) {
        let (fill, outline) = match self.tone {
            PointerTone::Light => ("#FFFFFF", "#111827"),
            PointerTone::Dark => ("#111827", "#FFFFFF"),
            PointerTone::Outline => ("transparent", "#FFFFFF"),
        };
        (
            self.color.clone().unwrap_or_else(|| fill.to_string()),
            self.outline_color
                .clone()
                .unwrap_or_else(|| outline.to_string()),
        )
    }

    fn ring_fallback_color<'a>(&self, fill: &'a str, outline: &'a str) -> &'a str {
        match self.tone {
            PointerTone::Outline => outline,
            PointerTone::Light | PointerTone::Dark => fill,
        }
    }

    fn add_contour(path: &mut PathBuilder, points: &[(f32, f32)], size: f32) {
        for (i, (x, y)) in points.iter().enumerate() {
            let p = (x * size, y * size);
            if i == 0 {
                path.move_to(p);
            } else {
                path.line_to(p);
            }
        }
        path.close();
    }

    fn add_smooth_contour(path: &mut PathBuilder, points: &[(f32, f32)], size: f32) {
        let n = points.len();
        if n < 3 {
            Self::add_contour(path, points, size);
            return;
        }
        let at = |i: usize| (points[i % n].0 * size, points[i % n].1 * size);
        let midpoint = |a: (f32, f32), b: (f32, f32)| ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);

        path.move_to(midpoint(at(0), at(1)));
        for i in 1..=n {
            let control = at(i);
            let end = midpoint(at(i), at(i + 1));
            path.quad_to(control, end);
        }
        path.close();
    }

    fn arrow_path(size: f32) -> Path {
        const OUTLINE: [(f32, f32); 7] = [
            (0.0, 0.0),
            (0.0, 0.72),
            (0.19, 0.56),
            (0.30, 0.84),
            (0.43, 0.78),
            (0.32, 0.51),
            (0.54, 0.51),
        ];
        let mut path = PathBuilder::new();
        Self::add_contour(&mut path, &OUTLINE, size);
        path.detach()
    }

    const HAND_OUTLINE: [(f32, f32); 16] = [
        (0.00, 0.00),
        (0.22, 0.00),
        (0.26, 0.34),
        (0.38, 0.30),
        (0.48, 0.36),
        (0.58, 0.34),
        (0.66, 0.46),
        (0.68, 0.64),
        (0.58, 0.86),
        (0.36, 0.96),
        (0.16, 0.90),
        (0.06, 0.72),
        (0.00, 0.58),
        (0.03, 0.46),
        (0.12, 0.42),
        (0.04, 0.34),
    ];

    const FIST_OUTLINE: [(f32, f32); 15] = [
        (0.18, 0.22),
        (0.30, 0.12),
        (0.40, 0.20),
        (0.50, 0.14),
        (0.60, 0.24),
        (0.66, 0.38),
        (0.66, 0.60),
        (0.56, 0.80),
        (0.38, 0.90),
        (0.20, 0.84),
        (0.10, 0.68),
        (0.02, 0.58),
        (0.04, 0.44),
        (0.14, 0.40),
        (0.12, 0.30),
    ];

    fn hand_path(size: f32) -> Path {
        let mut path = PathBuilder::new();
        Self::add_smooth_contour(&mut path, &Self::HAND_OUTLINE, size);
        path.detach()
    }

    fn grab_path(size: f32) -> Path {
        let mut path = PathBuilder::new();
        Self::add_smooth_contour(&mut path, &Self::FIST_OUTLINE, size);
        path.detach()
    }

    fn glyph_path(glyph: PointerGlyph, size: f32) -> Path {
        match glyph {
            PointerGlyph::Arrow => Self::arrow_path(size),
            PointerGlyph::Hand => Self::hand_path(size),
            PointerGlyph::Grab => Self::grab_path(size),
        }
    }

    fn unfilled_contrast_color(&self, outline: &str) -> Option<&'static str> {
        if self.tone != PointerTone::Outline || self.outline_color.is_some() {
            return None;
        }
        let (_, _, _, alpha) = parse_hex_color(outline);
        if alpha == 0 {
            return None;
        }
        Some("#111827")
    }

    fn active_glyph(&self, click: Option<f32>) -> PointerGlyph {
        match click {
            Some(_) => self.click_glyph.unwrap_or(self.glyph),
            None => self.glyph,
        }
    }
}

impl Painter for Pointer {
    fn paint_content(
        &self,
        canvas: &Canvas,
        _layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let click = self.click_progress(ctx.time);
        let (fill, outline) = self.colors();

        canvas.save();

        if let (Some(p), Some((stroke_f, travel_f))) = (click, self.click_ring.metrics()) {
            let ring_fallback = self.ring_fallback_color(&fill, &outline);
            let (r, g, b, _) = parse_hex_color(self.ring_color.as_deref().unwrap_or(ring_fallback));
            let alpha = ((1.0 - p) * 200.0) as u8;
            if alpha > 0 {
                let mut ring = Paint::default();
                ring.set_style(PaintStyle::Stroke);
                ring.set_anti_alias(true);
                ring.set_stroke_width(stroke_f * self.size);
                ring.set_color(skia_safe::Color::from_argb(alpha, r, g, b));
                canvas.draw_circle((0.0, 0.0), p * travel_f * self.size, &ring);
            }
        }

        if let Some(p) = click {
            let scale = if p < 0.35 {
                1.0 - 0.12 * (p / 0.35)
            } else {
                0.88 + 0.12 * ((p - 0.35) / 0.65)
            };
            canvas.scale((scale, scale));
        }

        let path = Self::glyph_path(self.active_glyph(click), self.size);
        let outline_width = (self.size * 0.07).max(1.0);
        let mut outline_paint = paint_from_hex(&outline);
        outline_paint.set_style(PaintStyle::Stroke);
        outline_paint.set_stroke_width(outline_width);
        outline_paint.set_stroke_join(skia_safe::PaintJoin::Round);
        outline_paint.set_anti_alias(true);

        let mut fill_paint = paint_from_hex(&fill);
        fill_paint.set_style(PaintStyle::Fill);
        fill_paint.set_anti_alias(true);

        if let Some(contrast) = self.unfilled_contrast_color(&outline) {
            let mut contrast_paint = paint_from_hex(contrast);
            contrast_paint.set_style(PaintStyle::Stroke);
            contrast_paint.set_stroke_width(outline_width * 1.5);
            contrast_paint.set_stroke_join(skia_safe::PaintJoin::Round);
            contrast_paint.set_anti_alias(true);
            canvas.draw_path(&path, &contrast_paint);
        }

        canvas.draw_path(&path, &fill_paint);
        canvas.draw_path(&path, &outline_paint);

        canvas.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor::waypoint_offset;

    fn pointer(json: serde_json::Value) -> Pointer {
        serde_json::from_value(json).expect("pointer fixture")
    }

    #[test]
    fn a_pointer_with_no_path_sits_at_its_own_origin() {
        let p = pointer(serde_json::json!({}));
        assert!(p.path.is_empty());
        assert_eq!(p.click_progress(0.0), None, "no clicks were asked for");
    }

    #[test]
    fn clicks_come_from_the_waypoints_when_a_path_is_given() {
        let p = pointer(serde_json::json!({
            "click_at": [9.0],
            "path": [
                { "time": 0.0, "x": 0.0, "y": 0.0 },
                { "time": 1.0, "x": 200.0, "y": 100.0 }
            ]
        }));
        assert_eq!(p.click_times(), vec![0.0, 1.0]);
        assert!(
            p.click_progress(9.1).is_none(),
            "a `click_at` entry must be ignored once the pointer has a path"
        );
    }

    #[test]
    fn a_click_runs_for_exactly_its_duration() {
        let p = pointer(serde_json::json!({
            "click_at": [1.0],
            "click_duration": 0.5
        }));
        assert_eq!(p.click_progress(0.9), None, "before the click");
        assert_eq!(p.click_progress(1.0), Some(0.0), "at the click");
        assert!(
            matches!(p.click_progress(1.25), Some(t) if (t - 0.5).abs() < 1e-5),
            "halfway through"
        );
        assert_eq!(p.click_progress(1.5), None, "the instant it ends");
    }

    #[test]
    fn overlapping_clicks_resolve_to_the_most_recent() {
        let p = pointer(serde_json::json!({
            "click_at": [1.0, 1.2],
            "click_duration": 0.5
        }));
        let at = p.click_progress(1.3).expect("a click is running at 1.3");
        assert!(
            (at - 0.2).abs() < 1e-5,
            "expected 0.1s into the second click (0.2 of its duration), got {at}"
        );
    }

    #[test]
    fn the_pointer_holds_its_first_waypoint_before_the_path_starts() {
        let p = pointer(serde_json::json!({
            "path": [
                { "time": 1.0, "x": 100.0, "y": 50.0 },
                { "time": 2.0, "x": 400.0, "y": 50.0 }
            ]
        }));
        assert_eq!(
            waypoint_offset(&p.path, 0.0, p.click_duration, p.path_easing),
            (100.0, 50.0),
            "before the first waypoint's time the pointer waits there, it does not fly in"
        );
    }

    #[test]
    fn none_removes_the_ring_without_removing_the_click() {
        let p = pointer(serde_json::json!({
            "click_at": [1.0],
            "click_ring": "none"
        }));
        assert!(p.click_ring.metrics().is_none(), "no ring to draw");
        assert!(
            p.click_progress(1.1).is_some(),
            "the click itself still runs — the arrow still dips"
        );
    }

    fn paint_ctx(time: f64, video_width: u32, video_height: u32) -> PaintCtx {
        PaintCtx {
            time,
            scenario_time: time,
            scene_duration: 1.0,
            frame_index: 0,
            fps: 30,
            video_width,
            video_height,
            stagger_offset: 0.0,
        }
    }

    fn render(
        p: &Pointer,
        w: i32,
        h: i32,
        time: f64,
        background: skia_safe::Color,
    ) -> skia_safe::Surface {
        let mut surface = skia_safe::surfaces::raster_n32_premul((w, h)).expect("raster surface");
        {
            let canvas = surface.canvas();
            canvas.clear(background);
            p.paint_content(
                canvas,
                &BoxLayout::default(),
                &AnimatedProperties::default(),
                &paint_ctx(time, w as u32, h as u32),
            );
        }
        surface
    }

    fn pixel(surface: &mut skia_safe::Surface, w: i32, h: i32, x: f32, y: f32) -> (u8, u8, u8, u8) {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (w, h),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (w * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        let ix = x.round() as i32;
        let iy = y.round() as i32;
        let idx = ((iy * w + ix) * 4) as usize;
        (buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3])
    }

    const PROBE_SIZE: f32 = 200.0;

    fn deep_interior_point() -> (f32, f32) {
        (PROBE_SIZE * 0.1946, PROBE_SIZE * 0.4390)
    }

    fn left_edge_point() -> (f32, f32) {
        let stroke_width = (PROBE_SIZE * 0.07f32).max(1.0);
        (stroke_width * 0.3, PROBE_SIZE * 0.36)
    }

    #[test]
    fn deep_interior_point_is_well_clear_of_the_outline_stroke() {
        let path = Pointer::arrow_path(PROBE_SIZE);
        let margin = (PROBE_SIZE * 0.07).max(1.0) + 3.0;
        let (cx, cy) = deep_interior_point();
        assert!(
            path.contains((cx, cy)),
            "probe point must be inside the arrow"
        );
        for (dx, dy) in [(-margin, 0.0), (margin, 0.0), (0.0, -margin), (0.0, margin)] {
            assert!(
                path.contains((cx + dx, cy + dy)),
                "probe point at ({cx}, {cy}) is too close to an edge in direction ({dx}, {dy})"
            );
        }
    }

    #[test]
    fn outline_tone_leaves_the_interior_transparent_and_paints_a_pointer_coloured_edge() {
        let p = pointer(serde_json::json!({ "tone": "outline", "size": PROBE_SIZE }));
        let background = skia_safe::Color::from_argb(255, 0, 128, 0);
        const W: i32 = 300;
        const H: i32 = 300;
        let mut surface = render(&p, W, H, 0.0, background);

        let (ix, iy) = deep_interior_point();
        let interior = pixel(&mut surface, W, H, ix, iy);
        assert_eq!(
            interior,
            (0, 128, 0, 255),
            "an outline pointer must let the background show through its interior, got {interior:?}"
        );

        let (ex, ey) = left_edge_point();
        let edge = pixel(&mut surface, W, H, ex, ey);
        assert!(
            edge.0 > 200 && edge.1 > 200 && edge.2 > 200 && edge.3 == 255,
            "the outline itself must still paint a solid, pointer-coloured edge, got {edge:?}"
        );
    }

    #[test]
    fn filled_tones_still_paint_pointer_colour_in_both_interior_and_edge() {
        let background = skia_safe::Color::from_argb(255, 0, 128, 0);
        const W: i32 = 300;
        const H: i32 = 300;
        let (ix, iy) = deep_interior_point();
        let (ex, ey) = left_edge_point();

        let light = pointer(serde_json::json!({ "tone": "light", "size": PROBE_SIZE }));
        let mut light_surface = render(&light, W, H, 0.0, background);
        assert_eq!(
            pixel(&mut light_surface, W, H, ix, iy),
            (255, 255, 255, 255),
            "light tone interior must stay pinned to its white fill"
        );
        assert_eq!(
            pixel(&mut light_surface, W, H, ex, ey),
            (0x11, 0x18, 0x27, 255),
            "light tone edge must stay pinned to its dark outline"
        );

        let dark = pointer(serde_json::json!({ "tone": "dark", "size": PROBE_SIZE }));
        let mut dark_surface = render(&dark, W, H, 0.0, background);
        assert_eq!(
            pixel(&mut dark_surface, W, H, ix, iy),
            (0x11, 0x18, 0x27, 255),
            "dark tone interior must stay pinned to its dark fill"
        );
        assert_eq!(
            pixel(&mut dark_surface, W, H, ex, ey),
            (255, 255, 255, 255),
            "dark tone edge must stay pinned to its white outline"
        );
    }

    #[test]
    fn outline_tone_keeps_the_click_ring_visible_and_matched_to_the_white_outline() {
        let p = pointer(serde_json::json!({
            "tone": "outline",
            "size": 120.0,
            "click_ring": "bold",
            "click_duration": 0.5,
            "click_at": [0.0]
        }));
        const W: i32 = 300;
        const H: i32 = 300;
        let background = skia_safe::Color::from_argb(255, 20, 20, 20);
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        {
            let canvas = surface.canvas();
            canvas.clear(background);
            canvas.translate((150.0, 150.0));
            p.paint_content(
                canvas,
                &BoxLayout::default(),
                &AnimatedProperties::default(),
                &paint_ctx(0.15, W as u32, H as u32),
            );
        }

        let radius = 0.3 * 1.25 * 120.0;
        let offset = radius * std::f32::consts::FRAC_1_SQRT_2;
        let (rx, ry) = (150.0 - offset, 150.0 - offset);
        let ring = pixel(&mut surface, W, H, rx, ry);
        assert!(
            ring.0 > 100 && ring.1 > 100 && ring.2 > 100,
            "the click ring on an outline pointer must default to the outline's white, not a hard-coded or transparent-derived colour, got {ring:?}"
        );
    }

    fn finger_probe(size: f32) -> (f32, f32) {
        (0.08 * size, 0.05 * size)
    }

    #[test]
    fn a_waypoint_can_be_passed_through_without_clicking() {
        let p = pointer(serde_json::json!({
            "size": 120,
            "click_duration": 0.4,
            "path": [
                { "time": 0.0, "x": 0.0, "y": 0.0 },
                { "time": 1.0, "x": 200.0, "y": 0.0, "click": false },
                { "time": 2.0, "x": 200.0, "y": 200.0 }
            ]
        }));
        assert_eq!(
            p.click_times(),
            vec![0.0, 2.0],
            "a pointer must be able to travel through an intermediate point without clicking \
             on arrival — it clicked at every waypoint, with no way to opt out"
        );
    }

    #[test]
    fn a_waypoint_clicks_by_default_so_an_existing_scenario_is_unchanged() {
        let p = pointer(serde_json::json!({
            "size": 120,
            "path": [
                { "time": 0.0, "x": 0.0, "y": 0.0 },
                { "time": 1.0, "x": 200.0, "y": 0.0 }
            ]
        }));
        assert_eq!(p.click_times(), vec![0.0, 1.0]);
    }

    #[test]
    fn opting_every_waypoint_out_leaves_a_pointer_that_travels_and_never_clicks() {
        let p = pointer(serde_json::json!({
            "size": 120,
            "click_at": [3.0],
            "path": [
                { "time": 0.0, "x": 0.0, "y": 0.0, "click": false },
                { "time": 1.0, "x": 200.0, "y": 0.0, "click": false }
            ]
        }));
        assert!(
            p.click_times().is_empty(),
            "click_at stays ignored while a path is present, which is what \
             clicks_come_from_the_waypoints_when_a_path_is_given pins; a pointer that should \
             click somewhere says so on the waypoint"
        );
    }

    #[test]
    fn an_outline_pointer_carries_a_dark_contour_so_it_reads_on_a_light_frame() {
        let p = pointer(serde_json::json!({ "size": 200, "tone": "outline" }));
        const W: i32 = 300;
        const H: i32 = 300;
        let mut frame = render(
            &p,
            W,
            H,
            0.0,
            skia_safe::Color::from_argb(255, 255, 255, 255),
        );

        let mut darkest = 255u8;
        for y in 0..H {
            for x in 0..W {
                let (r, g, b, a) = pixel(&mut frame, W, H, x as f32, y as f32);
                if a > 200 {
                    darkest = darkest.min(r.max(g).max(b));
                }
            }
        }
        assert!(
            darkest < 100,
            "tone: outline is a white stroke on a transparent fill, so on a white frame it was \
             invisible while its own documentation promised it reads on any background; \
             darkest painted channel was {darkest}"
        );
    }

    #[test]
    fn finger_probe_is_inside_the_open_hand_but_outside_the_closed_fist() {
        const SIZE: f32 = 200.0;
        let (fx, fy) = finger_probe(SIZE);
        assert!(
            Pointer::hand_path(SIZE).contains((fx, fy)),
            "probe point must sit on the extended finger of the open hand"
        );
        assert!(
            !Pointer::grab_path(SIZE).contains((fx, fy)),
            "probe point must fall outside the fist alone, or it cannot prove the finger retracted"
        );
    }

    #[test]
    fn a_default_pointer_still_draws_the_classic_arrow() {
        let p = pointer(serde_json::json!({}));
        assert_eq!(
            p.glyph,
            PointerGlyph::Arrow,
            "arrow stays the default glyph"
        );
        assert_eq!(
            p.active_glyph(None),
            PointerGlyph::Arrow,
            "no glyph/click_glyph configured must resolve to the classic arrow"
        );
    }

    #[test]
    fn hand_glyph_closes_into_a_grab_during_a_click_and_reopens_after() {
        let p = pointer(serde_json::json!({
            "glyph": "hand",
            "click_glyph": "grab",
            "tone": "light",
            "size": SIZE_FOR_HAND_TEST,
            "click_at": [1.0],
            "click_duration": 0.5
        }));
        const W: i32 = 300;
        const H: i32 = 300;
        let background = skia_safe::Color::from_argb(255, 0, 128, 0);
        let (fx, fy) = finger_probe(SIZE_FOR_HAND_TEST);

        let mut before_click = render(&p, W, H, 0.0, background);
        let open = pixel(&mut before_click, W, H, fx, fy);
        assert_eq!(
            open,
            (255, 255, 255, 255),
            "before any click the open hand must paint its extended finger (white fill), got {open:?}"
        );

        let mut mid_click = render(&p, W, H, 1.0, background);
        let closed = pixel(&mut mid_click, W, H, fx, fy);
        assert_eq!(
            closed,
            (0, 128, 0, 255),
            "at the moment of the click the finger must have retracted into the fist, \
             leaving the background showing through at the same point, got {closed:?}"
        );

        let mut after_click = render(&p, W, H, 1.6, background);
        let reopened = pixel(&mut after_click, W, H, fx, fy);
        assert_eq!(
            reopened, open,
            "once the click finishes the hand must reopen to exactly its resting pose"
        );
    }

    const SIZE_FOR_HAND_TEST: f32 = 200.0;

    #[test]
    fn the_open_hand_pose_is_stable_when_no_click_is_happening() {
        let p = pointer(serde_json::json!({
            "glyph": "hand",
            "click_glyph": "grab",
            "tone": "light",
            "size": SIZE_FOR_HAND_TEST,
            "click_at": [1.0],
            "click_duration": 0.5
        }));
        const W: i32 = 300;
        const H: i32 = 300;
        let background = skia_safe::Color::from_argb(255, 0, 128, 0);
        let (fx, fy) = finger_probe(SIZE_FOR_HAND_TEST);

        let mut at_zero = render(&p, W, H, 0.0, background);
        let mut long_before_the_click = render(&p, W, H, 0.4, background);
        let mut long_after_the_click = render(&p, W, H, 3.0, background);

        let a = pixel(&mut at_zero, W, H, fx, fy);
        let b = pixel(&mut long_before_the_click, W, H, fx, fy);
        let c = pixel(&mut long_after_the_click, W, H, fx, fy);
        assert_eq!(
            a, b,
            "resting hand pose must not drift with time before a click"
        );
        assert_eq!(
            a, c,
            "resting hand pose must not drift with time after a click"
        );
    }
}
