use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, PaintStyle, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{font_mgr, paint_from_hex, emoji_typeface, draw_text_with_fallback, measure_text_with_fallback};
use rustmotion_core::schema::TimelineStep as AnimTimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

/// A horizontal or vertical pipeline/timeline component.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Timeline {
    /// The steps in the pipeline.
    pub steps: Vec<TimelineStep>,
    /// Overall width of the timeline.
    #[serde(default = "default_timeline_width")]
    pub width: f32,
    /// Direction: "horizontal" (default) or "vertical".
    #[serde(default)]
    pub direction: TimelineDirection,
    /// Radius of the step circles.
    #[serde(default = "default_node_radius")]
    pub node_radius: f32,
    /// Color of the connecting bar background.
    #[serde(default = "default_bar_color")]
    pub bar_color: String,
    /// Color of the filled portion of the bar.
    #[serde(default = "default_bar_fill_color")]
    pub bar_fill_color: String,
    /// Bar thickness.
    #[serde(default = "default_bar_height")]
    pub bar_height: f32,
    /// Fill progress (0.0 to 1.0) — animates the bar from left to right.
    #[serde(default = "default_fill_progress")]
    pub fill_progress: f32,
    /// Font size for labels.
    #[serde(default = "default_label_font_size")]
    pub font_size: f32,
    /// Label color.
    #[serde(default = "default_label_color")]
    pub label_color: String,
    /// Sublabel color.
    #[serde(default = "default_sublabel_color")]
    pub sublabel_color: String,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<AnimTimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TimelineStep {
    /// Label text displayed below/beside the node.
    pub label: String,
    /// Optional sublabel (smaller text under the label).
    #[serde(default)]
    pub sublabel: Option<String>,
    /// Node color (fill).
    #[serde(default = "default_node_color")]
    pub color: String,
    /// Optional icon text inside the circle (emoji or single char).
    #[serde(default)]
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum TimelineDirection {
    #[default]
    Horizontal,
    Vertical,
}

fn default_timeline_width() -> f32 { 800.0 }
fn default_node_radius() -> f32 { 24.0 }
fn default_bar_color() -> String { "#333333".to_string() }
fn default_bar_fill_color() -> String { "#58A6FF".to_string() }
fn default_bar_height() -> f32 { 4.0 }
fn default_fill_progress() -> f32 { 1.0 }
fn default_label_font_size() -> f32 { 16.0 }
fn default_label_color() -> String { "#FFFFFF".to_string() }
fn default_sublabel_color() -> String { "#8B949E".to_string() }
fn default_node_color() -> String { "#58A6FF".to_string() }

rustmotion_core::impl_traits!(Timeline {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Timeline {
    fn paint(&self, canvas: &Canvas, props: &AnimatedProperties) {
        let n = self.steps.len();
        if n == 0 { return; }

        let fm = font_mgr();
        let typeface = fm.match_family_style("Inter", FontStyle::normal())
            .or_else(|| fm.match_family_style("Helvetica", FontStyle::normal()))
            .or_else(|| fm.match_family_style("Arial", FontStyle::normal()))
            .or_else(|| fm.match_family_style("sans-serif", FontStyle::normal()))
            .unwrap_or_else(|| fm.legacy_make_typeface(None, FontStyle::normal()).expect("No fallback font"));
        let font = Font::from_typeface(&typeface, self.font_size);
        let icon_font = Font::from_typeface(&typeface, self.node_radius * 0.8);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, self.node_radius * 0.8));
        let sublabel_font = Font::from_typeface(&typeface, self.font_size * 0.8);
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;

        let r = self.node_radius;
        let fill_progress = if props.draw_progress >= 0.0 { props.draw_progress } else { self.fill_progress };

        match self.direction {
            TimelineDirection::Horizontal => {
                self.render_horizontal(canvas, n, r, fill_progress, &font, &icon_font, &emoji_font, &sublabel_font, ascent);
            }
            TimelineDirection::Vertical => {
                self.render_vertical(canvas, n, r, fill_progress, &font, &icon_font, &emoji_font, &sublabel_font, ascent);
            }
        }
    }
}

impl Timeline {
    fn render_horizontal(
        &self,
        canvas: &Canvas,
        n: usize,
        r: f32,
        fill_progress: f32,
        font: &Font,
        icon_font: &Font,
        emoji_font: &Option<Font>,
        sublabel_font: &Font,
        ascent: f32,
    ) {
        let total_w = self.width;
        let bar_y = r; // Center of nodes
        let spacing = if n > 1 { total_w / (n - 1) as f32 } else { 0.0 };

        // Draw background bar
        let bar_rect = Rect::from_xywh(
            0.0, bar_y - self.bar_height / 2.0,
            total_w, self.bar_height,
        );
        let mut bar_paint = paint_from_hex(&self.bar_color);
        bar_paint.set_style(PaintStyle::Fill);
        canvas.draw_round_rect(bar_rect, self.bar_height / 2.0, self.bar_height / 2.0, &bar_paint);

        // Draw filled bar
        if fill_progress > 0.001 {
            let fill_w = total_w * fill_progress.clamp(0.0, 1.0);
            let fill_rect = Rect::from_xywh(
                0.0, bar_y - self.bar_height / 2.0,
                fill_w, self.bar_height,
            );
            let mut fill_paint = paint_from_hex(&self.bar_fill_color);
            fill_paint.set_style(PaintStyle::Fill);
            canvas.save();
            canvas.clip_rect(Rect::from_xywh(0.0, bar_y - self.bar_height / 2.0, total_w, self.bar_height), skia_safe::ClipOp::Intersect, false);
            canvas.draw_round_rect(fill_rect, self.bar_height / 2.0, self.bar_height / 2.0, &fill_paint);
            canvas.restore();
        }

        // Draw nodes and labels
        for (i, step) in self.steps.iter().enumerate() {
            let cx = if n > 1 { i as f32 * spacing } else { total_w / 2.0 };
            let cy = bar_y;

            // Determine if this step is "active" (filled bar has reached it)
            let step_progress = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.0 };
            let is_active = fill_progress >= step_progress;

            // Node circle
            let node_color = if is_active { &step.color } else { &self.bar_color };
            let mut node_paint = paint_from_hex(node_color);
            node_paint.set_style(PaintStyle::Fill);
            node_paint.set_anti_alias(true);
            canvas.draw_circle((cx, cy), r, &node_paint);

            // Icon/text inside node
            if let Some(ref icon) = step.icon {
                let icon_w = measure_text_with_fallback(icon, icon_font, emoji_font, 0.0);
                let (_, icon_metrics) = icon_font.metrics();
                let icon_ascent = -icon_metrics.ascent;
                let icon_descent = icon_metrics.descent;
                let ix = cx - icon_w / 2.0;
                let iy = cy + (icon_ascent - icon_descent) / 2.0;
                let mut icon_paint = paint_from_hex("#FFFFFF");
                icon_paint.set_anti_alias(true);
                draw_text_with_fallback(canvas, icon, icon_font, emoji_font, 0.0, ix, iy, &icon_paint);
            }

            // Label below
            let label_w = measure_text_with_fallback(&step.label, font, &None, 0.0);
            let lx = cx - label_w / 2.0;
            let ly = cy + r + 8.0 + ascent;
            let mut label_paint = paint_from_hex(&self.label_color);
            label_paint.set_anti_alias(true);
            draw_text_with_fallback(canvas, &step.label, font, &None, 0.0, lx, ly, &label_paint);

            // Sublabel
            if let Some(ref sublabel) = step.sublabel {
                let sub_w = measure_text_with_fallback(sublabel, sublabel_font, &None, 0.0);
                let sx = cx - sub_w / 2.0;
                let sy = ly + self.font_size * 1.2;
                let mut sub_paint = paint_from_hex(&self.sublabel_color);
                sub_paint.set_anti_alias(true);
                draw_text_with_fallback(canvas, sublabel, sublabel_font, &None, 0.0, sx, sy, &sub_paint);
            }
        }
    }

    fn render_vertical(
        &self,
        canvas: &Canvas,
        n: usize,
        r: f32,
        fill_progress: f32,
        font: &Font,
        icon_font: &Font,
        emoji_font: &Option<Font>,
        _sublabel_font: &Font,
        _ascent: f32,
    ) {
        let spacing = 80.0;
        let bar_x = r;
        let total_h = if n > 1 { (n - 1) as f32 * spacing } else { 0.0 };

        // Background bar
        let bar_rect = Rect::from_xywh(
            bar_x - self.bar_height / 2.0, 0.0,
            self.bar_height, total_h,
        );
        let mut bar_paint = paint_from_hex(&self.bar_color);
        bar_paint.set_style(PaintStyle::Fill);
        canvas.draw_round_rect(bar_rect, self.bar_height / 2.0, self.bar_height / 2.0, &bar_paint);

        // Filled bar
        if fill_progress > 0.001 {
            let fill_h = total_h * fill_progress.clamp(0.0, 1.0);
            let fill_rect = Rect::from_xywh(
                bar_x - self.bar_height / 2.0, 0.0,
                self.bar_height, fill_h,
            );
            let mut fill_paint = paint_from_hex(&self.bar_fill_color);
            fill_paint.set_style(PaintStyle::Fill);
            canvas.draw_round_rect(fill_rect, self.bar_height / 2.0, self.bar_height / 2.0, &fill_paint);
        }

        for (i, step) in self.steps.iter().enumerate() {
            let cx = bar_x;
            let cy = if n > 1 { i as f32 * spacing } else { 0.0 };

            let step_progress = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.0 };
            let is_active = fill_progress >= step_progress;

            let node_color = if is_active { &step.color } else { &self.bar_color };
            let mut node_paint = paint_from_hex(node_color);
            node_paint.set_style(PaintStyle::Fill);
            node_paint.set_anti_alias(true);
            canvas.draw_circle((cx, cy), r, &node_paint);

            if let Some(ref icon) = step.icon {
                let icon_w = measure_text_with_fallback(icon, icon_font, emoji_font, 0.0);
                let (_, icon_metrics) = icon_font.metrics();
                let icon_ascent = -icon_metrics.ascent;
                let icon_descent = icon_metrics.descent;
                let ix = cx - icon_w / 2.0;
                let iy = cy + (icon_ascent - icon_descent) / 2.0;
                let mut icon_paint = paint_from_hex("#FFFFFF");
                icon_paint.set_anti_alias(true);
                draw_text_with_fallback(canvas, icon, icon_font, emoji_font, 0.0, ix, iy, &icon_paint);
            }

            // Label to the right
            let lx = cx + r + 12.0;
            let (_, font_metrics) = font.metrics();
            let ly = cy + (-font_metrics.ascent - font_metrics.descent) / 2.0;
            let mut label_paint = paint_from_hex(&self.label_color);
            label_paint.set_anti_alias(true);
            draw_text_with_fallback(canvas, &step.label, font, &None, 0.0, lx, ly, &label_paint);
        }
    }
}

impl Painter for Timeline {
    fn paint_content(
        &self,
        canvas: &Canvas,
        _layout: &BoxLayout,
        props: &AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        self.paint(canvas, props);
    }
}
