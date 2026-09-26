use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Path, PathBuilder};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::{ease, AnimatedProperties};
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{paint_from_hex, parse_hex_color};
use rustmotion_core::schema::{EasingType, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_check_size() -> f32 {
    82.0
}

fn default_check_tint() -> String {
    "#22C55E".to_string()
}

fn default_check_ring() -> f32 {
    0.28
}

fn default_check_spin() -> f32 {
    1.0
}

fn default_check_duration() -> f64 {
    0.7
}

/// A checkmark that draws itself inside a halo.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[deprecated(
    since = "0.7.1",
    note = "`success_check` is a frozen composition (issue #333). Compose an `svg` \
            checkmark (`reveal: \"stroke\"` draw-on) inside a `shape` circle halo with a \
            pop-in scale instead — see \
            crates/rustmotion/skills/rules/composition-recipes.md. Kept for compatibility; \
            scheduled for removal in a future major version via `rustmotion migrate` \
            (#335)."
)]
pub struct SuccessCheck {
    /// Diameter of the halo in px. The stroke scales with it.
    #[serde(default = "default_check_size")]
    pub size: f32,
    /// Colour of the mark (hex).
    #[serde(default = "default_check_tint")]
    pub tint: String,
    /// Halo opacity, 0..1. The halo takes `tint` unless `ring_color` says
    /// otherwise, so the default reads as the mark's own colour behind it.
    #[serde(default = "default_check_ring")]
    pub ring: f32,
    /// Halo colour (hex), overriding `tint`.
    #[serde(default)]
    pub ring_color: Option<String>,
    /// Multiplier on the entrance rotation. `0` lands the mark square with
    /// no swing; `2` gives it a pronounced one. The mark always finishes
    /// upright whatever this is.
    #[serde(default = "default_check_spin")]
    pub spin: f32,
    /// Stroke width of the mark in px. Defaults to 9% of `size`.
    #[serde(default)]
    pub stroke_width: Option<f32>,
    /// Delay before the mark starts arriving (seconds).
    #[serde(default)]
    pub delay: f64,
    /// How long the arrival takes (seconds).
    #[serde(default = "default_check_duration")]
    pub duration: f64,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(SuccessCheck {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CheckPhase {
    pub arrival: f32,
    pub stroke: f32,
}

impl SuccessCheck {
    const STROKE_START: f64 = 0.35;

    pub(crate) fn phase_at(&self, time: f64) -> CheckPhase {
        if self.duration <= 0.0 {
            return CheckPhase {
                arrival: 1.0,
                stroke: 1.0,
            };
        }
        let raw = ((time - self.delay) / self.duration).clamp(0.0, 1.0);
        if raw <= 0.0 {
            return CheckPhase {
                arrival: 0.0,
                stroke: 0.0,
            };
        }
        let stroke_raw = ((raw - Self::STROKE_START) / (1.0 - Self::STROKE_START)).clamp(0.0, 1.0);
        CheckPhase {
            arrival: ease(raw, &EasingType::EaseOutBack) as f32,
            stroke: ease(stroke_raw, &EasingType::EaseOutQuad) as f32,
        }
    }

    pub(crate) fn check_path(size: f32) -> Path {
        let mut path = PathBuilder::new();
        path.move_to((0.28 * size, 0.52 * size));
        path.line_to((0.44 * size, 0.69 * size));
        path.line_to((0.73 * size, 0.33 * size));
        path.detach()
    }
}

impl Painter for SuccessCheck {
    fn paint_content(
        &self,
        canvas: &Canvas,
        _layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let phase = self.phase_at(ctx.time);
        if phase.arrival <= 0.0 {
            return;
        }
        let size = self.size;
        let centre = size / 2.0;

        let scale = 0.72 + 0.28 * phase.arrival;
        let angle = -18.0 * self.spin * (1.0 - phase.arrival);

        canvas.save();
        canvas.translate((centre, centre));
        canvas.scale((scale, scale));
        canvas.rotate(angle, None);
        canvas.translate((-centre, -centre));

        if self.ring > 0.0 {
            let hex = self.ring_color.as_deref().unwrap_or(&self.tint);
            let (r, g, b, _) = parse_hex_color(hex);
            let alpha = (self.ring.clamp(0.0, 1.0) * phase.arrival.clamp(0.0, 1.0) * 255.0) as u8;
            let mut halo = skia_safe::Paint::default();
            halo.set_style(PaintStyle::Fill);
            halo.set_anti_alias(true);
            halo.set_color(skia_safe::Color::from_argb(alpha, r, g, b));
            canvas.draw_circle((centre, centre), size * 0.5, &halo);
        }

        let path = Self::check_path(size);
        let mut stroke = paint_from_hex(&self.tint);
        stroke.set_style(PaintStyle::Stroke);
        stroke.set_anti_alias(true);
        stroke.set_stroke_width(self.stroke_width.unwrap_or(size * 0.09));
        stroke.set_stroke_cap(skia_safe::PaintCap::Round);
        stroke.set_stroke_join(skia_safe::PaintJoin::Round);

        if phase.stroke <= 0.0 {
            canvas.restore();
            return;
        }
        if phase.stroke < 1.0 {
            let mut measure = skia_safe::PathMeasure::new(&path, false, None);
            let len = measure.length();
            let drawn = len * phase.stroke;
            if let Some(dash) = skia_safe::PathEffect::dash(&[drawn, len - drawn + 1.0], 0.0) {
                stroke.set_path_effect(dash);
            }
        }
        canvas.draw_path(&path, &stroke);

        canvas.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(json: serde_json::Value) -> SuccessCheck {
        serde_json::from_value(json).expect("success_check fixture")
    }

    #[test]
    fn nothing_is_drawn_before_the_delay() {
        let c = check(serde_json::json!({ "delay": 1.0 }));
        let p = c.phase_at(0.5);
        assert_eq!(p.arrival, 0.0, "the mark has not started arriving");
        assert_eq!(p.stroke, 0.0, "and nothing of it is drawn");
    }

    #[test]
    fn the_stroke_starts_after_the_halo_has_landed() {
        let c = check(serde_json::json!({ "duration": 1.0 }));
        let early = c.phase_at(0.2);
        assert!(
            early.arrival > 0.0,
            "the halo is already arriving at 20% of the window"
        );
        assert_eq!(
            early.stroke, 0.0,
            "but the stroke has not begun — it waits for the landing"
        );
        assert!(
            c.phase_at(0.6).stroke > 0.0,
            "by 60% the stroke is under way"
        );
    }

    #[test]
    fn both_phases_are_complete_once_the_window_has_passed() {
        let c = check(serde_json::json!({ "duration": 0.5, "delay": 0.25 }));
        let done = c.phase_at(5.0);
        assert!((done.arrival - 1.0).abs() < 1e-5);
        assert!((done.stroke - 1.0).abs() < 1e-5);
    }

    #[test]
    fn a_zero_duration_lands_immediately_instead_of_dividing_by_zero() {
        let c = check(serde_json::json!({ "duration": 0.0 }));
        let p = c.phase_at(0.0);
        assert_eq!((p.arrival, p.stroke), (1.0, 1.0));
    }

    #[test]
    fn the_mark_finishes_upright_whatever_the_spin() {
        for spin in [0.0, 1.0, 2.0, 5.0] {
            let c = check(serde_json::json!({ "spin": spin, "duration": 0.5 }));
            let settled = c.phase_at(2.0).arrival;
            let angle = -18.0 * spin * (1.0 - settled);
            assert!(
                angle.abs() < 1e-4,
                "spin={spin} left the settled mark rotated by {angle}°"
            );
        }
    }
}
