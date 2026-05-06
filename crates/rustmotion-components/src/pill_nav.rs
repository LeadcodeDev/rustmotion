use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, RRect, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, font_mgr, measure_text_with_fallback, paint_from_hex,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_pill_color() -> String {
    "#3B82F6".to_string()
}
fn default_text_color() -> String {
    "#FFFFFF".to_string()
}
fn default_inactive_text_color() -> String {
    "#9CA3AF".to_string()
}
fn default_background_color() -> String {
    "#1E293B".to_string()
}
fn default_height() -> f32 {
    44.0
}
fn default_border_radius() -> f32 {
    22.0
}
fn default_gap() -> f32 {
    4.0
}
fn default_transition_duration() -> f64 {
    0.3
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PillTransition {
    pub to: u32,
    pub at: f64,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct PillNav {
    pub items: Vec<String>,
    #[serde(default)]
    pub active_index: u32,
    #[serde(default)]
    pub transitions: Vec<PillTransition>,
    #[serde(default = "default_pill_color")]
    pub pill_color: String,
    #[serde(default = "default_text_color")]
    pub text_color: String,
    #[serde(default = "default_inactive_text_color")]
    pub inactive_text_color: String,
    #[serde(default = "default_background_color")]
    pub background_color: String,
    #[serde(default = "default_height")]
    pub height: f32,
    #[serde(default = "default_border_radius")]
    pub border_radius: f32,
    #[serde(default = "default_gap")]
    pub gap: f32,
    #[serde(default = "default_transition_duration")]
    pub transition_duration: f64,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(PillNav {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl PillNav {
    fn resolved_font_size(&self) -> f32 {
        self.style.font_size_px_or(14.0)
    }

    fn make_font(&self, bold: bool) -> skia_safe::Font {
        let fm = font_mgr();
        let font_style = if bold {
            skia_safe::FontStyle::bold()
        } else {
            skia_safe::FontStyle::normal()
        };
        let family = self.style.font_family.as_deref().unwrap_or("Inter");
        let typeface = fm
            .match_family_style(family, font_style)
            .or_else(|| fm.match_family_style("Helvetica", font_style))
            .unwrap_or_else(|| fm.legacy_make_typeface(None, font_style).unwrap());
        skia_safe::Font::from_typeface(typeface, self.resolved_font_size())
    }

    fn compute_tab_layout(&self) -> (f32, Vec<f32>, Vec<f32>) {
        let font = self.make_font(false);
        let font_size = self.resolved_font_size();
        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));
        let h_pad = font_size * 1.2;

        let mut tab_widths: Vec<f32> = Vec::new();
        for label in &self.items {
            let text_w = measure_text_with_fallback(label, &font, &emoji_font, 0.0);
            tab_widths.push(text_w + h_pad * 2.0);
        }

        let inner_pad = self.gap;
        let mut tab_positions: Vec<f32> = Vec::new();
        let mut x = inner_pad;
        for (i, tw) in tab_widths.iter().enumerate() {
            tab_positions.push(x);
            x += tw;
            if i < tab_widths.len() - 1 {
                x += self.gap;
            }
        }

        let total_w = x + inner_pad;
        (total_w, tab_positions, tab_widths)
    }

    fn active_at_time(&self, time: f64) -> (u32, Option<(u32, f64)>) {
        let mut current = self.active_index;
        let mut _prev_index = self.active_index;
        let mut transition_info: Option<(u32, f64)> = None;

        let mut sorted_transitions = self.transitions.clone();
        sorted_transitions.sort_by(|a, b| a.at.partial_cmp(&b.at).unwrap());

        for tr in &sorted_transitions {
            if time < tr.at {
                break;
            }
            let end_time = tr.at + self.transition_duration;
            if time < end_time {
                // Mid-transition
                let progress = (time - tr.at) / self.transition_duration;
                _prev_index = current;
                current = tr.to;
                transition_info = Some((_prev_index, progress));
                break;
            }
            _prev_index = current;
            current = tr.to;
        }

        (current, transition_info)
    }
}

impl PillNav {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, time: f64) {
        if self.items.is_empty() {
            return;
        }

        let w = layout_w;
        let h = layout_h;

        // Outer container
        let outer_rect = Rect::from_xywh(0.0, 0.0, w, h);
        let outer_rrect = RRect::new_rect_xy(outer_rect, self.border_radius, self.border_radius);
        let mut bg_paint = paint_from_hex(&self.background_color);
        bg_paint.set_style(PaintStyle::Fill);
        bg_paint.set_anti_alias(true);
        canvas.draw_rrect(outer_rrect, &bg_paint);

        let (_total_w, tab_positions, tab_widths) = self.compute_tab_layout();
        let (active, transition_info) = self.active_at_time(time);

        // Pill indicator
        let pill_h = h - self.gap * 2.0;
        let pill_y = self.gap;
        let pill_radius = self.border_radius - self.gap;

        let active_idx = active.min(self.items.len() as u32 - 1) as usize;
        let (pill_x, pill_w) = if let Some((from_idx, progress)) = transition_info {
            let from = from_idx.min(self.items.len() as u32 - 1) as usize;
            let to = active_idx;
            let t = progress as f32;
            let from_x = tab_positions[from];
            let to_x = tab_positions[to];
            let from_w = tab_widths[from];
            let to_w = tab_widths[to];
            (
                from_x + (to_x - from_x) * t,
                from_w + (to_w - from_w) * t,
            )
        } else {
            (tab_positions[active_idx], tab_widths[active_idx])
        };

        let pill_rect = Rect::from_xywh(pill_x, pill_y, pill_w, pill_h);
        let pill_rrect = RRect::new_rect_xy(pill_rect, pill_radius, pill_radius);
        let mut pill_paint = paint_from_hex(&self.pill_color);
        pill_paint.set_style(PaintStyle::Fill);
        pill_paint.set_anti_alias(true);
        canvas.draw_rrect(pill_rrect, &pill_paint);

        // Tab labels
        let font = self.make_font(false);
        let font_size = self.resolved_font_size();
        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));
        let (_, metrics) = font.metrics();
        let text_y = (h + (-metrics.ascent)) / 2.0;

        for (i, label) in self.items.iter().enumerate() {
            let is_active = i == active_idx;
            let color = if is_active {
                &self.text_color
            } else {
                &self.inactive_text_color
            };
            let mut label_paint = paint_from_hex(color);
            label_paint.set_anti_alias(true);

            let text_w = measure_text_with_fallback(label, &font, &emoji_font, 0.0);
            let text_x = tab_positions[i] + (tab_widths[i] - text_w) / 2.0;

            draw_text_with_fallback(
                canvas,
                label,
                &font,
                &emoji_font,
                0.0,
                text_x,
                text_y,
                &label_paint,
            );
        }
    }
}

impl Painter for PillNav {
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
