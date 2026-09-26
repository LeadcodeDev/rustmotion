use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Paint, PaintStyle, Path, PathBuilder};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{paint_from_hex, parse_hex_color};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

use crate::cursor::{waypoint_offset, CursorPathEasing, CursorWaypoint};

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
            self.path.iter().map(|w| w.time).collect()
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
        for (i, (x, y)) in OUTLINE.iter().enumerate() {
            let p = (x * size, y * size);
            if i == 0 {
                path.move_to(p);
            } else {
                path.line_to(p);
            }
        }
        path.close();
        path.detach()
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
        let (dx, dy) = if self.path.is_empty() {
            (0.0, 0.0)
        } else {
            waypoint_offset(&self.path, ctx.time, self.click_duration, self.path_easing)
        };
        let click = self.click_progress(ctx.time);
        let (fill, outline) = self.colors();

        canvas.save();
        canvas.translate((dx, dy));

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

        let path = Self::arrow_path(self.size);
        let mut outline_paint = paint_from_hex(&outline);
        outline_paint.set_style(PaintStyle::Stroke);
        outline_paint.set_stroke_width((self.size * 0.07).max(1.0));
        outline_paint.set_stroke_join(skia_safe::PaintJoin::Round);
        outline_paint.set_anti_alias(true);

        let mut fill_paint = paint_from_hex(&fill);
        fill_paint.set_style(PaintStyle::Fill);
        fill_paint.set_anti_alias(true);

        canvas.draw_path(&path, &fill_paint);
        canvas.draw_path(&path, &outline_paint);

        canvas.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "path": [{ "time": 0.0, "x": 150.0, "y": 150.0 }]
        }));
        const W: i32 = 300;
        const H: i32 = 300;
        let background = skia_safe::Color::from_argb(255, 20, 20, 20);
        let mut surface = render(&p, W, H, 0.15, background);

        let radius = 0.3 * 1.25 * 120.0;
        let offset = radius * std::f32::consts::FRAC_1_SQRT_2;
        let (rx, ry) = (150.0 - offset, 150.0 - offset);
        let ring = pixel(&mut surface, W, H, rx, ry);
        assert!(
            ring.0 > 100 && ring.1 > 100 && ring.2 > 100,
            "the click ring on an outline pointer must default to the outline's white, not a hard-coded or transparent-derived colour, got {ring:?}"
        );
    }
}
