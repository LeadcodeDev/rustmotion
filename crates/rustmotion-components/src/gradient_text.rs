use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Font, FontStyle, Point};

use rustmotion_core::css::style::{
    FontStyle as CssFontStyle, FontWeight as CssFontWeight, FontWeightKw,
    WhiteSpace as CssWhiteSpace,
};
use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    parse_hex_color, typeface_with_fallback, wrap_text_with_tracking,
};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_colors() -> Vec<String> {
    vec!["#3B82F6".to_string(), "#8B5CF6".to_string()]
}

fn default_angle() -> f32 {
    90.0
}

fn default_speed() -> f32 {
    0.5
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GradientText {
    pub content: String,
    #[serde(default = "default_colors")]
    pub colors: Vec<String>,
    #[serde(default = "default_angle")]
    pub angle: f32,
    #[serde(default)]
    pub animate_angle: bool,
    #[serde(default = "default_speed")]
    pub speed: f32,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(GradientText {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl GradientText {
    fn resolve_font(&self) -> Option<(Font, Option<Font>)> {
        let font_size = self.style.font_size_px_or(48.0);
        let font_family = self.style.font_family_or("Inter");

        let slant = match self.style.font_style {
            Some(CssFontStyle::Italic) => skia_safe::font_style::Slant::Italic,
            Some(CssFontStyle::Oblique) => skia_safe::font_style::Slant::Oblique,
            _ => skia_safe::font_style::Slant::Upright,
        };
        let weight = match &self.style.font_weight {
            Some(CssFontWeight::Keyword(FontWeightKw::Bold | FontWeightKw::Bolder)) => {
                skia_safe::font_style::Weight::BOLD
            }
            Some(CssFontWeight::Number(n)) => skia_safe::font_style::Weight::from(*n as i32),
            _ => skia_safe::font_style::Weight::NORMAL,
        };
        let skia_style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);

        let typeface = typeface_with_fallback(font_family, skia_style).ok()?;

        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));
        Some((font, emoji_font))
    }
}

impl GradientText {
    fn paint(&self, canvas: &Canvas, layout_width: f32, time: f64, ctx: &PaintCtx) {
        if self.content.is_empty() || self.colors.is_empty() {
            return;
        }

        let Some((font, emoji_font)) = self.resolve_font() else {
            return;
        };
        // `font-size` stays context-free — see the identical note in
        // `text.rs`'s `paint`: resolving its own `em`/`%` correctly needs a
        // cascade.rs change (parent font-size as a resolved px value, not a
        // raw `Length`), which is out of scope here (issue #125 §2).
        let font_size = self.style.font_size_px_or(48.0);
        // `letter-spacing`/`line-height`'s `em`/`%` are relative to this
        // element's own font-size, no cascade dependency — a real
        // `LengthContext` is available here, so use the context-aware
        // resolvers (issue #125 §2: correctly handles `vw`/`vh`/`rem`/
        // line-height-`%`). `letter_spacing` itself: this component never
        // read `style.letter-spacing` before this fix — the wrap/measure/
        // draw calls below always tracked at 0.0 regardless of what was
        // set, which is the same class of measure/paint disagreement issue
        // #125 §1 describes elsewhere. It's threaded through consistently
        // now.
        let type_ctx = rustmotion_core::css::units::LengthContext {
            viewport_width: ctx.video_width as f32,
            viewport_height: ctx.video_height as f32,
            parent_size: layout_width.max(0.0),
            font_size,
            root_font_size: 16.0,
        };
        let line_height_val = self.style.line_height_for_ctx(font_size, &type_ctx);
        let letter_spacing = self.style.letter_spacing_px_ctx(&type_ctx);

        // M1: `white-space: nowrap|pre` keeps the whole content on one line
        // even past `layout_width` (it bleeds); anything else word-wraps at
        // the box width — same rule `text.rs` uses.
        let nowrap = matches!(
            self.style.white_space,
            Some(CssWhiteSpace::Nowrap | CssWhiteSpace::Pre)
        );
        let wrap_at = if nowrap {
            None
        } else if layout_width.is_finite() && layout_width > 0.0 {
            Some(layout_width)
        } else {
            None
        };
        // Tracking-aware wrap (issue #125 §1), consistent with the
        // real-tracking measurement/draw below.
        let lines =
            wrap_text_with_tracking(&self.content, &font, &emoji_font, wrap_at, letter_spacing);

        // Measure the overall (possibly multi-line) bounding box. The
        // gradient is defined once across this whole block rather than
        // per-line, so wrapping reflows the text without fragmenting the
        // colour transition.
        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;
        let descent = metrics.descent;
        let text_w = lines
            .iter()
            .map(|l| measure_text_with_fallback(l, &font, &emoji_font, letter_spacing))
            .fold(0.0f32, f32::max);
        let text_h = (lines.len().max(1) - 1) as f32 * line_height_val + ascent + descent;

        // Compute angle (possibly animated)
        let angle = if self.animate_angle {
            self.angle + time as f32 * self.speed * 360.0
        } else {
            self.angle
        };

        // Compute gradient endpoints from angle, spanning the full block.
        let angle_rad = angle * std::f32::consts::PI / 180.0;
        let cx = text_w / 2.0;
        let cy = text_h / 2.0;
        let half_diag = (text_w.powi(2) + text_h.powi(2)).sqrt() / 2.0;
        let start = Point::new(
            cx - angle_rad.cos() * half_diag,
            cy - angle_rad.sin() * half_diag,
        );
        let end = Point::new(
            cx + angle_rad.cos() * half_diag,
            cy + angle_rad.sin() * half_diag,
        );

        // Parse gradient colors
        let skia_colors: Vec<skia_safe::Color> = self
            .colors
            .iter()
            .map(|hex| {
                let (r, g, b, a) = parse_hex_color(hex);
                skia_safe::Color::from_argb(a, r, g, b)
            })
            .collect();

        // Build linear gradient shader
        let positions: Option<&[f32]> = None;
        let shader = skia_safe::shader::Shader::linear_gradient(
            (start, end),
            skia_safe::gradient_shader::GradientShaderColors::Colors(&skia_colors),
            positions,
            skia_safe::TileMode::Clamp,
            None,
            None,
        );

        let fill_paint = match shader {
            Some(shader) => {
                let mut p = skia_safe::Paint::default();
                p.set_anti_alias(true);
                p.set_shader(shader);
                p
            }
            None => {
                // Fallback: draw with the first color, no gradient.
                let mut p = paint_from_hex(&self.colors[0]);
                p.set_anti_alias(true);
                p
            }
        };

        for (i, line) in lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            let y = i as f32 * line_height_val + ascent;
            draw_text_with_fallback(
                canvas,
                line,
                &font,
                &emoji_font,
                letter_spacing,
                0.0,
                y,
                &fill_paint,
            );
        }
    }
}

impl Painter for GradientText {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        self.paint(canvas, layout.width, ctx.time, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::css::style::CssStyle;
    use rustmotion_core::css::Length;

    fn make_gradient_text(content: &str, white_space: Option<CssWhiteSpace>) -> GradientText {
        GradientText {
            content: content.into(),
            colors: default_colors(),
            angle: default_angle(),
            animate_angle: false,
            speed: default_speed(),
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(28.0)),
                white_space,
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
        }
    }

    fn alpha_grid(surface: &mut skia_safe::Surface, width: i32, height: i32) -> Vec<u8> {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (width, height),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (width * height * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (width * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        (0..(width * height) as usize)
            .map(|i| buf[i * 4 + 3])
            .collect()
    }

    fn test_ctx() -> PaintCtx {
        PaintCtx {
            time: 0.0,
            scene_duration: 1.0,
            frame_index: 0,
            fps: 30,
            video_width: 1920,
            video_height: 1080,
            stagger_offset: 0.0,
        }
    }

    fn has_ink_in(grid: &[u8], surface_width: i32, x0: i32, x1: i32, y0: i32, y1: i32) -> bool {
        for y in y0..y1 {
            for x in x0..x1 {
                if grid[(y * surface_width + x) as usize] > 0 {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn nowrap_paints_a_single_line_past_the_layout_width() {
        // M1 render-level proof, gradient_text: `white-space: nowrap` stays
        // on one line and bleeds past `layout_width`.
        let gt = make_gradient_text(
            "the quick brown fox jumps over the lazy dog",
            Some(CssWhiteSpace::Nowrap),
        );
        const W: i32 = 600;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let canvas = surface.canvas();
        gt.paint(canvas, 80.0, 0.0, &test_ctx());
        let grid = alpha_grid(&mut surface, W, H);

        assert!(
            has_ink_in(&grid, W, 300, W, 0, 45),
            "nowrap gradient_text must paint past its 80px box on line 1"
        );
        assert!(
            !has_ink_in(&grid, W, 0, W, 55, H),
            "nowrap gradient_text must stay on a single line"
        );
    }

    #[test]
    fn normal_white_space_wraps_within_the_layout_width() {
        let gt = make_gradient_text("the quick brown fox jumps over the lazy dog", None);
        const W: i32 = 600;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let canvas = surface.canvas();
        gt.paint(canvas, 80.0, 0.0, &test_ctx());
        let grid = alpha_grid(&mut surface, W, H);

        assert!(
            !has_ink_in(&grid, W, 300, W, 0, 45),
            "wrapped gradient_text must not reach x∈[300,600) on line 1 within an 80px box"
        );
        assert!(
            has_ink_in(&grid, W, 0, W, 55, H),
            "wrapped gradient_text must spill onto a second line within the box width"
        );
    }
}
