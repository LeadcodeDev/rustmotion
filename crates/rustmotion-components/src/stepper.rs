use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle};

use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, font_mgr, measure_text_with_fallback, paint_from_hex,
    parse_hex_color,
};
use rustmotion_core::layout::{Constraints, LayoutNode};
use rustmotion_core::schema::{LayerStyle, Size};
use rustmotion_core::traits::{PaintCtx, Painter, RenderContext, TimingConfig, Widget};

fn default_active_step() -> u32 {
    0
}

fn default_transition_duration() -> f64 {
    0.5
}

fn default_orientation() -> String {
    "horizontal".to_string()
}

fn default_active_color() -> String {
    "#3B82F6".to_string()
}

fn default_completed_color() -> String {
    "#22C55E".to_string()
}

fn default_pending_color() -> String {
    "#6B7280".to_string()
}

fn default_node_size() -> f32 {
    32.0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StepItem {
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Stepper {
    /// The steps to display.
    pub steps: Vec<StepItem>,
    /// The currently active step (0-indexed).
    #[serde(default = "default_active_step")]
    pub active_step: u32,
    /// Animate active step to this index.
    #[serde(default)]
    pub animate_to: Option<u32>,
    /// Time at which the animation starts.
    #[serde(default)]
    pub animate_at: Option<f64>,
    /// Duration of the step transition animation.
    #[serde(default = "default_transition_duration")]
    pub transition_duration: f64,
    /// Layout direction: "horizontal" or "vertical".
    #[serde(default = "default_orientation")]
    pub orientation: String,
    #[serde(default)]
    pub size: Option<Size>,
    /// Color of the active step node.
    #[serde(default = "default_active_color")]
    pub active_color: String,
    /// Color of completed step nodes.
    #[serde(default = "default_completed_color")]
    pub completed_color: String,
    /// Color of pending step nodes.
    #[serde(default = "default_pending_color")]
    pub pending_color: String,
    /// Diameter of step nodes.
    #[serde(default = "default_node_size")]
    pub node_size: f32,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(Stepper {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Stepper {
    fn current_step_at(&self, time: f64) -> f32 {
        if let (Some(target), Some(start_at)) = (self.animate_to, self.animate_at) {
            let elapsed = (time - start_at).max(0.0);
            let p = (elapsed / self.transition_duration).clamp(0.0, 1.0) as f32;
            let eased = 1.0 - (1.0 - p).powi(3);
            let from = self.active_step as f32;
            let to = target as f32;
            from + (to - from) * eased
        } else {
            self.active_step as f32
        }
    }
}

impl Stepper {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) {
        let w = layout_w;
        let h = layout_h;
        let n = self.steps.len();
        if n == 0 {
            return;
        }

        let current = self.current_step_at(time);
        let r = self.node_size / 2.0;

        let fm = font_mgr();
        let font_style = skia_safe::FontStyle::normal();
        let typeface = fm
            .match_family_style("Inter", font_style)
            .or_else(|| fm.match_family_style("Helvetica", font_style))
            .unwrap_or_else(|| fm.legacy_make_typeface(None, font_style).unwrap());

        let bold_style = skia_safe::FontStyle::bold();
        let bold_typeface = fm
            .match_family_style("Inter", bold_style)
            .or_else(|| fm.match_family_style("Helvetica", bold_style))
            .unwrap_or_else(|| fm.legacy_make_typeface(None, bold_style).unwrap());

        let number_font_size = r * 0.9;
        let number_font = skia_safe::Font::from_typeface(&bold_typeface, number_font_size);
        let emoji_number_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, number_font_size));

        let label_font_size = 14.0;
        let label_font = skia_safe::Font::from_typeface(&typeface, label_font_size);
        let emoji_label_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, label_font_size));

        let desc_font_size = 11.0;
        let desc_font = skia_safe::Font::from_typeface(&typeface, desc_font_size);
        let emoji_desc_font =
            emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, desc_font_size));

        let is_horizontal = self.orientation == "horizontal";

        if is_horizontal {
            let padding = r + 8.0;
            let available = w - padding * 2.0;
            let spacing = if n > 1 {
                available / (n - 1) as f32
            } else {
                0.0
            };
            let cy = r + 4.0;

            // Draw connector lines between nodes
            for i in 0..(n - 1) {
                let x1 = padding + i as f32 * spacing + r;
                let x2 = padding + (i + 1) as f32 * spacing - r;

                let step_f = i as f32;
                let is_completed = current > step_f + 0.5;

                let (cr, cg, cb, _) = if is_completed {
                    parse_hex_color(&self.completed_color)
                } else {
                    parse_hex_color(&self.pending_color)
                };

                let alpha = if is_completed { 255u8 } else { 80u8 };
                let color = skia_safe::Color::from_argb(alpha, cr, cg, cb);
                let mut line_paint = skia_safe::Paint::default();
                line_paint.set_color(color);
                line_paint.set_style(PaintStyle::Stroke);
                line_paint.set_stroke_width(2.0);
                line_paint.set_anti_alias(true);

                canvas.draw_line((x1, cy), (x2, cy), &line_paint);
            }

            // Draw nodes and labels
            for (i, step) in self.steps.iter().enumerate() {
                let cx = padding + i as f32 * spacing;
                let step_f = i as f32;

                let (node_color, is_active) = if step_f < current.floor() {
                    // Completed
                    (&self.completed_color, false)
                } else if (step_f - current).abs() < 0.5 {
                    // Active
                    (&self.active_color, true)
                } else {
                    // Pending
                    (&self.pending_color, false)
                };

                // Node circle
                let mut node_paint = paint_from_hex(node_color);
                node_paint.set_style(PaintStyle::Fill);
                node_paint.set_anti_alias(true);

                if is_active {
                    // Filled circle for active
                    canvas.draw_circle((cx, cy), r, &node_paint);
                } else if step_f < current.floor() {
                    // Filled circle for completed
                    canvas.draw_circle((cx, cy), r, &node_paint);
                } else {
                    // Ring for pending
                    node_paint.set_style(PaintStyle::Stroke);
                    node_paint.set_stroke_width(2.0);
                    canvas.draw_circle((cx, cy), r - 1.0, &node_paint);
                }

                // Step number inside circle
                let num_text = format!("{}", i + 1);
                let num_w =
                    measure_text_with_fallback(&num_text, &number_font, &emoji_number_font, 0.0);
                let (_, num_metrics) = number_font.metrics();
                let num_x = cx - num_w / 2.0;
                let num_y = cy + (-num_metrics.ascent - num_metrics.descent) / 2.0;

                let mut num_paint = paint_from_hex("#FFFFFF");
                num_paint.set_anti_alias(true);
                draw_text_with_fallback(
                    canvas,
                    &num_text,
                    &number_font,
                    &emoji_number_font,
                    0.0,
                    num_x,
                    num_y,
                    &num_paint,
                );

                // Label below circle
                let label_w =
                    measure_text_with_fallback(&step.label, &label_font, &emoji_label_font, 0.0);
                let label_x = cx - label_w / 2.0;
                let (_, label_metrics) = label_font.metrics();
                let label_y = cy + r + 12.0 + (-label_metrics.ascent);

                let mut label_paint = paint_from_hex("#FFFFFF");
                label_paint.set_anti_alias(true);
                draw_text_with_fallback(
                    canvas,
                    &step.label,
                    &label_font,
                    &emoji_label_font,
                    0.0,
                    label_x,
                    label_y,
                    &label_paint,
                );

                // Description below label
                if let Some(desc) = &step.description {
                    let desc_w =
                        measure_text_with_fallback(desc, &desc_font, &emoji_desc_font, 0.0);
                    let desc_x = cx - desc_w / 2.0;
                    let desc_y = label_y + label_font_size + 4.0;

                    let mut desc_paint = paint_from_hex("#8B949E");
                    desc_paint.set_anti_alias(true);
                    draw_text_with_fallback(
                        canvas,
                        desc,
                        &desc_font,
                        &emoji_desc_font,
                        0.0,
                        desc_x,
                        desc_y,
                        &desc_paint,
                    );
                }
            }
        } else {
            // Vertical layout
            let padding = r + 8.0;
            let available = h - padding * 2.0;
            let spacing = if n > 1 {
                available / (n - 1) as f32
            } else {
                0.0
            };
            let cx = r + 4.0;

            // Draw connector lines
            for i in 0..(n - 1) {
                let y1 = padding + i as f32 * spacing + r;
                let y2 = padding + (i + 1) as f32 * spacing - r;

                let step_f = i as f32;
                let is_completed = current > step_f + 0.5;

                let (cr, cg, cb, _) = if is_completed {
                    parse_hex_color(&self.completed_color)
                } else {
                    parse_hex_color(&self.pending_color)
                };

                let alpha = if is_completed { 255u8 } else { 80u8 };
                let color = skia_safe::Color::from_argb(alpha, cr, cg, cb);
                let mut line_paint = skia_safe::Paint::default();
                line_paint.set_color(color);
                line_paint.set_style(PaintStyle::Stroke);
                line_paint.set_stroke_width(2.0);
                line_paint.set_anti_alias(true);

                canvas.draw_line((cx, y1), (cx, y2), &line_paint);
            }

            // Draw nodes and labels
            for (i, step) in self.steps.iter().enumerate() {
                let cy = padding + i as f32 * spacing;
                let step_f = i as f32;

                let (node_color, is_active) = if step_f < current.floor() {
                    (&self.completed_color, false)
                } else if (step_f - current).abs() < 0.5 {
                    (&self.active_color, true)
                } else {
                    (&self.pending_color, false)
                };

                let mut node_paint = paint_from_hex(node_color);
                node_paint.set_style(PaintStyle::Fill);
                node_paint.set_anti_alias(true);

                if is_active || step_f < current.floor() {
                    canvas.draw_circle((cx, cy), r, &node_paint);
                } else {
                    node_paint.set_style(PaintStyle::Stroke);
                    node_paint.set_stroke_width(2.0);
                    canvas.draw_circle((cx, cy), r - 1.0, &node_paint);
                }

                // Step number
                let num_text = format!("{}", i + 1);
                let num_w =
                    measure_text_with_fallback(&num_text, &number_font, &emoji_number_font, 0.0);
                let (_, num_metrics) = number_font.metrics();
                let num_x = cx - num_w / 2.0;
                let num_y = cy + (-num_metrics.ascent - num_metrics.descent) / 2.0;

                let mut num_paint = paint_from_hex("#FFFFFF");
                num_paint.set_anti_alias(true);
                draw_text_with_fallback(
                    canvas,
                    &num_text,
                    &number_font,
                    &emoji_number_font,
                    0.0,
                    num_x,
                    num_y,
                    &num_paint,
                );

                // Label to the right
                let lx = cx + r + 12.0;
                let (_, lm) = label_font.metrics();
                let ly = cy + (-lm.ascent - lm.descent) / 2.0;
                let mut label_paint = paint_from_hex("#FFFFFF");
                label_paint.set_anti_alias(true);
                draw_text_with_fallback(
                    canvas,
                    &step.label,
                    &label_font,
                    &emoji_label_font,
                    0.0,
                    lx,
                    ly,
                    &label_paint,
                );

                // Description below label
                if let Some(desc) = &step.description {
                    let desc_y = ly + label_font_size + 2.0;
                    let mut desc_paint = paint_from_hex("#8B949E");
                    desc_paint.set_anti_alias(true);
                    draw_text_with_fallback(
                        canvas,
                        desc,
                        &desc_font,
                        &emoji_desc_font,
                        0.0,
                        lx,
                        desc_y,
                        &desc_paint,
                    );
                }
            }
        }
    }
}

impl Widget for Stepper {
    fn render(
        &self,
        canvas: &Canvas,
        layout: &LayoutNode,
        ctx: &RenderContext,
        _props: &AnimatedProperties,
        _pipeline: &dyn rustmotion_core::traits::RenderPipeline,
    ) -> Result<()> {
        self.paint(canvas, layout.width, layout.height, ctx.time);
        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        if let Some(size) = &self.size {
            return (size.width, size.height);
        }
        let n = self.steps.len().max(1);
        if self.orientation == "horizontal" {
            let w = (n as f32 * 100.0).max(200.0);
            let h = self.node_size + 60.0;
            (w, h)
        } else {
            let w = self.node_size + 200.0;
            let h = (n as f32 * 80.0).max(100.0);
            (w, h)
        }
    }
}

impl Painter for Stepper {
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
