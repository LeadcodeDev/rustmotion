use crate::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use crate::engine::renderer::{font_mgr, paint_from_hex, emoji_typeface, draw_text_with_fallback, measure_text_with_fallback};
use crate::error::RustmotionError;
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{LayerStyle, Size};
use crate::traits::{RenderContext, TimingConfig, Widget};

/// Text alignment for table columns.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ColumnAlign {
    Left,
    Center,
    Right,
}

impl Default for ColumnAlign {
    fn default() -> Self {
        Self::Left
    }
}

fn default_cell_padding() -> f32 {
    12.0
}

fn default_show_borders() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Table {
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    #[serde(default)]
    pub header_color: Option<String>,
    #[serde(default)]
    pub row_colors: Option<Vec<String>>,
    #[serde(default)]
    pub border_color: Option<String>,
    #[serde(default)]
    pub header_text_color: Option<String>,
    /// Explicit column widths in pixels. If omitted, columns share width equally.
    #[serde(default)]
    pub column_widths: Option<Vec<f32>>,
    /// Per-column text alignment.
    #[serde(default)]
    pub column_align: Option<Vec<ColumnAlign>>,
    /// Cell padding in pixels (default: 12).
    #[serde(default = "default_cell_padding")]
    pub cell_padding: f32,
    /// Show grid borders (default: true).
    #[serde(default = "default_show_borders")]
    pub show_borders: bool,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(Table {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Table {
    fn font_size(&self) -> f32 {
        self.style.font_size.unwrap_or(14.0)
    }

    fn row_height(&self) -> f32 {
        self.font_size() * 2.5
    }

    fn make_font(&self, bold: bool) -> skia_safe::Font {
        let fm = font_mgr();
        let font_style = if bold {
            skia_safe::FontStyle::bold()
        } else {
            skia_safe::FontStyle::normal()
        };

        let family = self
            .style
            .font_family
            .as_deref()
            .unwrap_or("Inter");

        let typeface = fm
            .match_family_style(family, font_style)
            .or_else(|| fm.match_family_style("Helvetica", font_style))
            .or_else(|| fm.match_family_style("Arial", font_style))
            .expect(&RustmotionError::FontNotFound.to_string());

        skia_safe::Font::from_typeface(typeface, self.font_size())
    }

    /// Resolve column widths: use explicit widths if provided, else equal distribution.
    fn resolve_column_widths(&self, total_w: f32) -> Vec<f32> {
        let col_count = self.headers.len().max(1);
        if let Some(widths) = &self.column_widths {
            let mut result: Vec<f32> = widths.iter().copied().collect();
            // Pad with equal-share for missing columns
            while result.len() < col_count {
                let remaining = total_w - result.iter().sum::<f32>();
                let remaining_cols = col_count - result.len();
                result.push(remaining / remaining_cols as f32);
            }
            result.truncate(col_count);
            result
        } else {
            vec![total_w / col_count as f32; col_count]
        }
    }

    /// Get alignment for a specific column.
    fn get_align(&self, col: usize) -> &ColumnAlign {
        self.column_align
            .as_ref()
            .and_then(|a| a.get(col))
            .unwrap_or(&ColumnAlign::Left)
    }

    /// Compute the x position for text given alignment, cell x, cell width, text width, and padding.
    fn align_text_x(&self, col: usize, cell_x: f32, cell_w: f32, text_w: f32) -> f32 {
        let pad = self.cell_padding;
        match self.get_align(col) {
            ColumnAlign::Left => cell_x + pad,
            ColumnAlign::Center => cell_x + (cell_w - text_w) / 2.0,
            ColumnAlign::Right => cell_x + cell_w - text_w - pad,
        }
    }
}

impl Widget for Table {
    fn render(
        &self,
        canvas: &Canvas,
        layout: &LayoutNode,
        _ctx: &RenderContext,
        _props: &crate::engine::animator::AnimatedProperties,
    ) -> Result<()> {
        let w = layout.width;
        let col_count = self.headers.len().max(1);
        let col_widths = self.resolve_column_widths(w);
        let row_h = self.row_height();

        let header_color = self.header_color.as_deref().unwrap_or("#374151");
        let border_color = self.border_color.as_deref().unwrap_or("#4B5563");
        let text_color = self.style.color.as_deref().unwrap_or("#FFFFFF");
        let header_text_color = self.header_text_color.as_deref().unwrap_or("#FFFFFF");
        let default_row_colors = vec!["#1F2937".to_string(), "#111827".to_string()];
        let row_colors = self.row_colors.as_ref().unwrap_or(&default_row_colors);

        // Clip to rounded rect if border-radius is set
        let has_radius = self.style.border_radius.unwrap_or(0.0) > 0.0;
        if has_radius {
            let radius = self.style.border_radius.unwrap_or(0.0);
            let rect = Rect::from_xywh(0.0, 0.0, w, layout.height);
            let rrect = skia_safe::RRect::new_rect_xy(rect, radius, radius);
            canvas.save();
            canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);
        }

        // Header row
        let header_font = self.make_font(true);
        let font_size = self.font_size();
        let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, font_size));
        let (_, header_metrics) = header_font.metrics();
        let header_ascent = -header_metrics.ascent;

        let mut header_bg = paint_from_hex(header_color);
        header_bg.set_style(PaintStyle::Fill);
        canvas.draw_rect(Rect::from_xywh(0.0, 0.0, w, row_h), &header_bg);

        let mut header_text_paint = paint_from_hex(header_text_color);
        header_text_paint.set_anti_alias(true);

        let mut col_x = 0.0_f32;
        for (i, header) in self.headers.iter().enumerate() {
            let cw = col_widths.get(i).copied().unwrap_or(0.0);
            let text_w = measure_text_with_fallback(header, &header_font, &emoji_font, 0.0);
            let x = self.align_text_x(i, col_x, cw, text_w);
            let y = (row_h - self.font_size()) / 2.0 + header_ascent;

            draw_text_with_fallback(canvas, header, &header_font, &emoji_font, 0.0, x, y, &header_text_paint);
            col_x += cw;
        }

        // Data rows
        let body_font = self.make_font(false);
        let (_, body_metrics) = body_font.metrics();
        let body_ascent = -body_metrics.ascent;

        let mut text_paint = paint_from_hex(text_color);
        text_paint.set_anti_alias(true);

        for (row_idx, row) in self.rows.iter().enumerate() {
            let y_base = (row_idx + 1) as f32 * row_h;

            // Row background
            let row_color_idx = row_idx % row_colors.len().max(1);
            let row_bg_color = &row_colors[row_color_idx];
            let mut row_bg = paint_from_hex(row_bg_color);
            row_bg.set_style(PaintStyle::Fill);
            canvas.draw_rect(Rect::from_xywh(0.0, y_base, w, row_h), &row_bg);

            // Cell text
            let mut cx = 0.0_f32;
            for (col_idx, cell) in row.iter().enumerate() {
                if col_idx >= col_count {
                    break;
                }
                let cw = col_widths.get(col_idx).copied().unwrap_or(0.0);
                let text_w = measure_text_with_fallback(cell, &body_font, &emoji_font, 0.0);
                let x = self.align_text_x(col_idx, cx, cw, text_w);
                let y = y_base + (row_h - self.font_size()) / 2.0 + body_ascent;

                draw_text_with_fallback(canvas, cell, &body_font, &emoji_font, 0.0, x, y, &text_paint);
                cx += cw;
            }
        }

        // Grid lines (only if show_borders is true)
        if self.show_borders {
            let mut border_paint = paint_from_hex(border_color);
            border_paint.set_style(PaintStyle::Stroke);
            border_paint.set_stroke_width(1.0);

            let total_h = (1 + self.rows.len()) as f32 * row_h;

            // Horizontal lines
            for i in 0..=(self.rows.len() + 1) {
                let y = i as f32 * row_h;
                canvas.draw_line((0.0, y), (w, y), &border_paint);
            }

            // Vertical lines
            let mut vx = 0.0_f32;
            for i in 0..=col_count {
                canvas.draw_line((vx, 0.0), (vx, total_h), &border_paint);
                if i < col_count {
                    vx += col_widths.get(i).copied().unwrap_or(0.0);
                }
            }
        }

        if has_radius {
            canvas.restore();
        }

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        if let Some(size) = &self.size {
            return (size.width, size.height);
        }

        let col_count = self.headers.len().max(1);
        let w = col_count as f32 * 120.0;
        let h = (1 + self.rows.len()) as f32 * self.row_height();

        (w, h)
    }
}
