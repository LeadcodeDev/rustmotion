use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, PaintStyle, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    typeface_with_fallback,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

/// Text alignment for table columns.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ColumnAlign {
    #[default]
    Left,
    Center,
    Right,
}

pub(crate) const DEFAULT_FONT_SIZE: f32 = 14.0;
/// row_height = DEFAULT_FONT_SIZE * 2.5
pub(crate) const DEFAULT_ROW_HEIGHT_RATIO: f32 = 2.5;
pub(crate) const DEFAULT_CELL_PADDING: f32 = 12.0;

fn default_cell_padding() -> f32 {
    DEFAULT_CELL_PADDING
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
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Table {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Table {
    /// `font_size` is resolved once by the caller (`paint`, against a real
    /// `LengthContext`) and passed in — this used to be a zero-argument
    /// method independently re-deriving the value via the context-free
    /// `font_size_px_or` at every call site (`row_height`, `make_font`,
    /// `paint` itself), which is exactly the kind of duplicate computation
    /// that let a relative unit silently diverge (lot B, wave S).
    fn row_height(&self, font_size: f32) -> f32 {
        font_size * 2.5
    }

    fn make_font(&self, bold: bool, font_size: f32) -> Option<skia_safe::Font> {
        let font_style = if bold {
            skia_safe::FontStyle::bold()
        } else {
            skia_safe::FontStyle::normal()
        };

        let family = self.style.font_family.as_deref().unwrap_or("Inter");
        let typeface = typeface_with_fallback(family, font_style).ok()?;
        Some(skia_safe::Font::from_typeface(typeface, font_size))
    }

    /// Resolve column widths: use explicit widths if provided, else equal distribution.
    fn resolve_column_widths(&self, total_w: f32) -> Vec<f32> {
        let col_count = self.headers.len().max(1);
        if let Some(widths) = &self.column_widths {
            let mut result: Vec<f32> = widths.to_vec();
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

impl Table {
    fn paint(&self, canvas: &Canvas, layout_w: f32, layout_h: f32, ctx: &PaintCtx) {
        let w = layout_w;
        // Resolved once, against the real per-frame viewport (`rem`/`vw`/
        // `vh` on `font-size` now resolve instead of silently dropping to
        // 0px — lot B, wave S) and threaded through every call below that
        // used to independently re-derive it via `font_size_px_or`.
        let font_size = self.style.font_size_px_ctx(
            &crate::intrinsic::font_size_ctx(
                ctx.video_width as f32,
                ctx.video_height as f32,
                w.max(0.0),
            ),
            14.0,
        );
        let col_count = self.headers.len().max(1);
        let col_widths = self.resolve_column_widths(w);
        let row_h = self.row_height(font_size);

        let header_color = self.header_color.as_deref().unwrap_or("#374151");
        let border_color = self.border_color.as_deref().unwrap_or("#4B5563");
        let text_color = self.style.color_str_or("#FFFFFF");
        let header_text_color = self.header_text_color.as_deref().unwrap_or("#FFFFFF");
        let default_row_colors = vec!["#1F2937".to_string(), "#111827".to_string()];
        // `"row_colors": []` deserializes to Some(vec![]), not None — a generator
        // writes it to mean "no striping". Row painting indexes this slice, so an
        // empty one has to fall back rather than reach the painter.
        let row_colors = self
            .row_colors
            .as_ref()
            .filter(|c| !c.is_empty())
            .unwrap_or(&default_row_colors);

        // Resolve fonts before the optional clip below so an early return on
        // font failure keeps canvas save/restore balanced.
        let Some(header_font) = self.make_font(true, font_size) else {
            return;
        };
        let Some(body_font) = self.make_font(false, font_size) else {
            return;
        };

        // Clip to rounded rect if border-radius is set
        let radius_px = self.style.border_radius_px_or(0.0);
        let has_radius = radius_px > 0.0;
        if has_radius {
            let rect = Rect::from_xywh(0.0, 0.0, w, layout_h);
            let rrect = skia_safe::RRect::new_rect_xy(rect, radius_px, radius_px);
            canvas.save();
            canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);
        }

        // Header row
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
            let y = (row_h - font_size) / 2.0 + header_ascent;

            draw_text_with_fallback(
                canvas,
                header,
                &header_font,
                &emoji_font,
                0.0,
                x,
                y,
                &header_text_paint,
            );
            col_x += cw;
        }

        // Data rows
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
                let y = y_base + (row_h - font_size) / 2.0 + body_ascent;

                draw_text_with_fallback(
                    canvas,
                    cell,
                    &body_font,
                    &emoji_font,
                    0.0,
                    x,
                    y,
                    &text_paint,
                );
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
    }
}

impl Painter for Table {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout.width, layout.height, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::css::Length;

    // ─── Lot B, wave S: relative `font-size` units ─────────────────────────

    #[test]
    fn rem_font_size_paints_visible_ink() {
        // Reproduction: `font-size: "2rem"` used to resolve to 0px via the
        // context-free `font_size_px_or`.
        let table = Table {
            headers: vec!["A".to_string(), "B".to_string()],
            rows: vec![vec!["1".to_string(), "2".to_string()]],
            header_color: None,
            row_colors: None,
            border_color: None,
            header_text_color: None,
            column_widths: None,
            column_align: None,
            cell_padding: DEFAULT_CELL_PADDING,
            show_borders: true,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::String("2rem".into())),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
        };
        const W: i32 = 400;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let ctx = PaintCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: 1.0,
            frame_index: 0,
            fps: 30,
            video_width: 400,
            video_height: 200,
            stagger_offset: 0.0,
        };
        {
            let canvas = surface.canvas();
            table.paint(canvas, W as f32, H as f32, &ctx);
        }
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (W, H),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (W * H * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (W * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        // Header text is white (#FFFFFF) on a #374151 header background —
        // probe specifically for near-white text ink rather than any lit
        // pixel (the header/row backgrounds paint regardless of font-size).
        let text_ink = buf
            .chunks_exact(4)
            .filter(|p| p[3] > 0 && p[0] > 200 && p[1] > 200 && p[2] > 200)
            .count();
        assert!(
            text_ink > 10,
            "table at font-size: 2rem must paint visible header/cell text, got {text_ink} pixels"
        );
    }
}
