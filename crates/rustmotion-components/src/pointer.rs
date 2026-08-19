//! A simulated mouse pointer — the arrow, plus the ring that pulses out of
//! its tip when it clicks.
//!
//! Not to be confused with [`crate::cursor::Cursor`], which is a *text* caret
//! (a blinking bar). Its `cursor_style: "pointer"` field has never drawn
//! anything but that bar. Product walkthroughs and agent demos need the other
//! thing: an arrow that travels to a control and visibly clicks it.
//!
//! Waypoint choreography — hold, glide, pause on the click — is shared with
//! `cursor` via [`crate::cursor::waypoint_offset`], so the two stay in step.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Paint, PaintStyle, Path};

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
    /// `(stroke width, travel)` as multiples of the pointer's `size`.
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

    /// The click running at `time`, as progress 0..1, if any.
    fn click_progress(&self, time: f64) -> Option<f32> {
        if self.click_duration <= 0.0 {
            return None;
        }
        self.click_times()
            .into_iter()
            // The *last* qualifying click, so overlapping clicks resolve to
            // the most recent rather than to whichever happens to be first.
            .rfind(|&t| time >= t && time < t + self.click_duration as f64)
            .map(|t| ((time - t) / self.click_duration as f64) as f32)
    }

    fn colors(&self) -> (String, String) {
        let (fill, outline) = match self.tone {
            PointerTone::Light => ("#FFFFFF", "#111827"),
            PointerTone::Dark => ("#111827", "#FFFFFF"),
        };
        (
            self.color.clone().unwrap_or_else(|| fill.to_string()),
            self.outline_color
                .clone()
                .unwrap_or_else(|| outline.to_string()),
        )
    }

    /// The classic arrow, drawn tip-first at the origin and scaled to
    /// `size`. Coordinates are in units of the pointer's height, so the
    /// glyph keeps its proportions at any size.
    fn arrow_path(size: f32) -> Path {
        const OUTLINE: [(f32, f32); 7] = [
            (0.0, 0.0),   // tip
            (0.0, 0.72),  // down the left edge
            (0.19, 0.56), // into the notch
            (0.30, 0.84), // out along the tail
            (0.43, 0.78), // tail's far side
            (0.32, 0.51), // back up
            (0.54, 0.51), // the right shoulder
        ];
        let mut path = Path::new();
        for (i, (x, y)) in OUTLINE.iter().enumerate() {
            let p = (x * size, y * size);
            if i == 0 {
                path.move_to(p);
            } else {
                path.line_to(p);
            }
        }
        path.close();
        path
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

        // The ring expands out of the tip and fades as it goes, so the eye
        // reads the click as happening *at* the tip rather than around the
        // whole pointer. Painted under the arrow so it never veils it.
        if let (Some(p), Some((stroke_f, travel_f))) = (click, self.click_ring.metrics()) {
            let (r, g, b, _) = parse_hex_color(self.ring_color.as_deref().unwrap_or(&fill));
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

        // A small dip on the press, released as the click finishes — the
        // arrow's own acknowledgement, independent of the ring (which
        // `click_ring: "none"` can switch off).
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
        // `click_at` is for a stationary pointer; a travelling one clicks on
        // arrival, so listing both must not produce two overlapping sets.
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
        // Two clicks closer together than `click_duration`: the second must
        // restart the animation, not be swallowed by the first still running.
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
}
