use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::paint_from_hex;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

/// A cursor waypoint with position and time.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CursorWaypoint {
    /// Time in seconds when the cursor reaches this point (and clicks).
    pub time: f64,
    /// X position relative to the cursor's origin.
    pub x: f32,
    /// Y position relative to the cursor's origin.
    pub y: f32,
}

/// A blinking cursor component (vertical bar) with optional motion path and click events.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Cursor {
    #[serde(default = "default_cursor_width")]
    pub width: f32,
    #[serde(default = "default_cursor_height")]
    pub height: f32,
    #[serde(default = "default_cursor_color")]
    pub color: String,
    /// Blink interval in seconds (0 = no blink, always visible).
    #[serde(default = "default_blink_interval")]
    pub blink: f32,
    #[serde(default = "default_cursor_radius")]
    pub radius: f32,
    /// Timestamps (seconds) at which the cursor "clicks" (scale bounce effect).
    /// Used when no auto_path waypoints are provided.
    #[serde(default)]
    pub click_at: Vec<f64>,
    /// Auto-path waypoints: cursor moves between these positions with smooth curves.
    /// Each waypoint has a time and position. The cursor clicks at each waypoint.
    #[serde(default)]
    pub auto_path: Vec<CursorWaypoint>,
    /// Click animation duration in seconds.
    #[serde(default = "default_click_duration")]
    pub click_duration: f32,
    /// Visual cursor style: "default" (arrow) or "pointer" (hand).
    /// Currently both render as a bar; this is metadata for future SVG cursors.
    #[serde(default)]
    pub cursor_style: CursorStyle,
    /// Easing for movement between waypoints: "ease_in_out" (default), "linear", "ease_out".
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

fn default_cursor_width() -> f32 {
    3.0
}

fn default_cursor_height() -> f32 {
    40.0
}

fn default_cursor_color() -> String {
    "#FFFFFF".to_string()
}

fn default_blink_interval() -> f32 {
    0.5
}

fn default_cursor_radius() -> f32 {
    1.5
}

fn default_click_duration() -> f32 {
    0.3
}

/// Visual cursor style. Closed documented set; JSON values unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CursorStyle {
    #[default]
    Default,
    Pointer,
}

/// Easing of the cursor's waypoint path. Closed set, now matched
/// exhaustively; previously-silent unknown values fail the typed parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CursorPathEasing {
    Linear,
    EaseOut,
    #[default]
    EaseInOut,
    /// No interpolation: hold each waypoint until the next one's time, then
    /// jump. What a text caret does — it never slides between two positions —
    /// and the only way to express that, since every other easing glides.
    Step,
}

rustmotion_core::impl_traits!(Cursor {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Cursor {
    /// Get all click times (from click_at or auto_path waypoints).
    fn click_times(&self) -> Vec<f64> {
        if !self.auto_path.is_empty() {
            self.auto_path.iter().map(|w| w.time).collect()
        } else {
            self.click_at.clone()
        }
    }

    /// Compute cursor position offset from auto_path at the given time.
    /// Returns (dx, dy) translation to apply.
    fn auto_path_offset(&self, time: f64) -> (f32, f32) {
        if self.auto_path.len() < 2 {
            if let Some(wp) = self.auto_path.first() {
                return (wp.x, wp.y);
            }
            return (0.0, 0.0);
        }

        let waypoints = &self.auto_path;

        // Before first waypoint: stay at first position
        if time <= waypoints[0].time {
            return (waypoints[0].x, waypoints[0].y);
        }

        // After last waypoint: stay at last position
        if time >= waypoints[waypoints.len() - 1].time {
            let last = &waypoints[waypoints.len() - 1];
            return (last.x, last.y);
        }

        // Find which segment we're in
        let mut seg_idx = 0;
        for i in 0..waypoints.len() - 1 {
            if time >= waypoints[i].time && time < waypoints[i + 1].time {
                seg_idx = i;
                break;
            }
        }

        let wp0 = &waypoints[seg_idx];
        let wp1 = &waypoints[seg_idx + 1];
        let seg_duration = wp1.time - wp0.time;
        if seg_duration <= 0.0 {
            return (wp1.x, wp1.y);
        }

        // Account for click pause: don't start moving until click animation finishes
        let click_end = wp0.time + self.click_duration as f64;
        let move_start = if seg_idx > 0 { click_end } else { wp0.time };
        let move_duration = wp1.time - move_start;

        if time < move_start || move_duration <= 0.0 {
            return (wp0.x, wp0.y);
        }

        let raw_t = ((time - move_start) / move_duration).clamp(0.0, 1.0);

        // Apply easing
        let t = match self.path_easing {
            // Hold the departure point for the whole segment; the jump happens
            // when `time` reaches the next waypoint and the segment changes.
            CursorPathEasing::Step => 0.0,
            CursorPathEasing::Linear => raw_t,
            CursorPathEasing::EaseOut => 1.0 - (1.0 - raw_t).powi(3),
            CursorPathEasing::EaseInOut => {
                if raw_t < 0.5 {
                    4.0 * raw_t * raw_t * raw_t
                } else {
                    1.0 - (-2.0 * raw_t + 2.0).powi(3) / 2.0
                }
            }
        } as f32;

        // Catmull-Rom interpolation for smooth curves
        let p_prev = if seg_idx > 0 {
            &waypoints[seg_idx - 1]
        } else {
            wp0
        };
        let p_next = if seg_idx + 2 < waypoints.len() {
            &waypoints[seg_idx + 2]
        } else {
            wp1
        };

        let x = catmull_rom(t, p_prev.x, wp0.x, wp1.x, p_next.x);
        let y = catmull_rom(t, p_prev.y, wp0.y, wp1.y, p_next.y);

        (x, y)
    }
}

/// Catmull-Rom spline interpolation
fn catmull_rom(t: f32, p0: f32, p1: f32, p2: f32, p3: f32) -> f32 {
    let t2 = t * t;
    let t3 = t2 * t;
    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

impl Painter for Cursor {
    fn paint_content(
        &self,
        canvas: &Canvas,
        _layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let click_times = self.click_times();

        let in_click = click_times.iter().any(|&t| {
            let dt = ctx.time - t;
            dt >= 0.0 && dt < self.click_duration as f64
        });

        if self.blink > 0.0 && !in_click {
            let cycle = (ctx.time as f32 % (self.blink * 2.0)) / self.blink;
            if cycle >= 1.0 {
                return;
            }
        }

        let (path_dx, path_dy) = if !self.auto_path.is_empty() {
            self.auto_path_offset(ctx.time)
        } else {
            (0.0, 0.0)
        };

        let click_scale = if in_click {
            let closest_click = click_times
                .iter()
                .filter(|&&t| ctx.time >= t && ctx.time < t + self.click_duration as f64)
                .copied()
                .last()
                .unwrap_or(0.0);
            let progress = ((ctx.time - closest_click) / self.click_duration as f64) as f32;
            if progress < 0.3 {
                1.0 + 0.5 * (progress / 0.3)
            } else {
                1.5 - 0.5 * ((progress - 0.3) / 0.7)
            }
        } else {
            1.0
        };

        if path_dx.abs() > 0.001 || path_dy.abs() > 0.001 {
            canvas.save();
            canvas.translate((path_dx, path_dy));
        }

        if (click_scale - 1.0).abs() > 0.001 {
            let cx = self.width / 2.0;
            let cy = self.height / 2.0;
            canvas.save();
            canvas.translate((cx, cy));
            canvas.scale((click_scale, click_scale));
            canvas.translate((-cx, -cy));
        }

        let paint = paint_from_hex(&self.color);
        let rect = skia_safe::Rect::from_xywh(0.0, 0.0, self.width, self.height);
        let rrect = skia_safe::RRect::new_rect_xy(rect, self.radius, self.radius);
        canvas.draw_rrect(rrect, &paint);

        if (click_scale - 1.0).abs() > 0.001 {
            canvas.restore();
        }

        if path_dx.abs() > 0.001 || path_dy.abs() > 0.001 {
            canvas.restore();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caret(easing: CursorPathEasing) -> Cursor {
        serde_json::from_value(serde_json::json!({
            "path_easing": easing,
            "click_duration": 0.0,
            "auto_path": [
                {"time": 1.0, "x": 100.0, "y": 0.0},
                {"time": 2.0, "x": 500.0, "y": 0.0},
            ],
        }))
        .expect("cursor fixture")
    }

    /// A caret jumps between positions; it must never be caught between two
    /// fields. Every other easing interpolates, so `step` is the only way to
    /// say that.
    #[test]
    fn step_easing_holds_the_departure_point_until_the_next_waypoint() {
        let c = caret(CursorPathEasing::Step);
        for t in [1.0, 1.25, 1.5, 1.75, 1.99] {
            assert_eq!(
                c.auto_path_offset(t),
                (100.0, 0.0),
                "step must hold the first waypoint at t={t}"
            );
        }
        assert_eq!(c.auto_path_offset(2.0), (500.0, 0.0));
        assert_eq!(c.auto_path_offset(9.0), (500.0, 0.0));
    }

    /// The other easings keep gliding — `step` is additive, not a change of
    /// default behaviour.
    #[test]
    fn linear_easing_still_interpolates() {
        let (x, _) = caret(CursorPathEasing::Linear).auto_path_offset(1.5);
        assert!(
            (x - 300.0).abs() < 0.5,
            "linear should be halfway at t=1.5, got {x}"
        );
    }

    #[test]
    fn step_is_spelled_snake_case_in_json() {
        let c: Cursor = serde_json::from_value(serde_json::json!({"path_easing": "step"}))
            .expect("`step` must parse");
        assert_eq!(c.path_easing, CursorPathEasing::Step);
    }
}
