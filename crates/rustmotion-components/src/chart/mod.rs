use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::typeface_with_fallback;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

mod axes;
mod bar;
mod funnel;
mod line;
mod pie;
mod radar;
mod radial;
mod scatter;
mod waterfall;

/// The engine's default series palette. `pub` so the studio can prefill an
/// empty `colors` list with what the canvas actually renders.
pub const DEFAULT_PALETTE: &[&str] = &[
    "#3B82F6", "#EF4444", "#22C55E", "#F59E0B", "#8B5CF6", "#EC4899", "#06B6D4", "#F97316",
];

/// Funnel flow direction. Closed set (painter matches both variants); JSON
/// values unchanged ("vertical"/"horizontal").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChartDirection {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChartType {
    Bar,
    Line,
    Pie,
    Donut,
    HorizontalBar,
    Area,
    StackedBar,
    Radar,
    Scatter,
    RadialBar,
    Funnel,
    Waterfall,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChartDataPoint {
    pub value: f64,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

/// A data point for scatter charts with explicit x/y coordinates.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScatterPoint {
    pub x: f64,
    pub y: f64,
    #[serde(default = "default_scatter_size")]
    pub size: f32,
    #[serde(default)]
    pub color: Option<String>,
}

fn default_scatter_size() -> f32 {
    8.0
}

/// A data series for stacked bar charts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChartSeries {
    pub name: String,
    pub data: Vec<f64>,
    #[serde(default)]
    pub color: Option<String>,
}

/// A data series for radar charts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RadarData {
    pub values: Vec<f64>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Chart {
    pub chart_type: ChartType,
    #[serde(default)]
    pub data: Vec<ChartDataPoint>,
    #[serde(default = "default_animated")]
    pub animated: bool,
    #[serde(default = "default_animation_duration")]
    pub animation_duration: f64,
    #[serde(default)]
    pub colors: Option<Vec<String>>,

    // Donut-specific
    #[serde(default = "default_inner_radius")]
    pub inner_radius: f64,

    // Area-specific
    #[serde(default = "default_fill_opacity")]
    pub fill_opacity: f32,
    #[serde(default)]
    pub smooth: bool,

    // Stacked bar
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub series: Vec<ChartSeries>,

    // Radar
    #[serde(default)]
    pub axes: Vec<String>,
    #[serde(default)]
    pub radar_data: Vec<RadarData>,

    // Scatter
    #[serde(default)]
    pub points: Vec<ScatterPoint>,

    // Funnel direction
    /// Direction for funnel chart: "vertical" (default) or "horizontal".
    #[serde(default)]
    pub direction: Option<ChartDirection>,

    // Axes, grid, labels
    #[serde(default)]
    pub show_grid: bool,
    #[serde(default)]
    pub show_x_labels: bool,
    #[serde(default)]
    pub show_y_labels: bool,
    #[serde(default = "default_grid_color")]
    pub grid_color: String,
    #[serde(default = "default_label_color")]
    pub label_color: String,
    #[serde(default = "default_label_font_size")]
    pub label_font_size: f32,
    #[serde(default)]
    pub show_labels: bool,

    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

fn default_animated() -> bool {
    true
}

fn default_animation_duration() -> f64 {
    1.5
}

fn default_inner_radius() -> f64 {
    0.6
}

fn default_fill_opacity() -> f32 {
    0.3
}

fn default_grid_color() -> String {
    "#FFFFFF15".to_string()
}

fn default_label_color() -> String {
    "#888888".to_string()
}

fn default_label_font_size() -> f32 {
    12.0
}

rustmotion_core::impl_traits!(Chart {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Chart {
    pub(super) fn get_color(&self, index: usize) -> &str {
        if let Some(colors) = &self.colors {
            if !colors.is_empty() {
                return &colors[index % colors.len()];
            }
        }
        DEFAULT_PALETTE[index % DEFAULT_PALETTE.len()]
    }

    fn progress_at(&self, time: f64) -> f32 {
        if !self.animated {
            return 1.0;
        }
        // Ramp measured from `start_at`, not from scene time zero — matches
        // `Counter::ramp_progress`. A chart delayed with `start_at` used to
        // read raw scene time, so it was already fully drawn on the very
        // first frame it became visible.
        let start = self.timing.start_at.unwrap_or(0.0);
        let elapsed = (time - start).max(0.0);
        let p = (elapsed / self.animation_duration).clamp(0.0, 1.0) as f32;
        // ease_out_cubic
        1.0 - (1.0 - p).powi(3)
    }

    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) -> Result<()> {
        let w = layout_w;
        let h = layout_h;
        let progress = self.progress_at(time);

        match self.chart_type {
            ChartType::Bar => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_bar(canvas, w, h, progress)
            }
            ChartType::Line => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_line(canvas, w, h, progress)
            }
            ChartType::Pie => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_pie(canvas, w, h, progress)
            }
            ChartType::Donut => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_donut(canvas, w, h, progress)
            }
            ChartType::HorizontalBar => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_horizontal_bar(canvas, w, h, progress)
            }
            ChartType::Area => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_area(canvas, w, h, progress)
            }
            ChartType::StackedBar => self.render_stacked_bar(canvas, w, h, progress),
            ChartType::Radar => self.render_radar(canvas, w, h, progress),
            ChartType::Scatter => self.render_scatter(canvas, w, h, progress),
            ChartType::RadialBar => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_radial_bar(canvas, w, h, progress)
            }
            ChartType::Funnel => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_funnel(canvas, w, h, progress)
            }
            ChartType::Waterfall => {
                if self.data.is_empty() {
                    return Ok(());
                }
                self.render_waterfall(canvas, w, h, progress)
            }
        }
    }

    /// Compute margins for axes/labels area.
    pub(super) fn chart_margins(&self) -> (f32, f32, f32, f32) {
        let left = if self.show_y_labels {
            self.label_font_size * 3.5
        } else {
            0.0
        };
        let bottom = if self.show_x_labels {
            self.label_font_size * 2.0
        } else {
            0.0
        };
        // top, right, bottom, left
        (8.0, 8.0, bottom + 8.0, left + 8.0)
    }

    pub(super) fn make_label_font(&self) -> Option<skia_safe::Font> {
        let font_style = skia_safe::FontStyle::normal();
        let typeface = typeface_with_fallback("Inter", font_style).ok()?;
        Some(skia_safe::Font::from_typeface(
            typeface,
            self.label_font_size,
        ))
    }
}

impl Painter for Chart {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let _ = self.paint(canvas, layout.width, layout.height, ctx.time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::traits::TimingConfig;

    fn base_chart() -> Chart {
        Chart {
            chart_type: ChartType::Bar,
            data: Vec::new(),
            animated: true,
            animation_duration: 1.5,
            colors: None,
            inner_radius: 0.6,
            fill_opacity: 0.3,
            smooth: false,
            categories: Vec::new(),
            series: Vec::new(),
            axes: Vec::new(),
            radar_data: Vec::new(),
            points: Vec::new(),
            direction: None,
            show_grid: false,
            show_x_labels: false,
            show_y_labels: false,
            grid_color: default_grid_color(),
            label_color: default_label_color(),
            label_font_size: default_label_font_size(),
            show_labels: false,
            timing: TimingConfig::default(),
            style: rustmotion_core::css::CssStyle::default(),
            timeline: Vec::new(),
            stagger: None,
        }
    }

    #[test]
    fn progress_ramp_starts_at_start_at_not_at_scene_time_zero() {
        // #3's exact repro: a chart delayed with `start_at: 2.0` and
        // `animation_duration: 1.5` was already fully drawn (progress 1.0)
        // on the very first frame it became visible, because the ramp read
        // raw scene time instead of time-since-`start_at` — the same defect
        // `Counter::ramp_progress` was fixed for.
        let mut chart = base_chart();
        chart.animation_duration = 1.5;
        chart.timing = TimingConfig {
            start_at: Some(2.0),
            end_at: None,
        };

        assert_eq!(
            chart.progress_at(2.0),
            0.0,
            "no time has elapsed since start_at yet"
        );
        assert!(
            chart.progress_at(2.75) < 1.0,
            "still mid-ramp half a second after start_at"
        );
        assert_eq!(
            chart.progress_at(3.5),
            1.0,
            "animation_duration has fully elapsed since start_at"
        );
    }

    #[test]
    fn progress_ramp_with_no_start_at_behaves_like_before() {
        let chart = base_chart();
        assert_eq!(chart.progress_at(0.0), 0.0);
        assert_eq!(chart.progress_at(1.5), 1.0);
    }

    #[test]
    fn progress_ramp_when_not_animated_is_always_complete() {
        let mut chart = base_chart();
        chart.animated = false;
        chart.timing = TimingConfig {
            start_at: Some(2.0),
            end_at: None,
        };
        assert_eq!(chart.progress_at(0.0), 1.0);
    }
}
