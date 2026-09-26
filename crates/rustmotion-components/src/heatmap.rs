use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, RRect, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::parse_hex_color;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_cell_size() -> f32 {
    14.0
}

fn default_cell_gap() -> f32 {
    3.0
}

fn default_cell_radius() -> f32 {
    2.0
}

fn default_animated() -> bool {
    true
}

fn default_animation_duration() -> f64 {
    1.5
}

fn default_color_scale() -> Vec<String> {
    vec![
        "#161B22".to_string(),
        "#0E4429".to_string(),
        "#006D32".to_string(),
        "#26A641".to_string(),
        "#39D353".to_string(),
    ]
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[deprecated(
    since = "0.7.1",
    note = "`heatmap` is a frozen composition. Compose a `for-each` over the cells with a \
            computed `fill` expression per cell instead — see \
            crates/rustmotion/skills/rules/composition-recipes.md. Kept for compatibility; \
            scheduled for removal in a future major version via `rustmotion migrate` (#335)."
)]
pub struct Heatmap {
    /// 2D array of values (rows x columns).
    pub data: Vec<Vec<f64>>,
    /// Gradient colors for the heatmap scale.
    #[serde(default = "default_color_scale")]
    pub color_scale: Vec<String>,
    /// Size of each cell in pixels.
    #[serde(default = "default_cell_size")]
    pub cell_size: f32,
    /// Gap between cells in pixels.
    #[serde(default = "default_cell_gap")]
    pub cell_gap: f32,
    /// Corner radius of each cell.
    #[serde(default = "default_cell_radius")]
    pub cell_radius: f32,
    /// Whether the heatmap animates in.
    #[serde(default = "default_animated")]
    pub animated: bool,
    /// Duration of the reveal animation in seconds.
    #[serde(default = "default_animation_duration")]
    pub animation_duration: f64,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Heatmap {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t)
        .round()
        .clamp(0.0, 255.0) as u8
}

fn interpolate_color(scale: &[String], t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    if scale.len() < 2 {
        let (r, g, b, _) = parse_hex_color(&scale[0]);
        return (r, g, b);
    }
    let n = scale.len() - 1;
    let scaled = t * n as f32;
    let segment = (scaled.floor() as usize).min(n - 1);
    let local_t = (scaled - segment as f32).clamp(0.0, 1.0);
    let (r1, g1, b1, _) = parse_hex_color(&scale[segment]);
    let (r2, g2, b2, _) = parse_hex_color(&scale[segment + 1]);
    (
        lerp_u8(r1, r2, local_t),
        lerp_u8(g1, g2, local_t),
        lerp_u8(b1, b2, local_t),
    )
}

impl Heatmap {
    fn progress_at(&self, time: f64) -> f32 {
        if !self.animated {
            return 1.0;
        }
        let start = self.timing.start_at.unwrap_or(0.0);
        let elapsed = (time - start).max(0.0);
        let p = (elapsed / self.animation_duration).clamp(0.0, 1.0) as f32;
        1.0 - (1.0 - p).powi(3)
    }

    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) {
        let w = layout_w;
        let h = layout_h;

        if self.data.is_empty() || self.color_scale.is_empty() {
            return;
        }

        let progress = self.progress_at(time);

        let clip_w = w * progress;
        canvas.save();
        canvas.clip_rect(
            Rect::from_xywh(0.0, 0.0, clip_w, h),
            skia_safe::ClipOp::Intersect,
            false,
        );

        let step = self.cell_size + self.cell_gap;

        for (row_idx, row) in self.data.iter().enumerate() {
            for (col_idx, &val) in row.iter().enumerate() {
                let normalized = (val as f32).clamp(0.0, 1.0);
                let (r, g, b) = interpolate_color(&self.color_scale, normalized);

                let x = col_idx as f32 * step;
                let y = row_idx as f32 * step;

                let rect = Rect::from_xywh(x, y, self.cell_size, self.cell_size);
                let rrect = RRect::new_rect_xy(rect, self.cell_radius, self.cell_radius);

                let color = skia_safe::Color::from_rgb(r, g, b);
                let mut paint = skia_safe::Paint::default();
                paint.set_color(color);
                paint.set_style(PaintStyle::Fill);
                paint.set_anti_alias(true);

                canvas.draw_rrect(rrect, &paint);
            }
        }

        canvas.restore();
    }
}

impl Painter for Heatmap {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout.width, layout.height, ctx.time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::traits::TimingConfig;

    fn base_heatmap(data: Vec<Vec<f64>>) -> Heatmap {
        Heatmap {
            data,
            color_scale: default_color_scale(),
            cell_size: default_cell_size(),
            cell_gap: default_cell_gap(),
            cell_radius: default_cell_radius(),
            animated: true,
            animation_duration: 1.5,
            timing: TimingConfig::default(),
            style: CssStyle::default(),
            timeline: Vec::new(),
            stagger: None,
        }
    }

    fn cell_color(heatmap: &Heatmap, w: i32, h: i32, time: f64) -> (u8, u8, u8) {
        let mut surface = skia_safe::surfaces::raster_n32_premul((w, h)).expect("raster surface");
        {
            let canvas = surface.canvas();
            heatmap.paint(canvas, w as f32, h as f32, time);
        }
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (1, 1),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = [0u8; 4];
        let x = (heatmap.cell_size / 2.0) as i32;
        let y = (heatmap.cell_size / 2.0) as i32;
        snapshot.read_pixels(
            &info,
            &mut buf,
            4,
            skia_safe::IPoint::new(x, y),
            skia_safe::image::CachingHint::Disallow,
        );
        (buf[0], buf[1], buf[2])
    }

    #[test]
    fn a_uniformly_low_grid_is_not_identical_to_an_all_zero_grid() {
        let uniform = base_heatmap(vec![vec![5.0, 5.0, 5.0], vec![5.0, 5.0, 5.0]]);
        let zero = base_heatmap(vec![vec![0.0, 0.0, 0.0], vec![0.0, 0.0, 0.0]]);
        let uniform_color = cell_color(&uniform, 200, 100, 10.0);
        let zero_color = cell_color(&zero, 200, 100, 10.0);
        assert_ne!(
            uniform_color, zero_color,
            "a grid of 5.0s must not render identically to a grid of 0.0s"
        );
    }

    #[test]
    fn absolute_values_are_not_renormalized_to_the_data_subrange() {
        let high = base_heatmap(vec![vec![0.8, 0.9, 1.0]]);
        let low = base_heatmap(vec![vec![0.0, 0.1, 0.2]]);
        let high_first_cell = cell_color(&high, 200, 100, 10.0);
        let low_first_cell = cell_color(&low, 200, 100, 10.0);
        assert_ne!(
            high_first_cell, low_first_cell,
            "0.8 and 0.0 must not render as the same color"
        );
    }

    #[test]
    fn interpolate_color_at_the_top_of_the_scale_returns_the_last_color() {
        let scale = default_color_scale();
        let (r, g, b) = interpolate_color(&scale, 1.0);
        let (er, eg, eb, _) = parse_hex_color(scale.last().unwrap());
        assert_eq!(
            (r, g, b),
            (er, eg, eb),
            "t=1.0 must resolve to the last color in the scale"
        );
    }

    #[test]
    fn progress_ramp_starts_at_start_at_not_at_scene_time_zero() {
        let mut heatmap = base_heatmap(vec![vec![1.0]]);
        heatmap.animation_duration = 1.5;
        heatmap.timing = TimingConfig {
            start_at: Some(2.0),
            end_at: None,
        };
        assert_eq!(heatmap.progress_at(2.0), 0.0);
        assert!(heatmap.progress_at(2.75) < 1.0);
        assert_eq!(heatmap.progress_at(3.5), 1.0);
    }
}
