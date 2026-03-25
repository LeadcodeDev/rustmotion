use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::Canvas;

use crate::engine::renderer::font_mgr;
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{LayerStyle, Size};
use crate::traits::{RenderContext, TimingConfig, Widget};

mod axes;
mod bar;
mod funnel;
mod line;
mod pie;
mod radar;
mod radial;
mod scatter;
mod waterfall;

pub(crate) const DEFAULT_PALETTE: &[&str] = &[
    "#3B82F6", "#EF4444", "#22C55E", "#F59E0B", "#8B5CF6", "#EC4899", "#06B6D4", "#F97316",
];

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
    #[serde(default)]
    pub size: Option<Size>,
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
    pub direction: Option<String>,

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
    pub style: LayerStyle,
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

crate::impl_traits!(Chart {
    Animatable => style,
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

    fn progress(&self, ctx: &RenderContext) -> f32 {
        if !self.animated {
            return 1.0;
        }
        let p = (ctx.time / self.animation_duration).clamp(0.0, 1.0) as f32;
        // ease_out_cubic
        1.0 - (1.0 - p).powi(3)
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

    pub(super) fn make_label_font(&self) -> skia_safe::Font {
        let fm = font_mgr();
        let font_style = skia_safe::FontStyle::normal();
        let typeface = fm
            .match_family_style("Inter", font_style)
            .or_else(|| fm.match_family_style("Helvetica", font_style))
            .or_else(|| fm.match_family_style("Arial", font_style))
            .or_else(|| fm.match_family_style("sans-serif", font_style))
            .unwrap_or_else(|| fm.legacy_make_typeface(None, font_style).unwrap());
        skia_safe::Font::from_typeface(typeface, self.label_font_size)
    }
}

impl Widget for Chart {
    fn render(
        &self,
        canvas: &Canvas,
        layout: &LayoutNode,
        ctx: &RenderContext,
        _props: &crate::engine::animator::AnimatedProperties,
    ) -> Result<()> {
        let w = layout.width;
        let h = layout.height;
        let progress = self.progress(ctx);

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

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        if let Some(size) = &self.size {
            return (size.width, size.height);
        }
        (300.0, 200.0)
    }
}
